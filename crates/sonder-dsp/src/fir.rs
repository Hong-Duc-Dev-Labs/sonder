//! Polyphase FIR core shared by every rate converter.
//!
//! Input frames are consumed one at a time and every output whose input base index is
//! already available is emitted immediately. That keeps the history at exactly
//! `taps / L` frames regardless of whether the ratio upsamples or downsamples, and lets the
//! caller hand in arbitrary block sizes. All buffers are allocated in
//! [`configure`](FirCore::configure); the processing loop allocates nothing.

use std::sync::Arc;

use crate::filter_source::{FilterSpec, FilterTable};

/// Largest input block the core copies into its history at once.
const BLOCK_FRAMES: usize = 4096;

/// One rational polyphase converter `L/M` with per-channel history.
pub struct FirCore {
    table: Arc<FilterTable>,
    taps: usize,
    channels: usize,
    /// `channels` buffers of `taps + BLOCK_FRAMES` frames: history then new input.
    buffer: Vec<Vec<f64>>,
    /// `(taps - 1) * channels` zeros for [`flush`](Self::flush), allocated in
    /// [`configure`](Self::configure).
    zeros: Vec<f64>,
    /// Next output's input base index `q = floor(n·M/L)`.
    base: u64,
    /// Next output's phase `n·M mod L`.
    phase: u32,
    /// Input frames consumed since [`reset`](Self::reset).
    input_index: u64,
}

impl FirCore {
    pub fn new(table: Arc<FilterTable>) -> Self {
        Self {
            taps: table.taps_per_phase(),
            channels: 0,
            buffer: Vec::new(),
            zeros: Vec::new(),
            base: 0,
            phase: 0,
            input_index: 0,
            table,
        }
    }

    pub fn spec(&self) -> FilterSpec {
        self.table.spec()
    }

    pub fn table(&self) -> &Arc<FilterTable> {
        &self.table
    }

    pub fn taps_per_phase(&self) -> usize {
        self.taps
    }

    /// Allocates history for `channels` interleaved channels and clears it.
    pub fn configure(&mut self, channels: usize) {
        let channels = channels.max(1);
        if self.channels == channels && !self.buffer.is_empty() {
            self.reset();
            return;
        }
        self.channels = channels;
        self.buffer = (0..channels)
            .map(|_| vec![0.0; self.taps + BLOCK_FRAMES])
            .collect();
        self.zeros = vec![0.0; self.taps.saturating_sub(1) * channels];
        self.reset();
    }

    /// Upper bound on frames written for `in_frames` input frames; `max_out(0)` bounds
    /// [`flush`](Self::flush).
    pub fn max_out(&self, in_frames: usize) -> usize {
        if in_frames == 0 {
            return self.tail_frames();
        }
        let produced = (in_frames as u128 + 1) * u128::from(self.table.spec().num)
            / u128::from(self.table.spec().den);
        usize::try_from(produced)
            .unwrap_or(usize::MAX)
            .saturating_add(3)
            .max(self.tail_frames())
    }

    fn tail_frames(&self) -> usize {
        let produced = (self.taps as u128) * u128::from(self.table.spec().num)
            / u128::from(self.table.spec().den);
        usize::try_from(produced)
            .unwrap_or(usize::MAX)
            .saturating_add(3)
    }

    /// Consumes all of `input` (interleaved) and writes the frames it can produce into
    /// `out`, which holds at least [`max_out`](Self::max_out) frames.
    pub fn process(&mut self, input: &[f64], out: &mut [f64]) -> usize {
        let channels = self.channels;
        debug_assert!(channels > 0, "FirCore::configure must run first");
        debug_assert_eq!(input.len() % channels, 0);
        let frames = input.len() / channels;
        let num = u64::from(self.table.spec().num);
        let den = u64::from(self.table.spec().den);
        let phases = self.table.phases();
        let taps = self.taps;
        let mut written = 0;

        for chunk_start in (0..frames).step_by(BLOCK_FRAMES) {
            let chunk = (frames - chunk_start).min(BLOCK_FRAMES);
            for t in 0..chunk {
                let pos = taps + t;
                let src = (chunk_start + t) * channels;
                for c in 0..channels {
                    self.buffer[c][pos] = input[src + c];
                }
                let available = self.input_index + t as u64 + 1;
                while self.base < available {
                    let base_in_buffer =
                        taps as isize + self.base as isize - self.input_index as isize;
                    let base_in_buffer = usize::try_from(base_in_buffer)
                        .expect("base never falls behind the retained history");
                    let start = self.phase as usize * taps;
                    let coefficients = &phases[start..start + taps];
                    for c in 0..channels {
                        let samples = &self.buffer[c][..=base_in_buffer];
                        let mut acc = 0.0;
                        for (&coefficient, &sample) in coefficients.iter().zip(samples.iter().rev())
                        {
                            acc += coefficient * sample;
                        }
                        out[written * channels + c] = acc;
                    }
                    written += 1;
                    self.phase += den as u32;
                    while u64::from(self.phase) >= num {
                        self.phase -= num as u32;
                        self.base += 1;
                    }
                }
            }
            // Retain the last `taps` frames, including the new chunk.
            for buffer in &mut self.buffer {
                buffer.copy_within(chunk..chunk + taps, 0);
            }
            self.input_index += chunk as u64;
        }
        written
    }

