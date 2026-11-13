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

use crate::dto::{AppResult, Device, PlayState, PlayerSnapshot, QueueItem};
use crate::loudness::LibraryLoudness;
use crate::settings::{Settings, SettingsStore};

/// Where Sonder keeps its files.
#[derive(Debug, Clone)]
pub struct AppPaths {
    pub db: PathBuf,
    pub art: PathBuf,
    pub settings: PathBuf,
    /// Cached online lyrics lookups.
    pub lyrics: PathBuf,
}

impl AppPaths {
    pub fn from_os() -> std::io::Result<Self> {
        let dirs = directories::ProjectDirs::from("", "", "Sonder")
            .ok_or_else(|| std::io::Error::other("no home directory for the app data"))?;
        Ok(AppPaths {
            db: dirs.data_dir().join("library.db"),
            art: dirs.data_dir().join("art"),
            settings: dirs.config_dir().join("settings.toml"),
            lyrics: dirs.data_dir().join("lyrics"),
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
    pub watcher: Mutex<Option<LibraryWatcher>>,
    pub scan: ScanControl,
}

impl AppState {
    /// Opens the library, loads settings and starts the player on the chosen device.
    pub fn open(paths: AppPaths) -> AppResult<(Self, Receiver<PlayerEvent>)> {
        let store = SettingsStore::new(paths.settings.clone());
        let settings = store.load();
        let library = Library::open(&paths.db, &paths.art)?;
        let loudness = Arc::new(Mutex::new(Library::open(&paths.db, &paths.art)?));

        let mut devices = DeviceList::new(backend());
        let found = devices.list().unwrap_or_default();
        let device = resolve_device(settings.output_device.as_deref(), &found);

        let mut config = PlayerConfig::new(device.clone());
        config.dsd_modes = settings.engine_dsd_modes();
        config.dsd_pcm_rate = settings.dsd_pcm_rate.into();
        config.dsp = settings.engine_dsp();
        config.src = settings.engine_src();
        config.loudness = Arc::new(LibraryLoudness::new(loudness.clone()));
        let (player, events) = Player::spawn(backend(), config)?;
        if settings.volume_db.is_some() {
            player.set_volume(settings.volume_db);
        }
        let snapshot = PlayerSnapshot {
            volume_db: settings.volume_db,
            ..PlayerSnapshot::default()
        };
        Ok((
            AppState {
                paths,
                library: Mutex::new(library),
                loudness,
                analysis: ScanControl::default(),
                player,
                queue: Mutex::new(Vec::new()),
                entry_ids: AtomicU32::new(1),
                snapshot: Mutex::new(snapshot),
                device: Mutex::new(device),
                devices: Mutex::new(devices),
                settings: Mutex::new(settings),
                store,
                watcher: Mutex::new(None),
                scan: ScanControl::default(),
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
