//! Parametric EQ: a cascade of RBJ biquads per channel behind one preamp gain.
//!
//! [`Peq`] is the plain data the settings file stores; [`PeqStage`] is its [`Stage`]
//! implementation. Bands are validated against the stream's sample rate when the stage is
//! configured, so a preset saved for 44.1 kHz still behaves on a 192 kHz stream.

use serde::{Deserialize, Serialize};

use crate::biquad::{Biquad, BiquadCoeffs, BiquadKind};
use crate::stage::{Stage, StreamFormat};

/// Highest number of bands accepted, so a corrupt settings file cannot build thousands of
/// sections per channel.
pub const MAX_BANDS: usize = 32;

/// One parametric EQ band.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PeqBand {
    pub kind: BiquadKind,
    /// Centre (peak, band-pass, notch) or corner (shelves, passes) frequency.
    pub freq_hz: f64,
    /// Boost or cut; ignored for the shapes that only filter.
    pub gain_db: f64,
    pub q: f64,
    pub enabled: bool,
}

impl Default for PeqBand {
    fn default() -> Self {
        Self {
            kind: BiquadKind::Peak,
            freq_hz: 1_000.0,
            gain_db: 0.0,
            q: 0.707,
            enabled: true,
        }
    }
}

impl PeqBand {
    /// The band with values inside the ranges the UI and settings file may produce.
    pub fn sanitized(self) -> Self {
        Self {
            freq_hz: self.freq_hz.clamp(10.0, 24_000.0),
            gain_db: self.gain_db.clamp(-24.0, 24.0),
            q: self.q.clamp(0.1, 20.0),
            ..self
        }
    }
}

/// A whole EQ preset: preamp plus bands, in processing order.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Peq {
    /// Gain in dB applied before the bands; the usual place for negative headroom.
    pub preamp_db: f64,
    pub bands: Vec<PeqBand>,
}

impl Peq {
    /// Whether the stage changes the signal at all: any enabled band or a non-zero preamp.
    pub fn active(&self) -> bool {
        self.preamp_db != 0.0 || self.bands.iter().any(|b| b.enabled)
    }

    /// Clamped copy: preamp within −60…+12 dB, at most [`MAX_BANDS`] bands.
    pub fn sanitized(mut self) -> Self {
        self.preamp_db = self.preamp_db.clamp(-60.0, 12.0);
        self.bands.truncate(MAX_BANDS);
        for band in &mut self.bands {
            *band = band.sanitized();
        }
        self
    }

    /// Linear preamp gain.
    pub fn preamp_gain(&self) -> f64 {
        10f64.powf(self.preamp_db / 20.0)
    }
}

/// The EQ as a [`Stage`]: one cascade per channel, filters rebuilt on `configure`.
pub struct PeqStage {
    peq: Peq,
    channels: usize,
    /// `channels × enabled bands`, channel-major.
    filters: Vec<Biquad>,
    preamp: f64,
}

impl PeqStage {
    pub fn new(peq: Peq) -> Self {
        Self {
            peq: peq.sanitized(),
            channels: 1,
            filters: Vec::new(),
            preamp: 1.0,
        }
    }

    pub fn peq(&self) -> &Peq {
        &self.peq
    }

    /// Coefficients of the first section, for tests and diagnostics.
    #[cfg(test)]
    fn first_coeffs(&self) -> Option<BiquadCoeffs> {
        self.filters.first().map(Biquad::coeffs)
    }
}

impl Stage for PeqStage {
    fn configure(&mut self, input: StreamFormat) -> StreamFormat {
        self.channels = usize::from(input.channels.max(1));
        self.preamp = self.peq.preamp_gain();
        let enabled = self.peq.bands.iter().filter(|band| band.enabled).count();
        self.filters.clear();
        self.filters.reserve(self.channels * enabled);
        for _ in 0..self.channels {
            for band in self.peq.bands.iter().filter(|band| band.enabled) {
                self.filters.push(Biquad::new(BiquadCoeffs::new(
                    band.kind,
                    band.freq_hz,
                    band.gain_db,
                    band.q,
                    input.rate,
                )));
            }
        }
        input
    }

    fn max_out(&self, in_frames: usize) -> usize {
        in_frames
    }

