//! Seekable progressive Range-cached reader (`Read + Seek + Send + Sync`) with optional
//! background read-ahead prefetching.

use std::io::{self, Read, Seek, SeekFrom};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle};

use crate::api::DriveClient;
use crate::cache::BlockCache;
use crate::error::GdriveError;

/// Abstraction over OAuth (`DriveClient`) and public (`PublicDriveClient`) Range downloaders so
/// both modes share the exact same [`RangeCachedReader`] and [`BlockCache`] pipeline.
pub trait RangeFetcher: Send + Sync {
    fn fetch_range(
        &self,
        file_id: &str,
        start: u64,
        end_inclusive: u64,
    ) -> Result<Vec<u8>, GdriveError>;
}

impl RangeFetcher for DriveClient {
    fn fetch_range(
        &self,
        file_id: &str,
        start: u64,
        end_inclusive: u64,
    ) -> Result<Vec<u8>, GdriveError> {
        DriveClient::fetch_range(self, file_id, start, end_inclusive)
    }
}

/// Number of chunks to prefetch ahead during continuous playback.
const READ_AHEAD_CHUNKS: u64 = 2;

struct Prefetcher {
    tx: Mutex<mpsc::Sender<u64>>,
    stop: Arc<AtomicBool>,
    handle: Mutex<Option<JoinHandle<()>>>,
}

impl Prefetcher {
    fn spawn(
        file_id: String,
        file_size: u64,
        client: Arc<dyn RangeFetcher>,
        cache: Arc<BlockCache>,
    ) -> Self {
        let (tx, rx) = mpsc::channel::<u64>();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_bg = stop.clone();
        let chunk_size = cache.chunk_size();
        let total_chunks = file_size.div_ceil(chunk_size);

        let handle = thread::Builder::new()
            .name("sonder-gdrive-prefetch".into())
            .spawn(move || {
                while let Ok(mut target_chunk) = rx.recv() {
                    // Drain any queued hints to always prefetch from the latest read position.
                    while let Ok(newer) = rx.try_recv() {
                        target_chunk = newer;
                    }
                    if stop_bg.load(Ordering::Relaxed) {
                        break;
                    }
                    for offset in 1..=READ_AHEAD_CHUNKS {
                        if stop_bg.load(Ordering::Relaxed) {
                            return;
                        }
                        let next_chunk = target_chunk + offset;
                        if next_chunk >= total_chunks {
                            break;
                        }
                        if cache.contains(&file_id, next_chunk) {
                            continue;
                        }
                        let start = next_chunk * chunk_size;
                        let end = (start + chunk_size - 1).min(file_size - 1);
                        let _ = cache.get_or_fetch(&file_id, next_chunk, || {
                            client.fetch_range(&file_id, start, end)
                        });
                    }
                }
            })
            .ok();

        Self {
            tx: Mutex::new(tx),
            stop,
            handle: Mutex::new(handle),
        }
    }

    fn hint(&self, current_chunk: u64) {
        if let Ok(tx) = self.tx.lock() {
            let _ = tx.send(current_chunk);
        }
    }
}

impl Drop for Prefetcher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // Drop sender by replacing or let handle finish naturally; don't block playback stop
        // if an HTTP call is mid-flight.
        if let Ok(mut h) = self.handle.lock() {
            let _ = h.take();
        }
    }
}

/// A seekable `Read + Seek` stream over a Google Drive file backed by HTTP Range requests
/// and a disk-backed LRU [`BlockCache`].
pub struct RangeCachedReader {
    file_id: String,
    file_size: u64,
    pos: u64,
    client: Arc<dyn RangeFetcher>,
    cache: Arc<BlockCache>,
    /// In-memory copy of the most recently read chunk so small sequential reads avoid disk I/O.
    current_chunk: Option<(u64, Vec<u8>)>,
    prefetcher: Option<Prefetcher>,
}

impl RangeCachedReader {
    /// Creates a new reader for `file_id` of known `file_size`.
    ///
    /// Set `read_ahead = true` for playback (smooth continuous streaming) and `false` for
    /// metadata/tag probing during library scans (avoids downloading extra chunks).
    pub fn new(
        file_id: impl Into<String>,
        file_size: u64,
        client: Arc<dyn RangeFetcher>,
        cache: Arc<BlockCache>,
        read_ahead: bool,
    ) -> Self {
        let file_id = file_id.into();
        let prefetcher = if read_ahead && file_size > 0 {
            Some(Prefetcher::spawn(
                file_id.clone(),
                file_size,
                client.clone(),
                cache.clone(),
            ))
        } else {
            None
        };
        Self {
            file_id,
            file_size,
            pos: 0,
            client,
            cache,
            current_chunk: None,
            prefetcher,
        }
    }

    pub fn byte_len(&self) -> u64 {
        self.file_size
    }

