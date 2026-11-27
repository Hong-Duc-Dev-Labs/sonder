//! Google Drive REST API v3 client: folder listing, recursive audio scanning,
//! and HTTP Range byte downloads with automatic 401 refresh + transient retry.

use std::collections::{HashSet, VecDeque};
use std::sync::Arc;
use std::time::Duration;

use chrono::DateTime;
use serde::Deserialize;
use url::Url;

use crate::auth::AuthManager;
use crate::error::GdriveError;

/// Default Google Drive REST API v3 base URL.
pub const DRIVE_API_BASE: &str = "https://www.googleapis.com/drive/v3";

const FOLDER_MIME: &str = "application/vnd.google-apps.folder";
const MAX_TRANSIENT_RETRIES: usize = 3;

/// Supported lossless and DSD audio file extensions (lowercase, without leading dot).
pub const SUPPORTED_AUDIO_EXTENSIONS: &[&str] = &[
    "flac", "m4a", "mp4", "wav", "aif", "aiff", "aifc", "dsf", "dff",
];

/// Returns `true` when `name` has an audio extension Sonder supports.
pub fn is_supported_audio_name(name: &str) -> bool {
    let Some((_, ext)) = name.rsplit_once('.') else {
        return false;
    };
    SUPPORTED_AUDIO_EXTENSIONS
        .iter()
        .any(|e| ext.eq_ignore_ascii_case(e))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriveFolder {
    pub id: String,
    pub name: String,
    pub parent_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriveFile {
    pub id: String,
    pub name: String,
    /// Folder names from the synced root down to this file's parent folder.
    pub folder_segments: Vec<String>,
    pub size: u64,
    /// Modified timestamp as Unix seconds.
    pub modified_time: i64,
    pub md5: Option<String>,
    pub mime_type: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileListResponse {
    #[serde(default)]
    files: Vec<RawDriveFile>,
    #[serde(default)]
    next_page_token: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawDriveFile {
    id: String,
    name: String,
    #[serde(default)]
    mime_type: String,
    #[serde(default)]
    size: Option<String>,
    #[serde(default)]
    modified_time: Option<String>,
    #[serde(default)]
    md5_checksum: Option<String>,
    #[serde(default)]
    parents: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct DriveErrorEnvelope {
    error: Option<DriveErrorDetail>,
}

#[derive(Debug, Deserialize)]
struct DriveErrorDetail {
    message: Option<String>,
}

/// Client for Google Drive API v3 backed by [`AuthManager`].
pub struct DriveClient {
    auth: Arc<AuthManager>,
    agent: ureq::Agent,
    api_base: String,
}

impl DriveClient {
    pub fn new(auth: Arc<AuthManager>, user_agent: &str) -> Self {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(30)))
            .user_agent(user_agent)
            .http_status_as_error(false)
            .build()
            .into();
        Self {
            auth,
            agent,
            api_base: DRIVE_API_BASE.to_owned(),
        }
    }

    /// Overrides the Drive API base URL (used in tests against a local mock HTTP server).
    pub fn with_api_base(mut self, api_base: impl Into<String>) -> Self {
        self.api_base = api_base.into().trim_end_matches('/').to_owned();
        self
    }

    pub fn auth(&self) -> &Arc<AuthManager> {
        &self.auth
    }

    /// Lists direct child folders under `parent_id` (or `"root"` when `None`).
    pub fn list_folders(&self, parent_id: Option<&str>) -> Result<Vec<DriveFolder>, GdriveError> {
        let parent = parent_id.unwrap_or("root");
        let escaped = escape_query(parent);
        let q = format!("'{escaped}' in parents and mimeType = '{FOLDER_MIME}' and trashed = false");
        let raw = self.list_files_query(&q, "nextPageToken,files(id,name,parents)", Some("name"))?;
        Ok(raw
            .into_iter()
            .map(|f| DriveFolder {
                id: f.id,
                name: f.name,
                parent_id: f.parents.into_iter().next(),
            })
            .collect())
    }

    /// Recursively scans `folder_id` for supported audio files.
    pub fn scan_audio_files(
        &self,
        folder_id: &str,
        folder_name: &str,
    ) -> Result<Vec<DriveFile>, GdriveError> {
        let mut results = Vec::new();
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        visited.insert(folder_id.to_owned());
        queue.push_back((folder_id.to_owned(), vec![folder_name.to_owned()]));

        while let Some((current_id, segments)) = queue.pop_front() {
            let escaped = escape_query(&current_id);
            let q = format!("'{escaped}' in parents and trashed = false");
            let entries = self.list_files_query(
                &q,
                "nextPageToken,files(id,name,mimeType,size,modifiedTime,md5Checksum)",
                Some("name"),
            )?;
            for entry in entries {
                if entry.mime_type == FOLDER_MIME {
                    if visited.insert(entry.id.clone()) {
                        let mut next_seg = segments.clone();
                        next_seg.push(entry.name);
                        queue.push_back((entry.id, next_seg));
                    }
                    continue;
                }
                if !is_supported_audio_name(&entry.name) {
                    continue;
                }
                let size = entry
                    .size
                    .as_deref()
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(0);
                if size == 0 {
                    continue;
                }
                let modified_time = entry
                    .modified_time
                    .as_deref()
                    .and_then(parse_rfc3339_unix)
                    .unwrap_or(0);
                results.push(DriveFile {
                    id: entry.id,
                    name: entry.name,
                    folder_segments: segments.clone(),
                    size,
                    modified_time,
                    md5: entry.md5_checksum,
                    mime_type: entry.mime_type,
                });
            }
        }
        Ok(results)
    }

    /// Fetches metadata for a single file by ID.
    pub fn file_metadata(&self, file_id: &str) -> Result<DriveFile, GdriveError> {
        let url = format!(
            "{}/files/{}?fields=id,name,mimeType,size,modifiedTime,md5Checksum&supportsAllDrives=true",
            self.api_base,
            urlencoding_segment(file_id)
        );
        let bytes = self.get_with_auth(&url, None)?;
        let raw: RawDriveFile = serde_json::from_slice(&bytes)?;
        Ok(DriveFile {
            id: raw.id,
            name: raw.name,
            folder_segments: Vec::new(),
            size: raw
                .size
                .as_deref()
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0),
            modified_time: raw
                .modified_time
                .as_deref()
                .and_then(parse_rfc3339_unix)
                .unwrap_or(0),
            md5: raw.md5_checksum,
            mime_type: raw.mime_type,
        })
    }

    /// Downloads an inclusive byte range `[start, end_inclusive]` of `file_id`.
    ///
    /// Automatically refreshes expired tokens once on HTTP 401 and retries transient 429/5xx or
    /// transport failures.
    pub fn fetch_range(
        &self,
        file_id: &str,
        start: u64,
        end_inclusive: u64,
    ) -> Result<Vec<u8>, GdriveError> {
        if end_inclusive < start {
            return Ok(Vec::new());
        }
        let url = format!(
            "{}/files/{}?alt=media&acknowledgeAbuse=true&supportsAllDrives=true",
            self.api_base,
            urlencoding_segment(file_id)
        );
        let range = format!("bytes={start}-{end_inclusive}");
        self.get_with_auth(&url, Some((&range, start, end_inclusive)))
    }

    fn list_files_query(
        &self,
        q: &str,
        fields: &str,
        order_by: Option<&str>,
    ) -> Result<Vec<RawDriveFile>, GdriveError> {
        let mut all = Vec::new();
        let mut page_token: Option<String> = None;
        loop {
            let mut url = Url::parse(&format!("{}/files", self.api_base))
                .map_err(|e| GdriveError::Protocol(format!("invalid api base: {e}")))?;
            {
                let mut pairs = url.query_pairs_mut();
                pairs.append_pair("q", q);
                pairs.append_pair("fields", fields);
                pairs.append_pair("pageSize", "1000");
                pairs.append_pair("includeItemsFromAllDrives", "true");
                pairs.append_pair("supportsAllDrives", "true");
                if let Some(order) = order_by {
                    pairs.append_pair("orderBy", order);
                }
                if let Some(ref token) = page_token {
                    pairs.append_pair("pageToken", token);
                }
            }
            let bytes = self.get_with_auth(url.as_str(), None)?;
            let parsed: FileListResponse = serde_json::from_slice(&bytes)?;
            all.extend(parsed.files);
            match parsed.next_page_token {
                Some(tok) if !tok.is_empty() => page_token = Some(tok),
                _ => break,
            }
        }
        Ok(all)
    }

    fn get_with_auth(
        &self,
        url: &str,
        range: Option<(&str, u64, u64)>,
    ) -> Result<Vec<u8>, GdriveError> {
        let mut refreshed = false;
        let mut attempt = 0usize;
        let mut token = self.auth.access_token()?;

        loop {
            let mut req = self
                .agent
                .get(url)
                .header("Authorization", &format!("Bearer {token}"));
            if let Some((range_header, _, _)) = range {
                req = req.header("Range", range_header);
            }

            let resp = match req.call() {
                Ok(r) => r,
                Err(e) => {
                    if attempt < MAX_TRANSIENT_RETRIES {
                        attempt += 1;
                        std::thread::sleep(backoff(attempt));
                        continue;
                    }
                    return Err(GdriveError::Http(e.to_string()));
                }
            };

            let status = resp.status().as_u16();
            if status == 401 && !refreshed {
                refreshed = true;
                token = self.auth.force_refresh(&token)?;
                continue;
            }
            if status == 401 {
                return Err(GdriveError::Unauthorized);
            }
            if (status == 429 || (500..=504).contains(&status)) && attempt < MAX_TRANSIENT_RETRIES {
                attempt += 1;
                std::thread::sleep(backoff(attempt));
                continue;
            }
            if status == 416 && range.is_some() {
                return Ok(Vec::new());
            }

            let mut body = resp.into_body();
            let bytes = body
                .read_to_vec()
                .map_err(|e| GdriveError::Http(e.to_string()))?;

            if status == 206 {
                return Ok(bytes);
            }
            if (200..300).contains(&status) {
                // If a server ignored Range and returned 200 OK with the full body, slice it.
                if let Some((_, start, end_inclusive)) = range {
                    let s = usize::try_from(start).unwrap_or(bytes.len());
                    if s >= bytes.len() {
                        return Ok(Vec::new());
                    }
                    let e = usize::try_from(end_inclusive.saturating_add(1))
                        .unwrap_or(bytes.len())
                        .min(bytes.len());
                    return Ok(bytes[s..e].to_vec());
                }
                return Ok(bytes);
            }

            let message = serde_json::from_slice::<DriveErrorEnvelope>(&bytes)
                .ok()
                .and_then(|e| e.error)
                .and_then(|e| e.message)
                .unwrap_or_else(|| format!("HTTP {status}"));
            return Err(GdriveError::Api { status, message });
        }
    }
}

fn backoff(attempt: usize) -> Duration {
    match attempt {
        1 => Duration::from_millis(150),
        2 => Duration::from_millis(400),
        _ => Duration::from_millis(800),
    }
}

fn escape_query(value: &str) -> String {
    value.replace('\\', "\\\\").replace('\'', "\\'")
}

fn urlencoding_segment(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for b in value.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn parse_rfc3339_unix(s: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(s).ok().map(|dt| dt.timestamp())
}
