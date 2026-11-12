//! Events pushed to the UI, and the bridge thread that turns engine events into them.
//!
//! Position updates arrive about ten times a second while playing. The bridge folds every
//! engine event into one [`PlayerSnapshot`] and emits it only when it changed: state, track
//! and format changes go out at once, position-only changes at most every
//! [`POSITION_INTERVAL`].

use std::path::Path;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, RecvTimeoutError};
use serde::Serialize;
use sonder_engine::{PlaybackState, PlayerEvent};
use specta::Type;
use tauri::{AppHandle, Manager};
use tauri_specta::Event;

use crate::dto::{Device, PlayState, PlayerSnapshot, QueueItem};
use crate::media::{self, NowPlaying, Playback};
use crate::state::{AppState, lock};

const POSITION_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Serialize, Type, Event)]
pub struct PlayerStateEvent(pub PlayerSnapshot);

#[derive(Debug, Clone, Serialize, Type, Event)]
pub struct QueueChangedEvent(pub Vec<QueueItem>);

#[derive(Debug, Clone, Copy, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ScanPhase {
    Walking,
    Reading,
    Writing,
}

#[derive(Debug, Clone, Serialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct ScanProgressEvent {
    pub phase: ScanPhase,
    pub done: u32,
    pub total: u32,
}

#[derive(Debug, Clone, Serialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct ScanFinishedEvent {
    pub added: u32,
    pub updated: u32,
    pub removed: u32,
    pub unchanged: u32,
    /// Files and folders that could not be read, as "path: reason" (first few only).
    pub failures: Vec<String>,
    pub failure_count: u32,
    /// The scan itself failed (database error); nothing was changed.
    pub error: Option<String>,
}

/// The index changed (scan, watcher, root removed): reload what is on screen.
#[derive(Debug, Clone, Serialize, Type, Event)]
pub struct LibraryChangedEvent;

#[derive(Debug, Clone, Serialize, Type, Event)]
pub struct DevicesChangedEvent(pub Vec<Device>);

#[derive(Debug, Clone, Copy, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum NoticeLevel {
    Warning,
    Error,
}

/// Something the user should see once (a skipped track, a lost device).
#[derive(Debug, Clone, Serialize, Type, Event)]
pub struct NoticeEvent {
    pub level: NoticeLevel,
    pub message: String,
}

pub fn notice(app: &AppHandle, level: NoticeLevel, message: impl Into<String>) {
    let _ = NoticeEvent {
        level,
        message: message.into(),
    }
    .emit(app);
}

