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
    silence: Silence,
    /// Next DoP marker (0x05 or 0xFA), shared by payload and idle frames so the alternation
    /// never breaks across an underrun, pause or drain.
    marker: u8,
}

/// What the device must hear when there is no queued audio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Silence {
    /// PCM: zero samples.
    Zero,
    /// DoP payload frames: marker byte in the top 8 bits, 0x69 0x69 DSD silence below.
    Dop,
    /// Native DSD: 0x69 (alternating bits) in every byte.
    Dsd,
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
        dop: bool,
    ) -> Self {
        let frame_bytes = format.frame_bytes();
        let silence = if dop {
            assert!(
                matches!(
                    format.container,
                    SampleContainer::S24In32 | SampleContainer::S32
                ),
                "DoP needs a 24-in-32 or 32-bit container"
            );
            Silence::Dop
        } else if format.container == SampleContainer::DsdU32Be {
            Silence::Dsd
        } else {
            Silence::Zero
        };
        Self {
            source,
            container: format.container,
            channels: usize::from(format.channels),
            frame_bytes,
            scratch: vec![0; max_frames * frame_bytes],
            tap,
            silence,
            marker: 0x05,
        }
    }

    /// Whether this stream carries DSD (DoP or native), which needs DSD idle frames.
    pub(crate) fn is_dsd(&self) -> bool {
        self.silence != Silence::Zero
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

    /// Discards everything queued if the producer asked for a flush since the last call.
    ///
    /// Called once per device period by [`fill`](Self::fill); backends that wait before their
    /// first period (prebuffering) call it directly so a flush is never left hanging.
    pub(crate) fn service_flush(&mut self) {
        let shared = &self.source.handle.0;
        let requested = shared.flush_requested.load(Ordering::Acquire);
        if requested == shared.flush_done.load(Ordering::Relaxed) {
            return;
        }
        let queued = self.source.consumer.slots();
        if let Ok(chunk) = self.source.consumer.read_chunk(queued) {
            chunk.commit_all();
        }
        shared
            .frames_discarded
            .fetch_add((queued / self.channels) as u64, Ordering::AcqRel);
        shared.flush_done.store(requested, Ordering::Release);
    }

    /// Records a device xrun the backend recovered from without reopening.
    pub(crate) fn note_xrun(&self) {
        self.source
            .handle
            .0
            .underruns
            .fetch_add(1, Ordering::Relaxed);
    }

    /// Prepares `frames` frames: queued audio first, silence for whatever is missing.
    ///
    /// A shortfall while the producer is still running counts as an underrun. While paused,
    /// the whole buffer is silence and the queue is left untouched.
    pub(crate) fn fill(&mut self, frames: usize) -> Fill {
        assert!(
            frames * self.frame_bytes <= self.scratch.len(),
            "fill larger than scratch"
        );
        self.service_flush();
        let shared = &self.source.handle.0;
        if shared.paused.load(Ordering::Acquire) {
            fill_idle(
                self.silence,
                self.channels,
                &mut self.marker,
                &mut self.scratch[..frames * self.frame_bytes],
            );
            return Fill {
                payload_frames: 0,
                input_exhausted: false,
            };
        }

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
            let total_bytes = take * self.container.bytes();
            if self.silence == Silence::Dop {
                // Ring positions stay frame-aligned, so each slice holds whole frames.
                debug_assert_eq!(a.len() % self.channels, 0);
                debug_assert_eq!(b.len() % self.channels, 0);
                pack_dop(
                    self.channels,
                    &mut self.marker,
                    a,
                    &mut self.scratch[..a_bytes],
                );
                pack_dop(
                    self.channels,
                    &mut self.marker,
                    b,
                    &mut self.scratch[a_bytes..total_bytes],
                );
            } else {
                pack(self.container, a, &mut self.scratch[..a_bytes]);
                pack(self.container, b, &mut self.scratch[a_bytes..total_bytes]);
            }
            chunk.commit_all();
            payload_bytes = total_bytes;
        }
        let payload_frames = payload_bytes / self.frame_bytes;

        let total_bytes = frames * self.frame_bytes;
        fill_idle(
            self.silence,
            self.channels,
            &mut self.marker,
            &mut self.scratch[payload_bytes..total_bytes],
        );

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

    /// Prepares `frames` frames of idle audio without touching the queue or the tap.
    ///
    /// PCM writes zero samples; DoP writes 0x69 0x69 with the alternating marker (continuing
    /// the packer's phase); native DSD writes 0x69 bytes. Every backend uses this for pause,
    /// underrun padding and drain, so no DoP stream ever contains a zero frame.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub(crate) fn silence(&mut self, frames: usize) {
        assert!(
            frames * self.frame_bytes <= self.scratch.len(),
            "silence larger than scratch"
        );
        fill_idle(
            self.silence,
            self.channels,
            &mut self.marker,
            &mut self.scratch[..frames * self.frame_bytes],
        );
    }

    /// Writes idle audio straight into `dst`, laid out as `channels` 4-byte samples per frame,
    /// continuing the DoP marker phase. For device buffers whose layout differs from the
    /// stream's (CoreAudio reporting another channel count), where zeros would drop a DAC
    /// out of DSD mode.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub(crate) fn idle_into(&mut self, channels: usize, dst: &mut [u8]) {
        fill_idle(self.silence, channels.max(1), &mut self.marker, dst);
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

