//! Dual-mode public Google Drive access (no OAuth login required):
//!
//! 1. **API Key mode** (when `SONDER_GDRIVE_API_KEY` or a saved API key is configured): uses
//!    official Google Drive API v3 (`files.list` + `files.get?alt=media&key=...`).
//! 2. **Zero-Key Public fallback** (works out of the box with zero configuration): parses
//!    `embeddedfolderview` HTML for shared folders and streams files via
//!    `drive.usercontent.google.com/download?id=...&export=download&confirm=t` with HTTP `Range`,
//!    handling Google's >100 MB virus-scan confirmation interstitial cleanly.

use std::collections::{HashSet, VecDeque};
use std::sync::Mutex;
use std::time::Duration;

use chrono::DateTime;
use serde::Deserialize;
use url::Url;

use crate::api::{DRIVE_API_BASE, DriveFile, is_supported_audio_name};
use crate::error::GdriveError;
use crate::link::{DriveLinkKind, ParsedDriveLink, is_valid_drive_id};
use crate::reader::RangeFetcher;

/// Environment variable supplying an optional Google Drive API Key for public links.
pub const ENV_API_KEY: &str = "SONDER_GDRIVE_API_KEY";

const DEFAULT_WEB_BASE: &str = "https://drive.google.com";
const DEFAULT_DOWNLOAD_BASE: &str = "https://drive.usercontent.google.com";
const FOLDER_MIME: &str = "application/vnd.google-apps.folder";
const MAX_TRANSIENT_RETRIES: usize = 3;
const MAX_FOLDER_DEPTH: usize = 16;

/// Result of resolving a pasted public Google Drive URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedPublicLink {
    Folder { id: String, name: String },
    File(DriveFile),
}

/// Entry extracted from Google Drive's `embeddedfolderview` HTML page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicHtmlEntry {
    pub id: String,
    pub name: String,
    pub is_folder: bool,
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
}

#[derive(Debug, Deserialize)]
struct DriveErrorEnvelope {
    error: Option<DriveErrorDetail>,
}

#[derive(Debug, Deserialize)]
struct DriveErrorDetail {
    message: Option<String>,
    #[serde(default)]
    errors: Vec<DriveErrorReason>,
}

#[derive(Debug, Deserialize)]
struct DriveErrorReason {
    #[serde(default)]
    reason: String,
}

/// Resolves the effective Drive API key (saved config first, else `SONDER_GDRIVE_API_KEY`).
pub fn resolve_api_key(saved: Option<&str>) -> Option<String> {
    if let Some(k) = saved.map(str::trim).filter(|s| !s.is_empty()) {
        return Some(k.to_owned());
    }
    std::env::var(ENV_API_KEY)
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
}

pub fn env_api_key_present() -> bool {
    std::env::var(ENV_API_KEY)
        .ok()
        .is_some_and(|s| !s.trim().is_empty())
}

/// Public Google Drive client supporting both API Key mode and Zero-Key fallback.
pub struct PublicDriveClient {
    agent: ureq::Agent,
    api_key: Mutex<Option<String>>,
    api_base: String,
    web_base: String,
    download_base: String,
}