/// Applies one engine event to the snapshot. Returns whether the change is more than a
/// position tick, i.e. should be shown without throttling.
fn apply(snap: &mut PlayerSnapshot, event: &PlayerEvent) -> bool {
    match event {
        PlayerEvent::State(state) => {
            snap.state = match state {
                PlaybackState::Stopped => PlayState::Stopped,
                PlaybackState::Playing => PlayState::Playing,
                PlaybackState::Paused => PlayState::Paused,
            };
            if *state == PlaybackState::Stopped {
                // The device is released on stop.
                snap.output = None;
            }
            true
        }
        PlayerEvent::TrackChanged {
            index,
            source,
            duration,
            ..
        } => {
            snap.index = Some(*index as u32);
            snap.source = Some((*source).into());
            snap.duration_ms = duration.map(|d| d.as_millis() as u64);
            snap.elapsed_ms = 0;
            true
        }
        PlayerEvent::OutputFormat(format) => {
            snap.output = Some((*format).into());
            true
        }
        PlayerEvent::Position { index, elapsed } => {
            let index = Some(*index as u32);
            let jumped = snap.index != index;
            snap.index = index;
            snap.elapsed_ms = elapsed.as_millis() as u64;
            jumped
        }
        PlayerEvent::Underruns(n) => {
            snap.underruns = *n;
            false
        }
        PlayerEvent::EndOfQueue => {
            snap.index = None;
            snap.elapsed_ms = 0;
            snap.duration_ms = None;
            snap.source = None;
            true
        }
        _ => false,
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// Runs until the engine's event channel closes.
pub fn run_bridge(app: AppHandle, events: Receiver<PlayerEvent>) {
    let mut last_emitted: Option<PlayerSnapshot> = None;
    let mut last_emit = Instant::now() - POSITION_INTERVAL;
    let mut last_now_playing: Option<NowPlaying> = None;
    let mut urgent = false;
    loop {
        match events.recv_timeout(POSITION_INTERVAL) {
            Ok(event) => {
                urgent |= handle(&app, &event);
                // Fold everything already queued before emitting once.
                while let Ok(event) = events.try_recv() {
                    urgent |= handle(&app, &event);
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        let state = app.state::<AppState>();
        let snap = state.snapshot().clone();
        if last_emitted.as_ref() != Some(&snap)
            && (urgent || last_emit.elapsed() >= POSITION_INTERVAL)
        {
            let _ = PlayerStateEvent(snap.clone()).emit(&app);
            last_emit = Instant::now();
            urgent = false;
            last_emitted = Some(snap.clone());
        }
        let now = now_playing(&state, &snap);
        if last_now_playing
            .as_ref()
            .is_none_or(|prev| prev.differs(&now))
        {
            media::update(&app, now.clone());
            last_now_playing = Some(now);
        }
    }
}

/// Folds `event` into the snapshot and reports what needs the user's attention.
fn handle(app: &AppHandle, event: &PlayerEvent) -> bool {
    let state = app.state::<AppState>();
    let urgent = apply(&mut state.snapshot(), event);
    match event {
        PlayerEvent::TrackFailed { path, error, .. } => notice(
            app,
            NoticeLevel::Warning,
            format!("Skipped {}: {error}", file_name(path)),
        ),
        PlayerEvent::DeviceLost(reason) => notice(
            app,
            NoticeLevel::Error,
            format!("Output device unavailable ({reason}). Playback resumes when it returns."),
        ),
        PlayerEvent::DeviceBusy => notice(
            app,
            NoticeLevel::Error,
            "Another app is using the output device exclusively. Press play to retry.",
        ),
        PlayerEvent::OutputFailed(reason) => notice(
            app,
            NoticeLevel::Error,
            format!("Output failed: {reason}. Press play to retry."),
        ),
        PlayerEvent::DevicesChanged(_) => {
            // Probing formats takes a while; keep the bridge responsive.
            let app = app.clone();
            std::thread::spawn(move || {
                if let Ok(devices) = app.state::<AppState>().device_list() {
                    let _ = DevicesChangedEvent(devices).emit(&app);
                }
            });
        }
        PlayerEvent::DsdMode {
            mode,
            reason: Some(reason),
        } => notice(
            app,
            NoticeLevel::Warning,
            format!("DSD: {mode:?} ({reason})"),
        ),
        _ => {}
    }
    urgent
}

fn now_playing(state: &AppState, snap: &PlayerSnapshot) -> NowPlaying {
    let at = Duration::from_millis(snap.elapsed_ms);
    let playback = match snap.state {
        PlayState::Stopped => Playback::Stopped,
        PlayState::Playing => Playback::Playing(at),
        PlayState::Paused => Playback::Paused(at),
    };
    let queue = lock(&state.queue);
    let Some(item) = snap.index.and_then(|i| queue.get(i as usize)) else {
        return NowPlaying {
            playback,
            ..NowPlaying::default()
        };
    };
    let cover_url = item.art.as_ref().and_then(|hash| {
        let file = sonder_library::ArtCache::new(&state.paths.art).thumbnail_path(hash, 1000);
        file.is_file()
            .then(|| format!("file://{}", file.to_string_lossy().replace('\\', "/")))
    });
    NowPlaying {
        title: Some(item.title.clone()),
        artist: item.artist.clone(),
        album: Some(item.album.clone()),
        cover_url,
        duration: snap.duration_ms.map(Duration::from_millis),
        playback,
    }
}

impl NowPlaying {
    /// Whether the OS panel needs an update: metadata or play state changed, or the position
    /// moved other than by playing on (a seek). Plain progress is extrapolated by the OS.
    fn differs(&self, next: &NowPlaying) -> bool {
        let same_meta = self.title == next.title
            && self.artist == next.artist
            && self.album == next.album
            && self.cover_url == next.cover_url
            && self.duration == next.duration;
        let same_playback = match (self.playback, next.playback) {
            (Playback::Playing(a), Playback::Playing(b)) => {
                // Up to a second of drift per tick is normal playback.
                b >= a && b - a < Duration::from_secs(2)
            }
            (a, b) => a == b,
        };
        !(same_meta && same_playback)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sonder_decode::SourceFormat;
    use sonder_output::{DeviceFormat, SampleContainer};

    #[test]
    fn snapshot_follows_engine_events_and_ticks_are_not_urgent() {
        let mut snap = PlayerSnapshot::default();
        assert!(apply(
            &mut snap,
            &PlayerEvent::State(PlaybackState::Playing)
        ));
        assert!(apply(
            &mut snap,
            &PlayerEvent::TrackChanged {
                index: 2,
                path: "a.flac".into(),
                source: SourceFormat {
                    rate: 96_000,
                    bits: 24,
                    channels: 2
                },
                duration: Some(Duration::from_millis(61_500)),
            }
        ));
        assert!(apply(
            &mut snap,
            &PlayerEvent::OutputFormat(DeviceFormat {
                rate: 96_000,
                container: SampleContainer::S24In32,
                channels: 2
            })
        ));
        assert!(!apply(
            &mut snap,
            &PlayerEvent::Position {
                index: 2,
                elapsed: Duration::from_millis(1_250)
            }
        ));
        assert_eq!(snap.index, Some(2));
        assert_eq!(snap.elapsed_ms, 1_250);
        assert_eq!(snap.duration_ms, Some(61_500));
        let output = snap.output.clone().unwrap();
        assert_eq!((output.bits, output.container_bits), (24, 32));

        assert!(apply(
            &mut snap,
            &PlayerEvent::State(PlaybackState::Stopped)
        ));
        assert_eq!(snap.output, None, "stop releases the device");
        assert!(apply(&mut snap, &PlayerEvent::EndOfQueue));
        assert_eq!((snap.index, snap.source), (None, None));
    }

    #[test]
    fn os_panel_updates_on_changes_and_seeks_not_on_progress() {
        let base = NowPlaying {
            title: Some("So What".into()),
            playback: Playback::Playing(Duration::from_secs(10)),
            ..NowPlaying::default()
        };
        let tick = NowPlaying {
            playback: Playback::Playing(Duration::from_millis(10_100)),
            ..base.clone()
        };
        assert!(!base.differs(&tick));
        let seek = NowPlaying {
            playback: Playback::Playing(Duration::from_secs(90)),
            ..base.clone()
        };
        assert!(base.differs(&seek));
        let paused = NowPlaying {
            playback: Playback::Paused(Duration::from_secs(10)),
            ..base.clone()
        };
        assert!(base.differs(&paused));
    }
}
