//! Bit-depth reduction with TPDF dither.
//!
//! The stage quantises to a fixed word length and adds triangular probability density noise
//! of two LSB peak-to-peak (half an LSB RMS after quantising), which turns the distortion of
//! truncation into a flat noise floor. It belongs at the end of the chain, once, and only when
//! the target is narrower than the signal it receives.
//!
//! The quantiser clamps to the target grid, so the later integer conversion never sees an
//! out-of-range sample; the stage counts the input samples it had to clamp and publishes them
//! through an optional shared counter, so clipping is still reported on a dithered path.
//!
//! The noise source is a seedable SplitMix64 generator: no allocation, no locks, and the
//! tests can compare exact sample sequences.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::stage::{Stage, StreamFormat};

/// Narrowest and widest target the stage quantises to.
pub const MIN_BITS: u32 = 2;
pub const MAX_BITS: u32 = 24;

/// TPDF dither and final quantiser for one output word length.
#[derive(Debug)]
pub struct Dither {
    bits: u32,
    /// Samples per full scale: `2^(bits - 1)`.
    scale: f64,
    state: u64,
    channels: usize,
    /// Receives the number of input samples outside `[-1.0, 1.0)`, once per block.
    clipped: Option<Arc<AtomicU64>>,
}

impl Dither {
    pub fn new(bits: u32) -> Self {
        let bits = bits.clamp(MIN_BITS, MAX_BITS);
        Self {
            bits,
            scale: 2f64.powi(bits as i32 - 1),
            state: 0x9E37_79B9_7F4A_7C15,
            channels: 1,
            clipped: None,
        }
    }

    /// Adds clamped input samples to `counter`, which the owner reads and resets.
    pub fn with_clip_counter(mut self, counter: Arc<AtomicU64>) -> Self {
        self.clipped = Some(counter);
        self
    }

    pub fn bits(&self) -> u32 {
        self.bits
    }

    /// One uniform sample in `[0, 1)`.
    fn uniform(&mut self) -> f64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        // 53 random bits scaled into [0, 1).
        (z >> 11) as f64 * (1.0 / 9_007_199_254_740_992.0)
    }
}

impl Stage for Dither {
    fn configure(&mut self, input: StreamFormat) -> StreamFormat {
        self.channels = usize::from(input.channels.max(1));
        input
    }

    fn max_out(&self, in_frames: usize) -> usize {
        in_frames
    }

    fn process(&mut self, input: &[f64], out: &mut [f64]) -> usize {
        let scale = self.scale;
        // The largest value the target grid holds, so conversion never has to clip.
        let ceiling = 1.0 - 1.0 / scale;
        let mut clipped = 0u64;
        for (o, &x) in out.iter_mut().zip(input) {
            // Judged on the input: dither noise alone pushing a near-full-scale sample one
            // LSB over is not the signal clipping.
            if !(-1.0..1.0).contains(&x) {
                clipped += 1;
            }
            let noise = self.uniform() - self.uniform();
            let quantised = (x * scale + noise).round() / scale;
            *o = quantised.clamp(-1.0, ceiling);
        }
        if clipped > 0
            && let Some(counter) = &self.clipped
        {
            counter.fetch_add(clipped, Ordering::Relaxed);
        }
        input.len() / self.channels
    }

    fn flush(&mut self, _out: &mut [f64]) -> usize {
        0
    }

    fn reset(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    const FORMAT: StreamFormat = StreamFormat {
        rate: 44_100,
        channels: 2,
    };

    fn run(bits: u32, samples: &[f64]) -> Vec<f64> {
        let mut dither = Dither::new(bits);
        dither.configure(FORMAT);
        let mut out = vec![0.0; samples.len()];
        dither.process(samples, &mut out);
        out
    }

    #[test]
    fn silence_becomes_the_expected_tpdf_noise_floor() {
        let bits = 16;
        let out = run(bits, &vec![0.0; 200_000]);
        let lsb = 2f64.powi(-(bits as i32 - 1));
        let rms = (out.iter().map(|s| s * s).sum::<f64>() / out.len() as f64).sqrt();
        // Two uniform samples give 2 LSB peak-to-peak TPDF: dither variance 1/6 LSB² plus
        // quantiser variance 1/12 LSB² leaves 1/4 LSB², so RMS = 0.5 LSB.
        let expected = lsb / 2.0;
        assert!(
            (rms - expected).abs() / expected < 0.02,
            "RMS {rms} vs {expected}"
        );
        // Every sample lands on the target grid.
        for s in &out {
            let steps = s * 2f64.powi(bits as i32 - 1);
            assert!((steps - steps.round()).abs() < 1e-6, "{s}");
            assert!(steps.abs() <= 2.0);
        }
        // Both polarities occur: TPDF is symmetric.
        assert!(out.iter().any(|&s| s > 0.0) && out.iter().any(|&s| s < 0.0));
    }

    #[test]
    fn a_signal_is_quantised_within_one_lsb() {
        let bits = 16;
        let lsb = 2f64.powi(-(bits as i32 - 1));
        let input: Vec<f64> = (0..10_000)
            .map(|i| (f64::from(i) / 10_000.0) * 0.9 - 0.45)
            .collect();
        let out = run(bits, &input);
        for (o, &i) in out.iter().zip(&input) {
            assert!((o - i).abs() <= 1.5 * lsb, "{o} vs {i}");
        }
    }

    #[test]
    fn full_scale_inputs_stay_inside_the_word_length() {
        let out = run(16, &[1.0, -1.0, 0.99999, -0.99999]);
        for s in out {
            assert!((-1.0..1.0).contains(&s), "{s}");
        }
    }

    #[test]
    fn clamped_inputs_are_counted_but_dither_overshoot_is_not() {
        let counter = Arc::new(AtomicU64::new(0));
        let mut dither = Dither::new(16).with_clip_counter(counter.clone());
        dither.configure(FORMAT);
        let input = [1.0, -1.5, 0.99999, -1.0, 2.0, 0.0];
        let mut out = vec![0.0; input.len()];
        dither.process(&input, &mut out);
        // 1.0, −1.5 and 2.0 are outside the range; 0.99999 and −1.0 are inside it even when
        // the noise rounds them onto the clamp.
        assert_eq!(counter.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn the_same_seed_gives_the_same_noise() {
        let a = run(16, &[0.0; 64]);
        let b = run(16, &[0.0; 64]);
        assert_eq!(a, b);
        let c = run(24, &[0.0; 64]);
        assert_ne!(a, c);
    }

    #[test]
    fn the_target_word_length_is_clamped_to_what_pcm_carries() {
        assert_eq!(Dither::new(1).bits(), MIN_BITS);
        assert_eq!(Dither::new(32).bits(), MAX_BITS);
    }
}
