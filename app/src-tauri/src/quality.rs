//! Lossless quality checks: one track on request (with its spectrogram), or a batch run one
//! track at a time on a worker thread. Results are cached in the library as a summary (verdict
//! and evidence, no charts); the UI reads them as track badges and reopens reports from them.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use sonder_library::{Library, QualityResult, QualitySubject};
use sonder_quality::{ANALYZER_VERSION, Confidence, Options, QualityError, Report, Verdict};
use specta::Type;
use tauri::{AppHandle, Manager};
use tauri_specta::Event;

use crate::dto::{AppError, AppResult};
use crate::events::{NoticeLevel, notice};
use crate::state::{AppState, lock};

/// Progress events are sent at most this often per check.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

/// The batch queue and the cancel flags.
#[derive(Default)]
pub struct QualityControl {
    batch: Mutex<Batch>,
    running: AtomicBool,
    cancel_batch: Arc<AtomicBool>,
    cancel_single: Arc<AtomicBool>,
}

#[derive(Default)]
struct Batch {
    queue: VecDeque<i64>,
    current: Option<i64>,
    done: u32,
    failed: u32,
    total: u32,
}

/// How far one check has got (single checks and the batch's current track).
#[derive(Debug, Clone, Serialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct QualityProgressEvent {
    pub track_id: i64,
    /// Decoded share of the track, 0–1.
    pub fraction: f32,
}

/// A track's check finished and its result was saved.
#[derive(Debug, Clone, Serialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct QualityCheckedEvent {
    pub track_id: i64,
    pub verdict: Verdict,
    pub confidence: Confidence,
}

/// The batch's state: sent when it starts, after every track, and when it ends.
#[derive(Debug, Clone, Default, Serialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct QualityBatchEvent {
    pub running: bool,
    pub current_track_id: Option<i64>,
    pub done: u32,
    pub failed: u32,
    pub total: u32,
    /// The batch ended because the user cancelled it.
    pub cancelled: bool,
}

impl QualityControl {
    fn status(&self, cancelled: bool) -> QualityBatchEvent {
        let batch = lock(&self.batch);
        QualityBatchEvent {
            running: self.running.load(Ordering::SeqCst),
            current_track_id: batch.current,
            done: batch.done,
            failed: batch.failed,
            total: batch.total,
            cancelled,
        }
    }
}

/// Removes results an older analyzer produced; run once at start-up.
pub fn forget_outdated(library: &mut Library) {
    if let Err(err) = library.remove_quality_before(ANALYZER_VERSION) {
        eprintln!("cannot clear outdated quality results: {err}");
    }
}

/// Checks one track with its spectrogram and saves the result.
pub async fn check_track(app: &AppHandle, track_id: i64) -> AppResult<Report> {
    let state = app.state::<AppState>();
    let subject = lock(&state.library)
        .quality_subject(track_id)?
        .ok_or_else(|| AppError("the track is no longer in the library".into()))?;
    let cancel = state.quality.cancel_single.clone();
    cancel.store(false, Ordering::SeqCst);
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let options = Options { spectrogram: true };
        let report = analyze(&app, &subject, &options, &cancel)?;
        let state = app.state::<AppState>();
        save(&app, &mut lock(&state.library), &subject, &report)?;
        Ok(report)
    })
    .await?
}

/// The cached result for a track, without charts; `None` when it was never checked,
/// the file changed, or the cache cannot be read.
pub fn cached(library: &Library, track_id: i64) -> AppResult<Option<Report>> {
    let Some(stored) = library.quality(track_id)? else {
        return Ok(None);
    };
    if stored.analyzer_version != ANALYZER_VERSION {
        return Ok(None);
    }
    Ok(serde_json::from_str(&stored.report_json).ok())
}

