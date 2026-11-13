//! Headphone crossfeed after Bauer's stereophonic-to-binaural (bs2b) design.
//!
//! Each ear hears its own channel through a high shelf and the opposite channel through a
//! one-pole low-pass, which imitates how a speaker's sound reaches the far ear later and
//! duller. A final gain makes a centred (mono) signal keep its level at low frequencies.
//! The presets are libbs2b's: cut-off frequency and how much quieter the crossfed low end is
//! than the direct one.

use std::f64::consts::PI;

use serde::{Deserialize, Serialize};

use crate::stage::{Stage, StreamFormat};

/// libbs2b's three standard settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CrossfeedPreset {
    /// 700 Hz, 4.5 dB: closest to listening to speakers.
    #[default]
    Default,
    /// 700 Hz, 6.0 dB.
    ChuMoy,
    /// 650 Hz, 9.5 dB: the lightest effect.
    JanMeier,
}

impl CrossfeedPreset {
    pub const fn cutoff_hz(self) -> f64 {
        match self {
            Self::Default | Self::ChuMoy => 700.0,
            Self::JanMeier => 650.0,
        }
    }

    /// Feed level in dB: how far the crossfed low end sits below the direct one.
    pub const fn feed_db(self) -> f64 {
        match self {
            Self::Default => 4.5,
            Self::ChuMoy => 6.0,
            Self::JanMeier => 9.5,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Default => "Default",
            Self::ChuMoy => "Chu Moy",
            Self::JanMeier => "Jan Meier",
        }
    }
}

/// Filter coefficients for one rate (libbs2b's `init`).
#[derive(Debug, Clone, Copy, Default)]
struct Coeffs {
    a0_lo: f64,
    b1_lo: f64,
    a0_hi: f64,
    a1_hi: f64,
    b1_hi: f64,
    gain: f64,
}

impl Coeffs {
    fn new(preset: CrossfeedPreset, rate: u32) -> Self {
        let rate = f64::from(rate.max(1));
        let level = preset.feed_db();
        let gb_lo = level * -5.0 / 6.0 - 3.0;
        let gb_hi = level / 6.0 - 3.0;
        let g_lo = 10f64.powf(gb_lo / 20.0);
        let g_hi = 1.0 - 10f64.powf(gb_hi / 20.0);
        let fc_lo = preset.cutoff_hz();
        let fc_hi = fc_lo * 2f64.powf((gb_lo - 20.0 * g_hi.log10()) / 12.0);
        let x = (-2.0 * PI * fc_lo / rate).exp();
        let (b1_lo, a0_lo) = (x, g_lo * (1.0 - x));
        let x = (-2.0 * PI * fc_hi / rate).exp();
        Self {
            a0_lo,
            b1_lo,
            a0_hi: 1.0 - g_hi * (1.0 - x),
            a1_hi: -x,
            b1_hi: x,
            gain: 1.0 / (1.0 - g_hi + g_lo),
        }
    }
}

/// The crossfeed [`Stage`]. Only a stereo stream is processed; any other channel count
/// passes through, and the engine says so in the signal path.
pub struct Crossfeed {
    preset: CrossfeedPreset,
    coeffs: Coeffs,
    channels: usize,
    active: bool,
    /// Low-pass, high-shelf and previous-input state per channel.
    lo: [f64; 2],
    hi: [f64; 2],
    previous: [f64; 2],
}

impl Crossfeed {
    pub fn new(preset: CrossfeedPreset) -> Self {
        Self {
            preset,
            coeffs: Coeffs::default(),
            channels: 2,
            active: false,
            lo: [0.0; 2],
            hi: [0.0; 2],
            previous: [0.0; 2],
        }
    }

    pub fn preset(&self) -> CrossfeedPreset {
        self.preset
    }
}

impl Stage for Crossfeed {
    fn configure(&mut self, input: StreamFormat) -> StreamFormat {
        self.channels = usize::from(input.channels.max(1));
        self.active = input.channels == 2;
        self.coeffs = Coeffs::new(self.preset, input.rate);
        self.reset();
        input
    }

    fn max_out(&self, in_frames: usize) -> usize {
        in_frames
    }

