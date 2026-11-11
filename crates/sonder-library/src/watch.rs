//! Keeps the index current while the app runs.
//!
//! File-system events from every root are debounced, coalesced and handed to
//! [`Library::scan_paths`] on a worker thread that owns its own [`Library`] connection.

use std::path::{Path, PathBuf};
use std::thread::JoinHandle;
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender, select};
use notify_debouncer_full::notify::{EventKind, RecommendedWatcher, RecursiveMode};
use notify_debouncer_full::{DebounceEventResult, Debouncer, RecommendedCache, new_debouncer};

use crate::{Library, LibraryError, ScanReport};

/// Quiet period after the last event before a rescan; long enough for a file copy to finish
/// writing in most cases (a file caught mid-copy is picked up by the next event).
const DEBOUNCE: Duration = Duration::from_secs(2);

#[derive(Debug, Clone)]
pub enum WatchEvent {
    /// A rescan triggered by file changes finished and changed the index or hit failures.
    Scanned(ScanReport),
    /// The watcher or a rescan failed; watching continues.
    Error(String),
}

/// Watches every library root and rescans what changes. Dropping it stops watching.
pub struct LibraryWatcher {
    debouncer: Option<Debouncer<RecommendedWatcher, RecommendedCache>>,
    stop: Option<Sender<()>>,
    worker: Option<JoinHandle<()>>,
}

impl LibraryWatcher {
    /// Starts watching the roots of `library`, which the worker thread takes over. Results
    /// go to `events`. A root that cannot be watched (for example, an unplugged drive) is
    /// reported on `events` and skipped.
    pub fn start(library: Library, events: Sender<WatchEvent>) -> Result<Self, LibraryError> {
        let roots = library.roots()?;
        let (paths_tx, paths_rx) = crossbeam_channel::unbounded::<Vec<PathBuf>>();
        let errors = events.clone();
        let mut debouncer =
            new_debouncer(
                DEBOUNCE,
                None,
                move |result: DebounceEventResult| match result {
                    Ok(batch) => {
                        let paths: Vec<PathBuf> = batch
                            .into_iter()
                            .filter(|e| !matches!(e.kind, EventKind::Access(_)))
                            .flat_map(|e| e.event.paths)
                            .collect();
                        if !paths.is_empty() {
                            let _ = paths_tx.send(paths);
                        }
                    }
                    Err(errs) => {
                        for err in errs {
                            let _ = errors.send(WatchEvent::Error(err.to_string()));
                        }
                    }
                },
            )?;
        for root in &roots {
            if let Err(err) = debouncer.watch(&root.path, RecursiveMode::Recursive) {
                let _ = events.send(WatchEvent::Error(format!(
                    "cannot watch {}: {err}",
                    root.path.display()
                )));
            }
        }

        let (stop_tx, stop_rx) = crossbeam_channel::bounded::<()>(1);
        let worker = std::thread::Builder::new()
            .name("sonder-library-watch".into())
            .spawn(move || run(library, paths_rx, stop_rx, events))?;
        Ok(LibraryWatcher {
            debouncer: Some(debouncer),
            stop: Some(stop_tx),
            worker: Some(worker),
        })
    }

    /// Starts watching a root added after [`start`](Self::start).
    pub fn watch_root(&mut self, path: &Path) -> Result<(), LibraryError> {
        if let Some(d) = self.debouncer.as_mut() {
            d.watch(path, RecursiveMode::Recursive)?;
        }
        Ok(())
    }

    /// Stops watching a removed root.
    pub fn unwatch_root(&mut self, path: &Path) -> Result<(), LibraryError> {
        if let Some(d) = self.debouncer.as_mut() {
            d.unwatch(path)?;
        }
        Ok(())
    }
}

impl Drop for LibraryWatcher {
    fn drop(&mut self) {
        if let Some(d) = self.debouncer.take() {
            d.stop();
        }
        drop(self.stop.take());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn run(
    mut library: Library,
    paths: Receiver<Vec<PathBuf>>,
    stop: Receiver<()>,
    events: Sender<WatchEvent>,
) {
    loop {
        let mut batch = select! {
            recv(paths) -> msg => match msg {
                Ok(p) => p,
                Err(_) => return,
            },
            recv(stop) -> _ => return,
        };
        // Coalesce everything that queued up during the previous rescan.
        while let Ok(more) = paths.try_recv() {
            batch.extend(more);
        }
        let mut batch = to_library_paths(&library, batch);
        batch.sort();
        batch.dedup();
        let event = match library.scan_paths(&batch) {
            Ok(report) if report.changed() || !report.failed.is_empty() => {
                WatchEvent::Scanned(report)
            }
            Ok(_) => continue,
            Err(err) => WatchEvent::Error(err.to_string()),
        };
        if events.send(event).is_err() {
            return;
        }
    }
}

/// Rewrites event paths into the form the roots were added with. Some platforms report the
/// resolved path (macOS FSEvents gives `/private/var/...` for a root added as `/var/...`), and
/// [`Library::scan_paths`] ignores paths outside every root.
fn to_library_paths(library: &Library, paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let roots = library.roots().unwrap_or_default();
    let aliases: Vec<(PathBuf, &Path)> = roots
        .iter()
        .filter_map(|r| {
            let real = std::fs::canonicalize(&r.path).ok()?;
            (real != r.path).then_some((real, r.path.as_path()))
        })
        .collect();
    paths
        .into_iter()
        .map(|path| {
            if roots.iter().any(|r| path.starts_with(&r.path)) {
                return path;
            }
            aliases
                .iter()
                .find_map(|(real, root)| path.strip_prefix(real).ok().map(|rest| root.join(rest)))
                .unwrap_or(path)
        })
        .collect()
}