/// Adds tracks to the batch (tracks already queued are not added twice) and starts the
/// worker when it is idle.
pub fn queue_batch(app: &AppHandle, track_ids: Vec<i64>) {
    let state = app.state::<AppState>();
    let control = &state.quality;
    {
        let mut batch = lock(&control.batch);
        // A finished batch leaves its counts for the UI; a new one starts from zero.
        if !control.running.load(Ordering::SeqCst) && batch.queue.is_empty() {
            *batch = Batch::default();
        }
        for id in track_ids {
            if batch.current != Some(id) && !batch.queue.contains(&id) {
                batch.queue.push_back(id);
                batch.total += 1;
            }
        }
        if batch.queue.is_empty() {
            return;
        }
    }
    if control.running.swap(true, Ordering::SeqCst) {
        let _ = control.status(false).emit(app);
        return;
    }
    control.cancel_batch.store(false, Ordering::SeqCst);
    let worker = app.clone();
    let spawned = std::thread::Builder::new()
        .name("sonder-quality".into())
        .spawn(move || run_batch(&worker));
    if let Err(err) = spawned {
        control.running.store(false, Ordering::SeqCst);
        notice(
            app,
            NoticeLevel::Error,
            format!("Cannot start the quality check: {err}"),
        );
    }
}

pub fn batch_status(app: &AppHandle) -> QualityBatchEvent {
    app.state::<AppState>().quality.status(false)
}

fn run_batch(app: &AppHandle) {
    let state = app.state::<AppState>();
    let control = &state.quality;
    let library = Library::open(&state.paths.db, &state.paths.art);
    let mut library = match library {
        Ok(library) => library,
        Err(err) => {
            notice(
                app,
                NoticeLevel::Error,
                format!("Quality check failed: {err}"),
            );
            lock(&control.batch).queue.clear();
            control.running.store(false, Ordering::SeqCst);
            let _ = control.status(false).emit(app);
            return;
        }
    };
    let options = Options::default();
    let mut cancelled = false;
    loop {
        let next = {
            let mut batch = lock(&control.batch);
            if control.cancel_batch.load(Ordering::SeqCst) {
                cancelled = true;
                batch.queue.clear();
            }
            batch.current = batch.queue.pop_front();
            batch.current
        };
        let Some(track_id) = next else { break };
        let _ = control.status(false).emit(app);
        let outcome = check_one(app, &mut library, track_id, &options, &control.cancel_batch);
        let mut batch = lock(&control.batch);
        match outcome {
            Ok(()) => batch.done += 1,
            Err(QualityFailure::Cancelled) => {}
            Err(QualityFailure::Other) => {
                batch.done += 1;
                batch.failed += 1;
            }
        }
    }
    {
        // Under the lock, so a request racing with the end sees either a running worker
        // that will take its tracks or an idle one it must start.
        let _batch = lock(&control.batch);
        control.running.store(false, Ordering::SeqCst);
    }
    let _ = control.status(cancelled).emit(app);
    // Tracks queued between the last pop and the stop flag: start again for them.
    let leftover = !lock(&control.batch).queue.is_empty();
    if leftover && !cancelled {
        queue_batch(app, Vec::new());
    }
}

enum QualityFailure {
    Cancelled,
    Other,
}

/// Checks one batch track unless a valid result is cached already.
fn check_one(
    app: &AppHandle,
    library: &mut Library,
    track_id: i64,
    options: &Options,
    cancel: &AtomicBool,
) -> Result<(), QualityFailure> {
    if let Ok(Some(report)) = cached(library, track_id) {
        let _ = QualityCheckedEvent {
            track_id,
            verdict: report.verdict,
            confidence: report.confidence,
        }
        .emit(app);
        return Ok(());
    }
    let subject = match library.quality_subject(track_id) {
        Ok(Some(subject)) => subject,
        _ => return Err(QualityFailure::Other),
    };
    let report = analyze(app, &subject, options, cancel).map_err(|e| {
        if cancel.load(Ordering::SeqCst) {
            QualityFailure::Cancelled
        } else {
            eprintln!(
                "quality check of {} failed: {}",
                subject.path.display(),
                e.0
            );
            QualityFailure::Other
        }
    })?;
    save(app, library, &subject, &report).map_err(|_| QualityFailure::Other)
}

