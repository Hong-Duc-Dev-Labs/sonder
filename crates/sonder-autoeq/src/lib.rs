//! Headphone correction EQs from the AutoEQ project (github.com/jaakkopasanen/AutoEq, MIT).
//!
//! AutoEQ publishes one `ParametricEQ.txt` per headphone and measurement source, plus an index
//! of every result. [`AutoEq::search`] downloads that index once (about 1 MB), keeps it on disk
//! for a week, and searches it locally: the name the user types never leaves the machine.
//! [`AutoEq::parametric_eq`] downloads the one file the user picks. Both go only to
//! raw.githubusercontent.com. With no connection, a stale index on disk is still used.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

mod index;

pub use index::{Entry, parse_index, search};

/// Where the AutoEQ results live.
const RESULTS: &str = "https://raw.githubusercontent.com/jaakkopasanen/AutoEq/master/results";
/// An index on disk younger than this is used without asking the network.
const INDEX_MAX_AGE: Duration = Duration::from_secs(7 * 24 * 3600);
const INDEX_FILE: &str = "INDEX.md";
/// Largest index accepted (it is about 1 MB).
const MAX_INDEX_BYTES: u64 = 16 * 1024 * 1024;
/// Largest parametric EQ file accepted (they are under 1 KB).
const MAX_EQ_BYTES: u64 = 256 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum AutoEqError {
    #[error("cannot reach AutoEQ: {0}")]
    Network(String),
    #[error("AutoEQ has no parametric EQ for {0}")]
    NotFound(String),
    #[error("the AutoEQ index could not be read")]
    BadIndex,
}

/// Fetches a URL's text: `Ok(None)` for a 404. Replaced in tests.
pub type Fetch = Box<dyn Fn(&str, u64) -> Result<Option<String>, AutoEqError> + Send + Sync>;

pub struct AutoEq {
    dir: PathBuf,
    fetch: Fetch,
    /// The parsed index, once loaded in this run.
    index: Mutex<Option<Arc<Vec<Entry>>>>,
}

