//! Real-time fill logic shared by all backends.
//!
//! [`Renderer::fill`] turns queued samples into device-container bytes. It never allocates,
//! locks or logs: the scratch buffer is sized up front and everything else is atomics and
//! slice copies.

use std::sync::atomic::Ordering;

use crate::format::{DeviceFormat, SampleContainer, pack};
use crate::ring::SampleSource;
use crate::stream::{StreamHandle, WriteTap};

pub(crate) struct Renderer {
    source: SampleSource,
    container: SampleContainer,
    channels: usize,
    frame_bytes: usize,
    scratch: Vec<u8>,
    tap: Option<Box<dyn WriteTap>>,
}

/// Outcome of one [`Renderer::fill`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Fill {
    /// Frames of real audio at the start of the buffer; the rest is silence.
    pub payload_frames: usize,
    /// The producer finished and every queued frame has been consumed.
    pub input_exhausted: bool,
}

impl Renderer {
    pub(crate) fn new(
        source: SampleSource,
        format: DeviceFormat,
        max_frames: usize,
        tap: Option<Box<dyn WriteTap>>,
    ) -> Self {
        let frame_bytes = format.frame_bytes();
        Self {
            source,
            container: format.container,
            channels: usize::from(format.channels),
            frame_bytes,
            scratch: vec![0; max_frames * frame_bytes],
            tap,
        }
    }

    pub(crate) fn handle(&self) -> &StreamHandle {
        &self.source.handle
    }

    /// Samples currently queued.
    pub(crate) fn queued_samples(&self) -> usize {
        self.source.consumer.slots()
    }

    pub(crate) fn queue_capacity(&self) -> usize {
        self.source.consumer.buffer().capacity()
    }

    pub(crate) fn input_finished(&self) -> bool {
        self.source.handle.0.end_of_input.load(Ordering::Acquire)
    }

    /// Prepares `frames` frames: queued audio first, silence for whatever is missing.
    ///
    /// A shortfall while the producer is still running counts as an underrun.
    pub(crate) fn fill(&mut self, frames: usize) -> Fill {
        assert!(
            frames * self.frame_bytes <= self.scratch.len(),
            "fill larger than scratch"
        );
        let shared = &self.source.handle.0;

        // Read the end-of-input flag before the queue length: once the flag is seen, every
        // sample pushed before it is visible, so an empty queue really means the end.
        let finished = shared.end_of_input.load(Ordering::Acquire);
        let want = frames * self.channels;
        let take = self.source.consumer.slots().min(want) / self.channels * self.channels;

        let mut payload_bytes = 0;
        if take > 0
            && let Ok(chunk) = self.source.consumer.read_chunk(take)
        {
            let (a, b) = chunk.as_slices();
            let a_bytes = a.len() * self.container.bytes();
            pack(self.container, a, &mut self.scratch[..a_bytes]);
            let total_bytes = take * self.container.bytes();
            pack(self.container, b, &mut self.scratch[a_bytes..total_bytes]);
            chunk.commit_all();
            payload_bytes = total_bytes;
        }
        let payload_frames = payload_bytes / self.frame_bytes;

        let total_bytes = frames * self.frame_bytes;
        self.scratch[payload_bytes..total_bytes].fill(0);

        let exhausted_now = || {
            shared.end_of_input.load(Ordering::Acquire)
                && self.source.consumer.slots() < self.channels
        };
        // Re-check after the read: the producer may have pushed its last block and finished
        // between the two loads above, which is the end of the track, not an underrun.
        if payload_frames < frames && !finished && !exhausted_now() {
            shared.underruns.fetch_add(1, Ordering::Relaxed);
        }
        if payload_frames > 0 {
            shared
                .frames_written
                .fetch_add(payload_frames as u64, Ordering::Relaxed);
            if let Some(tap) = self.tap.as_mut() {
                tap.on_audio(&self.scratch[..payload_bytes]);
            }
        }

        Fill {
            payload_frames,
            input_exhausted: finished && self.source.consumer.slots() < self.channels,
        }
    }

    /// Prepares `frames` frames of silence without touching the queue or the tap.
    pub(crate) fn silence(&mut self, frames: usize) {
        assert!(
            frames * self.frame_bytes <= self.scratch.len(),
            "silence larger than scratch"
        );
        self.scratch[..frames * self.frame_bytes].fill(0);
    }

    /// The first `frames` frames prepared by [`fill`](Self::fill) or [`silence`](Self::silence).
    pub(crate) fn bytes(&self, frames: usize) -> &[u8] {
        &self.scratch[..frames * self.frame_bytes]
    }