    fn load_chunk(&mut self, chunk_index: u64) -> io::Result<&[u8]> {
        let need_load = self
            .current_chunk
            .as_ref()
            .is_none_or(|(idx, _)| *idx != chunk_index);
        if need_load {
            let chunk_size = self.cache.chunk_size();
            let start = chunk_index.saturating_mul(chunk_size);
            if start >= self.file_size {
                return Ok(&[]);
            }
            let end = start
                .saturating_add(chunk_size)
                .saturating_sub(1)
                .min(self.file_size - 1);
            let data = self
                .cache
                .get_or_fetch(&self.file_id, chunk_index, || {
                    self.client.fetch_range(&self.file_id, start, end)
                })
                .map_err(|e| io::Error::other(e.to_string()))?;
            self.current_chunk = Some((chunk_index, data));
            if let Some(ref prefetcher) = self.prefetcher {
                prefetcher.hint(chunk_index);
            }
        }
        Ok(&self.current_chunk.as_ref().unwrap().1)
    }
}

impl Read for RangeCachedReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() || self.pos >= self.file_size {
            return Ok(0);
        }
        let chunk_size = self.cache.chunk_size();
        let chunk_index = self.pos / chunk_size;
        let offset_in_chunk = (self.pos % chunk_size) as usize;

        let chunk = self.load_chunk(chunk_index)?;
        if offset_in_chunk >= chunk.len() {
            return Ok(0);
        }
        let available = &chunk[offset_in_chunk..];
        let to_copy = available.len().min(buf.len());
        buf[..to_copy].copy_from_slice(&available[..to_copy]);
        self.pos += to_copy as u64;
        Ok(to_copy)
    }
}

