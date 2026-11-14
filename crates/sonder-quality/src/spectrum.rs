//! Short-time spectra, 100 Hz band levels, and the cutoff / slope measurements on them.
//!
//! Every frame is Hann-windowed with a 50 % hop. Left and right are transformed once each;
//! the mid `(L+R)/2` and side `(L−R)/2` spectra follow from them linearly. Levels are kept in
//! 100 Hz bands: narrow enough to tell a 19.5 kHz lid from a 20 kHz one, wide enough
//! (≥ 18 bins at 44.1 kHz) that a single frame's band level is not just one noisy bin.

use std::sync::Arc;

use realfft::num_complex::Complex;
use realfft::{RealFftPlanner, RealToComplex};

/// Width of one analysis band.
pub(crate) const BAND_HZ: f64 = 100.0;

/// Bands whose median level is the music's reference loudness (2–8 kHz).
const REFERENCE_BANDS: std::ops::Range<usize> = 20..80;

/// A spectral drop at least this steep (dB between the bands just below and just above the
/// cutoff) is a wall: a codec's low-pass or a resampler's brick-wall, not music rolling off.
pub(crate) const WALL_DB: f64 = 25.0;

/// Frames quieter than this (mid channel RMS) say nothing about the spectrum's top edge.
pub(crate) const LOUD_FRAME_DBFS: f64 = -60.0;

pub(crate) fn to_db(power: f64) -> f64 {
    10.0 * power.max(1e-30).log10()
}

/// The FFT length for a sample rate: about 0.1–0.19 s, so a band holds ≥ 18 bins.
pub(crate) fn window_len(rate: u32) -> usize {
    if rate <= 96_000 { 8192 } else { 16_384 }
}

/// Bin ranges of the 100 Hz bands from 0 Hz up to `top_hz`.
#[derive(Debug, Clone)]
pub(crate) struct Bands {
    ranges: Vec<(usize, usize)>,
}

impl Bands {
    /// The 100 Hz analysis bands up to `top_hz`.
    pub(crate) fn new(rate: u32, fft_len: usize, top_hz: f64) -> Self {
        Self::linear(rate, fft_len, BAND_HZ, (top_hz / BAND_HZ).floor() as usize)
    }

    /// `count` equal bands of `width_hz` from 0 Hz.
    pub(crate) fn linear(rate: u32, fft_len: usize, width_hz: f64, count: usize) -> Self {
        let bin_hz = f64::from(rate) / fft_len as f64;
        let bins = fft_len / 2 + 1;
        let ranges = (0..count)
            .map(|b| {
                let lo = ((b as f64 * width_hz) / bin_hz).ceil() as usize;
                let hi = (((b + 1) as f64 * width_hz) / bin_hz).ceil() as usize;
                (lo.min(bins), hi.max(lo + 1).min(bins))
            })
            .collect();
        Self { ranges }
    }

    pub(crate) fn len(&self) -> usize {
        self.ranges.len()
    }

    /// Mean power of each band.
    pub(crate) fn powers(&self, power: &[f64], out: &mut Vec<f64>) {
        out.clear();
        out.extend(self.ranges.iter().map(|&(lo, hi)| {
            let slice = &power[lo..hi];
            if slice.is_empty() {
                0.0
            } else {
                slice.iter().sum::<f64>() / slice.len() as f64
            }
        }));
    }
}

/// Upper edge of band `band`, in Hz.
pub(crate) fn band_top_hz(band: usize) -> f64 {
    (band + 1) as f64 * BAND_HZ
}

