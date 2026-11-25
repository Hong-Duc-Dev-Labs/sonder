//! Persistent playback session (queue order, active track, elapsed position, repeat mode).

use std::collections::HashMap;
use std::fs;
use std::io::ErrorKind;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::dto::RepeatMode;
use crate::state::{AppState, lock};

const SESSION_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SavedSession {
    pub version: u32,
    pub paths: Vec<String>,
    pub index: Option<usize>,
    pub elapsed_ms: u64,
    pub repeat: RepeatMode,
}

impl Default for SavedSession {
    fn default() -> Self {
        Self {
            version: SESSION_VERSION,
            paths: Vec::new(),
            index: None,
            elapsed_ms: 0,
            repeat: RepeatMode::Off,
        }
    }
}

/// Atomically persisted `session.json`.
#[derive(Debug, Clone)]
pub struct SessionStore {
    path: PathBuf,
}

impl SessionStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Reads the saved session; returns `None` if missing or unreadable.
    pub fn load(&self) -> Option<SavedSession> {
        match fs::read_to_string(&self.path) {
            Ok(text) => match serde_json::from_str::<SavedSession>(&text) {
                Ok(mut session) => {
                    session.version = SESSION_VERSION;
                    Some(session)
                }
                Err(err) => {
                    eprintln!("sonder: ignoring unreadable {}: {err}", self.path.display());
                    None
                }
            },
            Err(err) if err.kind() == ErrorKind::NotFound => None,
            Err(err) => {
                eprintln!("sonder: cannot read {}: {err}", self.path.display());
                None
            }
        }
    }

    /// Writes atomically so an unexpected exit leaves the previous session file intact.
    pub fn save(&self, session: &SavedSession) -> std::io::Result<()> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        let text = serde_json::to_string_pretty(session).map_err(std::io::Error::other)?;
        let tmp = self.path.with_extension("json.tmp");
        fs::write(&tmp, text)?;
        fs::rename(&tmp, &self.path)
    }

    #[cfg(test)]
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }
}

/// Maps `saved_index` from `saved_paths` onto `kept_paths` when missing files were dropped.
pub fn restore_index(
    saved_paths: &[String],
    kept_paths: &[PathBuf],
    saved_index: Option<usize>,
) -> Option<usize> {
    let target = saved_index?;
    if kept_paths.is_empty() || target >= saved_paths.len() {
        return None;
    }
    let mut kept_iter = kept_paths.iter().enumerate().peekable();
    let mut new_indices: HashMap<usize, usize> = HashMap::new();
    for (old_i, saved) in saved_paths.iter().enumerate() {
        if let Some(&(new_i, kept)) = kept_iter.peek()
            && kept.as_os_str() == std::ffi::OsStr::new(saved)
        {
            new_indices.insert(old_i, new_i);
            kept_iter.next();
        }
    }
    if let Some(&exact) = new_indices.get(&target) {
        return Some(exact);
    }
    ((target + 1)..saved_paths.len())
        .find_map(|i| new_indices.get(&i).copied())
        .or_else(|| (0..target).rev().find_map(|i| new_indices.get(&i).copied()))
}

/// Snapshots the active queue, selected index, elapsed time, and repeat mode.
///
/// Must be called without holding `state.queue` or `state.snapshot`.
pub fn capture(state: &AppState) -> SavedSession {
    let paths: Vec<String> = {
        let queue = lock(&state.queue);
        queue.iter().map(|item| item.path.clone()).collect()
    };
    let (index, elapsed_ms, repeat) = {
        let snap = state.snapshot();
        let index = snap.index.map(|i| i as usize).filter(|&i| i < paths.len());
        let elapsed_ms = if index.is_some() { snap.elapsed_ms } else { 0 };
        (index, elapsed_ms, snap.repeat)
    };
    SavedSession {
        version: SESSION_VERSION,
        paths,
        index,
        elapsed_ms,
        repeat,
    }
}

/// Captures the current playback session and saves it to disk.
///
/// Must be called without holding `state.queue` or `state.snapshot`.
pub fn capture_and_save(state: &AppState) {
    let session = capture(state);
    if let Err(err) = state.session_store.save(&session) {
        eprintln!("sonder: cannot save session: {err}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_ignores_missing_or_corrupt_files() {
        let dir = std::env::temp_dir().join(format!("sonder-session-{}", std::process::id()));
        let store = SessionStore::new(dir.join("session.json"));
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(store.load(), None);

        fs::create_dir_all(&dir).unwrap();
        fs::write(store.path(), "{not json").unwrap();
        assert_eq!(store.load(), None);

        let saved = SavedSession {
            version: 1,
            paths: vec!["/a.flac".into(), "/b.flac".into()],
            index: Some(1),
            elapsed_ms: 42_500,
            repeat: RepeatMode::All,
        };
        store.save(&saved).unwrap();
        assert_eq!(store.load(), Some(saved));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn restore_index_remaps_around_missing_files_including_duplicates() {
        let saved: Vec<String> = ["a", "b", "c", "b", "d"]
            .into_iter()
            .map(Into::into)
            .collect();
        let kept: Vec<PathBuf> = ["a", "b", "d"].into_iter().map(PathBuf::from).collect();
        // index 3 ('b') dropped -> advances to next surviving 'd' (new index 2)
        assert_eq!(restore_index(&saved, &kept, Some(3)), Some(2));
        // index 1 ('b') survived at new index 1
        assert_eq!(restore_index(&saved, &kept, Some(1)), Some(1));
        // if tail dropped, falls back backward
        let head_only = vec![PathBuf::from("a")];
        assert_eq!(restore_index(&saved, &head_only, Some(4)), Some(0));
        assert_eq!(restore_index(&saved, &[], Some(0)), None);
        assert_eq!(restore_index(&saved, &kept, None), None);
    }
}