    fn process(&mut self, input: &[f64], out: &mut [f64]) -> usize {
        out[..input.len()].copy_from_slice(input);
        if !self.active {
            return input.len() / self.channels;
        }
        let c = self.coeffs;
        let (frames, _) = out[..input.len()].as_chunks_mut::<2>();
        for frame in frames {
            let x = *frame;
            for (((lo, hi), previous), x) in self
                .lo
                .iter_mut()
                .zip(&mut self.hi)
                .zip(&self.previous)
                .zip(x)
            {
                *lo = c.a0_lo * x + c.b1_lo * *lo;
                *hi = c.a0_hi * x + c.a1_hi * previous + c.b1_hi * *hi;
            }
            self.previous = x;
            frame[0] = (self.hi[0] + self.lo[1]) * c.gain;
            frame[1] = (self.hi[1] + self.lo[0]) * c.gain;
        }
        input.len() / 2
    }

    fn flush(&mut self, _out: &mut [f64]) -> usize {
        0
    }

    fn reset(&mut self) {
        self.lo = [0.0; 2];
        self.hi = [0.0; 2];
        self.previous = [0.0; 2];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    /// Output amplitude of each channel for a sine of `freq` Hz with `left`/`right` input
    /// amplitudes, after the filters have settled.
    fn response(preset: CrossfeedPreset, freq: f64, left: f64, right: f64) -> (f64, f64) {
        let mut stage = Crossfeed::new(preset);
        stage.configure(StreamFormat {
            rate: RATE,
            channels: 2,
        });
        let frames = RATE as usize;
        let input: Vec<f64> = (0..frames)
            .flat_map(|n| {
                let s = (2.0 * PI * freq * n as f64 / f64::from(RATE)).sin();
                [left * s, right * s]
            })
            .collect();
        let mut out = vec![0.0; input.len()];
        stage.process(&input, &mut out);
        let settled = &out[out.len() / 2..];
        let amplitude = |ch: usize| {
            let power: f64 = settled
                .iter()
                .skip(ch)
                .step_by(2)
                .map(|v| v * v)
                .sum::<f64>()
                / (settled.len() / 2) as f64;
            (2.0 * power).sqrt()
        };
        (amplitude(0), amplitude(1))
    }

    fn db(x: f64) -> f64 {
        20.0 * x.log10()
    }

    #[test]
    fn a_centred_signal_keeps_its_low_end_and_loses_little_above() {
        for preset in [
            CrossfeedPreset::Default,
            CrossfeedPreset::ChuMoy,
            CrossfeedPreset::JanMeier,
        ] {
            let (low, _) = response(preset, 50.0, 1.0, 1.0);
            assert!(db(low).abs() < 0.1, "{preset:?} 50 Hz: {} dB", db(low));
            for freq in [100.0, 1_000.0, 5_000.0, 10_000.0] {
                let (l, r) = response(preset, freq, 1.0, 1.0);
                assert!((l - r).abs() < 1e-9);
                // bs2b dips centred sounds above the cut-off by about 1–1.75 dB (the level
                // compensation is exact only at low frequencies); never a boost.
                assert!(
                    db(l) < 0.05 && db(l) > -1.8,
                    "{preset:?} {freq} Hz: {} dB",
                    db(l)
                );
            }
        }
    }

    #[test]
    fn a_hard_panned_signal_leaks_its_low_end_more_than_its_high_end() {
        for preset in [
            CrossfeedPreset::Default,
            CrossfeedPreset::ChuMoy,
            CrossfeedPreset::JanMeier,
        ] {
            let (direct_low, leak_low) = response(preset, 100.0, 1.0, 0.0);
            let (direct_high, leak_high) = response(preset, 5_000.0, 1.0, 0.0);
            let separation_low = db(direct_low) - db(leak_low);
            let separation_high = db(direct_high) - db(leak_high);
            // At low frequencies the far ear sits at the preset's feed level below the near
            // one (bs2b's definition), within a dB.
            assert!(
                (separation_low - preset.feed_db()).abs() < 1.0,
                "{preset:?}: {separation_low} dB"
            );
            assert!(separation_high > separation_low + 10.0, "{preset:?}");
        }
        // A stronger feed means less separation.
        let sep = |p| {
            let (d, l) = response(p, 100.0, 1.0, 0.0);
            db(d) - db(l)
        };
        assert!(sep(CrossfeedPreset::ChuMoy) < sep(CrossfeedPreset::JanMeier));
    }

    #[test]
    fn other_channel_counts_pass_through() {
        let mut stage = Crossfeed::new(CrossfeedPreset::Default);
        for channels in [1u16, 6] {
            stage.configure(StreamFormat {
                rate: RATE,
                channels,
            });
            let input: Vec<f64> = (0..60).map(|n| n as f64 / 60.0).collect();
            let mut out = vec![0.0; input.len()];
            let frames = stage.process(&input, &mut out);
            assert_eq!(out, input);
            assert_eq!(frames, input.len() / usize::from(channels));
        }
    }
}