    fn process(&mut self, input: &[f64], out: &mut [f64]) -> usize {
        let channels = self.channels;
        // `configure` keeps at least one channel, so this division is safe.
        let bands = self.filters.len() / channels;
        for (samples, outputs) in input
            .chunks_exact(channels)
            .zip(out.chunks_exact_mut(channels))
        {
            for (ch, (x, o)) in samples.iter().zip(outputs).enumerate() {
                let filters = &mut self.filters[ch * bands..(ch + 1) * bands];
                let mut s = x * self.preamp;
                for filter in filters {
                    s = filter.process(s);
                }
                *o = s;
            }
        }
        input.len() / channels
    }

    fn flush(&mut self, _out: &mut [f64]) -> usize {
        0
    }

    fn reset(&mut self) {
        for filter in &mut self.filters {
            filter.reset();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FORMAT: StreamFormat = StreamFormat {
        rate: 48_000,
        channels: 2,
    };

    fn band(kind: BiquadKind, freq_hz: f64, gain_db: f64, q: f64) -> PeqBand {
        PeqBand {
            kind,
            freq_hz,
            gain_db,
            q,
            enabled: true,
        }
    }

    /// Runs a steady sine through the stage and returns the gain in dB after settling.
    fn sine_gain(peq: Peq, freq: f64) -> f64 {
        let mut stage = PeqStage::new(peq);
        stage.configure(FORMAT);
        let frames = 48_000;
        let input: Vec<f64> = (0..frames)
            .flat_map(|i| {
                let s = (2.0 * std::f64::consts::PI * freq * i as f64 / 48_000.0).sin() * 0.25;
                [s, s]
            })
            .collect();
        let mut out = vec![0.0; input.len()];
        stage.process(&input, &mut out);
        // Measure the last 200 ms so the filters have settled.
        let start = out.len() - 2 * 9_600;
        let rms =
            (out[start..].iter().map(|s| s * s).sum::<f64>() / (out.len() - start) as f64).sqrt();
        20.0 * (rms / (0.25 / std::f64::consts::SQRT_2)).log10()
    }

    #[test]
    fn one_peak_band_hits_its_requested_gain_at_f0() {
        for gain in [-9.0, -3.0, 3.0, 6.0] {
            let peq = Peq {
                preamp_db: 0.0,
                bands: vec![band(BiquadKind::Peak, 1_000.0, gain, 1.4)],
            };
            let actual = sine_gain(peq, 1_000.0);
            assert!(
                (actual - gain).abs() < 0.02,
                "requested {gain} dB, measured {actual} dB"
            );
        }
    }

    #[test]
    fn every_band_applies_and_the_preamp_adds_to_it() {
        let peq = Peq {
            preamp_db: -6.0,
            bands: vec![
                band(BiquadKind::Peak, 1_000.0, 3.0, 1.0),
                band(BiquadKind::Peak, 1_000.0, 3.0, 1.0),
            ],
        };
        let actual = sine_gain(peq, 1_000.0);
        assert!((actual - 0.0).abs() < 0.05, "measured {actual} dB");
    }

    #[test]
    fn disabled_bands_are_skipped() {
        let peq = Peq {
            preamp_db: 0.0,
            bands: vec![PeqBand {
                enabled: false,
                gain_db: 12.0,
                ..band(BiquadKind::Peak, 1_000.0, 12.0, 1.0)
            }],
        };
        assert!(!peq.active());
        let actual = sine_gain(peq, 1_000.0);
        assert!(actual.abs() < 1e-6, "measured {actual} dB");
    }

    #[test]
    fn channels_keep_separate_filter_state() {
        let mut stage = PeqStage::new(Peq {
            preamp_db: 0.0,
            bands: vec![band(BiquadKind::Peak, 1_000.0, 6.0, 1.0)],
        });
        stage.configure(FORMAT);
        let mut out = vec![0.0; 8];
        // Left gets an impulse, right silence; after the first frame the left channel must
        // still ring while the right stays at zero.
        let input = [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        stage.process(&input, &mut out);
        assert!(out[2].abs() > 0.0 && out[4].abs() > 0.0);
        assert!(out[3] == 0.0 && out[5] == 0.0 && out[7] == 0.0);
        assert!(stage.first_coeffs().is_some());
    }

    #[test]
    fn zero_gain_and_no_bands_is_a_pass_through() {
        let mut stage = PeqStage::new(Peq::default());
        stage.configure(FORMAT);
        let input: Vec<f64> = (0..100).map(|i| f64::from(i) / 100.0).collect();
        let mut out = vec![0.0; input.len()];
        stage.process(&input, &mut out);
        assert_eq!(out, input);
    }
}
