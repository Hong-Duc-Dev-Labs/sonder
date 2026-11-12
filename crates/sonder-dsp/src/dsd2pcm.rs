//! DSD→PCM conversion by multistage decimation.
//!
//! The 1-bit stream is low-pass filtered and decimated in two stages: always by 8 at the bit
//! rate, then by the factor that lands on the requested PCM rate. Both stages use the
//! Sonder-designed Kaiser filters from `build.rs` (passband to 20 kHz, stopband at the
//! output Nyquist, ≥120 dB), so high-frequency DSD noise cannot fold back into the audio band.
//!
//! The converter works on interleaved MSB-first DSD bytes, the representation both
//! `sonder-decode` readers produce. Output is interleaved `f64` in `[-1, 1)`.
//!
//! Allocation happens in [`DsdToPcm::new`] and when the caller's buffers grow; processing
//! reuses those buffers.

use crate::dsd_tables::{
    TAPS_A_DSD64, TAPS_A_DSD128, TAPS_A_DSD256, TAPS_B_176_2, TAPS_B_176_4, TAPS_B_176_8,
    TAPS_B_352_2, TAPS_B_352_4,
};

/// PCM rates the DSD→PCM path can produce.
pub const DSD_PCM_RATES: [u32; 2] = [176_400, 352_800];

/// One decimating FIR, processing interleaved `f64` frames.
struct Decimator {
    taps: &'static [f64],
    channels: usize,
    factor: usize,
    history: Vec<f64>,
    /// Next history slot to write, in frames.
    at: usize,
    /// Input frames since the last output frame.
    phase: usize,
}

impl Decimator {
    fn new(taps: &'static [f64], channels: usize, factor: usize) -> Self {
        Self {
            taps,
            channels,
            factor,
            history: vec![0.0; taps.len() * channels],
            at: 0,
            phase: 0,
        }
    }

    fn reset(&mut self) {
        self.history.fill(0.0);
        self.at = 0;
        self.phase = 0;
    }

    /// Appends every output frame the new input completes.
    fn push(&mut self, input: &[f64], out: &mut Vec<f64>) {
        let channels = self.channels;
        let taps = self.taps;
        let n = taps.len();
        for frame in input.chunks_exact(channels) {
            let base = self.at * channels;
            self.history[base..base + channels].copy_from_slice(frame);
            self.at = (self.at + 1) % n;
            self.phase += 1;
            if self.phase < self.factor {
                continue;
            }
            self.phase = 0;
            let newest = self.at;
            for ch in 0..channels {
                let mut acc = 0.0;
                // The newest sample sits just before `at`; walk backwards through history.
                for (k, &tap) in taps.iter().enumerate() {
                    let pos = (newest + n - 1 - k) % n;
                    acc += tap * self.history[pos * channels + ch];
                }
                out.push(acc);
            }
        }
    }
}

/// Converts an interleaved MSB-first DSD byte stream to `f64` PCM.
pub struct DsdToPcm {
    channels: usize,
    output_rate: u32,
    stage_a: Decimator,
    stage_b: Option<Decimator>,
    /// Bytes of an incomplete input frame carried to the next call.
    pending: Vec<u8>,
    /// Scratch holding the ±1 samples of the bytes being processed.
    samples: Vec<f64>,
    mid: Vec<f64>,
    stage_out: Vec<f64>,
}

