//! The `sonder-art` URI scheme: serves cover thumbnails from the library's art cache so the
//! webview loads them like any image, without base64 over IPC.
//!
//! URL path: `/<hash>/<size>`, where `size` is one of [`THUMB_SIZES`]. Webviews reach it as
//! `sonder-art://localhost/...` (macOS, Linux) or `http://sonder-art.localhost/...` (Windows).

use std::borrow::Cow;

use sonder_library::{ArtCache, THUMB_SIZES};
use tauri::http::{Request, Response, StatusCode, header};

pub const SCHEME: &str = "sonder-art";

/// Thumbnail file for a request path, or `None` when the path is not a well-formed art URL.
/// Only hex hashes and known sizes are accepted, so a request can never leave the cache.
pub fn thumbnail_for(cache: &ArtCache, path: &str) -> Option<std::path::PathBuf> {
    let mut parts = path.trim_start_matches('/').split('/');
    let (hash, size) = (parts.next()?, parts.next()?);
    if parts.next().is_some()
        || hash.len() < 16
        || hash.len() > 128
        || !hash.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return None;
    }
    let size: u32 = size.parse().ok().filter(|s| THUMB_SIZES.contains(s))?;
    Some(cache.thumbnail_path(hash, size))
}

pub fn respond(cache: &ArtCache, request: &Request<Vec<u8>>) -> Response<Cow<'static, [u8]>> {
    let file = thumbnail_for(cache, request.uri().path());
    let Some(bytes) = file.and_then(|f| std::fs::read(f).ok()) else {
        return Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Cow::Borrowed(&[][..]))
            .expect("static response");
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "image/jpeg")
        // A hash names one image forever.
        .header(header::CACHE_CONTROL, "public, max-age=31536000, immutable")
        .body(Cow::Owned(bytes))
        .expect("valid response")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_hex_hashes_and_known_sizes() {
        let cache = ArtCache::new("cache");
        let hash = "0123456789abcdef0123456789abcdef";
        assert_eq!(
            thumbnail_for(&cache, &format!("/{hash}/300")),
            Some(cache.thumbnail_path(hash, 300))
        );
        assert!(thumbnail_for(&cache, &format!("/{hash}/1000")).is_some());
        assert_eq!(thumbnail_for(&cache, &format!("/{hash}/301")), None);
        assert_eq!(thumbnail_for(&cache, &format!("/{hash}/300/x")), None);
        assert_eq!(thumbnail_for(&cache, "/../../etc/passwd/300"), None);
        assert_eq!(
            thumbnail_for(&cache, "/..%2F..%2Fsecret0000000000/300"),
            None
        );
        assert_eq!(thumbnail_for(&cache, "/"), None);
    }
}
