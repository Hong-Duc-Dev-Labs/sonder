//! Control and reporting types shared between the feeding thread and the render thread.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::JoinHandle;

use crate::error::OutputError;
use crate::format::DeviceFormat;

#[derive(Default)]
pub(crate) struct StreamShared {
    pub(crate) end_of_input: AtomicBool,
    pub(crate) stop: AtomicBool,
    pub(crate) paused: AtomicBool,
    /// Flush generation requested by the producer; the render thread discards everything
    /// queued and stores the generation in `flush_done` to acknowledge.
    pub(crate) flush_requested: AtomicU64,
    pub(crate) flush_done: AtomicU64,
    pub(crate) underruns: AtomicU64,
    pub(crate) frames_written: AtomicU64,
    pub(crate) frames_discarded: AtomicU64,
}

/// Cheap, cloneable view of a running (or about to run) stream.
#[derive(Clone)]
pub struct StreamHandle(pub(crate) Arc<StreamShared>);

impl StreamHandle {
    pub(crate) fn new() -> Self {
        Self(Arc::new(StreamShared::default()))
    }

    /// Asks the render thread to stop as soon as possible.
    pub fn stop(&self) {
        self.0.stop.store(true, Ordering::Release);
    }

    pub fn is_stopping(&self) -> bool {
        self.0.stop.load(Ordering::Acquire)
    }

    /// Keeps the device open but plays silence without consuming queued audio.
    pub fn set_paused(&self, paused: bool) {
        self.0.paused.store(paused, Ordering::Release);
    }

    pub fn is_paused(&self) -> bool {
        self.0.paused.load(Ordering::Acquire)
    }

    /// Times the queue ran dry while the producer had not finished, plus device xruns the
    /// backend recovered from in place.
    pub fn underruns(&self) -> u64 {
        self.0.underruns.load(Ordering::Relaxed)
    }

    /// Audio frames (not silence) handed to the device so far.
    pub fn frames_written(&self) -> u64 {
        self.0.frames_written.load(Ordering::Relaxed)
    }

    /// Frames taken off the queue: written to the device or discarded by a flush.
    ///
    /// This counts in the same index space as the frames pushed by the producer, so the
    /// feeding side can tell which of its frames the device has reached.
    pub fn frames_consumed(&self) -> u64 {
        // Read the discard count first: a flush adds to it before the frames it removed could
        // ever have been counted as written, so this never over-reports.
        let discarded = self.0.frames_discarded.load(Ordering::Acquire);
        discarded + self.frames_written()
    }
}

/// Observes the exact bytes of real audio written into the device buffer.
///
/// Called on the render thread: implementations must not allocate or block in
/// [`WriteTap::on_audio`]. [`WriteTap::finish`] runs once after the stream ends and may do both.
pub trait WriteTap: Send {
    fn on_audio(&mut self, bytes: &[u8]);
    fn finish(&mut self) {}
}

#[derive(Default)]
pub struct StreamOptions {
    pub tap: Option<Box<dyn WriteTap>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamReport {
    pub frames_written: u64,
    pub underruns: u64,
}

/// A running output stream. Dropping it stops playback and joins the render thread.
pub struct Stream {
    handle: StreamHandle,
    format: DeviceFormat,
    thread: Option<JoinHandle<Result<StreamReport, OutputError>>>,
}

impl Stream {
    pub(crate) fn new(
        handle: StreamHandle,
        format: DeviceFormat,
        thread: JoinHandle<Result<StreamReport, OutputError>>,
    ) -> Self {
        Self {
            handle,
            format,
            thread: Some(thread),
        }
    }

    pub fn handle(&self) -> StreamHandle {
        self.handle.clone()
    }

    /// The device format the stream was opened with.
    pub fn format(&self) -> DeviceFormat {
        self.format
    }

    /// True once the render thread has exited (end of input, stop, or failure).
    pub fn is_finished(&self) -> bool {
        self.thread.as_ref().is_none_or(JoinHandle::is_finished)
    }

    /// Blocks until playback ends and returns the final report.
    pub fn wait(mut self) -> Result<StreamReport, OutputError> {
        self.join()
    }

    /// Stops playback and returns the final report.
    pub fn stop(mut self) -> Result<StreamReport, OutputError> {
        self.handle.stop();
        self.join()
    }

    fn join(&mut self) -> Result<StreamReport, OutputError> {
        match self.thread.take() {
            Some(thread) => thread
                .join()
                .unwrap_or_else(|_| Err(OutputError::Backend("render thread panicked".into()))),
            None => Err(OutputError::Backend("stream already finished".into())),
        }
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        if self.thread.is_some() {
            self.handle.stop();
            let _ = self.join();
        }
    }
}
