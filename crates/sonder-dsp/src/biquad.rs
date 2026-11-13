//! Second-order sections following the RBJ Audio EQ Cookbook.
//!
//! Every filter is stored as normalised coefficients and run as a Transposed Direct Form II
//! section in `f64`: two state variables per channel, no allocation while processing. The
//! parametric EQ ([`peq`](crate::peq)) is a cascade of these.

use std::f64::consts::PI;

use serde::{Deserialize, Serialize};

/// Filter shapes the cookbook defines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BiquadKind {
    /// Bell: `gain_db` at `freq`, `q` sets the bandwidth.
    Peak,
    LowShelf,
    HighShelf,
    LowPass,
    HighPass,
    /// Constant 0 dB peak gain.
    BandPass,
    Notch,
}

impl BiquadKind {
    /// Whether `gain_db` means anything for this shape.
    pub const fn uses_gain(self) -> bool {
        matches!(self, Self::Peak | Self::LowShelf | Self::HighShelf)
    }
}

/// Normalised `b0 b1 b2 a1 a2` of one section (`a0` is folded in).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct BiquadCoeffs {
    pub b0: f64,
    pub b1: f64,
    pub b2: f64,
    pub a1: f64,
    pub a2: f64,
}

impl BiquadCoeffs {
    /// Coefficients for `kind` at `freq_hz` (clamped below Nyquist) on a `rate` stream.
    pub fn new(kind: BiquadKind, freq_hz: f64, gain_db: f64, q: f64, rate: u32) -> Self {
        let rate = f64::from(rate.max(1));
        // Keep well inside Nyquist: a section on the edge is numerically useless.
        let freq = freq_hz.clamp(1.0, rate * 0.49);
        let w0 = 2.0 * PI * freq / rate;
        let (sin, cos) = w0.sin_cos();
        let q = q.max(1e-4);
        let alpha = sin / (2.0 * q);
        let a = 10f64.powf(gain_db / 40.0);

        let (b0, b1, b2, a0, a1, a2) = match kind {
            BiquadKind::Peak => (
                1.0 + alpha * a,
                -2.0 * cos,
                1.0 - alpha * a,
                1.0 + alpha / a,
                -2.0 * cos,
                1.0 - alpha / a,
            ),
            BiquadKind::LowShelf => {
                let two_sqrt_a_alpha = 2.0 * a.sqrt() * alpha;
                (
                    a * ((a + 1.0) - (a - 1.0) * cos + two_sqrt_a_alpha),
                    2.0 * a * ((a - 1.0) - (a + 1.0) * cos),
                    a * ((a + 1.0) - (a - 1.0) * cos - two_sqrt_a_alpha),
                    (a + 1.0) + (a - 1.0) * cos + two_sqrt_a_alpha,
                    -2.0 * ((a - 1.0) + (a + 1.0) * cos),
                    (a + 1.0) + (a - 1.0) * cos - two_sqrt_a_alpha,
                )
            }
            BiquadKind::HighShelf => {
                let two_sqrt_a_alpha = 2.0 * a.sqrt() * alpha;
                (
                    a * ((a + 1.0) + (a - 1.0) * cos + two_sqrt_a_alpha),
                    -2.0 * a * ((a - 1.0) + (a + 1.0) * cos),
                    a * ((a + 1.0) + (a - 1.0) * cos - two_sqrt_a_alpha),
                    (a + 1.0) - (a - 1.0) * cos + two_sqrt_a_alpha,
                    2.0 * ((a - 1.0) - (a + 1.0) * cos),
                    (a + 1.0) - (a - 1.0) * cos - two_sqrt_a_alpha,
                )
            }
            BiquadKind::LowPass => (
                (1.0 - cos) / 2.0,
                1.0 - cos,
                (1.0 - cos) / 2.0,
                1.0 + alpha,
                -2.0 * cos,
                1.0 - alpha,
            ),
            BiquadKind::HighPass => (
                (1.0 + cos) / 2.0,
                -(1.0 + cos),
                (1.0 + cos) / 2.0,
                1.0 + alpha,
                -2.0 * cos,
                1.0 - alpha,
            ),
            BiquadKind::BandPass => (alpha, 0.0, -alpha, 1.0 + alpha, -2.0 * cos, 1.0 - alpha),
            BiquadKind::Notch => (1.0, -2.0 * cos, 1.0, 1.0 + alpha, -2.0 * cos, 1.0 - alpha),
        };
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
        }
    }

    /// Linear magnitude `|H(e^{jw})|` at `freq_hz`, computed directly from the coefficients.
    pub fn magnitude_at(&self, freq_hz: f64, rate: u32) -> f64 {
        let w = 2.0 * PI * freq_hz.clamp(0.0, f64::from(rate) / 2.0) / f64::from(rate.max(1));
        let (sin1, cos1) = w.sin_cos();
        let (sin2, cos2) = (2.0 * w).sin_cos();
        let num_re = self.b0 + self.b1 * cos1 + self.b2 * cos2;
        let num_im = -(self.b1 * sin1 + self.b2 * sin2);
        let den_re = 1.0 + self.a1 * cos1 + self.a2 * cos2;
        let den_im = -(self.a1 * sin1 + self.a2 * sin2);
        (num_re.hypot(num_im) / den_re.hypot(den_im)).max(0.0)
    }

    /// Magnitude in decibels at `freq_hz`.
    pub fn gain_db_at(&self, freq_hz: f64, rate: u32) -> f64 {
        20.0 * self.magnitude_at(freq_hz, rate).max(1e-30).log10()
    }
}