impl AutoEq {
    /// An AutoEQ client keeping its index in `dir`. `user_agent` names the app to GitHub.
    pub fn new(dir: impl Into<PathBuf>, user_agent: &str) -> Self {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(20)))
            .user_agent(user_agent)
            .http_status_as_error(false)
            .build()
            .into();
        Self::with_fetch(
            dir,
            Box::new(move |url, limit| {
                let net = |e: ureq::Error| AutoEqError::Network(e.to_string());
                let mut resp = agent.get(url).call().map_err(net)?;
                match resp.status().as_u16() {
                    200 => Ok(Some(
                        resp.body_mut()
                            .with_config()
                            .limit(limit)
                            .read_to_string()
                            .map_err(net)?,
                    )),
                    404 => Ok(None),
                    code => Err(AutoEqError::Network(format!("HTTP {code}"))),
                }
            }),
        )
    }

    /// An AutoEQ client with its own way of fetching URLs.
    pub fn with_fetch(dir: impl Into<PathBuf>, fetch: Fetch) -> Self {
        Self {
            dir: dir.into(),
            fetch,
            index: Mutex::new(None),
        }
    }

    /// Up to `limit` headphones matching `query`, best first. Loads the index on first use.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<Entry>, AutoEqError> {
        Ok(search(&self.index()?, query, limit))
    }

    /// The Equalizer APO parametric text AutoEQ publishes for `entry`.
    pub fn parametric_eq(&self, entry: &Entry) -> Result<String, AutoEqError> {
        let url = entry
            .parametric_eq_url(RESULTS)
            .ok_or_else(|| AutoEqError::NotFound(entry.name.clone()))?;
        (self.fetch)(&url, MAX_EQ_BYTES)?.ok_or_else(|| AutoEqError::NotFound(entry.name.clone()))
    }

    fn index(&self) -> Result<Arc<Vec<Entry>>, AutoEqError> {
        let mut loaded = self.index.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(index) = loaded.as_ref() {
            return Ok(index.clone());
        }
        let index = Arc::new(self.load_index()?);
        *loaded = Some(index.clone());
        Ok(index)
    }

    /// The index from disk while it is fresh, else from the network (saved for next time),
    /// else whatever is on disk.
    fn load_index(&self) -> Result<Vec<Entry>, AutoEqError> {
        let path = self.dir.join(INDEX_FILE);
        let cached = std::fs::read_to_string(&path).ok();
        let fresh = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| SystemTime::now().duration_since(t).ok())
            .is_some_and(|age| age < INDEX_MAX_AGE);
        if let (true, Some(text)) = (fresh, cached.as_deref()) {
            let entries = parse_index(text);
            if !entries.is_empty() {
                return Ok(entries);
            }
        }
        let url = format!("{RESULTS}/{INDEX_FILE}");
        let downloaded = (self.fetch)(&url, MAX_INDEX_BYTES)
            .ok()
            .flatten()
            .filter(|text| !parse_index(text).is_empty());
        if let Some(text) = downloaded {
            // A failed save only means downloading again next time.
            let _ = std::fs::create_dir_all(&self.dir).and_then(|()| std::fs::write(&path, &text));
            return Ok(parse_index(&text));
        }
        match cached.map(|text| parse_index(&text)) {
            Some(entries) if !entries.is_empty() => Ok(entries),
            _ => Err(AutoEqError::Network(
                "the headphone list could not be downloaded".into(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const INDEX: &str = "# Index\nIntro text.\n\n\
        - [Sennheiser HD 600](./oratory1990/over-ear/Sennheiser%20HD%20600) by oratory1990\n\
        - [Sennheiser HD 600 (2020)](./crinacle/GRAS%2043AG-7%20over-ear/Sennheiser%20HD%20600%20(2020)) by crinacle on GRAS 43AG-7\n";

    fn client(dir: &std::path::Path, online: bool, calls: Arc<AtomicUsize>) -> AutoEq {
        AutoEq::with_fetch(
            dir,
            Box::new(move |url, _| {
                calls.fetch_add(1, Ordering::SeqCst);
                if !online {
                    return Err(AutoEqError::Network("offline".into()));
                }
                if url.ends_with("/INDEX.md") {
                    Ok(Some(INDEX.into()))
                } else if url.ends_with("/Sennheiser%20HD%20600%20ParametricEQ.txt") {
                    Ok(Some("Preamp: -6.3 dB\n".into()))
                } else {
                    Ok(None)
                }
            }),
        )
    }

    #[test]
    fn the_index_is_downloaded_once_and_kept_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let eq = client(dir.path(), true, calls.clone());
        assert_eq!(eq.search("hd 600", 10).unwrap().len(), 2);
        assert_eq!(eq.search("hd600", 10).unwrap().len(), 2);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(dir.path().join(INDEX_FILE).exists());

        // A new run reads the fresh copy on disk without the network.
        let offline = Arc::new(AtomicUsize::new(0));
        let again = client(dir.path(), false, offline.clone());
        assert_eq!(again.search("600", 10).unwrap().len(), 2);
        assert_eq!(offline.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_stale_index_is_used_when_offline() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(INDEX_FILE);
        std::fs::write(&path, INDEX).unwrap();
        let old = SystemTime::now() - INDEX_MAX_AGE - Duration::from_secs(60);
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(old)
            .unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let eq = client(dir.path(), false, calls.clone());
        assert_eq!(eq.search("hd 600", 10).unwrap().len(), 2);
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "it tried the network first"
        );
    }

    #[test]
    fn no_index_and_no_network_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let eq = client(dir.path(), false, Arc::new(AtomicUsize::new(0)));
        assert!(matches!(
            eq.search("hd 600", 10),
            Err(AutoEqError::Network(_))
        ));
    }

    #[test]
    fn the_parametric_eq_of_a_result_is_fetched() {
        let dir = tempfile::tempdir().unwrap();
        let eq = client(dir.path(), true, Arc::new(AtomicUsize::new(0)));
        let found = eq.search("hd 600", 10).unwrap();
        let plain = found
            .iter()
            .find(|e| e.name == "Sennheiser HD 600")
            .unwrap();
        assert_eq!(eq.parametric_eq(plain).unwrap(), "Preamp: -6.3 dB\n");
        let other = found
            .iter()
            .find(|e| e.name != "Sennheiser HD 600")
            .unwrap();
        assert!(matches!(
            eq.parametric_eq(other),
            Err(AutoEqError::NotFound(_))
        ));
    }

    /// Reaches the real AutoEQ repository: `cargo test -p sonder-autoeq -- --ignored`.
    #[test]
    #[ignore = "network"]
    fn the_live_index_finds_a_well_known_headphone() {
        let dir = tempfile::tempdir().unwrap();
        let eq = AutoEq::new(dir.path(), "Sonder-tests");
        let found = eq.search("Sennheiser HD 600", 5).unwrap();
        let first = found.first().expect("HD 600 is in AutoEQ");
        assert!(eq.parametric_eq(first).unwrap().contains("Preamp:"));
    }
}