impl Seek for RangeCachedReader {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let next: i128 = match pos {
            SeekFrom::Start(offset) => i128::from(offset),
            SeekFrom::End(delta) => i128::from(self.file_size) + i128::from(delta),
            SeekFrom::Current(delta) => i128::from(self.pos) + i128::from(delta),
        };
        if next < 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "cannot seek before start of Google Drive stream",
            ));
        }
        self.pos = u64::try_from(next).unwrap_or(u64::MAX);
        Ok(self.pos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::{AuthManager, OAuthConfig, TokenSet, TokenStore};
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::sync::atomic::AtomicUsize;

    struct MockServer {
        base_url: String,
        range_requests: Arc<AtomicUsize>,
        refresh_requests: Arc<AtomicUsize>,
        stop: Arc<AtomicBool>,
        handle: Option<JoinHandle<()>>,
    }

    impl Drop for MockServer {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            if let Some(h) = self.handle.take() {
                let _ = h.join();
            }
        }
    }

    /// Spawns a local HTTP mock server supporting Drive Range downloads and OAuth token refresh.
    fn spawn_mock_server(payload: Vec<u8>, valid_token: &'static str, new_token: &'static str) -> MockServer {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();

        let range_requests = Arc::new(AtomicUsize::new(0));
        let refresh_requests = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));

        let rr = range_requests.clone();
        let ref_req = refresh_requests.clone();
        let st = stop.clone();

        let handle = thread::spawn(move || {
            while !st.load(Ordering::Relaxed) {
                let Ok((mut stream, _)) = listener.accept() else {
                    thread::sleep(std::time::Duration::from_millis(5));
                    continue;
                };
                let _ = stream.set_nonblocking(false);
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut request_line = String::new();
                if reader.read_line(&mut request_line).is_err() {
                    continue;
                }
                let mut headers = Vec::new();
                let mut content_len = 0usize;
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).is_err() || line.trim().is_empty() {
                        break;
                    }
                    if let Some((k, v)) = line.split_once(':') {
                        let kl = k.trim().to_ascii_lowercase();
                        let val = v.trim().to_owned();
                        if kl == "content-length" {
                            content_len = val.parse().unwrap_or(0);
                        }
                        headers.push((kl, val));
                    }
                }
                if content_len > 0 {
                    let mut body_buf = vec![0u8; content_len];
                    let _ = reader.read_exact(&mut body_buf);
                }

                let mut parts = request_line.split_whitespace();
                let method = parts.next().unwrap_or("");
                let path = parts.next().unwrap_or("");

                if method == "POST" && path == "/token" {
                    ref_req.fetch_add(1, Ordering::SeqCst);
                    let body = format!(
                        r#"{{"access_token":"{new_token}","expires_in":3600,"token_type":"Bearer"}}"#
                    );
                    let _ = write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    continue;
                }

                let auth = headers
                    .iter()
                    .find(|(k, _)| k == "authorization")
                    .map(|(_, v)| v.as_str())
                    .unwrap_or("");
                if auth != format!("Bearer {valid_token}") && auth != format!("Bearer {new_token}") {
                    let body = r#"{"error":{"message":"Invalid Credentials"}}"#;
                    let _ = write!(
                        stream,
                        "HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    continue;
                }

                rr.fetch_add(1, Ordering::SeqCst);
                let range_hdr = headers
                    .iter()
                    .find(|(k, _)| k == "range")
                    .map(|(_, v)| v.as_str());
                if let Some(r) = range_hdr.and_then(|h| h.strip_prefix("bytes="))
                    && let Some((s, e)) = r.split_once('-')
                {
                    let start: usize = s.parse().unwrap_or(0);
                    let end: usize = e.parse().unwrap_or(payload.len().saturating_sub(1));
                    if start >= payload.len() {
                        let _ = write!(
                            stream,
                            "HTTP/1.1 416 Range Not Satisfiable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                        );
                        continue;
                    }
                    let end = end.min(payload.len() - 1);
                    let slice = &payload[start..=end];
                    let _ = write!(
                        stream,
                        "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {start}-{end}/{}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        payload.len(),
                        slice.len()
                    );
                    let _ = stream.write_all(slice);
                } else {
                    let _ = write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        payload.len()
                    );
                    let _ = stream.write_all(&payload);
                }
            }
        });

        MockServer {
            base_url: format!("http://{addr}"),
            range_requests,
            refresh_requests,
            stop,
            handle: Some(handle),
        }
    }

    #[test]
    fn chunked_reads_seeks_cache_hit_and_lru_eviction() {
        // 24 KiB deterministic payload, 4 KiB chunks -> 6 chunks.
        let payload: Vec<u8> = (0..(24 * 1024)).map(|i| (i % 251) as u8).collect();
        let server = spawn_mock_server(payload.clone(), "good-token", "refreshed-token");
        let dir = tempfile::tempdir().unwrap();

        let auth = Arc::new(
            AuthManager::new(
                TokenStore::new(dir.path().join("tokens.json")),
                Some(OAuthConfig {
                    client_id: "cid".into(),
                    client_secret: None,
                }),
                "sonder-test",
            )
            .with_endpoints(
                format!("{}/auth", server.base_url),
                format!("{}/token", server.base_url),
                format!("{}/revoke", server.base_url),
                format!("{}/about", server.base_url),
            ),
        );
        auth.set_tokens(TokenSet {
            access_token: "good-token".into(),
            refresh_token: Some("ref-tok".into()),
            expires_at_unix: crate::auth::unix_now() + 3600,
            account_email: None,
        })
        .unwrap();

        let client = Arc::new(
            DriveClient::new(auth, "sonder-test").with_api_base(server.base_url.clone()),
        );
        // Cap cache at 12 KiB (3 chunks of 4 KiB) to test LRU eviction.
        let cache = Arc::new(BlockCache::open(dir.path().join("cache"), 12 * 1024, 4096));

        let mut reader =
            RangeCachedReader::new("file-1", payload.len() as u64, client.clone(), cache.clone(), false);

        // Read first 5000 bytes (chunks 0 and 1).
        let mut buf = vec![0u8; 5000];
        reader.read_exact(&mut buf).unwrap();
        assert_eq!(buf, &payload[..5000]);
        assert_eq!(server.range_requests.load(Ordering::SeqCst), 2);

        // Seek back to 0 and re-read: should be 100% cache hit (still 2 HTTP requests).
        reader.seek(SeekFrom::Start(0)).unwrap();
        let mut buf2 = vec![0u8; 5000];
        reader.read_exact(&mut buf2).unwrap();
        assert_eq!(buf2, &payload[..5000]);
        assert_eq!(server.range_requests.load(Ordering::SeqCst), 2);

        // Seek near end and read chunks 2, 3, 4, 5 -> triggers LRU eviction to stay <= 12 KiB.
        reader.seek(SeekFrom::Start(8192)).unwrap();
        let mut tail = Vec::new();
        reader.read_to_end(&mut tail).unwrap();
        assert_eq!(tail, &payload[8192..]);
        assert!(cache.stats().total_bytes <= 12 * 1024);
        assert_eq!(cache.stats().chunk_count, 3);
    }

    #[test]
    fn http_401_triggers_single_token_refresh_and_retries_range() {
        let payload: Vec<u8> = (0..8192).map(|i| (i % 199) as u8).collect();
        // Server rejects "stale-token" with 401 and accepts "fresh-token" returned by /token.
        let server = spawn_mock_server(payload.clone(), "fresh-token", "fresh-token");
        let dir = tempfile::tempdir().unwrap();

        let auth = Arc::new(
            AuthManager::new(
                TokenStore::new(dir.path().join("tokens.json")),
                Some(OAuthConfig {
                    client_id: "cid".into(),
                    client_secret: None,
                }),
                "sonder-test",
            )
            .with_endpoints(
                format!("{}/auth", server.base_url),
                format!("{}/token", server.base_url),
                format!("{}/revoke", server.base_url),
                format!("{}/about", server.base_url),
            ),
        );
        auth.set_tokens(TokenSet {
            access_token: "stale-token".into(),
            refresh_token: Some("ref-tok".into()),
            expires_at_unix: crate::auth::unix_now() + 3600,
            account_email: None,
        })
        .unwrap();

        let client = Arc::new(
            DriveClient::new(auth, "sonder-test").with_api_base(server.base_url.clone()),
        );
        let cache = Arc::new(BlockCache::open(dir.path().join("cache"), 64 * 1024, 4096));

        let mut reader =
            RangeCachedReader::new("file-401", payload.len() as u64, client, cache, false);
        let mut got = Vec::new();
        reader.read_to_end(&mut got).unwrap();
        assert_eq!(got, payload);
        assert_eq!(server.refresh_requests.load(Ordering::SeqCst), 1);
    }
}