    /// Pushes the filter's tail with zero input and resets the converter.
    pub fn flush(&mut self, out: &mut [f64]) -> usize {
        let zeros = std::mem::take(&mut self.zeros);
        if zeros.is_empty() {
            self.reset();
            return 0;
        }
        let mut written = 0;
        let mut consumed = 0;
        let mut remaining = zeros.len() / self.channels;
        while remaining > 0 {
            let chunk = remaining.min(BLOCK_FRAMES);
            written += self.process(
                &zeros[consumed * self.channels..][..chunk * self.channels],
                &mut out[written * self.channels..],
            );
            consumed += chunk;
            remaining -= chunk;
        }
        self.zeros = zeros;
        self.reset();
        written
    }

    /// Forgets history and restarts the output phase at zero.
    pub fn reset(&mut self) {
        self.base = 0;
        self.phase = 0;
        self.input_index = 0;
        for buffer in &mut self.buffer {
            buffer.fill(0.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter_source::{FilterPhase, FilterSourceKind, FilterVariant};

    fn table(num: u32, den: u32, coeffs: Vec<f64>) -> Arc<FilterTable> {
        let spec = FilterSpec::new(num, den, FilterPhase::Linear, FilterVariant::Precise);
        Arc::new(FilterTable::from_prototype(spec, FilterSourceKind::Sonder, coeffs).unwrap())
    }

    #[test]
    fn impulse_response_is_the_table_at_unity_ratio() {
        let coeffs = vec![0.1, 0.2, 0.4, 0.2, 0.1];
        let table = table(1, 1, coeffs.clone());
        let mut core = FirCore::new(table);
        core.configure(1);
        let mut input = vec![0.0; 8];
        input[0] = 1.0;
        let mut out = vec![0.0; core.max_out(8)];
        let n = core.process(&input, &mut out);
        let tail = core.flush(&mut out[n..]);
        let mut got = out[..n + tail].to_vec();
        got.truncate(coeffs.len());
        for (a, b) in got.iter().zip(&coeffs) {
            assert!((a - b).abs() < 1e-12, "{a} vs {b}");
        }
    }

    #[test]
    fn every_branch_has_unity_dc_gain() {
        let table = table(4, 1, vec![1.0 / 8.0; 8]);
        let core_table = table.clone();
        let mut core = FirCore::new(core_table);
        core.configure(1);
        let mut input = vec![1.0; 64];
        input.extend(std::iter::repeat_n(0.0, 64));
        let mut out = vec![0.0; core.max_out(input.len())];
        let n = core.process(&input[..64], &mut out);
        // After the filter's group delay, DC input yields DC output of the same size.
        for (i, y) in out[..n].iter().enumerate().skip(8) {
            assert!((y - 1.0).abs() < 1e-12, "output {i} = {y}");
        }
    }

    #[test]
    fn downsampling_keeps_a_bounded_history_and_unity_dc() {
        let table = table(1, 2, vec![0.25, 0.5, 0.25]);
        let mut core = FirCore::new(table);
        core.configure(1);
        let input = vec![1.0; 32];
        let mut out = vec![0.0; core.max_out(input.len())];
        let n = core.process(&input, &mut out);
        assert!((15..=17).contains(&n), "{n} frames from 32");
        for y in &out[2..n] {
            assert!((y - 1.0).abs() < 1e-12, "{y}");
        }
    }

    #[test]
    fn chunking_matches_a_single_call() {
        let make = || {
            let mut core = FirCore::new(table(3, 2, vec![1.0 / 12.0; 12]));
            core.configure(2);
            core
        };
        let input: Vec<f64> = (0..10_000).map(|i| (i as f64 * 0.01).sin()).collect();
        let mut whole = vec![0.0; make().max_out(input.len() / 2) * 2];
        let mut a = make();
        let n = a.process(&input, &mut whole);

        let mut b = make();
        let mut split = Vec::new();
        for chunk in input.chunks(2 * 777) {
            let mut out = vec![0.0; b.max_out(chunk.len() / 2) * 2];
            let wrote = b.process(chunk, &mut out);
            split.extend_from_slice(&out[..wrote * 2]);
        }
        assert_eq!(n * 2, split.len());
        for (x, y) in whole[..n * 2].iter().zip(&split) {
            assert!((x - y).abs() < 1e-12);
        }
    }
}
