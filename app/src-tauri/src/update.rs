//! In-app updates from GitHub Releases.
//!
//! The updater plugin reads `latest.json` from the newest published release, compares its
//! version with this build and verifies every download against the minisign public key in
//! `tauri.conf.json` before installing it. Checking and installing run here, behind typed
//! commands, so the webview gets no updater permission of its own.

use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use specta::Type;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_updater::{Update, Updater, UpdaterExt};
use tauri_specta::Event;

use crate::dto::{AppError, AppResult};
use crate::state::{AppState, lock};

/// Longest a check may take, start to finish.
const CHECK_TIMEOUT: Duration = Duration::from_secs(30);
/// Longest a connection may go without sending anything, so a stalled download fails instead
/// of leaving the install waiting forever. A slow but moving download is never cut off.
const STALL_TIMEOUT: Duration = Duration::from_secs(30);

fn updater(app: &AppHandle) -> AppResult<Updater> {
    app.updater_builder()
        .timeout(CHECK_TIMEOUT)
        .configure_client(|client| {
            client
                .connect_timeout(STALL_TIMEOUT)
                .read_timeout(STALL_TIMEOUT)
        })
        // The default hook tears the app down and hides every window before the Windows
        // installer launches; when the launch then fails (blocked, or elevation declined)
        // Sonder would keep running with no window. The player is stopped explicitly instead.
        .on_before_exit(|| {})
        .build()
        .map_err(|e| AppError(format!("cannot check for updates: {e}")))
}

/// The update the last check found, kept until it is installed or a later check replaces it.
#[derive(Default)]
pub struct PendingUpdate(Mutex<Option<Update>>);

/// A newer release than the running build.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    pub current_version: String,
    /// Release notes from `latest.json`, fixed when the release was built.
    pub notes: Option<String>,
}

impl From<&Update> for UpdateInfo {
    fn from(update: &Update) -> Self {
        UpdateInfo {
            version: update.version.clone(),
            current_version: update.current_version.clone(),
            notes: update
                .body
                .as_ref()
                .map(|b| b.trim().to_string())
                .filter(|b| !b.is_empty()),
        }
    }
}

/// The launch check found a newer release.
#[derive(Debug, Clone, Serialize, Type, Event)]
pub struct UpdateAvailableEvent {
    pub update: UpdateInfo,
}

/// Download progress of the update being installed.
#[derive(Debug, Clone, Serialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProgressEvent {
    pub downloaded: u64,
    /// `None` when the server does not send a length.
    pub total: Option<u64>,
}

/// Downloaded bytes at which the next progress event is due: every whole percent, or every
/// 512 KiB when the length is unknown, so a large installer does not flood the IPC channel.
fn next_progress_mark(downloaded: u64, total: Option<u64>) -> u64 {
    match total {
        Some(total) if total > 0 => {
            let step = (total / 100).max(1);
            (downloaded / step + 1) * step
        }
        _ => (downloaded / (512 * 1024) + 1) * 512 * 1024,
    }
}

/// Asks GitHub for the newest release and remembers it when it is newer than this build.
async fn check(app: &AppHandle) -> AppResult<Option<UpdateInfo>> {
    let update = updater(app)?
        .check()
        .await
        .map_err(|e| AppError(format!("cannot check for updates: {e}")))?;
    let info = update.as_ref().map(UpdateInfo::from);
    *lock(&app.state::<PendingUpdate>().0) = update;
    Ok(info)
}

/// Puts `update` back so the user can retry without checking again, and describes `err`.
fn keep_for_retry(
    app: &AppHandle,
    update: Update,
    step: &str,
    err: tauri_plugin_updater::Error,
) -> AppError {
    *lock(&app.state::<PendingUpdate>().0) = Some(update);
    AppError(format!("cannot {step} the update: {err}"))
}

/// Checks once at launch when the setting allows it. A failure stays quiet (offline, or the
/// release feed is unreachable): the user did not ask, and Settings can check again.
pub fn check_on_launch(app: &AppHandle) {
    if !lock(&app.state::<AppState>().settings).update_check {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        match check(&app).await {
            Ok(Some(update)) => {
                let _ = UpdateAvailableEvent { update }.emit(&app);
            }
            Ok(None) => {}
            Err(err) => eprintln!("sonder: {}", err.0),
        }
    });
}

/// The running build's version.
#[tauri::command]
#[specta::specta]
pub async fn app_version(app: AppHandle) -> String {
    app.package_info().version.to_string()
}

/// The update a previous check found, if any; the launch check can finish before the UI
/// listens for `UpdateAvailableEvent`.
#[tauri::command]
#[specta::specta]
pub async fn update_pending(pending: State<'_, PendingUpdate>) -> AppResult<Option<UpdateInfo>> {
    Ok(lock(&pending.0).as_ref().map(UpdateInfo::from))
}

/// Checks GitHub Releases now. `None` means this build is the newest.
#[tauri::command]
#[specta::specta]
pub async fn update_check(app: AppHandle) -> AppResult<Option<UpdateInfo>> {
    check(&app).await
}

/// Downloads, verifies and installs the update the last check found, then restarts Sonder.
/// Progress arrives as `UpdateProgressEvent`. On Windows the installer closes the app itself.
#[tauri::command]
#[specta::specta]
pub async fn update_install(app: AppHandle) -> AppResult<()> {
    let Some(update) = lock(&app.state::<PendingUpdate>().0).take() else {
        return Err(AppError(
            "no update to install; check for updates first".into(),
        ));
    };
    let mut downloaded = 0u64;
    let mut mark = 0u64;
    let progress_app = app.clone();
    // `download` checks the signature before returning the bytes; music keeps playing until
    // a verified installer is in hand.
    let bytes = update
        .download(
            |chunk, total| {
                downloaded += chunk as u64;
                if downloaded >= mark {
                    mark = next_progress_mark(downloaded, total);
                    let _ = UpdateProgressEvent { downloaded, total }.emit(&progress_app);
                }
            },
            || {},
        )
        .await;
    let bytes = match bytes {
        Ok(bytes) => bytes,
        Err(err) => return Err(keep_for_retry(&app, update, "download", err)),
    };

    // The installer replaces the binary under a running engine; release the exclusive
    // device first so it is not left held. On Windows `install` exits the process once the
    // installer has started, and the installer restarts Sonder.
    app.state::<AppState>().player.stop();
    let installing = update.clone();
    let installed = tauri::async_runtime::spawn_blocking(move || installing.install(bytes)).await?;
    if let Err(err) = installed {
        return Err(keep_for_retry(&app, update, "install", err));
    }
    app.restart()
}

#[cfg(test)]
mod tests {
    use super::next_progress_mark;

    #[test]
    fn progress_is_reported_once_per_percent() {
        let total = Some(10_000);
        assert_eq!(next_progress_mark(0, total), 100);
        assert_eq!(next_progress_mark(100, total), 200);
        assert_eq!(next_progress_mark(150, total), 200);
        assert_eq!(next_progress_mark(9_999, total), 10_000);
    }

    #[test]
    fn tiny_or_unknown_lengths_still_advance() {
        assert_eq!(next_progress_mark(0, Some(50)), 1);
        assert_eq!(next_progress_mark(7, Some(50)), 8);
        assert_eq!(next_progress_mark(0, None), 512 * 1024);
        assert_eq!(next_progress_mark(0, Some(0)), 512 * 1024);
        assert_eq!(next_progress_mark(600 * 1024, None), 1024 * 1024);
    }
}
