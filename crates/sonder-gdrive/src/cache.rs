//! Disk-backed progressive block cache with configurable LRU size cap and in-flight
//! request deduplication.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::UNIX_EPOCH;

use crate::error::GdriveError;

/// Default chunk size: 512 KiB (good balance between HTTP round-trips and instant seek latency).
pub const DEFAULT_CHUNK_SIZE: u64 = 512 * 1024;
/// Default on-disk cache cap: 2 GiB.
pub const DEFAULT_MAX_CACHE_BYTES: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheStats {
    pub total_bytes: u64,
    pub max_bytes: u64,
    pub chunk_count: u64,
    pub file_count: u64,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct ChunkKey {
    file_id: String,
    chunk_index: u64,
}

#[derive(Debug, Clone)]
struct ChunkEntry {
    path: PathBuf,
    size: u64,
    last_access: u64,
}

struct CacheIndex {
    entries: HashMap<ChunkKey, ChunkEntry>,
    total_bytes: u64,
    max_bytes: u64,
    clock: u64,
}

impl CacheIndex {
    fn touch(&mut self, key: &ChunkKey) -> Option<PathBuf> {
        self.clock = self.clock.saturating_add(1);
        let tick = self.clock;
        let entry = self.entries.get_mut(key)?;
        entry.last_access = tick;
        Some(entry.path.clone())
    }

    fn insert(&mut self, key: ChunkKey, path: PathBuf, size: u64) -> Vec<PathBuf> {
        self.clock = self.clock.saturating_add(1);
        let tick = self.clock;
        if let Some(prev) = self.entries.insert(
            key,
            ChunkEntry {
                path,
                size,
                last_access: tick,
            },
        ) {
            self.total_bytes = self.total_bytes.saturating_sub(prev.size);
        }
        self.total_bytes = self.total_bytes.saturating_add(size);
        self.evict_over_limit()
    }

    fn evict_over_limit(&mut self) -> Vec<PathBuf> {
        if self.total_bytes <= self.max_bytes {
            return Vec::new();
        }
        let mut order: Vec<(u64, ChunkKey)> = self
            .entries
            .iter()
            .map(|(k, v)| (v.last_access, k.clone()))
            .collect();
        order.sort_by_key(|(ts, _)| *ts);

        let mut removed = Vec::new();
        for (_, key) in order {
            if self.total_bytes <= self.max_bytes {
                break;
            }
            if let Some(entry) = self.entries.remove(&key) {
                self.total_bytes = self.total_bytes.saturating_sub(entry.size);
                removed.push(entry.path);
            }
        }
        removed
    }

    fn stats(&self) -> CacheStats {
        let files: HashSet<&str> = self.entries.keys().map(|k| k.file_id.as_str()).collect();
        CacheStats {
            total_bytes: self.total_bytes,
            max_bytes: self.max_bytes,
            chunk_count: self.entries.len() as u64,
            file_count: files.len() as u64,
        }
    }
}

type InFlightSlot = Arc<(Mutex<Option<Result<Vec<u8>, String>>>, Condvar)>;

/// Disk-backed LRU cache for fixed-size Google Drive chunks.
pub struct BlockCache {
    root: PathBuf,
    chunk_size: u64,
    index: Mutex<CacheIndex>,
    inflight: Mutex<HashMap<ChunkKey, InFlightSlot>>,
}

impl BlockCache {
    pub fn open(root: impl Into<PathBuf>, max_bytes: u64, chunk_size: u64) -> Self {
        let root = root.into();
        let chunk_size = chunk_size.max(4096);
        let mut index = CacheIndex {
            entries: HashMap::new(),
            total_bytes: 0,
            max_bytes,
            clock: 0,
        };
        scan_existing(&root, &mut index);
        let to_remove = index.evict_over_limit();
        for path in to_remove {
            let _ = fs::remove_file(path);
        }
        Self {
            root,
            chunk_size,
            index: Mutex::new(index),
            inflight: Mutex::new(HashMap::new()),
        }
    }

    pub fn chunk_size(&self) -> u64 {
        self.chunk_size
    }

    pub fn stats(&self) -> CacheStats {
        self.index
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .stats()
    }

    pub fn set_max_bytes(&self, max_bytes: u64) {
        let to_remove = {
            let mut idx = self.index.lock().unwrap_or_else(|e| e.into_inner());
            idx.max_bytes = max_bytes;
            idx.evict_over_limit()
        };
        for path in to_remove {
            let _ = fs::remove_file(path);
        }
    }

    pub fn clear(&self) -> Result<(), GdriveError> {
        let mut idx = self.index.lock().unwrap_or_else(|e| e.into_inner());
        idx.entries.clear();
        idx.total_bytes = 0;
        let files_dir = self.root.join("files");
        if files_dir.exists() {
            fs::remove_dir_all(&files_dir)?;
        }
        Ok(())
    }

    /// Returns `true` if the chunk is already cached on disk.
    pub fn contains(&self, file_id: &str, chunk_index: u64) -> bool {
        let key = ChunkKey {
            file_id: file_id.to_owned(),
            chunk_index,
        };
        self.index
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entries
            .contains_key(&key)
    }

    /// Reads a cached chunk or fetches it once via `fetch` (deduplicating concurrent callers).
    pub fn get_or_fetch(
        &self,
        file_id: &str,
        chunk_index: u64,
        fetch: impl FnOnce() -> Result<Vec<u8>, GdriveError>,
    ) -> Result<Vec<u8>, GdriveError> {
        let key = ChunkKey {
            file_id: file_id.to_owned(),
            chunk_index,
        };

        // Fast path: already on disk.
        if let Some(bytes) = self.read_cached(&key) {
            return Ok(bytes);
        }

        // Deduplicate concurrent fetches for the same chunk (e.g. prefetcher + reader).
        let (slot, is_leader) = {
            let mut map = self.inflight.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(existing) = map.get(&key) {
                (existing.clone(), false)
            } else {
                let slot: InFlightSlot = Arc::new((Mutex::new(None), Condvar::new()));
                map.insert(key.clone(), slot.clone());
                (slot, true)
            }
        };

        if !is_leader {
            let (lock, cvar) = &*slot;
            let mut guard = lock.lock().unwrap_or_else(|e| e.into_inner());
            while guard.is_none() {
                guard = cvar.wait(guard).unwrap_or_else(|e| e.into_inner());
            }
            return match guard.as_ref().unwrap() {
                Ok(bytes) => Ok(bytes.clone()),
                Err(msg) => Err(GdriveError::Http(msg.clone())),
            };
        }

        // Leader executes fetch, persists to disk, and wakes any waiters.
        let result = fetch();
        let outcome_for_waiters = match &result {
            Ok(bytes) => {
                let _ = self.store_chunk(&key, bytes);
                Ok(bytes.clone())
            }
            Err(e) => Err(e.to_string()),
        };

        {
            let (lock, cvar) = &*slot;
            let mut guard = lock.lock().unwrap_or_else(|e| e.into_inner());
            *guard = Some(outcome_for_waiters);
            cvar.notify_all();
        }
        {
            let mut map = self.inflight.lock().unwrap_or_else(|e| e.into_inner());
            map.remove(&key);
        }

        result
    }

    fn read_cached(&self, key: &ChunkKey) -> Option<Vec<u8>> {
        let path = {
            let mut idx = self.index.lock().unwrap_or_else(|e| e.into_inner());
            idx.touch(key)?
        };
        match fs::read(&path) {
            Ok(bytes) => Some(bytes),
            Err(_) => {
                // File vanished externally; remove stale index entry.
                let mut idx = self.index.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(removed) = idx.entries.remove(key) {
                    idx.total_bytes = idx.total_bytes.saturating_sub(removed.size);
                }
                None
            }
        }
    }

    fn store_chunk(&self, key: &ChunkKey, bytes: &[u8]) -> std::io::Result<()> {
        let dir = self.root.join("files").join(sanitize_id(&key.file_id));
        fs::create_dir_all(&dir)?;
        let path = dir.join(format!("c_{}.bin", key.chunk_index));
        let tmp = dir.join(format!("c_{}.{}.tmp", key.chunk_index, std::process::id()));
        fs::write(&tmp, bytes)?;
        fs::rename(&tmp, &path)?;

        let to_remove = {
            let mut idx = self.index.lock().unwrap_or_else(|e| e.into_inner());
            idx.insert(key.clone(), path, bytes.len() as u64)
        };
        for victim in to_remove {
            let _ = fs::remove_file(&victim);
            if let Some(parent) = victim.parent() {
                let _ = fs::remove_dir(parent);
            }
        }
        Ok(())
    }
}

fn sanitize_id(file_id: &str) -> String {
    file_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn scan_existing(root: &Path, index: &mut CacheIndex) {
    let files_dir = root.join("files");
    let Ok(file_dirs) = fs::read_dir(&files_dir) else {
        return;
    };
    let mut found: Vec<(u64, ChunkKey, PathBuf, u64)> = Vec::new();
    for entry in file_dirs.flatten() {
        let Ok(ft) = entry.file_type() else {
            continue;
        };
        if !ft.is_dir() {
            continue;
        }
        let file_id = entry.file_name().to_string_lossy().into_owned();
        let Ok(chunks) = fs::read_dir(entry.path()) else {
            continue;
        };
        for chunk in chunks.flatten() {
            let name = chunk.file_name().to_string_lossy().into_owned();
            let Some(idx_str) = name
                .strip_prefix("c_")
                .and_then(|s| s.strip_suffix(".bin"))
            else {
                continue;
            };
            let Ok(chunk_index) = idx_str.parse::<u64>() else {
                continue;
            };
            let Ok(meta) = chunk.metadata() else {
                continue;
            };
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_secs());
            found.push((
                mtime,
                ChunkKey {
                    file_id: file_id.clone(),
                    chunk_index,
                },
                chunk.path(),
                meta.len(),
            ));
        }
    }
    found.sort_by_key(|(mtime, _, _, _)| *mtime);
    for (_, key, path, size) in found {
        index.clock = index.clock.saturating_add(1);
        index.total_bytes = index.total_bytes.saturating_add(size);
        index.entries.insert(
            key,
            ChunkEntry {
                path,
                size,
                last_access: index.clock,
            },
        );
    }
}