fn analyze(
    app: &AppHandle,
    subject: &QualitySubject,
    options: &Options,
    cancel: &AtomicBool,
) -> AppResult<Report> {
    let mut last = Instant::now();
    let track_id = subject.track_id;
    sonder_quality::analyze(&subject.path, options, cancel, |fraction| {
        if last.elapsed() >= PROGRESS_INTERVAL {
            last = Instant::now();
            let _ = QualityProgressEvent { track_id, fraction }.emit(app);
        }
    })
    .map_err(|e| match e {
        QualityError::Cancelled => AppError("cancelled".into()),
        e => AppError(format!("{}: {e}", subject.path.display())),
    })
}

/// Saves `report` (without its charts) for `subject` and tells the UI.
fn save(
    app: &AppHandle,
    library: &mut Library,
    subject: &QualitySubject,
    report: &Report,
) -> AppResult<()> {
    // Charts are left out: about 1 KB per track instead of ~25 KB. A report reopened from
    // the cache shows its verdict and evidence at once and redraws the charts by checking again.
    let stored = Report {
        spectrum: Vec::new(),
        cutoff_track: Vec::new(),
        spectrogram: None,
        ..report.clone()
    };
    library.save_quality(
        subject,
        &QualityResult {
            analyzer_version: report.analyzer_version,
            verdict: report.verdict.as_str().into(),
            confidence: report.confidence.as_str().into(),
            report_json: serde_json::to_string(&stored)?,
        },
    )?;
    let _ = QualityCheckedEvent {
        track_id: subject.track_id,
        verdict: report.verdict,
        confidence: report.confidence,
    }
    .emit(app);
    Ok(())
}

/// Checks one track (full report with spectrogram) and caches the result. Progress arrives as
/// `QualityProgressEvent`; `quality_cancel` stops it.
#[tauri::command]
#[specta::specta]
pub async fn quality_check_track(app: AppHandle, track_id: i64) -> AppResult<Report> {
    check_track(&app, track_id).await
}

/// The cached report for a track (no spectrogram), if it was checked and has not changed.
#[tauri::command]
#[specta::specta]
pub async fn quality_cached(
    state: tauri::State<'_, AppState>,
    track_id: i64,
) -> AppResult<Option<Report>> {
    cached(&lock(&state.library), track_id)
}

/// Queues tracks for checking one at a time. Tracks with a valid cached result are not
/// re-analysed. Progress arrives as `QualityBatchEvent`, results as `QualityCheckedEvent`.
#[tauri::command]
#[specta::specta]
pub async fn quality_check_batch(app: AppHandle, track_ids: Vec<i64>) -> AppResult<()> {
    queue_batch(&app, track_ids);
    Ok(())
}

/// Stops the single check in progress (the report being closed). A batch keeps running.
#[tauri::command]
#[specta::specta]
pub async fn quality_cancel_check(state: tauri::State<'_, AppState>) -> AppResult<()> {
    state.quality.cancel_single.store(true, Ordering::SeqCst);
    Ok(())
}

/// Stops the batch; the track being analysed ends unsaved and the rest of the queue is dropped.
#[tauri::command]
#[specta::specta]
pub async fn quality_cancel_batch(state: tauri::State<'_, AppState>) -> AppResult<()> {
    state.quality.cancel_batch.store(true, Ordering::SeqCst);
    Ok(())
}

/// Where the batch stands, for a view that opens while it runs.
#[tauri::command]
#[specta::specta]
pub async fn quality_batch_status(app: AppHandle) -> AppResult<QualityBatchEvent> {
    Ok(batch_status(&app))
}
