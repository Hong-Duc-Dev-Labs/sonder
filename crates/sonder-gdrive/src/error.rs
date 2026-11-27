//! Error types for Google Drive auth, API calls, and progressive range caching.
//!
//! Error messages never include OAuth tokens or client secrets.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum GdriveError {
    #[error("Google Drive OAuth Client ID is not configured")]
    NotConfigured,
    #[error("not signed in to Google Drive")]
    NotAuthenticated,
    #[error("Google Drive sign-in was cancelled")]
    AuthCancelled,
    #[error("Google Drive authentication failed: {0}")]
    AuthFailed(String),
    #[error("Google Drive authorization expired; please sign in again")]
    Unauthorized,
    #[error("Google Drive API error ({status}): {message}")]
    Api { status: u16, message: String },
    #[error("network request failed: {0}")]
    Http(String),
    #[error("invalid response from Google Drive: {0}")]
    Protocol(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}
