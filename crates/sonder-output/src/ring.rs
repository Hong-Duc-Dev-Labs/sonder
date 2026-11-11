//! Lock-free sample queue between the decoder side and the render thread.

use std::time::Duration;

use rtrb::{Consumer, Producer, RingBuffer};

use crate::stream::StreamHandle;

/// Feeding side: interleaved left-justified `i32` samples.
pub struct SampleProducer {
    inner: Producer<i32>,
    handle: StreamHandle,
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
    let (inner, consumer) = RingBuffer::new(capacity_frames.max(1) * usize::from(channels));
    (
        SampleProducer {
            inner,
            handle: handle.clone(),
        },
        SampleSource {
            consumer,
            handle,
            channels,
        },
    )
}

impl SampleProducer {
    /// Handle for stopping the stream and reading its counters.
    pub fn handle(&self) -> StreamHandle {
        self.handle.clone()
    }

    /// Queues all of `samples`, sleeping while the queue is full.
    ///
    /// Returns `false` when the stream was stopped or the render side is gone; the remaining
    /// samples are then dropped.
    pub fn push(&mut self, mut samples: &[i32]) -> bool {
        while !samples.is_empty() {
            if self.handle.is_stopping() || self.inner.is_abandoned() {
                return false;
            }
            let (_, rest) = self.inner.push_partial_slice(samples);
            if rest.len() == samples.len() {
                std::thread::sleep(Duration::from_millis(1));
            }
            samples = rest;
        }
        true
    }
}

impl Drop for SampleProducer {
    /// Dropping the producer marks the end of input, so a decoder that finishes, fails or
    /// panics never leaves the render thread waiting.
    fn drop(&mut self) {
        self.handle
            .0
            .end_of_input
            .store(true, std::sync::atomic::Ordering::Release);
    }
}
