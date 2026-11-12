//! On-disk cache of online lookups, one small JSON file per track query. A miss is cached
//! too, so a track without lyrics anywhere is not looked up on every play; misses expire
//! after a while because services keep adding lyrics.

use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{Lyrics, Query};

/// How long "no lyrics found" is trusted.
const MISS_TTL: Duration = Duration::from_secs(14 * 24 * 3600);

#[derive(Serialize, Deserialize)]
struct Entry {
    /// Seconds since the Unix epoch.
    fetched: u64,
    lyrics: Option<Lyrics>,
}

pub struct Cache {
    dir: PathBuf,
}

impl Cache {
    pub fn new(dir: PathBuf) -> Self {
        Cache { dir }
    }

    /// `Some(result)` when this query was answered before (`Some(None)` = nothing found).
    pub fn get(&self, query: &Query) -> Option<Option<Lyrics>> {
        let text = fs::read_to_string(self.path(query)).ok()?;
        let entry: Entry = serde_json::from_str(&text).ok()?;
        if entry.lyrics.is_none() && now().saturating_sub(entry.fetched) > MISS_TTL.as_secs() {
            return None;
        }
        Some(entry.lyrics)
    }

    /// Stores a result; a failure to write only costs a lookup next time.
    pub fn put(&self, query: &Query, lyrics: Option<&Lyrics>) {
        let entry = Entry {
            fetched: now(),
            lyrics: lyrics.cloned(),
        };
        let Ok(text) = serde_json::to_string(&entry) else {
            return;
        };
        if fs::create_dir_all(&self.dir).is_err() {
            return;
        }
        let path = self.path(query);
        let tmp = path.with_extension("tmp");
        if fs::write(&tmp, text).is_ok() {
            let _ = fs::rename(&tmp, &path);
        }
    }

    fn path(&self, query: &Query) -> PathBuf {
        let mut h = Sha256::new();
        for part in [
            query.title.as_str(),
            query.artist.as_deref().unwrap_or(""),
            query.album.as_deref().unwrap_or(""),
        ] {
            h.update(part.trim().to_lowercase().as_bytes());
            h.update([0]);
        }
        // Whole seconds: a re-rip a few milliseconds longer is the same recording.
        h.update((query.duration_ms / 1000).to_le_bytes());
        let hex: String = h.finalize()[..16]
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        self.dir.join(format!("{hex}.json"))
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query() -> Query {
        Query {
            title: "Song".into(),
            artist: Some("Artist".into()),
            album: None,
            duration_ms: 200_400,
        }
    }

    #[test]
    fn stores_hits_and_misses() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::new(dir.path().join("lyrics"));
        assert_eq!(cache.get(&query()), None);
        cache.put(&query(), None);
        assert_eq!(cache.get(&query()), Some(None));
        let lyrics = Lyrics::plain("LRCLIB", "la").unwrap();
        cache.put(&query(), Some(&lyrics));
        // Case and sub-second differences land on the same entry.
        let mut same = query();
        same.title = "SONG ".into();
        same.duration_ms = 200_900;
        assert_eq!(cache.get(&same), Some(Some(lyrics)));
    }

    #[test]
    fn old_misses_expire() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::new(dir.path().to_path_buf());
        let old = Entry {
            fetched: now() - MISS_TTL.as_secs() - 10,
            lyrics: None,
        };
        fs::write(cache.path(&query()), serde_json::to_string(&old).unwrap()).unwrap();
        assert_eq!(cache.get(&query()), None);
    }
}