impl PublicDriveClient {
    pub fn new(api_key: Option<String>, user_agent: &str) -> Self {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(30)))
            .user_agent(user_agent)
            .http_status_as_error(false)
            .build()
            .into();
        Self {
            agent,
            api_key: Mutex::new(api_key),
            api_base: DRIVE_API_BASE.to_owned(),
            web_base: DEFAULT_WEB_BASE.to_owned(),
            download_base: DEFAULT_DOWNLOAD_BASE.to_owned(),
        }
    }

    /// Overrides base URLs for local mock HTTP server tests.
    pub fn with_bases(
        mut self,
        api_base: impl Into<String>,
        web_base: impl Into<String>,
        download_base: impl Into<String>,
    ) -> Self {
        self.api_base = api_base.into().trim_end_matches('/').to_owned();
        self.web_base = web_base.into().trim_end_matches('/').to_owned();
        self.download_base = download_base.into().trim_end_matches('/').to_owned();
        self
    }

    pub fn set_api_key(&self, api_key: Option<String>) {
        let mut guard = self.api_key.lock().unwrap_or_else(|e| e.into_inner());
        *guard = api_key;
    }

    fn current_api_key(&self) -> Option<String> {
        self.api_key
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Resolves a pasted Google Drive link (folder or single file), verifying public access and
    /// extracting its display name.
    pub fn resolve_link(&self, parsed: &ParsedDriveLink) -> Result<ResolvedPublicLink, GdriveError> {
        if let Some(key) = self.current_api_key() {
            return self.resolve_link_api_key(parsed, &key);
        }
        self.resolve_link_zero_key(parsed)
    }

    /// Recursively scans a public folder for supported audio files.
    pub fn scan_folder(
        &self,
        folder_id: &str,
        folder_name: &str,
    ) -> Result<Vec<DriveFile>, GdriveError> {
        if let Some(key) = self.current_api_key() {
            return self.scan_folder_api_key(folder_id, folder_name, &key);
        }
        self.scan_folder_zero_key(folder_id, folder_name)
    }

    /// Resolves metadata for a single public file (name + byte size).
    pub fn file_metadata(&self, file_id: &str) -> Result<DriveFile, GdriveError> {
        if let Some(key) = self.current_api_key() {
            return self.file_metadata_api_key(file_id, &key);
        }
        self.probe_public_file(file_id, None)
    }

    /// Downloads an inclusive byte range `[start, end_inclusive]` of a public Drive file.
    pub fn fetch_range(
        &self,
        file_id: &str,
        start: u64,
        end_inclusive: u64,
    ) -> Result<Vec<u8>, GdriveError> {
        if end_inclusive < start {
            return Ok(Vec::new());
        }
        if let Some(key) = self.current_api_key() {
            return self.fetch_range_api_key(file_id, start, end_inclusive, &key);
        }
        self.fetch_range_zero_key(file_id, start, end_inclusive)
    }

    // ---------- API Key Mode ----------

    fn resolve_link_api_key(
        &self,
        parsed: &ParsedDriveLink,
        api_key: &str,
    ) -> Result<ResolvedPublicLink, GdriveError> {
        let raw = self.raw_metadata_api_key(&parsed.id, api_key)?;
        if raw.mime_type == FOLDER_MIME {
            return Ok(ResolvedPublicLink::Folder {
                id: raw.id,
                name: raw.name,
            });
        }
        if parsed.kind == DriveLinkKind::Folder {
            return Err(GdriveError::Protocol(
                "That Google Drive link is not a folder".into(),
            ));
        }
        Ok(ResolvedPublicLink::File(raw_to_drive_file(raw, Vec::new())))
    }

    fn file_metadata_api_key(&self, file_id: &str, api_key: &str) -> Result<DriveFile, GdriveError> {
        let raw = self.raw_metadata_api_key(file_id, api_key)?;
        Ok(raw_to_drive_file(raw, Vec::new()))
    }

    fn raw_metadata_api_key(
        &self,
        file_id: &str,
        api_key: &str,
    ) -> Result<RawDriveFile, GdriveError> {
        let mut url = Url::parse(&format!(
            "{}/files/{}",
            self.api_base,
            urlencoding_segment(file_id)
        ))
        .map_err(|e| GdriveError::Protocol(format!("invalid api base: {e}")))?;
        url.query_pairs_mut()
            .append_pair("fields", "id,name,mimeType,size,modifiedTime,md5Checksum")
            .append_pair("supportsAllDrives", "true")
            .append_pair("key", api_key);
        let bytes = self.get_public(url.as_str(), None)?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    fn scan_folder_api_key(
        &self,
        folder_id: &str,
        folder_name: &str,
        api_key: &str,
    ) -> Result<Vec<DriveFile>, GdriveError> {
        let mut results = Vec::new();
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        visited.insert(folder_id.to_owned());
        queue.push_back((folder_id.to_owned(), vec![folder_name.to_owned()], 0usize));

        while let Some((current_id, segments, depth)) = queue.pop_front() {
            let escaped = current_id.replace('\\', "\\\\").replace('\'', "\\'");
            let q = format!("'{escaped}' in parents and trashed = false");
            let mut page_token: Option<String> = None;
            loop {
                let mut url = Url::parse(&format!("{}/files", self.api_base))
                    .map_err(|e| GdriveError::Protocol(format!("invalid api base: {e}")))?;
                {
                    let mut pairs = url.query_pairs_mut();
                    pairs.append_pair("q", &q);
                    pairs.append_pair(
                        "fields",
                        "nextPageToken,files(id,name,mimeType,size,modifiedTime,md5Checksum)",
                    );
                    pairs.append_pair("pageSize", "1000");
                    pairs.append_pair("orderBy", "name");
                    pairs.append_pair("includeItemsFromAllDrives", "true");
                    pairs.append_pair("supportsAllDrives", "true");
                    pairs.append_pair("key", api_key);
                    if let Some(ref tok) = page_token {
                        pairs.append_pair("pageToken", tok);
                    }
                }
                let bytes = self.get_public(url.as_str(), None)?;
                let parsed: FileListResponse = serde_json::from_slice(&bytes)?;
                for entry in parsed.files {
                    if entry.mime_type == FOLDER_MIME {
                        if depth < MAX_FOLDER_DEPTH && visited.insert(entry.id.clone()) {
                            let mut next_seg = segments.clone();
                            next_seg.push(entry.name);
                            queue.push_back((entry.id, next_seg, depth + 1));
                        }
                        continue;
                    }
                    if !is_supported_audio_name(&entry.name) {
                        continue;
                    }
                    let df = raw_to_drive_file(entry, segments.clone());
                    if df.size > 0 {
                        results.push(df);
                    }
                }
                match parsed.next_page_token {
                    Some(tok) if !tok.is_empty() => page_token = Some(tok),
                    _ => break,
                }
            }
        }
        Ok(results)
    }

    fn fetch_range_api_key(
        &self,
        file_id: &str,
        start: u64,
        end_inclusive: u64,
        api_key: &str,
    ) -> Result<Vec<u8>, GdriveError> {
        let mut url = Url::parse(&format!(
            "{}/files/{}",
            self.api_base,
            urlencoding_segment(file_id)
        ))
        .map_err(|e| GdriveError::Protocol(format!("invalid api base: {e}")))?;
        url.query_pairs_mut()
            .append_pair("alt", "media")
            .append_pair("acknowledgeAbuse", "true")
            .append_pair("supportsAllDrives", "true")
            .append_pair("key", api_key);
        let range = format!("bytes={start}-{end_inclusive}");
        self.get_public(url.as_str(), Some((&range, start, end_inclusive)))
    }

    fn get_public(
        &self,
        url: &str,
        range: Option<(&str, u64, u64)>,
    ) -> Result<Vec<u8>, GdriveError> {
        let mut attempt = 0usize;
        loop {
            let mut req = self.agent.get(url);
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
            if (status == 429 || (500..=504).contains(&status)) && attempt < MAX_TRANSIENT_RETRIES {
                attempt += 1;
                std::thread::sleep(backoff(attempt));
                continue;
            }
            if status == 416 && range.is_some() {
                return Ok(Vec::new());
            }
            let bytes = match resp.into_body().read_to_vec() {
                Ok(b) => b,
                Err(e) => {
                    if attempt < MAX_TRANSIENT_RETRIES {
                        attempt += 1;
                        std::thread::sleep(backoff(attempt));
                        continue;
                    }
                    return Err(GdriveError::Http(e.to_string()));
                }
            };
            if status == 206 {
                return Ok(bytes);
            }
            if (200..300).contains(&status) {
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
            let envelope = serde_json::from_slice::<DriveErrorEnvelope>(&bytes).ok();
            let is_rate_limit_403 = status == 403
                && envelope
                    .as_ref()
                    .and_then(|e| e.error.as_ref())
                    .is_some_and(|d| {
                        d.errors.iter().any(|r| {
                            matches!(
                                r.reason.as_str(),
                                "userRateLimitExceeded" | "rateLimitExceeded" | "quotaExceeded"
                            )
                        })
                    });
            if is_rate_limit_403 && attempt < MAX_TRANSIENT_RETRIES {
                attempt += 1;
                std::thread::sleep(backoff(attempt));
                continue;
            }
            if status == 401 || status == 403 || status == 404 {
                return Err(private_or_missing_error(status));
            }
            let message = envelope
                .and_then(|e| e.error)
                .and_then(|e| e.message)
                .unwrap_or_else(|| format!("HTTP {status}"));
            return Err(GdriveError::Api { status, message });
        }
    }

    // ---------- Zero-Key Public Fallback ----------

    fn resolve_link_zero_key(
        &self,
        parsed: &ParsedDriveLink,
    ) -> Result<ResolvedPublicLink, GdriveError> {
        match parsed.kind {
            DriveLinkKind::Folder => {
                let (title, _) = self.fetch_embedded_folder(&parsed.id)?;
                Ok(ResolvedPublicLink::Folder {
                    id: parsed.id.clone(),
                    name: title.unwrap_or_else(|| format!("Google Drive ({})", &parsed.id[..8.min(parsed.id.len())])),
                })
            }
            DriveLinkKind::File => {
                let file = self.probe_public_file(&parsed.id, None)?;
                Ok(ResolvedPublicLink::File(file))
            }
            DriveLinkKind::Unknown => {
                if let Ok((title, entries)) = self.fetch_embedded_folder(&parsed.id)
                    && (title.is_some() || !entries.is_empty())
                {
                    return Ok(ResolvedPublicLink::Folder {
                        id: parsed.id.clone(),
                        name: title.unwrap_or_else(|| {
                            format!("Google Drive ({})", &parsed.id[..8.min(parsed.id.len())])
                        }),
                    });
                }
                let file = self.probe_public_file(&parsed.id, None)?;
                Ok(ResolvedPublicLink::File(file))
            }
        }
    }

    fn fetch_embedded_folder(
        &self,
        folder_id: &str,
    ) -> Result<(Option<String>, Vec<PublicHtmlEntry>), GdriveError> {
        let url = format!(
            "{}/embeddedfolderview?id={}#list",
            self.web_base,
            urlencoding_segment(folder_id)
        );
        let mut attempt = 0usize;
        let (status, html) = loop {
            let resp = match self.agent.get(&url).call() {
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
            if (status == 429 || (500..=504).contains(&status)) && attempt < MAX_TRANSIENT_RETRIES {
                attempt += 1;
                std::thread::sleep(backoff(attempt));
                continue;
            }
            let body = resp
                .into_body()
                .read_to_string()
                .map_err(|e| GdriveError::Http(e.to_string()))?;
            break (status, body);
        };

        if status == 401 || status == 403 || status == 404 {
            return Err(private_or_missing_error(status));
        }
        if !(200..300).contains(&status) {
            return Err(GdriveError::Api {
                status,
                message: format!("HTTP {status} while listing shared folder"),
            });
        }
        if looks_like_signin_page(&html) {
            return Err(private_or_missing_error(403));
        }
        Ok(parse_embedded_folder_html(&html))
    }

    fn scan_folder_zero_key(
        &self,
        folder_id: &str,
        folder_name: &str,
    ) -> Result<Vec<DriveFile>, GdriveError> {
        let mut discovered: Vec<(String, String, Vec<String>)> = Vec::new();
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        visited.insert(folder_id.to_owned());
        queue.push_back((folder_id.to_owned(), vec![folder_name.to_owned()], 0usize));

        while let Some((current_id, segments, depth)) = queue.pop_front() {
            let (_, entries) = self.fetch_embedded_folder(&current_id)?;
            for entry in entries {
                if entry.is_folder {
                    if depth < MAX_FOLDER_DEPTH && visited.insert(entry.id.clone()) {
                        let mut next_seg = segments.clone();
                        next_seg.push(entry.name);
                        queue.push_back((entry.id, next_seg, depth + 1));
                    }
                } else if is_supported_audio_name(&entry.name) {
                    discovered.push((entry.id, entry.name, segments.clone()));
                }
            }
        }

        // Probe exact byte sizes via `Range: bytes=0-0` so RangeCachedReader + SeekFrom::End work.
        let mut files = Vec::with_capacity(discovered.len());
        for (id, name, segments) in discovered {
            if let Ok(mut probed) = self.probe_public_file(&id, Some(&name)) {
                probed.folder_segments = segments;
                if probed.size > 0 {
                    files.push(probed);
                }
            }
        }
        Ok(files)
    }

    fn probe_public_file(
        &self,
        file_id: &str,
        known_name: Option<&str>,
    ) -> Result<DriveFile, GdriveError> {
        let url = format!(
            "{}/download?id={}&export=download&confirm=t",
            self.download_base,
            urlencoding_segment(file_id)
        );
        let (size, disp_name, _) = self.request_public_range_raw(&url, 0, 0, None, true)?;
        let name = known_name
            .map(str::to_owned)
            .or(disp_name)
            .unwrap_or_else(|| format!("{file_id}.flac"));
        Ok(DriveFile {
            id: file_id.to_owned(),
            name,
            folder_segments: Vec::new(),
            size,
            modified_time: 0,
            md5: None,
            mime_type: String::new(),
        })
    }

    fn fetch_range_zero_key(
        &self,
        file_id: &str,
        start: u64,
        end_inclusive: u64,
    ) -> Result<Vec<u8>, GdriveError> {
        let url = format!(
            "{}/download?id={}&export=download&confirm=t",
            self.download_base,
            urlencoding_segment(file_id)
        );
        let (_, _, bytes) =
            self.request_public_range_raw(&url, start, end_inclusive, None, true)?;
        Ok(bytes)
    }

    /// Executes a public `Range` download and handles Google's >100 MB virus-scan HTML confirmation
    /// form if returned. Returns `(total_file_size, optional_filename, requested_bytes)`.
    fn request_public_range_raw(
        &self,
        url: &str,
        start: u64,
        end_inclusive: u64,
        cookie: Option<&str>,
        allow_confirm_retry: bool,
    ) -> Result<(u64, Option<String>, Vec<u8>), GdriveError> {
        let range_hdr = format!("bytes={start}-{end_inclusive}");
        let mut attempt = 0usize;
        loop {
            let mut req = self.agent.get(url).header("Range", &range_hdr);
            if let Some(c) = cookie {
                req = req.header("Cookie", c);
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
            if (status == 429 || (500..=504).contains(&status)) && attempt < MAX_TRANSIENT_RETRIES {
                attempt += 1;
                std::thread::sleep(backoff(attempt));
                continue;
            }
            if status == 401 || status == 403 || status == 404 {
                return Err(private_or_missing_error(status));
            }
            if status == 416 {
                let total = resp
                    .headers()
                    .get("content-range")
                    .and_then(|v| v.to_str().ok())
                    .and_then(parse_content_range_total)
                    .unwrap_or(0);
                return Ok((total, None, Vec::new()));
            }

            let content_type = resp
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_ascii_lowercase();
            let content_range_total = resp
                .headers()
                .get("content-range")
                .and_then(|v| v.to_str().ok())
                .and_then(parse_content_range_total);
            let content_length = resp
                .headers()
                .get("content-length")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok());
            let disp_name = resp
                .headers()
                .get("content-disposition")
                .and_then(|v| v.to_str().ok())
                .and_then(parse_content_disposition_filename);
            let set_cookies: Vec<String> = resp
                .headers()
                .get_all("set-cookie")
                .iter()
                .filter_map(|v| v.to_str().ok())
                .filter_map(|raw| raw.split(';').next())
                .map(str::to_owned)
                .collect();

            let bytes = match resp.into_body().read_to_vec() {
                Ok(b) => b,
                Err(e) => {
                    if attempt < MAX_TRANSIENT_RETRIES {
                        attempt += 1;
                        std::thread::sleep(backoff(attempt));
                        continue;
                    }
                    return Err(GdriveError::Http(e.to_string()));
                }
            };

            if !(200..300).contains(&status) {
                return Err(GdriveError::Api {
                    status,
                    message: format!("HTTP {status} from Google Drive"),
                });
            }

            // Detect Google's HTML virus-scan interstitial / quota / sign-in page.
            if content_type.contains("text/html") || bytes.starts_with(b"<!DOCTYPE html") || bytes.starts_with(b"<html") {
                let html = String::from_utf8_lossy(&bytes);
                if looks_like_quota_exceeded(&html) {
                    return Err(GdriveError::Api {
                        status: 403,
                        message: "Google Drive download quota exceeded for this public file. Try again later or sign in with OAuth2 under Advanced.".into(),
                    });
                }
                if allow_confirm_retry
                    && let Some(confirm_url) = extract_virus_scan_confirm_url(&html, url)
                {
                    let cookie_hdr = (!set_cookies.is_empty()).then(|| set_cookies.join("; "));
                    return self.request_public_range_raw(
                        &confirm_url,
                        start,
                        end_inclusive,
                        cookie_hdr.as_deref().or(cookie),
                        false,
                    );
                }
                return Err(private_or_missing_error(403));
            }

            if status == 206 {
                let total = content_range_total.unwrap_or_else(|| {
                    start.saturating_add(bytes.len() as u64)
                });
                return Ok((total, disp_name, bytes));
            }

            // Server returned 200 OK with full file body; slice to requested range.
            let total = content_length.unwrap_or(bytes.len() as u64);
            let s = usize::try_from(start).unwrap_or(bytes.len());
            if s >= bytes.len() {
                return Ok((total, disp_name, Vec::new()));
            }
            let e = usize::try_from(end_inclusive.saturating_add(1))
                .unwrap_or(bytes.len())
                .min(bytes.len());
            return Ok((total, disp_name, bytes[s..e].to_vec()));
        }
    }
}

impl RangeFetcher for PublicDriveClient {
    fn fetch_range(
        &self,
        file_id: &str,
        start: u64,
        end_inclusive: u64,
    ) -> Result<Vec<u8>, GdriveError> {
        PublicDriveClient::fetch_range(self, file_id, start, end_inclusive)
    }
}

fn raw_to_drive_file(raw: RawDriveFile, folder_segments: Vec<String>) -> DriveFile {
    let size = raw
        .size
        .as_deref()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);
    let modified_time = raw
        .modified_time
        .as_deref()
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok().map(|dt| dt.timestamp()))
        .unwrap_or(0);
    DriveFile {
        id: raw.id,
        name: raw.name,
        folder_segments,
        size,
        modified_time,
        md5: raw.md5_checksum,
        mime_type: raw.mime_type,
    }
}

fn private_or_missing_error(status: u16) -> GdriveError {
    GdriveError::Api {
        status,
        message: "This Google Drive folder or file is private or unavailable. Share it as \"Anyone with the link (Viewer)\" or sign in with OAuth2 under Advanced.".into(),
    }
}

fn looks_like_signin_page(html: &str) -> bool {
    let lower = html.to_ascii_lowercase();
    lower.contains("accounts.google.com/servicelogin")
        || lower.contains("accounts.google.com/v3/signin")
        || lower.contains("you need access")
}

fn looks_like_quota_exceeded(html: &str) -> bool {
    let lower = html.to_ascii_lowercase();
    lower.contains("quota exceeded")
        || lower.contains("too many users have viewed or downloaded")
}

/// Parses `Content-Range: bytes 0-0/123456` or `bytes */123456` -> `Some(123456)`.
pub(crate) fn parse_content_range_total(header: &str) -> Option<u64> {
    let (_, total) = header.rsplit_once('/')?;
    total.trim().parse::<u64>().ok()
}

/// Extracts filename from `Content-Disposition: attachment; filename="01 - Song.flac"`.
pub(crate) fn parse_content_disposition_filename(header: &str) -> Option<String> {
    // Prefer RFC 5987 filename*=UTF-8''... when present.
    for part in header.split(';') {
        let trimmed = part.trim();
        if let Some(rest) = trimmed
            .strip_prefix("filename*=")
            .or_else(|| trimmed.strip_prefix("FILENAME*="))
        {
            let encoded = rest
                .split_once("''")
                .map(|(_, v)| v)
                .unwrap_or(rest)
                .trim_matches('"');
            let decoded = percent_decode(encoded);
            if !decoded.is_empty() {
                return Some(decoded);
            }
        }
    }
    for part in header.split(';') {
        let trimmed = part.trim();
        if let Some(rest) = trimmed
            .strip_prefix("filename=")
            .or_else(|| trimmed.strip_prefix("FILENAME="))
        {
            let name = rest.trim().trim_matches('"');
            if !name.is_empty() {
                return Some(name.to_owned());
            }
        }
    }
    None
}

/// Extracts Google's >100 MB virus-scan confirmation URL from the download interstitial HTML.
pub(crate) fn extract_virus_scan_confirm_url(html: &str, current_url: &str) -> Option<String> {
    // Look for `<form id="download-form" action="..."` or any `<form ... action="...download..."`.
    let lower = html.to_ascii_lowercase();
    let form_pos = lower
        .find("id=\"download-form\"")
        .or_else(|| lower.find("id='download-form'"))
        .or_else(|| lower.find("<form"))?;
    let form_start = lower[..=form_pos].rfind("<form")?;
    let form_end = lower[form_start..]
        .find("</form>")
        .map(|i| form_start + i)
        .unwrap_or(html.len());
    let form_html = &html[form_start..form_end];

    let action = extract_attr(form_html, "action")
        .map(html_unescape)
        .unwrap_or_else(|| current_url.to_owned());
    let base = Url::parse(current_url).ok()?;
    let mut target = base.join(&action).ok()?;

    // Collect hidden `<input name="..." value="...">` fields.
    let mut params: Vec<(String, String)> = Vec::new();
    let mut cursor = 0usize;
    let form_lower = form_html.to_ascii_lowercase();
    while let Some(rel) = form_lower[cursor..].find("<input") {
        let tag_start = cursor + rel;
        let Some(rel_end) = form_html[tag_start..].find('>') else {
            break;
        };
        let tag = &form_html[tag_start..=tag_start + rel_end];
        if let (Some(name), Some(value)) = (extract_attr(tag, "name"), extract_attr(tag, "value")) {
            params.push((html_unescape(name), html_unescape(value)));
        }
        cursor = tag_start + rel_end + 1;
    }

    if params.is_empty() {
        return None;
    }
    target.set_query(None);
    {
        let mut qp = target.query_pairs_mut();
        for (k, v) in params {
            qp.append_pair(&k, &v);
        }
    }
    Some(target.into())
}

/// Parses Google Drive's `embeddedfolderview?id=<ID>` HTML into `(folder_title, entries)`.
pub fn parse_embedded_folder_html(html: &str) -> (Option<String>, Vec<PublicHtmlEntry>) {
    let title = extract_tag_text(html, "title").map(|raw| {
        let clean = html_unescape(&raw).trim().to_owned();
        clean
            .strip_suffix(" - Google Drive")
            .unwrap_or(&clean)
            .trim()
            .to_owned()
    }).filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("Google Drive"));

    let mut entries = Vec::new();
    let mut seen = HashSet::new();
    let mut cursor = 0usize;

    // Each row in embeddedfolderview contains an `<a href="...drive.google.com/...">` and a
    // `<div class="flip-entry-title">Name</div>` inside the same `flip-entry` block.
    while let Some(rel) = html[cursor..].find("flip-entry\"") {
        let block_start = cursor + rel;
        // Bound this entry block to the next `flip-entry"` or 4 KiB ahead.
        let search_from = block_start + "flip-entry\"".len();
        let block_end = html[search_from..]
            .find("flip-entry\"")
            .map(|i| search_from + i)
            .unwrap_or(html.len())
            .min(block_start + 4096);
        let block = &html[block_start..block_end];
        cursor = search_from;

        let Some(href) = extract_attr(block, "href").map(html_unescape) else {
            continue;
        };
        let Some(name) = extract_class_text(block, "flip-entry-title").map(|s| html_unescape(&s).trim().to_owned()) else {
            continue;
        };
        if name.is_empty() {
            continue;
        }
        if let Some((id, is_folder)) = extract_id_from_href(&href)
            && seen.insert(id.clone())
        {
            entries.push(PublicHtmlEntry {
                id,
                name,
                is_folder,
            });
        }
    }

    (title, entries)
}

