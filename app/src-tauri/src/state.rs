//! App state shared by commands, the engine event bridge and the scan worker.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use crossbeam_channel::Receiver;
use sonder_engine::{Player, PlayerConfig, PlayerEvent};
use sonder_library::{Library, LibraryWatcher};
use sonder_output::{DeviceFormat, DeviceId, DeviceInfo, OutputBackend};

use crate::dto::{AppResult, Device, PlayState, PlayerSnapshot, QueueItem, TrackItem};
use crate::loudness::LibraryLoudness;
use crate::session::{SessionStore, restore_index};
use crate::settings::{Settings, SettingsStore};

/// Where Sonder keeps its files.
#[derive(Debug, Clone)]
pub struct AppPaths {
    pub db: PathBuf,
    pub art: PathBuf,
    pub settings: PathBuf,
    pub session: PathBuf,
    /// The downloaded AutoEQ headphone index.
    pub autoeq: PathBuf,
    /// Cached online lyrics lookups.
    pub lyrics: PathBuf,
    /// Restricted-permission Google Drive OAuth & cache configuration.
    pub gdrive_config: PathBuf,
    /// Persisted Google Drive OAuth tokens.
    pub gdrive_tokens: PathBuf,
    /// Progressive Range chunk cache for Google Drive streams.
    pub gdrive_cache: PathBuf,
}

impl AppPaths {
    pub fn from_os() -> std::io::Result<Self> {
        let dirs = directories::ProjectDirs::from("", "", "Sonder")
            .ok_or_else(|| std::io::Error::other("no home directory for the app data"))?;
        Ok(AppPaths {
            db: dirs.data_dir().join("library.db"),
            art: dirs.data_dir().join("art"),
            settings: dirs.config_dir().join("settings.toml"),
            session: dirs.data_dir().join("session.json"),
            lyrics: dirs.data_dir().join("lyrics"),
            autoeq: dirs.data_dir().join("autoeq"),
            gdrive_config: dirs.config_dir().join("gdrive_config.json"),
            gdrive_tokens: dirs.config_dir().join("gdrive_tokens.json"),
            gdrive_cache: dirs.cache_dir().join("gdrive"),
        })
    }
}

/// The exclusive-mode backend for this OS (a fresh instance per call: the player owns one,
/// the device list another).
pub fn backend() -> Box<dyn OutputBackend> {
    #[cfg(windows)]
    return Box::new(sonder_output::wasapi::WasapiExclusive::new());
    #[cfg(target_os = "macos")]
    return Box::new(sonder_output::coreaudio::CoreAudioHog::new());
    #[cfg(target_os = "linux")]
    return Box::new(sonder_output::alsa::AlsaHw::new());
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    return Box::new(sonder_output::null::NullBackend::new());
}

/// Lists devices for the UI. Probing a device the player holds exclusively can fail, so the
/// last formats seen for each device are kept and shown instead of an empty list.
pub struct DeviceList {
    backend: Box<dyn OutputBackend>,
    known: HashMap<DeviceId, Vec<DeviceFormat>>,
}

impl DeviceList {
    pub fn new(backend: Box<dyn OutputBackend>) -> Self {
        DeviceList {
            backend,
            known: HashMap::new(),
        }
    }

    pub fn list(&mut self) -> AppResult<Vec<DeviceInfo>> {
        let mut devices = self.backend.devices()?;
        for d in &mut devices {
            if d.formats.is_empty() {
                if let Some(formats) = self.known.get(&d.id) {
                    d.formats = formats.clone();
                }
            } else {
                self.known.insert(d.id.clone(), d.formats.clone());
            }
        }
        Ok(devices)
    }
}

/// The device to open: the saved one if present, else the system default, else the first.
pub fn resolve_device(saved: Option<&str>, devices: &[DeviceInfo]) -> DeviceId {
    saved
        .and_then(|id| devices.iter().find(|d| d.id.0 == id))
        .or_else(|| devices.iter().find(|d| d.is_default))
        .or_else(|| devices.first())
        .map(|d| d.id.clone())
        // Nothing present yet: keep the saved id so a later hot-plug can still find it.
        .unwrap_or_else(|| DeviceId(saved.unwrap_or_default().to_owned()))
}

#[derive(Default)]
pub struct ScanControl {
    pub running: AtomicBool,
    /// Another scan was requested while one ran; it starts when that one ends.
    pub pending: AtomicBool,
}

