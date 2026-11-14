//! Lock-free sample queue between the decoder side and the render thread.

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use rtrb::{Consumer, Producer, RingBuffer};

use crate::stream::StreamHandle;

/// Feeding side: interleaved left-justified `i32` samples.
pub struct SampleProducer {
    inner: Producer<i32>,
    handle: StreamHandle,
    channels: usize,
}

/// Render side of the queue, handed to [`crate::OutputBackend::open`].
pub struct SampleSource {
    pub(crate) consumer: Consumer<i32>,
    pub(crate) handle: StreamHandle,
    pub(crate) channels: u16,
}

/// Creates a queue holding `capacity_frames` interleaved frames.
pub fn sample_ring(capacity_frames: usize, channels: u16) -> (SampleProducer, SampleSource) {
    let handle = StreamHandle::new();
    let channels = channels.max(1);
    let (inner, consumer) = RingBuffer::new(capacity_frames.max(1) * usize::from(channels));
    (
        SampleProducer {
            inner,
            handle: handle.clone(),
            channels: usize::from(channels),
        },
        SampleSource {
            consumer,
            handle,
            channels,
        },
    )
}

impl SampleSource {
    /// Handle for stopping the stream and reading its counters.
    pub fn handle(&self) -> StreamHandle {
        self.handle.clone()
    }
}

impl SampleProducer {
    /// Handle for stopping the stream and reading its counters.
    pub fn handle(&self) -> StreamHandle {
        self.handle.clone()
    }

    /// True when nothing more can be delivered: the stream stopped or the render side is gone.
    pub fn is_closed(&self) -> bool {
        self.handle.is_stopping() || self.inner.is_abandoned()
    }

    /// Queues all of `samples`, sleeping while the queue is full.
    ///
    /// Returns `false` when the stream was stopped or the render side is gone; the remaining
    /// samples are then dropped.
    pub fn push(&mut self, mut samples: &[i32]) -> bool {
        while !samples.is_empty() {
            if self.is_closed() {
                return false;
            }
            let n = self.push_some(samples);
            if n == 0 {
                std::thread::sleep(Duration::from_millis(1));
            }
            samples = &samples[n..];
        }
        true
    }

    /// Queues as many whole frames of `samples` as fit right now and returns the number of
    /// samples taken. Never blocks. A trailing partial frame is never queued.
    pub fn push_some(&mut self, samples: &[i32]) -> usize {
        let room = self.inner.slots() / self.channels * self.channels;
        let n = room.min(samples.len() / self.channels * self.channels);
        if n == 0 {
            return 0;
        }
        let (_, rest) = self.inner.push_partial_slice(&samples[..n]);
        n - rest.len()
    }

    /// Marks the end of input: the render thread plays what is queued and then the stream
    /// finishes, unless [`reopen`](Self::reopen) takes the end back first.
    pub fn finish(&mut self) {
        self.handle.0.end_input();
    }

    /// Whether [`finish`](Self::finish) was called and not taken back.
    pub fn is_finished(&self) -> bool {
        self.handle.0.input_ended()
    }

    /// Takes back a [`finish`](Self::finish), so more samples can follow without a gap.
    /// `true` when the input is open again (or never ended); `false` once the render thread
    /// has played everything and is finishing the stream, which then has to be reopened.
    pub fn reopen(&mut self) -> bool {
        self.handle.0.resume_input()
    }

    /// Discards everything queued but not yet played, so the next push is heard next.
    ///
    /// Blocks until the render thread acknowledges, which takes about one device period.
    /// Returns `false` if the render thread did not answer within `timeout` or is gone; the
    /// caller should then reopen the stream.
    pub fn flush(&mut self, timeout: Duration) -> bool {
        let shared = &self.handle.0;
        let generation = shared.flush_requested.fetch_add(1, Ordering::AcqRel) + 1;
        let started = Instant::now();
        while shared.flush_done.load(Ordering::Acquire) < generation {
            if self.is_closed() || started.elapsed() > timeout {
                return false;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        true
    }
}

impl Drop for SampleProducer {
    /// Dropping the producer marks the end of input, so a decoder that finishes, fails or
    /// panics never leaves the render thread waiting.
    fn drop(&mut self) {
        self.handle.0.end_input();
    }
}
