//! Google Drive lossless audio streaming: OAuth2 PKCE loopback auth, Drive REST API v3 client,
//! disk-backed LRU progressive block cache, and a seekable [`RangeCachedReader`].

pub mod api;
pub mod auth;
pub mod cache;
pub mod error;
pub mod reader;

use std::path::{Path, PathBuf};

pub use api::{DriveClient, DriveFile, DriveFolder, SUPPORTED_AUDIO_EXTENSIONS, is_supported_audio_name};
pub use auth::{
    AuthManager, AuthStatus, ENV_CLIENT_ID, ENV_CLIENT_SECRET, OAuthConfig, OAuthConfigStore,
    PersistedGdriveConfig, TokenSet, TokenStore,
};
pub use cache::{BlockCache, CacheStats, DEFAULT_CHUNK_SIZE, DEFAULT_MAX_CACHE_BYTES};
pub use error::GdriveError;
pub use reader::RangeCachedReader;

/// URI scheme prefix used for Google Drive tracks in Sonder's library and play queue.
pub const GDRIVE_SCHEME: &str = "gdrive://";

/// Builds the canonical virtual path for a Google Drive audio file: `gdrive://<file_id>/<file_name>`.
///
/// Keeping the original filename (including extension) ensures container probing, extension checks,
/// and filename fallbacks work seamlessly across the library and decoder.
pub fn gdrive_virtual_path(file_id: &str, file_name: &str) -> PathBuf {
    let clean_name = file_name.replace(['/', '\\'], "_");
    PathBuf::from(format!("{GDRIVE_SCHEME}{file_id}/{clean_name}"))
}

/// Returns `true` if `path` represents a Google Drive virtual track path.
pub fn is_gdrive_path(path: &Path) -> bool {
    path.to_str().is_some_and(|s| s.starts_with(GDRIVE_SCHEME))
}

/// Parses a `gdrive://<file_id>/<file_name>` path into `(file_id, file_name)`.
pub fn parse_gdrive_path(path: &Path) -> Option<(&str, &str)> {
    let raw = path.to_str()?.strip_prefix(GDRIVE_SCHEME)?;
    let (id, name) = raw.split_once('/')?;
    if id.is_empty() || name.is_empty() {
        return None;
    }
    Some((id, name))
}

#[cfg(test)]
mod path_tests {
    use super::*;

    #[test]
    fn virtual_path_round_trips_and_preserves_extension() {
        let p = gdrive_virtual_path("1abcXYZ_99", "01 - Track Name.flac");
        assert!(is_gdrive_path(&p));
        assert_eq!(parse_gdrive_path(&p), Some(("1abcXYZ_99", "01 - Track Name.flac")));
        assert_eq!(p.extension().and_then(|e| e.to_str()), Some("flac"));
        assert!(!is_gdrive_path(Path::new("C:/Music/song.flac")));
        assert_eq!(parse_gdrive_path(Path::new("gdrive://onlyid")), None);
    }
}