fn extract_id_from_href(href: &str) -> Option<(String, bool)> {
    if let Some(pos) = href.find("/folders/") {
        let rest = &href[pos + "/folders/".len()..];
        let id: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
            .collect();
        if is_valid_drive_id(&id) {
            return Some((id, true));
        }
    }
    if let Some(pos) = href.find("/file/d/") {
        let rest = &href[pos + "/file/d/".len()..];
        let id: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
            .collect();
        if is_valid_drive_id(&id) {
            return Some((id, false));
        }
    }
    if let Ok(url) = Url::parse(href)
        && let Some((_, id)) = url.query_pairs().find(|(k, _)| k == "id")
        && is_valid_drive_id(&id)
    {
        let is_folder = url.path().contains("embeddedfolderview") || url.path().contains("folders");
        return Some((id.into_owned(), is_folder));
    }
    None
}

fn extract_attr<'a>(snippet: &'a str, attr: &str) -> Option<&'a str> {
    let lower = snippet.to_ascii_lowercase();
    let needle_dq = format!("{attr}=\"");
    if let Some(pos) = lower.find(&needle_dq) {
        let val_start = pos + needle_dq.len();
        let val_end = snippet[val_start..].find('"')?;
        return Some(&snippet[val_start..val_start + val_end]);
    }
    let needle_sq = format!("{attr}='");
    if let Some(pos) = lower.find(&needle_sq) {
        let val_start = pos + needle_sq.len();
        let val_end = snippet[val_start..].find('\'')?;
        return Some(&snippet[val_start..val_start + val_end]);
    }
    None
}