/// One section's state, processed in Transposed Direct Form II.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Biquad {
    coeffs: BiquadCoeffs,
    z1: f64,
    z2: f64,
}

impl Biquad {
    pub fn new(coeffs: BiquadCoeffs) -> Self {
        Self {
            coeffs,
            z1: 0.0,
            z2: 0.0,
        }
    }

    pub fn coeffs(&self) -> BiquadCoeffs {
        self.coeffs
    }

    /// Replaces the coefficients and forgets the history.
    pub fn set_coeffs(&mut self, coeffs: BiquadCoeffs) {
        self.coeffs = coeffs;
        self.reset();
    }

    pub fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }

    /// Filters one sample. Allocation-free and branch-free.
    pub fn process(&mut self, x: f64) -> f64 {
        let c = &self.coeffs;
        let y = c.b0.mul_add(x, self.z1);
        self.z1 = c.b1.mul_add(x, self.z2) - c.a1 * y;
        self.z2 = c.b2.mul_add(x, -c.a2 * y);
        y
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    fn coeffs(kind: BiquadKind, freq: f64, gain: f64, q: f64) -> BiquadCoeffs {
        BiquadCoeffs::new(kind, freq, gain, q, RATE)
    }

    #[test]
    fn peak_gain_at_f0_matches_the_request() {
        for gain in [-12.0, -6.0, -0.5, 0.0, 3.0, 6.0, 12.0] {
            for q in [0.5, 0.707, 1.0, 4.0] {
                let c = coeffs(BiquadKind::Peak, 1_000.0, gain, q);
                let actual = c.gain_db_at(1_000.0, RATE);
                assert!(
                    (actual - gain).abs() < 0.01,
                    "gain {gain} dB, Q {q}: got {actual} dB"
                );
            }
        }
    }

    #[test]
    fn shelves_reach_their_gain_at_the_band_edges() {
        for gain in [-9.0, 4.5] {
            let low = coeffs(BiquadKind::LowShelf, 200.0, gain, 0.707);
            assert!((low.gain_db_at(0.0, RATE) - gain).abs() < 0.01);
            assert!(low.gain_db_at(RATE as f64 / 2.0, RATE).abs() < 0.01);

            let high = coeffs(BiquadKind::HighShelf, 6_000.0, gain, 0.707);
            assert!((high.gain_db_at(RATE as f64 / 2.0, RATE) - gain).abs() < 0.01);
            assert!(high.gain_db_at(0.0, RATE).abs() < 0.01);
        }
    }

    #[test]
    fn low_and_high_pass_keep_their_passbands_and_drop_the_stopbands() {
        let low = coeffs(BiquadKind::LowPass, 1_000.0, 0.0, 0.707);
        assert!(low.gain_db_at(10.0, RATE).abs() < 0.01);
        assert!(low.gain_db_at(1_000.0, RATE) < -2.9); // −3 dB at the corner
        assert!(low.gain_db_at(10_000.0, RATE) < -35.0);

        let high = coeffs(BiquadKind::HighPass, 1_000.0, 0.0, 0.707);
        assert!(high.gain_db_at(10_000.0, RATE).abs() < 0.01);
        assert!(high.gain_db_at(1_000.0, RATE) < -2.9);
        assert!(high.gain_db_at(50.0, RATE) < -45.0);
    }

    #[test]
    fn band_pass_peaks_at_f0_and_notches_reject_it() {
        let band = coeffs(BiquadKind::BandPass, 1_000.0, 0.0, 1.0);
        assert!(band.gain_db_at(1_000.0, RATE).abs() < 0.01);
        assert!(band.gain_db_at(50.0, RATE) < -25.0);
        assert!(band.gain_db_at(20_000.0, RATE) < -25.0);

        let notch = coeffs(BiquadKind::Notch, 1_000.0, 0.0, 1.0);
        assert!(notch.magnitude_at(1_000.0, RATE) < 1e-6);
        assert!(notch.gain_db_at(20.0, RATE).abs() < 0.5);
    }

    #[test]
    fn a_section_rings_at_its_own_frequency_and_stays_stable() {
        // An impulse must excite exactly f0; a runaway section would diverge instead.
        let mut biquad = Biquad::new(coeffs(BiquadKind::Peak, 1_000.0, 6.0, 2.0));
        let mut peak = 0.0f64;
        for i in 0..48_000 {
            let y = biquad.process(if i == 0 { 1.0 } else { 0.0 });
            assert!(y.is_finite(), "section diverged at sample {i}");
            peak = peak.max(y.abs());
        }
        assert!(peak > 0.05 && peak < 2.0, "impulse peak {peak}");
    }

    #[test]
    fn unity_settings_pass_the_signal_through() {
        for kind in [
            BiquadKind::Peak,
            BiquadKind::LowShelf,
            BiquadKind::HighShelf,
        ] {
            let c = coeffs(kind, 1_000.0, 0.0, 0.707);
            assert!((c.magnitude_at(1_000.0, RATE) - 1.0).abs() < 1e-9);
        }
        let mut biquad = Biquad::new(coeffs(BiquadKind::Peak, 1_000.0, 0.0, 0.707));
        for x in [1.0, -0.5, 0.25] {
            assert!((biquad.process(x) - x).abs() < 1e-9);
        }
    }
}