impl DsdToPcm {
    /// Builds a converter for a supported [`super::DSD_PCM_RATES`] pair, or `None`.
    pub fn new(dsd_rate: u32, output_rate: u32, channels: u16) -> Option<Self> {
        if channels == 0 {
            return None;
        }
        let (taps_a, stage_b): (&'static [f64], Option<(&'static [f64], usize)>) =
            match (dsd_rate, output_rate) {
                (2_822_400, 176_400) => (&TAPS_A_DSD64, Some((&TAPS_B_176_2, 2))),
                (2_822_400, 352_800) => (&TAPS_A_DSD64, None),
                (5_644_800, 176_400) => (&TAPS_A_DSD128, Some((&TAPS_B_176_4, 4))),
                (5_644_800, 352_800) => (&TAPS_A_DSD128, Some((&TAPS_B_352_2, 2))),
                (11_289_600, 176_400) => (&TAPS_A_DSD256, Some((&TAPS_B_176_8, 8))),
                (11_289_600, 352_800) => (&TAPS_A_DSD256, Some((&TAPS_B_352_4, 4))),
                _ => return None,
            };
        let channels = usize::from(channels);
        Some(Self {
            channels,
            output_rate,
            stage_a: Decimator::new(taps_a, channels, 8),
            stage_b: stage_b.map(|(taps, factor)| Decimator::new(taps, channels, factor)),
            pending: Vec::with_capacity(channels),
            samples: Vec::new(),
            mid: Vec::new(),
            stage_out: Vec::new(),
        })
    }

    pub fn output_rate(&self) -> u32 {
        self.output_rate
    }

    pub fn channels(&self) -> u16 {
        self.channels as u16
    }

    /// Appends the PCM frames `input` completes to `out`.
    ///
    /// `input` may split a byte frame; the remainder is carried to the next call.
    pub fn process(&mut self, input: &[u8], out: &mut Vec<f64>) {
        let channels = self.channels;
        let mut at = 0;
        if !self.pending.is_empty() {
            let need = channels - self.pending.len();
            if input.len() < need {
                self.pending.extend_from_slice(input);
                return;
            }
            self.pending.extend_from_slice(&input[..need]);
            at = need;
            let pending = std::mem::take(&mut self.pending);
            self.feed(&pending, out);
        }
        let usable = (input.len() - at) / channels * channels;
        if usable > 0 {
            self.feed(&input[at..at + usable], out);
        }
        self.pending.extend_from_slice(&input[at + usable..]);
    }

    /// Forgets filter history and any carried bytes (seek, stop).
    pub fn reset(&mut self) {
        self.stage_a.reset();
        if let Some(stage_b) = &mut self.stage_b {
            stage_b.reset();
        }
        self.pending.clear();
    }

    fn feed(&mut self, bytes: &[u8], out: &mut Vec<f64>) {
        let channels = self.channels;
        self.samples.clear();
        self.samples.reserve(bytes.len() * 8);
        for frame in bytes.chunks_exact(channels) {
            for shift in (0..8).rev() {
                for &byte in frame {
                    let bit = (byte >> shift) & 1;
                    self.samples.push(if bit == 1 { 1.0 } else { -1.0 });
                }
            }
        }

        self.mid.clear();
        self.stage_a.push(&self.samples, &mut self.mid);
        match &mut self.stage_b {
            Some(stage_b) => {
                self.stage_out.clear();
                stage_b.push(&self.mid, &mut self.stage_out);
                out.extend_from_slice(&self.stage_out);
            }
            None => out.extend_from_slice(&self.mid),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DSD64: u32 = 2_822_400;

    /// First-order sigma-delta encoding of `signal` (one sample per DSD bit), MSB-first.
    fn encode(signal: impl Fn(usize) -> f64, bits: usize) -> Vec<u8> {
        let mut integrator = 0.0;
        let mut quantised = 0.0;
        let mut frames = Vec::with_capacity(bits / 8);
        let mut byte = 0u8;
        for n in 0..bits {
            integrator += signal(n) - quantised;
            quantised = if integrator > 0.0 { 1.0 } else { -1.0 };
            byte = (byte << 1) | u8::from(quantised > 0.0);
            if n % 8 == 7 {
                frames.push(byte);
                byte = 0;
            }
        }
        frames
    }

    fn convert(dsd_rate: u32, out_rate: u32, bytes: &[u8]) -> Vec<f64> {
        let mut converter = DsdToPcm::new(dsd_rate, out_rate, 1).unwrap();
        let mut out = Vec::new();
        // Feed in awkward chunks to exercise the carried-byte path.
        for chunk in bytes.chunks(777) {
            converter.process(chunk, &mut out);
        }
        out
    }

    /// Amplitude of a real sinusoid by the Goertzel algorithm.
    fn goertzel(samples: &[f64], rate: f64, freq: f64) -> f64 {
        let n = samples.len() as f64;
        let w = std::f64::consts::TAU * freq / rate;
        let coeff = 2.0 * w.cos();
        let (mut s1, mut s2) = (0.0, 0.0);
        for &x in samples {
            let s0 = x + coeff * s1 - s2;
            s2 = s1;
            s1 = s0;
        }
        let power = s1 * s1 + s2 * s2 - coeff * s1 * s2;
        2.0 * power.max(0.0).sqrt() / n
    }

    fn rms(samples: &[f64]) -> f64 {
        (samples.iter().map(|x| x * x).sum::<f64>() / samples.len() as f64).sqrt()
    }

    #[test]
    fn dc_levels_and_silence() {
        for out_rate in DSD_PCM_RATES {
            let ones = convert(DSD64, out_rate, &vec![0xFF; 4_000]);
            assert!((ones.last().copied().unwrap() - 1.0).abs() < 1e-6);
            let zeros = convert(DSD64, out_rate, &vec![0x00; 4_000]);
            assert!((zeros.last().copied().unwrap() + 1.0).abs() < 1e-6);
            // 0x69 is the DoP idle pattern: alternating bits, mean zero.
            let silence = convert(DSD64, out_rate, &vec![0x69; 4_000]);
            assert!(silence.last().copied().unwrap().abs() < 1e-3);
        }
    }

    #[test]
    fn one_khz_tone_survives_with_its_amplitude() {
        let amp = 0.5;
        let bits = DSD64 as usize / 10; // 100 ms
        let dsd = encode(
            |n| amp * (std::f64::consts::TAU * 1_000.0 * n as f64 / f64::from(DSD64)).sin(),
            bits,
        );
        let pcm = convert(DSD64, 176_400, &dsd);
        // Skip the filter transient.
        let body = &pcm[1_000..];
        let measured = goertzel(body, 176_400.0, 1_000.0);
        assert!(
            (measured - amp).abs() < 0.05,
            "1 kHz amplitude {measured} vs {amp}"
        );
    }

    #[test]
    fn out_of_band_square_is_rejected() {
        // 0xAA/0x55 alternation is a full-scale tone at half the DSD bit rate, far above the
        // stopband. A converter without the low-pass would output a full-scale square.
        let mut bytes = Vec::with_capacity(8_000);
        for i in 0..8_000 {
            bytes.push(if i % 2 == 0 { 0xAA } else { 0x55 });
        }
        for out_rate in DSD_PCM_RATES {
            let pcm = convert(DSD64, out_rate, &bytes);
            let body = &pcm[500..];
            assert!(rms(body) < 1e-3, "{out_rate} Hz residual {}", rms(body));
        }
    }

    #[test]
    fn every_supported_rate_pair_converts() {
        for dsd_rate in [2_822_400, 5_644_800, 11_289_600] {
            for out_rate in DSD_PCM_RATES {
                let mut converter = DsdToPcm::new(dsd_rate, out_rate, 2);
                assert!(converter.is_some(), "{dsd_rate} -> {out_rate}");
                let mut out = Vec::new();
                converter
                    .as_mut()
                    .unwrap()
                    .process(&vec![0x69; 4_000], &mut out);
                assert!(!out.is_empty());
            }
        }
        assert!(DsdToPcm::new(DSD64, 48_000, 2).is_none());
    }

    #[test]
    fn reset_clears_history_and_carried_bytes() {
        let mut converter = DsdToPcm::new(DSD64, 176_400, 2).unwrap();
        let mut out = Vec::new();
        converter.process(&[0xFF, 0xFF, 0xFF], &mut out);
        converter.reset();
        out.clear();
        converter.process(&[0x69, 0x69, 0x69], &mut out);
        assert!(out.is_empty() || out.iter().all(|v| v.abs() < 1e-2));
    }
}