fn extract_tag_text(html: &str, tag: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let open = format!("<{tag}");
    let start = lower.find(&open)?;
    let gt = lower[start..].find('>')?;
    let content_start = start + gt + 1;
    let close = format!("</{tag}>");
    let content_end = lower[content_start..].find(&close)?;
    Some(html[content_start..content_start + content_end].to_owned())
}

fn extract_class_text(html: &str, class_name: &str) -> Option<String> {
    let pos = html
        .find(&format!("class=\"{class_name}\""))
        .or_else(|| html.find(&format!("class='{class_name}'")))?;
    let gt = html[pos..].find('>')?;
    let content_start = pos + gt + 1;
    let content_end = html[content_start..].find('<')?;
    Some(html[content_start..content_start + content_end].to_owned())
}

fn html_unescape(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
        .replace("&apos;", "'")
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(hex) = std::str::from_utf8(&bytes[i + 1..i + 3])
            && let Ok(v) = u8::from_str_radix(hex, 16)
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
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

fn backoff(attempt: usize) -> Duration {
    match attempt {
        1 => Duration::from_millis(150),
        2 => Duration::from_millis(400),
        _ => Duration::from_millis(800),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::thread::{self, JoinHandle};

    struct MockPublicServer {
        base_url: String,
        stop: Arc<AtomicBool>,
        hits: Arc<AtomicUsize>,
        handle: Option<JoinHandle<()>>,
    }

    impl Drop for MockPublicServer {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            if let Some(h) = self.handle.take() {
                let _ = h.join();
            }
        }
    }

    fn spawn_server(
        handler: impl Fn(&str, &[(String, String)]) -> (String, Vec<(&'static str, String)>, Vec<u8>)
            + Send
            + Sync
            + 'static,
    ) -> MockPublicServer {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let hits = Arc::new(AtomicUsize::new(0));
        let st = stop.clone();
        let h_hits = hits.clone();
        let handler = Arc::new(handler);

        let handle = thread::spawn(move || {
            while !st.load(Ordering::Relaxed) {
                let Ok((mut stream, _)) = listener.accept() else {
                    thread::sleep(Duration::from_millis(5));
                    continue;
                };
                let _ = stream.set_nonblocking(false);
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut req_line = String::new();
                if reader.read_line(&mut req_line).is_err() {
                    continue;
                }
                let path = req_line
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or("/")
                    .to_owned();
                let mut headers = Vec::new();
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).is_err() || line.trim().is_empty() {
                        break;
                    }
                    if let Some((k, v)) = line.split_once(':') {
                        headers.push((k.trim().to_ascii_lowercase(), v.trim().to_owned()));
                    }
                }
                h_hits.fetch_add(1, Ordering::SeqCst);
                let (status, resp_headers, body) = handler(&path, &headers);
                let mut head = format!("HTTP/1.1 {status}\r\n");
                for (k, v) in resp_headers {
                    head.push_str(&format!("{k}: {v}\r\n"));
                }
                head.push_str(&format!(
                    "Content-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                ));
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(&body);
            }
        });

        MockPublicServer {
            base_url: format!("http://{addr}"),
            stop,
            hits,
            handle: Some(handle),
        }
    }

    #[test]
    fn parses_embedded_folder_html_with_entities_and_subfolders() {
        let html = r#"
        <html><head><title>Jazz &amp; Blues - Google Drive</title></head><body>
          <div class="flip-entry" id="entry-1">
            <a href="https://drive.google.com/drive/folders/1SubFolderId12345?usp=drive_web">
              <div class="flip-entry-title">Miles Davis &#39;59</div>
            </a>
          </div>
          <div class="flip-entry" id="entry-2">
            <a href="https://drive.google.com/file/d/1TrackFileId12345/view?usp=drive_web">
              <div class="flip-entry-title">01 - So What.flac</div>
            </a>
          </div>
          <div class="flip-entry" id="entry-3">
            <a href="https://drive.google.com/file/d/1CoverImgId123456/view">
              <div class="flip-entry-title">cover.jpg</div>
            </a>
          </div>
        </body></html>
        "#;
        let (title, entries) = parse_embedded_folder_html(html);
        assert_eq!(title.as_deref(), Some("Jazz & Blues"));
        assert_eq!(entries.len(), 3);
        assert_eq!(
            entries[0],
            PublicHtmlEntry {
                id: "1SubFolderId12345".into(),
                name: "Miles Davis '59".into(),
                is_folder: true,
            }
        );
        assert_eq!(
            entries[1],
            PublicHtmlEntry {
                id: "1TrackFileId12345".into(),
                name: "01 - So What.flac".into(),
                is_folder: false,
            }
        );
    }

    #[test]
    fn zero_key_folder_scan_cycle_protection_range_streaming_and_virus_confirm() {
        let payload: Vec<u8> = (0..12_288).map(|i| (i % 251) as u8).collect();
        let payload_srv = payload.clone();

        let server = spawn_server(move |path, headers| {
            // Root folder points to subfolder AND subfolder points back to root (cycle!).
            if path.starts_with("/embeddedfolderview?id=1RootFolder12345") {
                let html = r#"
                <html><head><title>My Lossless - Google Drive</title></head><body>
                  <div class="flip-entry">
                    <a href="https://drive.google.com/drive/folders/1SubFolder123456"><div class="flip-entry-title">Disc 1</div></a>
                  </div>
                </body></html>"#;
                return (
                    "200 OK".into(),
                    vec![("Content-Type", "text/html; charset=utf-8".into())],
                    html.as_bytes().to_vec(),
                );
            }
            if path.starts_with("/embeddedfolderview?id=1SubFolder123456") {
                let html = r#"
                <html><head><title>Disc 1 - Google Drive</title></head><body>
                  <div class="flip-entry">
                    <a href="https://drive.google.com/drive/folders/1RootFolder12345"><div class="flip-entry-title">LoopBack</div></a>
                  </div>
                  <div class="flip-entry">
                    <a href="https://drive.google.com/file/d/1AudioFile123456/view"><div class="flip-entry-title">01 - Track.flac</div></a>
                  </div>
                </body></html>"#;
                return (
                    "200 OK".into(),
                    vec![("Content-Type", "text/html; charset=utf-8".into())],
                    html.as_bytes().to_vec(),
                );
            }
            if path.starts_with("/embeddedfolderview?id=1PrivateFolder12") {
                return (
                    "403 Forbidden".into(),
                    vec![("Content-Type", "text/html".into())],
                    b"Forbidden".to_vec(),
                );
            }
            // Simulate >100 MB virus-scan HTML interstitial on first hit without uuid,
            // then serve 206 Partial Content once confirmed with uuid=abc123.
            if path.starts_with("/download?") && !path.contains("uuid=abc123") {
                let html = r#"<!DOCTYPE html><html><body>
                  <form id="download-form" action="/download" method="get">
                    <input type="hidden" name="id" value="1AudioFile123456">
                    <input type="hidden" name="export" value="download">
                    <input type="hidden" name="confirm" value="t">
                    <input type="hidden" name="uuid" value="abc123">
                  </form></body></html>"#;
                return (
                    "200 OK".into(),
                    vec![
                        ("Content-Type", "text/html; charset=utf-8".into()),
                        ("Set-Cookie", "download_warning=ok; Path=/".into()),
                    ],
                    html.as_bytes().to_vec(),
                );
            }
            if path.starts_with("/download?") && path.contains("uuid=abc123") {
                let range = headers
                    .iter()
                    .find(|(k, _)| k == "range")
                    .and_then(|(_, v)| v.strip_prefix("bytes="))
                    .and_then(|r| r.split_once('-'));
                if let Some((s, e)) = range {
                    let s: usize = s.parse().unwrap();
                    let e: usize = e.parse::<usize>().unwrap().min(payload_srv.len() - 1);
                    let slice = payload_srv[s..=e].to_vec();
                    return (
                        "206 Partial Content".into(),
                        vec![
                            ("Content-Type", "audio/flac".into()),
                            (
                                "Content-Range",
                                format!("bytes {s}-{e}/{}", payload_srv.len()),
                            ),
                            (
                                "Content-Disposition",
                                "attachment; filename=\"01 - Track.flac\"".into(),
                            ),
                        ],
                        slice,
                    );
                }
            }
            ("404 Not Found".into(), vec![], b"not found".to_vec())
        });

        let client = Arc::new(
            PublicDriveClient::new(None, "sonder-test").with_bases(
                &server.base_url,
                &server.base_url,
                &server.base_url,
            ),
        );

        // Private folder returns user-friendly error.
        let err = client
            .scan_folder("1PrivateFolder12", "Private")
            .unwrap_err()
            .to_string();
        assert!(err.contains("Anyone with the link"), "got: {err}");

        // Recursive scan terminates despite cycle and probes exact file size through virus-scan confirm.
        let files = client
            .scan_folder("1RootFolder12345", "My Lossless")
            .unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].id, "1AudioFile123456");
        assert_eq!(files[0].name, "01 - Track.flac");
        assert_eq!(files[0].size, payload.len() as u64);
        assert_eq!(files[0].folder_segments, vec!["My Lossless", "Disc 1"]);

        // RangeCachedReader streams & seeks over PublicDriveClient identically to OAuth!
        let dir = tempfile::tempdir().unwrap();
        let cache = Arc::new(crate::cache::BlockCache::open(dir.path(), 64 * 1024, 4096));
        let mut reader = crate::reader::RangeCachedReader::new(
            "1AudioFile123456",
            payload.len() as u64,
            client,
            cache,
            false,
        );
        let mut all = Vec::new();
        reader.read_to_end(&mut all).unwrap();
        assert_eq!(all, payload);
        assert!(server.hits.load(Ordering::SeqCst) > 0);
    }

    #[test]
    fn api_key_mode_uses_drive_v3_with_key_param() {
        let payload = b"FLAC-BYTES-0123456789".to_vec();
        let payload_srv = payload.clone();
        let server = spawn_server(move |path, headers| {
            assert!(path.contains("key=my-test-key"), "missing key in {path}");
            if path.starts_with("/files/1ApiFile12345678?alt=media") {
                let range = headers
                    .iter()
                    .find(|(k, _)| k == "range")
                    .and_then(|(_, v)| v.strip_prefix("bytes="))
                    .and_then(|r| r.split_once('-'));
                let (s, e) = range
                    .map(|(s, e)| {
                        (
                            s.parse::<usize>().unwrap(),
                            e.parse::<usize>().unwrap().min(payload_srv.len() - 1),
                        )
                    })
                    .unwrap_or((0, payload_srv.len() - 1));
                return (
                    "206 Partial Content".into(),
                    vec![(
                        "Content-Range",
                        format!("bytes {s}-{e}/{}", payload_srv.len()),
                    )],
                    payload_srv[s..=e].to_vec(),
                );
            }
            if path.starts_with("/files?") {
                let json = format!(
                    r#"{{"files":[{{"id":"1ApiFile12345678","name":"track.flac","mimeType":"audio/flac","size":"{}"}}]}}"#,
                    payload_srv.len()
                );
                return (
                    "200 OK".into(),
                    vec![("Content-Type", "application/json".into())],
                    json.into_bytes(),
                );
            }
            ("404 Not Found".into(), vec![], b"{}".to_vec())
        });

        let client = PublicDriveClient::new(Some("my-test-key".into()), "sonder-test")
            .with_bases(&server.base_url, &server.base_url, &server.base_url);
        let files = client.scan_folder("1ApiFolder123456", "API Folder").unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].id, "1ApiFile12345678");
        let chunk = client.fetch_range("1ApiFile12345678", 5, 9).unwrap();
        assert_eq!(chunk, b"BYTES");
    }
}