    pub(crate) fn finish(mut self) {
        if let Some(tap) = self.tap.as_mut() {
            tap.finish();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::format::unpack;
    use crate::ring::sample_ring;

    const FORMAT: DeviceFormat = DeviceFormat {
        rate: 44_100,
        container: SampleContainer::S24Packed,
        channels: 2,
    };

    #[derive(Default)]
    struct Capture(Arc<Mutex<Vec<u8>>>);

    impl WriteTap for Capture {
        fn on_audio(&mut self, bytes: &[u8]) {
            self.0.lock().unwrap().extend_from_slice(bytes);
        }
    }

    fn samples(n: usize) -> Vec<i32> {
        (0..n as i32)
            .map(|i| (i.wrapping_mul(2_654_435) % (1 << 23)) << 8)
            .collect()
    }

    #[test]
    fn output_equals_packed_source_across_wraparound() {
        let (mut producer, source) = sample_ring(16, 2);
        let captured = Arc::new(Mutex::new(Vec::new()));
        let mut renderer =
            Renderer::new(source, FORMAT, 8, Some(Box::new(Capture(captured.clone()))));

        let input = samples(2 * 100);
        let mut fed = 0;
        let mut written = Vec::new();
        while fed < input.len() || renderer.queued_samples() > 0 {
            if fed < input.len() {
                let n = (input.len() - fed).min(14);
                // Feed only what fits so the single test thread never blocks.
                let room = renderer.queue_capacity() - renderer.queued_samples();
                let n = n.min(room) / 2 * 2;
                assert!(producer.push(&input[fed..fed + n]));
                fed += n;
            }
            let fill = renderer.fill(5);
            written.extend_from_slice(renderer.bytes(fill.payload_frames));
        }

        let mut expected = vec![0u8; input.len() * 3];
        pack(SampleContainer::S24Packed, &input, &mut expected);
        assert_eq!(written, expected);
        assert_eq!(*captured.lock().unwrap(), expected);
        assert_eq!(renderer.handle().frames_written(), 100);
        assert_eq!(renderer.handle().underruns(), 0);
    }

    #[test]
    fn shortfall_while_producing_is_an_underrun_and_pads_with_silence() {
        let (mut producer, source) = sample_ring(16, 2);
        let mut renderer = Renderer::new(source, FORMAT, 8, None);
        assert!(producer.push(&[1 << 8, 2 << 8, 3 << 8, 4 << 8]));

        let fill = renderer.fill(4);
        assert_eq!(fill.payload_frames, 2);
        assert!(!fill.input_exhausted);
        assert_eq!(renderer.handle().underruns(), 1);

        let mut head = [0i32; 4];
        unpack(
            SampleContainer::S24Packed,
            &renderer.bytes(2)[..12],
            &mut head,
        );
        assert_eq!(head, [1 << 8, 2 << 8, 3 << 8, 4 << 8]);
        assert!(renderer.bytes(4)[12..].iter().all(|&b| b == 0));
    }

    #[test]
    fn end_of_input_is_not_an_underrun() {
        let (mut producer, source) = sample_ring(16, 2);
        let mut renderer = Renderer::new(source, FORMAT, 8, None);
        let handle = renderer.handle().clone();
        assert!(producer.push(&[1 << 8, 2 << 8]));
        drop(producer);

        let fill = renderer.fill(4);
        assert_eq!(fill.payload_frames, 1);
        assert!(fill.input_exhausted);
        assert_eq!(handle.underruns(), 0);
    }

    #[test]
    fn silence_does_not_reach_the_tap_or_counters() {
        let (_producer, source) = sample_ring(16, 2);
        let captured = Arc::new(Mutex::new(Vec::new()));
        let mut renderer =
            Renderer::new(source, FORMAT, 8, Some(Box::new(Capture(captured.clone()))));
        renderer.silence(3);
        assert_eq!(renderer.bytes(3), &[0u8; 18]);
        assert!(captured.lock().unwrap().is_empty());
        assert_eq!(renderer.handle().frames_written(), 0);
    }

    #[test]
    fn push_returns_false_after_stop() {
        let (mut producer, _source) = sample_ring(4, 2);
        producer.handle().stop();
        assert!(!producer.push(&[0; 2]));
    }

    #[test]
    fn push_returns_false_when_render_side_is_gone() {
        let (mut producer, source) = sample_ring(2, 2);
        drop(source);
        assert!(!producer.push(&[0; 8]));
    }
}
