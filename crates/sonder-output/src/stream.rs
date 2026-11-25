//! Control and reporting types shared between the feeding thread and the render thread.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::JoinHandle;

use crate::error::OutputError;
use crate::format::DeviceFormat;

/// Where the input of a stream is, in the low two bits of [`StreamShared::input`]; the rest
/// counts how often the producer took an end back, so the render thread can tell an end it
/// saw from a later one.
const INPUT_OPEN: u64 = 0;
const INPUT_ENDING: u64 = 1;
const INPUT_EXHAUSTED: u64 = 2;
const INPUT_FLAG: u64 = 3;

/// Whether an input state read from [`StreamShared::input`] has ended.
pub(crate) fn input_ended(state: u64) -> bool {
    state & INPUT_FLAG != INPUT_OPEN
}

#[derive(Default)]
pub(crate) struct StreamShared {
    /// Input state: open, ending (the producer has pushed its last sample) or exhausted (the
    /// render thread played everything after an end and stops). See [`INPUT_FLAG`].
    pub(crate) input: AtomicU64,
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

impl StreamShared {
    /// The input state, for [`input_ended`](Self::input_ended) and
    /// [`exhaust_input`](Self::exhaust_input). Acquire: every sample pushed before an end is
    /// visible once the end is.
    pub(crate) fn input(&self) -> u64 {
        self.input.load(Ordering::Acquire)
    }

    /// The producer has ended its input (it may still take the end back).
    pub(crate) fn input_ended(&self) -> bool {
        input_ended(self.input())
    }

    /// Producer side: no more samples follow the ones already pushed.
    pub(crate) fn end_input(&self) {
        let _ = self
            .input
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |s| {
                (s & INPUT_FLAG == INPUT_OPEN).then_some(s | INPUT_ENDING)
            });
    }

    /// Producer side: takes an end back so more samples can follow. `false` once the render
    /// thread has acted on the end: the stream is finishing and the samples would be lost.
    pub(crate) fn resume_input(&self) -> bool {
        self.input
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |s| {
                match s & INPUT_FLAG {
                    INPUT_ENDING => Some(((s >> 2) + 1) << 2),
                    _ => None,
                }
            })
            .map_or_else(|s| s & INPUT_FLAG == INPUT_OPEN, |_| true)
    }

    /// Render side: commits to the end read as `seen`, with the queue found empty after it.
    /// `false` when the producer took that end back meanwhile, even if it has ended again
    /// since: samples may have arrived after the queue was read.
    pub(crate) fn exhaust_input(&self, seen: u64) -> bool {
        match seen & INPUT_FLAG {
            INPUT_EXHAUSTED => true,
            INPUT_ENDING => self
                .input
                .compare_exchange(
                    seen,
                    (seen & !INPUT_FLAG) | INPUT_EXHAUSTED,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .is_ok(),
            _ => false,
        }
    }
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

    /// Frames a flush removed from the queue unplayed, in the producer's index space.
    pub fn frames_discarded(&self) -> u64 {
        self.0.frames_discarded.load(Ordering::Acquire)
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
    /// The ring carries DoP payload frames: the renderer writes the 0x05/0xFA markers and
    /// uses DoP idle frames for pauses, underruns and drain. Never set for PCM streams.
    pub dop: bool,
    /// Exclusive hardware access (default) or shared OS audio mixer.
    pub share_mode: crate::ShareMode,
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