pub(crate) fn median(values: &mut [f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    let mid = values.len() / 2;
    Some(if values.len().is_multiple_of(2) {
        f64::midpoint(values[mid - 1], values[mid])
    } else {
        values[mid]
    })
}

/// Interquartile range of `values` (sorted in place).
pub(crate) fn interquartile_range(values: &mut [f64]) -> Option<f64> {
    if values.len() < 4 {
        return None;
    }
    values.sort_by(f64::total_cmp);
    let at = |q: f64| values[((values.len() - 1) as f64 * q).round() as usize];
    Some(at(0.75) - at(0.25))
}

/// The level a band must exceed to count as content: well below the 2–8 kHz reference, but
/// above the spectrum's own floor when it has one. A flat (full-band) spectrum has its floor
/// at the reference, so the `reference − 30` cap keeps every band of it above the threshold.
pub(crate) fn content_threshold(levels_db: &[f64]) -> Option<f64> {
    if levels_db.len() < REFERENCE_BANDS.end {
        return None;
    }
    let mut reference: Vec<f64> = levels_db[REFERENCE_BANDS].to_vec();
    let reference = median(&mut reference)?;
    let window = 10;
    let floor = (10..levels_db.len().saturating_sub(window))
        .map(|b| levels_db[b..b + window].iter().sum::<f64>() / window as f64)
        .fold(f64::INFINITY, f64::min);
    let floor = if floor.is_finite() { floor } else { reference };
    Some((reference - 30.0).min((reference - 75.0).max(floor + 10.0)))
}

/// Index of the highest band holding content: above the threshold both on its own and on a
/// 3-band smoothed spectrum (smoothing alone would spill a wall one band upward; the raw
/// level alone would let one stray band above the floor count).
pub(crate) fn cutoff_band(levels_db: &[f64]) -> Option<usize> {
    let threshold = content_threshold(levels_db)?;
    let smooth = smoothed(levels_db);
    (0..levels_db.len())
        .rev()
        .find(|&b| levels_db[b] > threshold && smooth[b] > threshold)
}

fn smoothed(levels_db: &[f64]) -> Vec<f64> {
    (0..levels_db.len())
        .map(|b| {
            let lo = b.saturating_sub(1);
            let hi = (b + 2).min(levels_db.len());
            levels_db[lo..hi].iter().sum::<f64>() / (hi - lo) as f64
        })
        .collect()
}

/// Level drop (dB) from the bands just below `cutoff` to the bands just above it, or `None`
/// when the cutoff sits at the top of the analysed range.
pub(crate) fn slope_db(levels_db: &[f64], cutoff: usize) -> Option<f64> {
    let above = cutoff + 2..(cutoff + 8).min(levels_db.len());
    if above.is_empty() {
        return None;
    }
    let below = cutoff.saturating_sub(6)..cutoff.saturating_sub(1).max(1);
    let mean =
        |r: std::ops::Range<usize>| levels_db[r.clone()].iter().sum::<f64>() / r.len() as f64;
    Some(mean(below) - mean(above))
}

/// Whether one frame has a hole: a run of at least 500 Hz above 12 kHz that falls below the
/// content threshold while content continues above it (lossy encoders drop whole
/// scale-factor bands they cannot afford, from frame to frame).
pub(crate) fn has_hole(levels_db: &[f64], threshold: f64, cutoff: usize) -> bool {
    let start = (12_000.0 / BAND_HZ) as usize;
    if cutoff <= start + 5 {
        return false;
    }
    let mut run = 0;
    for &level in &levels_db[start..cutoff.saturating_sub(2)] {
        if level < threshold {
            run += 1;
            if run >= 5 {
                return true;
            }
        } else {
            run = 0;
        }
    }
    false
}

/// Running STFT over stereo input, handing each frame's spectra to a callback.
pub(crate) struct Stft {
    fft: Arc<dyn RealToComplex<f64>>,
    window: Vec<f64>,
    len: usize,
    hop: usize,
    left: Vec<f64>,
    right: Vec<f64>,
    scratch_in: Vec<f64>,
    scratch: Vec<Complex<f64>>,
    spec_l: Vec<Complex<f64>>,
    spec_r: Vec<Complex<f64>>,
}

/// Power spectra of one frame, plus the mid channel's time-domain RMS in dBFS.
pub(crate) struct Frame<'a> {
    pub left: &'a [f64],
    pub right: &'a [f64],
    pub mid: &'a [f64],
    pub side: &'a [f64],
    pub rms_dbfs: f64,
}

