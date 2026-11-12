//! Library scans on a worker thread with its own database connection, so browsing stays
//! responsive while a scan runs, plus the file watcher that keeps the index current.

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use std::sync::Mutex;

use sonder_library::{Library, LibraryWatcher, ScanProgress, WatchEvent};
use tauri::{AppHandle, Manager};
use tauri_specta::Event;

use crate::events::{
    LibraryChangedEvent, NoticeLevel, ScanFinishedEvent, ScanPhase, ScanProgressEvent, notice,
};
use crate::state::{AppState, lock};

/// Most progress events per second during the read phase.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);
const MAX_LISTED_FAILURES: usize = 20;

/// Starts a full rescan, or queues one if a scan is already running.
pub fn start(app: &AppHandle) {
    let state = app.state::<AppState>();
    state.scan.pending.store(true, Ordering::SeqCst);
    if state.scan.running.swap(true, Ordering::SeqCst) {
        return;
    }
    let worker = app.clone();
    let spawned = std::thread::Builder::new()
        .name("sonder-scan".into())
        .spawn(move || {
            let app = worker;
            let state = app.state::<AppState>();
            loop {
                while state.scan.pending.swap(false, Ordering::SeqCst) {
                    scan_once(&app);
                }
                state.scan.running.store(false, Ordering::SeqCst);
                // A request that raced with the end of the loop: take it back up.
                if !state.scan.pending.load(Ordering::SeqCst)
                    || state.scan.running.swap(true, Ordering::SeqCst)
                {
                    return;
                }
            }
        });
    if let Err(err) = spawned {
        state.scan.running.store(false, Ordering::SeqCst);
        notice(
            app,
            NoticeLevel::Error,
            format!("Cannot start a scan: {err}"),
        );
    }
}

fn scan_once(app: &AppHandle) {
    let state = app.state::<AppState>();
    let mut library = match Library::open(&state.paths.db, &state.paths.art) {
        Ok(l) => l,
        Err(err) => return finished_with_error(app, err.to_string()),
    };
    let last = Mutex::new(Instant::now() - PROGRESS_INTERVAL);
    let progress = |p: ScanProgress| {
        let (phase, done, total) = match p {
            ScanProgress::Walking => (ScanPhase::Walking, 0, 0),
            ScanProgress::Reading { done, total } => (ScanPhase::Reading, done, total),
            ScanProgress::Writing => (ScanPhase::Writing, 0, 0),
        };
        let mut last = lock(&last);
        let boundary = !matches!(p, ScanProgress::Reading { .. }) || done == 0 || done == total;
        if boundary || last.elapsed() >= PROGRESS_INTERVAL {
            *last = Instant::now();
            let _ = ScanProgressEvent {
                phase,
                done: done as u32,
                total: total as u32,
            }
            .emit(app);
        }
    };
    match library.scan_with_progress(&progress) {
        Ok(report) => {
            let _ = ScanFinishedEvent {
                added: report.added as u32,
                updated: report.updated as u32,
                removed: report.removed as u32,
                unchanged: report.unchanged as u32,
                failures: report
                    .failed
                    .iter()
                    .take(MAX_LISTED_FAILURES)
                    .map(|f| format!("{}: {}", f.path.display(), f.reason))
                    .collect(),
                failure_count: report.failed.len() as u32,
                error: None,
            }
            .emit(app);
            if report.changed() {
                let _ = LibraryChangedEvent.emit(app);
            }
        }
        Err(err) => finished_with_error(app, err.to_string()),
    }
}

fn finished_with_error(app: &AppHandle, error: String) {
    let _ = ScanFinishedEvent {
        added: 0,
        updated: 0,
        removed: 0,
        unchanged: 0,
        failures: Vec::new(),
        failure_count: 0,
        error: Some(error),
    }
    .emit(app);
}

/// Works out the colours of covers indexed before colours were recorded, on a background
/// thread with its own connection, and refreshes the UI when any changed. A failure only
/// leaves the glass untinted for those covers until the next rescan.
pub fn backfill_cover_colors(app: &AppHandle) {
    let app = app.clone();
    let _ = std::thread::Builder::new()
        .name("sonder-art-colors".into())
        .spawn(move || {
            let state = app.state::<AppState>();
            let updated = Library::open(&state.paths.db, &state.paths.art)
                .and_then(|mut library| library.backfill_art_colors());
            match updated {
                Ok(0) => {}
                Ok(_) => {
                    let _ = LibraryChangedEvent.emit(&app);
                }
                Err(err) => eprintln!("sonder: cannot fill in cover colours: {err}"),
            }
        });
}

/// Starts the file watcher over every root; its rescans refresh the UI.
pub fn start_watcher(app: &AppHandle) {
    let state = app.state::<AppState>();
    let library = match Library::open(&state.paths.db, &state.paths.art) {
        Ok(l) => l,
        Err(err) => {
            return notice(
                app,
                NoticeLevel::Warning,
                format!("Folder watching is off: {err}"),
            );
        }
    };
    let (tx, rx) = crossbeam_channel::unbounded();
    match LibraryWatcher::start(library, tx) {
        Ok(watcher) => *lock(&state.watcher) = Some(watcher),
        Err(err) => {
            return notice(
                app,
                NoticeLevel::Warning,
                format!("Folder watching is off: {err}"),
            );
        }
    }
    let app = app.clone();
    let _ = std::thread::Builder::new()
        .name("sonder-watch-events".into())
        .spawn(move || {
            for event in rx {
                match event {
                    WatchEvent::Scanned(report) => {
                        if report.changed() {
                            let _ = LibraryChangedEvent.emit(&app);
                        }
                    }
                    WatchEvent::Error(err) => notice(&app, NoticeLevel::Warning, err),
                }
            }
        });
}