pub struct AppState {
    pub paths: AppPaths,
    pub library: Mutex<Library>,
    /// A second connection for the engine's levelling lookups and the pending count, so
    /// neither waits behind the UI queries that share `library`. Analysis passes open their own.
    pub loudness: Arc<Mutex<Library>>,
    /// The loudness analysis worker: at most one pass runs, further requests queue one more.
    pub analysis: ScanControl,
    pub player: Player,
    /// Mirror of the engine queue: the engine holds paths, the UI needs track metadata. Every
    /// enqueue and clear goes through this lock so the two stay in the same order.
    pub queue: Mutex<Vec<QueueItem>>,
    /// Source of `QueueItem::entry_id`.
    entry_ids: AtomicU32,
    pub snapshot: Mutex<PlayerSnapshot>,
    pub device: Mutex<DeviceId>,
    pub devices: Mutex<DeviceList>,
    pub settings: Mutex<Settings>,
    pub store: SettingsStore,
    pub session_store: SessionStore,
    pub watcher: Mutex<Option<LibraryWatcher>>,
    pub scan: ScanControl,
    /// Quality checks: the batch queue and cancel flags.
    pub quality: crate::quality::QualityControl,
    /// The sleep timer, checked by the engine event bridge.
    pub sleep: Mutex<crate::sleep::SleepTimer>,
    /// AutoEQ headphone search; the index loads on the first search.
    pub autoeq: Arc<sonder_autoeq::AutoEq>,
    /// Google Drive OAuth, API client, and progressive chunk cache.
    pub gdrive: Arc<crate::gdrive::GdriveService>,
}

impl AppState {
    /// Opens the library, loads settings and starts the player on the chosen device.
    pub fn open(paths: AppPaths) -> AppResult<(Self, Receiver<PlayerEvent>)> {
        let store = SettingsStore::new(paths.settings.clone());
        let mut settings = store.load();
        let mut library = Library::open(&paths.db, &paths.art)?;
        if !settings.onboarding_completed {
            let has_local = library.roots().map(|r| !r.is_empty()).unwrap_or(false);
            let has_gdrive = library.gdrive_roots().map(|r| !r.is_empty()).unwrap_or(false);
            if settings.migrate_onboarding_for_existing_user(has_local || has_gdrive) {
                let _ = store.save(&settings);
            }
        }
        crate::quality::forget_outdated(&mut library);
        let autoeq = Arc::new(sonder_autoeq::AutoEq::new(
            &paths.autoeq,
            crate::commands::USER_AGENT,
        ));
        let loudness = Arc::new(Mutex::new(Library::open(&paths.db, &paths.art)?));
        let gdrive = Arc::new(crate::gdrive::GdriveService::new(
            paths.gdrive_config.clone(),
            paths.gdrive_tokens.clone(),
            paths.gdrive_cache.clone(),
        ));

        let mut devices = DeviceList::new(backend());
        let found = devices.list().unwrap_or_default();
        let device = resolve_device(settings.output_device.as_deref(), &found);

        let mut config = PlayerConfig::new(device.clone());
        config.share_mode = settings.output_mode.into();
        config.dsd_modes = settings.engine_dsd_modes();
        config.dsd_pcm_rate = settings.dsd_pcm_rate.into();
        config.sdm = settings.engine_sdm();
        config.dsp = settings.engine_dsp();
        config.src = settings.engine_src();
        config.crossfade = settings.engine_crossfade();
        config.loudness = Arc::new(LibraryLoudness::new(loudness.clone()));
        config.source_opener = Arc::new(crate::gdrive::HybridSourceOpener::new(
            gdrive.clone(),
            loudness.clone(),
        ));
        let (player, events) = Player::spawn(backend(), config)?;
        if settings.volume_db.is_some() {
            player.set_volume(settings.volume_db);
        }

        let session_store = SessionStore::new(paths.session.clone());
        let entry_ids = AtomicU32::new(1);
        let mut restored_queue = Vec::new();
        let mut snapshot = PlayerSnapshot {
            volume_db: settings.volume_db,
            ..PlayerSnapshot::default()
        };
        if let Some(saved) = session_store.load() {
            snapshot.repeat = saved.repeat;
            player.set_repeat(saved.repeat.into());
            let mut restored_paths = Vec::new();
            for raw in &saved.paths {
                let path = PathBuf::from(raw);
                let is_remote = sonder_gdrive::is_gdrive_path(&path);
                if !is_remote && !path.is_file() {
                    continue;
                }
                let entry_id = entry_ids.fetch_add(1, Ordering::Relaxed);
                let item = match library.track_by_path(&path).ok().flatten() {
                    Some(track) => QueueItem::new(entry_id, &TrackItem::from(track)),
                    None if is_remote => continue,
                    None => QueueItem::from_external(entry_id, &path),
                };
                restored_queue.push(item);
                restored_paths.push(path);
            }
            if !restored_paths.is_empty() {
                player.enqueue(restored_paths.clone());
                if let Some(idx) = restore_index(&saved.paths, &restored_paths, saved.index) {
                    let same_track =
                        saved
                            .index
                            .and_then(|i| saved.paths.get(i))
                            .is_some_and(|old| {
                                restored_paths[idx].as_os_str() == std::ffi::OsStr::new(old)
                            });
                    let dur = restored_queue[idx].duration_ms;
                    let elapsed_ms = if !same_track {
                        0
                    } else if dur > 0 {
                        saved.elapsed_ms.min(dur)
                    } else {
                        saved.elapsed_ms
                    };
                    player.select(idx, Duration::from_millis(elapsed_ms));
                    snapshot.index = Some(idx as u32);
                    snapshot.elapsed_ms = elapsed_ms;
                    snapshot.duration_ms = (dur > 0).then_some(dur);
                }
            }
        }
        Ok((
            AppState {
                paths,
                library: Mutex::new(library),
                loudness,
                analysis: ScanControl::default(),
                player,
                queue: Mutex::new(restored_queue),
                entry_ids,
                snapshot: Mutex::new(snapshot),
                device: Mutex::new(device),
                devices: Mutex::new(devices),
                settings: Mutex::new(settings),
                store,
                session_store,
                watcher: Mutex::new(None),
                scan: ScanControl::default(),
                quality: crate::quality::QualityControl::default(),
                sleep: Mutex::default(),
                autoeq,
                gdrive,
            },
            events,
        ))
    }

