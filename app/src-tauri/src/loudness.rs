//! Library-backed levelling values for the engine, and the background loudness analysis.
//!
//! The engine asks for a track's ReplayGain or measured loudness when the track starts; this
//! adapter reads it from its own library connection, so the UI's connection stays free and the
//! engine never links against the database crate's query types.
//!
//! The analysis measures tracks without ReplayGain tags and rebuilds album values a rescan
//! cleared. It runs on its own worker thread, queued like scans: scans, the folder watcher and
//! the Settings button all ask for a pass, and at most one runs at a time.

use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use sonder_dsp::Gain;
use sonder_engine::{LoudnessSource, TrackGain};
use sonder_library::{Library, LibraryError, LoudnessProgress, LoudnessReport};
use tauri::{AppHandle, Manager};
use tauri_specta::Event;

use crate::events::{LoudnessFinishedEvent, LoudnessProgressEvent, NoticeLevel, notice};
use crate::state::{AppState, lock};

/// Starts a loudness analysis pass, or queues one if a pass is already running.
pub fn start_analysis(app: &AppHandle) {
    let state = app.state::<AppState>();
    let control = &state.analysis;
    control.pending.store(true, Ordering::SeqCst);
    if control.running.swap(true, Ordering::SeqCst) {
        return;
    }
    let worker = app.clone();
    let spawned = std::thread::Builder::new()
        .name("sonder-loudness".into())
        .spawn(move || {
            let app = worker;
            let state = app.state::<AppState>();
            let control = &state.analysis;
            loop {
                while control.pending.swap(false, Ordering::SeqCst) {
                    analyze_once(&app);
                }
                control.running.store(false, Ordering::SeqCst);
                // A request that raced with the end of the loop: take it back up.
                if !control.pending.load(Ordering::SeqCst)
                    || control.running.swap(true, Ordering::SeqCst)
                {
                    return;
                }
            }
        });
    if let Err(err) = spawned {
        control.running.store(false, Ordering::SeqCst);
        notice(
            app,
            NoticeLevel::Error,
            format!("Cannot start the loudness analysis: {err}"),
        );
    }
}

/// Runs one pass on its own connection and reports it to the UI.
fn analyze_once(app: &AppHandle) {
    let state = app.state::<AppState>();
    let progress = |p: LoudnessProgress| {
        let _ = LoudnessProgressEvent {
            done: count(p.done),
            total: count(p.total),
            failed: count(p.failed),
        }
        .emit(app);
    };
    let result = Library::open(&state.paths.db, &state.paths.art).and_then(|mut library| {
        let report = library.analyze_loudness_with_progress(&progress)?;
        Ok::<_, LibraryError>((report, library.loudness_pending()?))
    });
    let finished = match result {
        Ok((report, pending)) => finished(report, pending, None),
        Err(err) => {
            let message = format!("Loudness analysis failed: {err}");
            notice(app, NoticeLevel::Error, message.clone());
            finished(LoudnessReport::default(), 0, Some(message))
        }
    };
    let _ = finished.emit(app);
}

fn finished(
    report: LoudnessReport,
    pending: usize,
    error: Option<String>,
) -> LoudnessFinishedEvent {
    LoudnessFinishedEvent {
        tracks: count(report.tracks),
        albums: count(report.albums),
        failed: count(report.failed),
        pending: count(pending),
        error,
    }
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// Wraps a library connection so the engine can read levelling values.
pub struct LibraryLoudness {
    library: Arc<Mutex<Library>>,
}

impl LibraryLoudness {
    pub fn new(library: Arc<Mutex<Library>>) -> Self {
        Self { library }
    }
}

impl LoudnessSource for LibraryLoudness {
    fn gain(&self, path: &Path, target_lufs: f64) -> Option<TrackGain> {
        let stored = lock(&self.library).track_loudness(path).ok().flatten()?;
        let (track_gain, track_peak) = stored.track_gain(target_lufs)?;
        // Without an album value, levelling an album falls back to the track.
        let (album_gain, album_peak) = stored
            .album_gain(target_lufs)
            .unwrap_or((track_gain, track_peak));
        Some(TrackGain {
            track: Gain {
                gain_db: track_gain,
                peak: track_peak,
            },
            album: Gain {
                gain_db: album_gain,
                peak: album_peak,
            },
            album_id: Some(stored.album_id),
        })
    }
}