impl Stft {
    pub(crate) fn new(len: usize) -> Self {
        let mut planner = RealFftPlanner::<f64>::new();
        let fft = planner.plan_fft_forward(len);
        let window = (0..len)
            .map(|n| 0.5 - 0.5 * (std::f64::consts::TAU * n as f64 / len as f64).cos())
            .collect();
        let scratch = fft.make_scratch_vec();
        let spec_l = fft.make_output_vec();
        let spec_r = fft.make_output_vec();
        Self {
            fft,
            window,
            len,
            hop: len / 2,
            left: Vec::with_capacity(len * 2),
            right: Vec::with_capacity(len * 2),
            scratch_in: vec![0.0; len],
            scratch,
            spec_l,
            spec_r,
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.len
    }

    /// Appends samples (`left.len() == right.len()`) and emits every complete frame.
    pub(crate) fn push(
        &mut self,
        left: &[f64],
        right: &[f64],
        powers: &mut FramePowers,
        mut on_frame: impl FnMut(&Frame<'_>),
    ) {
        self.left.extend_from_slice(left);
        self.right.extend_from_slice(right);
        let mut start = 0;
        while self.left.len() - start >= self.len {
            let l = &self.left[start..start + self.len];
            let r = &self.right[start..start + self.len];
            let mut sum_sq = 0.0;
            for (a, b) in l.iter().zip(r) {
                let m = 0.5 * (a + b);
                sum_sq += m * m;
            }
            let rms_dbfs = to_db(sum_sq / self.len as f64 * 2.0);

            transform(
                &*self.fft,
                &self.window,
                l,
                &mut self.scratch_in,
                &mut self.spec_l,
                &mut self.scratch,
            );
            transform(
                &*self.fft,
                &self.window,
                r,
                &mut self.scratch_in,
                &mut self.spec_r,
                &mut self.scratch,
            );
            powers.fill(&self.spec_l, &self.spec_r);
            on_frame(&Frame {
                left: &powers.left,
                right: &powers.right,
                mid: &powers.mid,
                side: &powers.side,
                rms_dbfs,
            });
            start += self.hop;
        }
        self.left.drain(..start);
        self.right.drain(..start);
    }
}

fn transform(
    fft: &dyn RealToComplex<f64>,
    window: &[f64],
    input: &[f64],
    buf: &mut [f64],
    out: &mut [Complex<f64>],
    scratch: &mut [Complex<f64>],
) {
    for ((b, x), w) in buf.iter_mut().zip(input).zip(window) {
        *b = x * w;
    }
    // Lengths come from the same plan, so this cannot fail.
    fft.process_with_scratch(buf, out, scratch)
        .expect("buffers sized by the FFT plan");
}

/// Per-bin power of the four channel views of one frame, normalised so a full-scale sine
/// reads about 0 dB.
#[derive(Default)]
pub(crate) struct FramePowers {
    pub left: Vec<f64>,
    pub right: Vec<f64>,
    pub mid: Vec<f64>,
    pub side: Vec<f64>,
    scale: f64,
}

impl FramePowers {
    pub(crate) fn new(fft_len: usize) -> Self {
        // Hann coherent gain is 1/2: a full-scale sine peaks at len/4 in magnitude.
        let peak = fft_len as f64 / 4.0;
        Self {
            scale: 1.0 / (peak * peak),
            ..Self::default()
        }
    }

    fn fill(&mut self, l: &[Complex<f64>], r: &[Complex<f64>]) {
        let scale = self.scale;
        for v in [
            &mut self.left,
            &mut self.right,
            &mut self.mid,
            &mut self.side,
        ] {
            v.clear();
        }
        for (a, b) in l.iter().zip(r) {
            self.left.push(a.norm_sqr() * scale);
            self.right.push(b.norm_sqr() * scale);
            self.mid.push(((a + b) * 0.5).norm_sqr() * scale);
            self.side.push(((a - b) * 0.5).norm_sqr() * scale);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat_until(len: usize, cutoff: usize, level: f64, floor: f64) -> Vec<f64> {
        (0..len)
            .map(|b| if b <= cutoff { level } else { floor })
            .collect()
    }

    #[test]
    fn a_full_band_spectrum_has_its_cutoff_at_the_top() {
        let levels = vec![-40.0; 220];
        assert_eq!(cutoff_band(&levels), Some(219));
        assert_eq!(slope_db(&levels, 219), None);
    }

    #[test]
    fn a_brick_wall_is_found_and_reads_as_steep() {
        let levels = flat_until(220, 159, -40.0, -140.0);
        let cutoff = cutoff_band(&levels).unwrap();
        assert!((158..=160).contains(&cutoff), "{cutoff}");
        assert!(slope_db(&levels, cutoff).unwrap() > WALL_DB);
    }

    #[test]
    fn a_gentle_roll_off_is_not_a_wall() {
        // 6 dB per kHz from 10 kHz on: music losing its treble, not a codec lid.
        let levels: Vec<f64> = (0..220)
            .map(|b| {
                if b < 100 {
                    -40.0
                } else {
                    -40.0 - 0.6 * (b - 100) as f64
                }
            })
            .collect();
        let cutoff = cutoff_band(&levels).unwrap();
        assert!(slope_db(&levels, cutoff).is_none_or(|s| s < WALL_DB));
    }

    #[test]
    fn a_hole_needs_content_above_it() {
        let mut levels = flat_until(220, 199, -40.0, -140.0);
        for l in &mut levels[150..158] {
            *l = -140.0;
        }
        let threshold = content_threshold(&levels).unwrap();
        assert!(has_hole(&levels, threshold, 199));
        let plain = flat_until(220, 199, -40.0, -140.0);
        assert!(!has_hole(&plain, threshold, 199));
    }

    #[test]
    fn iqr_of_a_constant_series_is_zero() {
        let mut v = vec![16_000.0; 20];
        assert_eq!(interquartile_range(&mut v), Some(0.0));
    }
}