fn toggle(marker: u8) -> u8 {
    if marker == 0x05 { 0xFA } else { 0x05 }
}

/// Writes idle audio into `dst`, continuing the marker phase.
fn fill_idle(silence: Silence, channels: usize, marker: &mut u8, dst: &mut [u8]) {
    match silence {
        Silence::Zero => dst.fill(0),
        Silence::Dsd => dst.fill(0x69),
        Silence::Dop => {
            let word = |marker: u8| ((u32::from(marker) << 24) | 0x0069_6900).to_le_bytes();
            let mut frames = dst.chunks_exact_mut(4 * channels);
            for frame in &mut frames {
                let bytes = word(*marker);
                for sample in frame.as_chunks_mut::<4>().0 {
                    sample.copy_from_slice(&bytes);
                }
                *marker = toggle(*marker);
            }
            // A buffer that ends mid-frame (only a foreign device layout can) still carries
            // idle words, never zeros; the partial frame does not advance the marker.
            let rest = frames.into_remainder();
            let bytes = word(*marker);
            for sample in rest.as_chunks_mut::<4>().0 {
                sample.copy_from_slice(&bytes);
            }
        }
    }
}

/// Writes DoP words: the packed payload frames with the top marker byte inserted.
fn pack_dop(channels: usize, marker: &mut u8, src: &[i32], dst: &mut [u8]) {
    let words = dst.as_chunks_mut::<4>().0;
    let mut word_at = 0;
    for frame in src.chunks_exact(channels) {
        let bits = u32::from(*marker) << 24;
        for &sample in frame {
            let word = (sample as u32 & 0xFFFF_FF00) | bits;
            words[word_at] = word.to_le_bytes();
            word_at += 1;
        }
        *marker = toggle(*marker);
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
        let mut renderer = Renderer::new(
            source,
            FORMAT,
            8,
            Some(Box::new(Capture(captured.clone()))),
            false,
        );

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
        let mut renderer = Renderer::new(source, FORMAT, 8, None, false);
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
        let mut renderer = Renderer::new(source, FORMAT, 8, None, false);
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
        let mut renderer = Renderer::new(
            source,
            FORMAT,
            8,
            Some(Box::new(Capture(captured.clone()))),
            false,
        );
        renderer.silence(3);
        assert_eq!(renderer.bytes(3), &[0u8; 18]);
        assert!(captured.lock().unwrap().is_empty());
        assert_eq!(renderer.handle().frames_written(), 0);
    }

    #[test]
    fn pause_plays_silence_and_keeps_the_queue() {
        let (mut producer, source) = sample_ring(16, 2);
        let mut renderer = Renderer::new(source, FORMAT, 8, None, false);
        assert!(producer.push(&[1 << 8, 2 << 8, 3 << 8, 4 << 8]));
        renderer.handle().set_paused(true);

        let fill = renderer.fill(4);
        assert_eq!(fill.payload_frames, 0);
        assert!(renderer.bytes(4).iter().all(|&b| b == 0));
        assert_eq!(renderer.queued_samples(), 4);
        assert_eq!(renderer.handle().underruns(), 0);

        renderer.handle().set_paused(false);
        assert_eq!(renderer.fill(2).payload_frames, 2);
    }

    #[test]
    fn flush_discards_queued_frames_and_counts_them_as_consumed() {
        let (mut producer, source) = sample_ring(16, 2);
        let handle = producer.handle();
        let mut renderer = Renderer::new(source, FORMAT, 8, None, false);
        assert!(producer.push(&[1 << 8; 12]));
        assert_eq!(renderer.fill(2).payload_frames, 2);

        // Pause first so the flush, not playback, empties the queue.
        handle.set_paused(true);
        let render = std::thread::spawn(move || {
            while renderer.queued_samples() > 0 {
                renderer.fill(1);
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            renderer
        });
        assert!(producer.flush(std::time::Duration::from_secs(1)));
        let renderer = render.join().unwrap();

        assert_eq!(renderer.queued_samples(), 0);
        assert_eq!(handle.frames_written(), 2);
        assert_eq!(handle.frames_consumed(), 6);
    }

    #[test]
    fn push_some_queues_whole_frames_only() {
        let (mut producer, source) = sample_ring(3, 2);
        let renderer = Renderer::new(source, FORMAT, 8, None, false);
        assert_eq!(producer.push_some(&[0; 5]), 4);
        assert_eq!(producer.push_some(&[0; 4]), 2);
        assert_eq!(producer.push_some(&[0; 4]), 0);
        assert_eq!(renderer.queued_samples(), 6);
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

    const DOP_FORMAT: DeviceFormat = DeviceFormat {
        rate: 176_400,
        container: SampleContainer::S24In32,
        channels: 2,
    };

    /// Two stereo DoP payload words for one frame: byte pairs per channel, marker byte zero.
    fn dop_frame(left: (u8, u8), right: (u8, u8)) -> [i32; 2] {
        let word = |(b0, b1): (u8, u8)| (i32::from(b0) << 16) | (i32::from(b1) << 8);
        [word(left), word(right)]
    }

    fn word_at(bytes: &[u8], index: usize) -> u32 {
        let at = index * 4;
        u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
    }

    fn assert_dop_word(bytes: &[u8], index: usize, marker: u32, payload: u16) {
        let word = word_at(bytes, index);
        assert_eq!(word >> 24, marker, "word {index} marker");
        assert_eq!(
            ((word >> 8) & 0xFFFF) as u16,
            payload,
            "word {index} payload"
        );
        assert_ne!(word, 0, "DoP stream must never contain a zero word");
    }

    /// Marker phase is shared between payload and idle frames, so an injected underrun or a
    /// pause can never leave two equal markers in a row (the DAC would lose DoP sync).
    #[test]
    fn dop_markers_alternate_across_payload_underrun_and_pause() {
        let (mut producer, source) = sample_ring(16, 2);
        let mut renderer = Renderer::new(source, DOP_FORMAT, 8, None, true);

        // Frame 0 and 1 of real audio.
        let mut samples = dop_frame((0x11, 0x12), (0x21, 0x22)).to_vec();
        samples.extend_from_slice(&dop_frame((0x13, 0x14), (0x23, 0x24)));
        assert!(producer.push(&samples));
        assert_eq!(renderer.fill(2).payload_frames, 2);
        let bytes = renderer.bytes(2);
        assert_dop_word(bytes, 0, 0x05, 0x1112);
        assert_dop_word(bytes, 1, 0x05, 0x2122);
        assert_dop_word(bytes, 2, 0xFA, 0x1314);
        assert_dop_word(bytes, 3, 0xFA, 0x2324);

        // Queue runs dry while the producer is alive: five idle frames, markers continue.
        let fill = renderer.fill(5);
        assert_eq!(fill.payload_frames, 0);
        assert_eq!(renderer.handle().underruns(), 1);
        let bytes = renderer.bytes(5);
        for (i, marker) in [0x05u32, 0xFA, 0x05, 0xFA, 0x05].into_iter().enumerate() {
            assert_dop_word(bytes, i * 2, marker, 0x6969);
            assert_dop_word(bytes, i * 2 + 1, marker, 0x6969);
        }

        // Payload resumes with the marker the idle frames left: no repeat at the boundary.
        let mut more = dop_frame((0x31, 0x32), (0x41, 0x42)).to_vec();
        more.extend_from_slice(&dop_frame((0x33, 0x34), (0x43, 0x44)));
        assert!(producer.push(&more));
        assert_eq!(renderer.fill(2).payload_frames, 2);
        let bytes = renderer.bytes(2);
        assert_dop_word(bytes, 0, 0xFA, 0x3132);
        assert_dop_word(bytes, 1, 0xFA, 0x4142);
        assert_dop_word(bytes, 2, 0x05, 0x3334);
        assert_dop_word(bytes, 3, 0x05, 0x4344);

        // Pause writes idle frames with the same phase, then resume keeps alternating.
        renderer.handle().set_paused(true);
        let fill = renderer.fill(2);
        assert_eq!(fill.payload_frames, 0);
        let bytes = renderer.bytes(2);
        assert_dop_word(bytes, 0, 0xFA, 0x6969);
        assert_dop_word(bytes, 2, 0x05, 0x6969);

        assert!(producer.push(&dop_frame((0x51, 0x52), (0x61, 0x62))));
        renderer.handle().set_paused(false);
        assert_eq!(renderer.fill(1).payload_frames, 1);
        let bytes = renderer.bytes(1);
        assert_dop_word(bytes, 0, 0xFA, 0x5152);
        assert_dop_word(bytes, 1, 0xFA, 0x6162);
    }

    /// A device buffer with another channel count still gets DoP idle that continues the
    /// stream's marker phase, never zeros.
    #[test]
    fn idle_into_a_foreign_layout_keeps_the_marker_phase() {
        let (_producer, source) = sample_ring(16, 2);
        let mut renderer = Renderer::new(source, DOP_FORMAT, 8, None, true);
        renderer.silence(1); // marker 0x05 used
        let mut dst = [0u8; 2 * 3 * 4]; // two frames of three channels
        renderer.idle_into(3, &mut dst);
        for (i, marker) in [0xFAu32, 0x05].into_iter().enumerate() {
            for channel in 0..3 {
                assert_dop_word(&dst, i * 3 + channel, marker, 0x6969);
            }
        }
        renderer.silence(1);
        assert_dop_word(renderer.bytes(1), 0, 0xFA, 0x6969);

        // One whole frame of two channels plus one stray sample: no zero word anywhere.
        let mut odd = [0u8; 3 * 4];
        renderer.idle_into(2, &mut odd);
        assert_dop_word(&odd, 0, 0x05, 0x6969);
        assert_dop_word(&odd, 1, 0x05, 0x6969);
        assert_dop_word(&odd, 2, 0xFA, 0x6969);
    }

    #[test]
    fn native_dsd_idle_is_the_dsd_silence_pattern() {
        let format = DeviceFormat {
            rate: 88_200,
            container: SampleContainer::DsdU32Be,
            channels: 2,
        };
        let (mut producer, source) = sample_ring(8, 2);
        let mut renderer = Renderer::new(source, format, 8, None, false);
        renderer.silence(2);
        assert!(renderer.bytes(2).iter().all(|&b| b == 0x69));

        assert!(producer.push(&[0x0102_0304, 0x0506_0708]));
        let fill = renderer.fill(1);
        assert_eq!(fill.payload_frames, 1);
        // Four DSD bytes per channel, most significant byte first.
        assert_eq!(renderer.bytes(1), &[1, 2, 3, 4, 5, 6, 7, 8]);
    }
}