    /// A queue entry id never handed out before in this run.
    pub fn next_entry_id(&self) -> u32 {
        self.entry_ids.fetch_add(1, Ordering::Relaxed)
    }

    pub fn snapshot(&self) -> MutexGuard<'_, PlayerSnapshot> {
        lock(&self.snapshot)
    }

    pub fn toggle(&self) {
        if self.snapshot().state == PlayState::Playing {
            self.player.pause();
        } else {
            self.player.play();
        }
    }

    pub fn seek_by(&self, direction: souvlaki::SeekDirection, by: Duration) {
        let elapsed = Duration::from_millis(self.snapshot().elapsed_ms);
        self.player
            .seek(crate::media::seek_target(elapsed, direction, by));
    }

    pub fn device_list(&self) -> AppResult<Vec<Device>> {
        let selected = lock(&self.device).clone();
        let infos = lock(&self.devices).list()?;
        Ok(infos
            .into_iter()
            .map(|d| {
                let is_selected = d.id == selected;
                Device::new(d, is_selected)
            })
            .collect())
    }

    pub fn update_settings(&self, change: impl FnOnce(&mut Settings)) -> AppResult<Settings> {
        let mut settings = lock(&self.settings);
        let mut next = settings.clone();
        change(&mut next);
        let next = next.sanitized();
        if next != *settings {
            self.store.save(&next)?;
            *settings = next;
        }
        Ok(settings.clone())
    }
}

/// Locks `m`, recovering the data if a thread panicked while holding it: every guarded value
/// here stays valid between statements.
pub fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(id: &str, is_default: bool) -> DeviceInfo {
        DeviceInfo {
            id: DeviceId(id.into()),
            name: id.into(),
            is_default,
            formats: Vec::new(),
        }
    }

    #[test]
    fn device_choice_prefers_saved_then_default_then_first() {
        let devices = [info("a", false), info("b", true), info("c", false)];
        assert_eq!(resolve_device(Some("c"), &devices).0, "c");
        assert_eq!(resolve_device(Some("gone"), &devices).0, "b");
        assert_eq!(resolve_device(None, &devices).0, "b");
        assert_eq!(resolve_device(None, &devices[..1]).0, "a");
        assert_eq!(resolve_device(Some("dac"), &[]).0, "dac");
    }
}
