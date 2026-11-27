//! Google Drive streaming service, hybrid decoder opener, sync worker, and Tauri commands.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use sonder_decode::{DecodeError, SourceDecoder};
use sonder_engine::SourceOpener;
use sonder_gdrive::{
    AuthManager, BlockCache, DEFAULT_CHUNK_SIZE, DriveClient, OAuthConfig, OAuthConfigStore,
    PersistedGdriveConfig, RangeCachedReader, TokenStore, gdrive_virtual_path, is_gdrive_path,
    parse_gdrive_path,
};
use sonder_library::{Library, RemoteAudioFile, ScanProgress, TagError, fallback_meta, read_stream};
use specta::Type;
use tauri::{AppHandle, Manager, State};
use tauri_specta::Event;

use crate::dto::{AppError, AppResult};
use crate::events::{LibraryChangedEvent, NoticeLevel, notice};
use crate::state::{AppState, lock};

/// Default disk cache cap in MiB (2 GiB).
pub const DEFAULT_CACHE_MAX_MB: u32 = 2048;
pub const MIN_CACHE_MAX_MB: u32 = 64;
pub const MAX_CACHE_MAX_MB: u32 = 65_536;

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GdriveStatus {
    pub configured: bool,
    pub client_id: String,
    pub has_client_secret: bool,
    pub env_override: bool,
    pub authenticated: bool,
    pub account_email: Option<String>,
    pub syncing: bool,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GdriveFolderItem {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GdriveSyncedFolder {
    pub id: i64,
    pub folder_id: String,
    pub name: String,
    pub added_at: i64,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GdriveCacheStats {
    pub total_bytes: u64,
    pub max_bytes: u64,
    pub chunk_count: u64,
    pub file_count: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum GdriveSyncPhase {
    Listing,
    Reading,
    Writing,
}

#[derive(Debug, Clone, Serialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct GdriveSyncProgressEvent {
    pub phase: GdriveSyncPhase,
    pub folder_name: String,
    pub done: u32,
    pub total: u32,
}

#[derive(Debug, Clone, Serialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct GdriveSyncFinishedEvent {
    pub added: u32,
    pub updated: u32,
    pub removed: u32,
    pub unchanged: u32,
    pub failure_count: u32,
    pub error: Option<String>,
}

/// Shared Google Drive runtime state managed inside [`AppState`].
pub struct GdriveService {
    pub auth: Arc<AuthManager>,
    pub client: Arc<DriveClient>,
    pub cache: Arc<BlockCache>,
    pub syncing: AtomicBool,
    config_store: OAuthConfigStore,
    config: Mutex<PersistedGdriveConfig>,
    auth_cancel: Mutex<Option<Arc<AtomicBool>>>,
}

impl GdriveService {
    pub fn new(config_path: PathBuf, tokens_path: PathBuf, cache_dir: PathBuf) -> Self {
        let config_store = OAuthConfigStore::new(config_path);
        let persisted = config_store.load();
        let oauth = OAuthConfig::resolve(
            persisted.client_id.as_deref(),
            persisted.client_secret.as_deref(),
        );
        let auth = Arc::new(AuthManager::new(
            TokenStore::new(tokens_path),
            oauth,
            crate::commands::USER_AGENT,
        ));
        let client = Arc::new(DriveClient::new(auth.clone(), crate::commands::USER_AGENT));
        let cache_max_mb = persisted
            .cache_max_mb
            .unwrap_or(DEFAULT_CACHE_MAX_MB)
            .clamp(MIN_CACHE_MAX_MB, MAX_CACHE_MAX_MB);
        let max_bytes = u64::from(cache_max_mb) * 1024 * 1024;
        let cache = Arc::new(BlockCache::open(cache_dir, max_bytes, DEFAULT_CHUNK_SIZE));
        Self {
            auth,
            client,
            cache,
            syncing: AtomicBool::new(false),
            config_store,
            config: Mutex::new(persisted),
            auth_cancel: Mutex::new(None),
        }
    }

    pub fn cache_stats(&self) -> GdriveCacheStats {
        let s = self.cache.stats();
        GdriveCacheStats {
            total_bytes: s.total_bytes,
            max_bytes: s.max_bytes,
            chunk_count: s.chunk_count,
            file_count: s.file_count,
        }
    }

    pub fn cancel_sign_in(&self) {
        if let Some(flag) = lock(&self.auth_cancel).as_ref() {
            flag.store(true, Ordering::Relaxed);
        }
    }
}

/// Opens local paths via the filesystem and `gdrive://` virtual paths via a seekable
/// Range-cached reader feeding the exact same bit-perfect decode/DSP pipeline.
pub struct HybridSourceOpener {
    gdrive: Arc<GdriveService>,
    library: Arc<Mutex<Library>>,
}

impl HybridSourceOpener {
    pub fn new(gdrive: Arc<GdriveService>, library: Arc<Mutex<Library>>) -> Self {
        Self { gdrive, library }
    }
}

impl SourceOpener for HybridSourceOpener {
    fn open(&self, path: &Path) -> Result<SourceDecoder, DecodeError> {
        if !is_gdrive_path(path) {
            return SourceDecoder::open(path);
        }
        let Some((file_id, file_name)) = parse_gdrive_path(path) else {
            return Err(DecodeError::Unsupported(format!(
                "malformed Google Drive track path: {}",
                path.display()
            )));
        };

        // Resolve file size from the library index first to avoid an extra metadata round-trip.
        let cached_size = lock(&self.library)
            .track_by_path(path)
            .ok()
            .flatten()
            .and_then(|t| t.remote_size)
            .filter(|&s| s > 0);
        let file_size = match cached_size {
            Some(size) => size,
            None => self
                .gdrive
                .client
                .file_metadata(file_id)
                .map_err(|e| DecodeError::Decode(format!("Google Drive metadata failed: {e}")))?
                .size,
        };
        if file_size == 0 {
            return Err(DecodeError::Decode(
                "Google Drive audio file is empty".into(),
            ));
        }

        let reader = RangeCachedReader::new(
            file_id,
            file_size,
            self.gdrive.client.clone(),
            self.gdrive.cache.clone(),
            true,
        );
        let ext = Path::new(file_name).extension().and_then(|e| e.to_str());
        SourceDecoder::open_source(Box::new(reader), Some(file_size), ext)
    }
}

fn build_status(state: &AppState) -> GdriveStatus {
    let persisted = lock(&state.gdrive.config).clone();
    let auth = state.gdrive.auth.status();
    let saved_id = persisted.client_id.unwrap_or_default();
    let env_override = saved_id.trim().is_empty() && OAuthConfig::env_client_id_present();
    GdriveStatus {
        configured: auth.configured,
        client_id: saved_id,
        has_client_secret: persisted
            .client_secret
            .as_deref()
            .is_some_and(|s| !s.trim().is_empty()),
        env_override,
        authenticated: auth.authenticated,
        account_email: auth.account_email,
        syncing: state.gdrive.syncing.load(Ordering::SeqCst),
    }
}

#[tauri::command]
#[specta::specta]
pub async fn gdrive_status(state: State<'_, AppState>) -> AppResult<GdriveStatus> {
    Ok(build_status(&state))
}

#[tauri::command]
#[specta::specta]
pub async fn gdrive_save_config(
    state: State<'_, AppState>,
    client_id: String,
    client_secret: Option<String>,
) -> AppResult<GdriveStatus> {
    let clean_id = client_id.trim().to_owned();
    let updated = {
        let mut cfg = lock(&state.gdrive.config);
        cfg.client_id = (!clean_id.is_empty()).then_some(clean_id);
        // None keeps the existing secret; Some("") clears it.
        if let Some(secret) = client_secret {
            let trimmed = secret.trim().to_owned();
            cfg.client_secret = (!trimmed.is_empty()).then_some(trimmed);
        }
        state.gdrive.config_store.save(&cfg)?;
        cfg.clone()
    };
    let resolved = OAuthConfig::resolve(
        updated.client_id.as_deref(),
        updated.client_secret.as_deref(),
    );
    state.gdrive.auth.set_config(resolved);
    Ok(build_status(&state))
}

#[tauri::command]
#[specta::specta]
pub async fn gdrive_sign_in(state: State<'_, AppState>) -> AppResult<GdriveStatus> {
    state.gdrive.cancel_sign_in();
    let cancel = Arc::new(AtomicBool::new(false));
    {
        *lock(&state.gdrive.auth_cancel) = Some(cancel.clone());
    }
    let auth = state.gdrive.auth.clone();
    let cancel_bg = cancel.clone();
    let result = tauri::async_runtime::spawn_blocking(move || auth.sign_in_interactive(cancel_bg))
        .await
        .map_err(|e| AppError(e.to_string()))?;
    {
        let mut slot = lock(&state.gdrive.auth_cancel);
        if slot.as_ref().is_some_and(|cur| Arc::ptr_eq(cur, &cancel)) {
            *slot = None;
        }
    }
    result.map_err(AppError::from)?;
    Ok(build_status(&state))
}

#[tauri::command]
#[specta::specta]
pub async fn gdrive_cancel_sign_in(state: State<'_, AppState>) -> AppResult<()> {
    state.gdrive.cancel_sign_in();
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn gdrive_sign_out(state: State<'_, AppState>) -> AppResult<GdriveStatus> {
    let auth = state.gdrive.auth.clone();
    tauri::async_runtime::spawn_blocking(move || auth.disconnect())
        .await
        .map_err(|e| AppError(e.to_string()))??;
    Ok(build_status(&state))
}

#[tauri::command]
#[specta::specta]
pub async fn gdrive_list_folders(
    state: State<'_, AppState>,
    parent_id: Option<String>,
) -> AppResult<Vec<GdriveFolderItem>> {
    let client = state.gdrive.client.clone();
    let folders = tauri::async_runtime::spawn_blocking(move || {
        client.list_folders(parent_id.as_deref())
    })
    .await
    .map_err(|e| AppError(e.to_string()))??;
    Ok(folders
        .into_iter()
        .map(|f| GdriveFolderItem {
            id: f.id,
            name: f.name,
        })
        .collect())
}

#[tauri::command]
#[specta::specta]
pub async fn gdrive_synced_folders(state: State<'_, AppState>) -> AppResult<Vec<GdriveSyncedFolder>> {
    let roots = lock(&state.library).gdrive_roots()?;
    Ok(roots
        .into_iter()
        .map(|r| GdriveSyncedFolder {
            id: r.id,
            folder_id: r.folder_id,
            name: r.name,
            added_at: r.added_at,
        })
        .collect())
}

#[tauri::command]
#[specta::specta]
pub async fn gdrive_add_folder(
    app: AppHandle,
    state: State<'_, AppState>,
    folder_id: String,
    name: String,
) -> AppResult<GdriveSyncedFolder> {
    let root = lock(&state.library).add_gdrive_root(&folder_id, &name)?;
    start_sync(&app);
    Ok(GdriveSyncedFolder {
        id: root.id,
        folder_id: root.folder_id,
        name: root.name,
        added_at: root.added_at,
    })
}

#[tauri::command]
#[specta::specta]
pub async fn gdrive_remove_folder(
    app: AppHandle,
    state: State<'_, AppState>,
    folder_id: String,
) -> AppResult<()> {
    if lock(&state.library).remove_gdrive_root(&folder_id)? {
        let _ = LibraryChangedEvent.emit(&app);
    }
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn gdrive_sync_now(app: AppHandle) -> AppResult<()> {
    start_sync(&app);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn gdrive_cache_stats(state: State<'_, AppState>) -> AppResult<GdriveCacheStats> {
    Ok(state.gdrive.cache_stats())
}

#[tauri::command]
#[specta::specta]
pub async fn gdrive_set_cache_limit(
    state: State<'_, AppState>,
    max_mb: u32,
) -> AppResult<GdriveCacheStats> {
    let clamped = max_mb.clamp(MIN_CACHE_MAX_MB, MAX_CACHE_MAX_MB);
    {
        let mut cfg = lock(&state.gdrive.config);
        cfg.cache_max_mb = Some(clamped);
        state.gdrive.config_store.save(&cfg)?;
    }
    state
        .gdrive
        .cache
        .set_max_bytes(u64::from(clamped) * 1024 * 1024);
    Ok(state.gdrive.cache_stats())
}

#[tauri::command]
#[specta::specta]
pub async fn gdrive_clear_cache(state: State<'_, AppState>) -> AppResult<GdriveCacheStats> {
    let cache = state.gdrive.cache.clone();
    tauri::async_runtime::spawn_blocking(move || cache.clear())
        .await
        .map_err(|e| AppError(e.to_string()))??;
    Ok(state.gdrive.cache_stats())
}

/// Starts a background sync of all registered Google Drive folders if one is not already running.
pub fn start_sync(app: &AppHandle) {
    let state = app.state::<AppState>();
    if state.gdrive.syncing.swap(true, Ordering::SeqCst) {
        return;
    }
    let handle = app.clone();
    let spawned = std::thread::Builder::new()
        .name("sonder-gdrive-sync".into())
        .spawn(move || run_sync(&handle));
    if let Err(err) = spawned {
        state.gdrive.syncing.store(false, Ordering::SeqCst);
        notice(
            app,
            NoticeLevel::Error,
            format!("Cannot start Google Drive sync: {err}"),
        );
    }
}

fn run_sync(app: &AppHandle) {
    let state = app.state::<AppState>();
    let result = sync_all_folders(app, &state);
    state.gdrive.syncing.store(false, Ordering::SeqCst);

    match result {
        Ok((added, updated, removed, unchanged, failures)) => {
            let _ = GdriveSyncFinishedEvent {
                added,
                updated,
                removed,
                unchanged,
                failure_count: failures,
                error: None,
            }
            .emit(app);
            if added + updated + removed > 0 {
                let _ = LibraryChangedEvent.emit(app);
            }
        }
        Err(err) => {
            let _ = GdriveSyncFinishedEvent {
                added: 0,
                updated: 0,
                removed: 0,
                unchanged: 0,
                failure_count: 0,
                error: Some(err.0.clone()),
            }
            .emit(app);
            notice(
                app,
                NoticeLevel::Error,
                format!("Google Drive sync failed: {}", err.0),
            );
        }
    }
}

fn sync_all_folders(app: &AppHandle, state: &AppState) -> AppResult<(u32, u32, u32, u32, u32)> {
    let roots = lock(&state.library).gdrive_roots()?;
    if roots.is_empty() {
        return Ok((0, 0, 0, 0, 0));
    }

    let mut total_added = 0u32;
    let mut total_updated = 0u32;
    let mut total_removed = 0u32;
    let mut total_unchanged = 0u32;
    let mut total_failures = 0u32;

    // Open a dedicated SQLite connection so UI queries are not blocked during network probing.
    let mut library = Library::open(&state.paths.db, &state.paths.art)?;
    let client = state.gdrive.client.clone();
    let cache = state.gdrive.cache.clone();

    for root in roots {
        let folder_name = root.name.clone();
        let _ = GdriveSyncProgressEvent {
            phase: GdriveSyncPhase::Listing,
            folder_name: folder_name.clone(),
            done: 0,
            total: 0,
        }
        .emit(app);

        let drive_files = client.scan_audio_files(&root.folder_id, &root.name)?;
        let remote_files: Vec<RemoteAudioFile> = drive_files
            .into_iter()
            .map(|f| {
                let mut grouping = PathBuf::from(format!("gdrive://folder/{}", root.folder_id));
                for seg in &f.folder_segments {
                    grouping.push(seg);
                }
                grouping.push(&f.name);
                RemoteAudioFile {
                    virtual_path: gdrive_virtual_path(&f.id, &f.name),
                    grouping_path: grouping,
                    remote_file_id: f.id,
                    remote_folder_id: root.folder_id.clone(),
                    remote_modified_time: f.modified_time,
                    remote_size: f.size,
                }
            })
            .collect();

        let report = library.sync_gdrive_folder(
            &root.folder_id,
            &remote_files,
            &|file| {
                // Probe headers/tags via Range requests without read-ahead prefetching.
                let reader = RangeCachedReader::new(
                    &file.remote_file_id,
                    file.remote_size,
                    client.clone(),
                    cache.clone(),
                    false,
                );
                match read_stream(reader, file.remote_size, &file.virtual_path) {
                    Ok(meta) => Ok(meta),
                    // Never swallow transient network/auth I/O errors into fallback metadata, or
                    // the track would be permanently indexed as 44.1 kHz / 0:00 and skipped on
                    // future syncs. Only fall back on container/tag parse failures.
                    Err(TagError::Io(io_err)) => Err(TagError::Io(io_err)),
                    Err(err) => {
                        let folder_hint = file
                            .grouping_path
                            .parent()
                            .and_then(|p| p.file_name())
                            .and_then(|n| n.to_str());
                        fallback_meta(&file.virtual_path, folder_hint).ok_or(err)
                    }
                }
            },
            &|p| {
                let (phase, done, total) = match p {
                    ScanProgress::Walking => (GdriveSyncPhase::Listing, 0, 0),
                    ScanProgress::Reading { done, total } => {
                        (GdriveSyncPhase::Reading, done as u32, total as u32)
                    }
                    ScanProgress::Writing => (GdriveSyncPhase::Writing, 0, 0),
                };
                let _ = GdriveSyncProgressEvent {
                    phase,
                    folder_name: folder_name.clone(),
                    done,
                    total,
                }
                .emit(app);
            },
        )?;

        for remote_id in &report.invalidated_remote_ids {
            cache.invalidate_file(remote_id);
        }

        total_added += report.added as u32;
        total_updated += report.updated as u32;
        total_removed += report.removed as u32;
        total_unchanged += report.unchanged as u32;
        total_failures += report.failed.len() as u32;
    }

    Ok((
        total_added,
        total_updated,
        total_removed,
        total_unchanged,
        total_failures,
    ))
}
