//! Decoding a track and reducing it to the measurements the criteria judge.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use sonder_decode::SourceDecoder;
use sonder_dsp::DsdToPcm;

use crate::spectrum::{
    Bands, FramePowers, LOUD_FRAME_DBFS, Stft, band_top_hz, content_threshold, cutoff_band,
    has_hole, interquartile_range, median, slope_db, to_db, window_len,
};
use crate::{Options, QualityError, SourceInfo, Spectrogram, SpectrumPoint};

/// DSD is converted to PCM at this rate for analysis.
const DSD_ANALYSIS_RATE: u32 = 176_400;
/// Above this the DSD modulator's own noise rises; the cutoff search stops here.
const DSD_ANALYSED_TOP_HZ: f64 = 24_000.0;

const SPECTRUM_POINTS: usize = 512;
const SPECTROGRAM_ROWS: usize = 256;
const MAX_COLUMNS: usize = 400;
/// Fewer loud frames than this make the frame statistics meaningless.
pub(crate) const MIN_LOUD_FRAMES: usize = 8;
/// Below this level a frame is digital silence, not the noise floor.
const SILENCE_DBFS: f64 = -150.0;

/// Everything [`crate::criteria`] and [`crate::verdict`] need, in plain numbers.
#[derive(Debug, Clone, Default)]
pub(crate) struct Measured {
    pub source: SourceInfo,
    pub duration_secs: f64,
    pub loud_frames: usize,
    pub cutoff_hz: Option<f64>,
    pub slope_db: Option<f64>,
    pub stability_iqr_hz: Option<f64>,
    pub hole_fraction: Option<f64>,
    pub stereo_collapse_db: Option<f64>,
    pub channel_delta_hz: Option<f64>,
    pub real_bits: Option<u32>,
    pub quiet_level_dbfs: Option<f64>,
    pub ultrasonic_db: Option<f64>,
    pub dsd_drop_db: Option<f64>,
    /// FLAC carries an MD5 of its decoded audio; other containers have nothing to check.
    pub flac: bool,
    pub md5: Option<bool>,
    pub spectrum: Vec<SpectrumPoint>,
    pub spectrum_top_hz: f64,
    pub cutoff_track: Vec<f32>,
    pub spectrogram: Option<Spectrogram>,
}

pub(crate) fn measure(
    path: &Path,
    options: &Options,
    cancel: &AtomicBool,
    mut progress: impl FnMut(f32),
) -> Result<Measured, QualityError> {
    let mut decoder = SourceDecoder::open_verifying(path)?;
    let total = decoder.total();
    let mut report_progress = |position: u64| {
        if let Some(total) = total.filter(|&t| t > 0) {
            progress((position as f64 / total as f64).min(1.0) as f32);
        }
    };
    let check_cancel = || {
        if cancel.load(Ordering::Relaxed) {
            Err(QualityError::Cancelled)
        } else {
            Ok(())
        }
    };

    match &mut decoder {
        SourceDecoder::Pcm(pcm) => {
            let format = pcm.format();
            let source = SourceInfo {
                rate: format.rate,
                bits: format.bits,
                channels: format.channels,
                dsd: false,
            };
            let nyquist = f64::from(format.rate) / 2.0;
            let mut acc = Accumulator::new(format.rate, nyquist, nyquist, options.spectrogram);
            let channels = usize::from(format.channels);
            let mut block = Vec::new();
            let (mut left, mut right) = (Vec::new(), Vec::new());
            let mut or_bits = 0u32;
            let mut frames = 0u64;
            loop {
                check_cancel()?;
                let n = pcm.next_i32(&mut block)?;
                if n == 0 {
                    break;
                }
                frames += n as u64;
                left.clear();
                right.clear();
                for frame in block.chunks_exact(channels) {
                    for &s in frame {
                        or_bits |= s as u32;
                    }
                    let l = f64::from(frame[0]) / 2_147_483_648.0;
                    let r = frame.get(1).map_or(l, |&s| f64::from(s) / 2_147_483_648.0);
                    left.push(l);
                    right.push(r);
                }
                acc.push(&left, &right);
                report_progress(pcm.position());
            }
            let md5 = pcm.verification();
            let mut measured = acc.finish(source, channels > 1);
            measured.duration_secs = frames as f64 / f64::from(format.rate);
            measured.real_bits = (or_bits != 0).then(|| 32 - or_bits.trailing_zeros());
            measured.md5 = md5;
            measured.flac = path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("flac"));
            Ok(measured)
        }
        SourceDecoder::Dsd(dsd) => {
            let format = dsd.format();
            let mut convert = DsdToPcm::new(format.rate, DSD_ANALYSIS_RATE, format.channels)
                .ok_or(QualityError::UnsupportedDsd(format.rate))?;
            let source = SourceInfo {
                rate: format.rate,
                bits: 1,
                channels: format.channels,
                dsd: true,
            };
            let mut acc = Accumulator::new(
                DSD_ANALYSIS_RATE,
                DSD_ANALYSED_TOP_HZ,
                f64::from(DSD_ANALYSIS_RATE) / 2.0,
                options.spectrogram,
            );
            let channels = usize::from(format.channels);
            let (mut bytes, mut pcm) = (Vec::new(), Vec::new());
            let (mut left, mut right) = (Vec::new(), Vec::new());
            let mut frames = 0u64;
            loop {
                check_cancel()?;
                if dsd.next_bytes(&mut bytes)? == 0 {
                    break;
                }
                pcm.clear();
                convert.process(&bytes, &mut pcm);
                left.clear();
                right.clear();
                for frame in pcm.chunks_exact(channels) {
                    left.push(frame[0]);
                    right.push(*frame.get(1).unwrap_or(&frame[0]));
                }
                frames += left.len() as u64;
                acc.push(&left, &right);
                report_progress(dsd.position());
            }
            let mut measured = acc.finish(source, channels > 1);
            measured.duration_secs = frames as f64 / f64::from(DSD_ANALYSIS_RATE);
            Ok(measured)
        }
    }
}

/// Collects frame statistics as the STFT runs.
struct Accumulator {
    rate: u32,
    stft: Stft,
    powers: FramePowers,
    /// 100 Hz bands up to the analysed top.
    bands: Bands,

    spectrum_top_hz: f64,
    band_buf: Vec<f64>,
    band_db: Vec<f64>,
    sum_mid_bins: Vec<f64>,
    sum_left: Vec<f64>,
    sum_right: Vec<f64>,
    sum_mid: Vec<f64>,
    sum_side: Vec<f64>,
    frames: usize,
    loud: usize,
    holes: usize,
    /// (frame index, cutoff Hz) of every loud frame with a measurable cutoff.
    frame_cutoffs: Vec<(usize, f64)>,
    frame_rms: Vec<f64>,
    /// Per-frame mid power in the spectrogram's rows, when one is built.
    map: Option<(Bands, Vec<f32>)>,
}

impl Accumulator {
    fn new(rate: u32, analysed_top_hz: f64, spectrum_top_hz: f64, spectrogram: bool) -> Self {
        let len = window_len(rate);
        let bands = Bands::new(rate, len, analysed_top_hz);
        let n = bands.len();
        let map = spectrogram.then(|| {
            (
                Bands::linear(
                    rate,
                    len,
                    spectrum_top_hz / SPECTROGRAM_ROWS as f64,
                    SPECTROGRAM_ROWS,
                ),
                Vec::new(),
            )
        });
        Self {
            rate,
            stft: Stft::new(len),
            powers: FramePowers::new(len),
            bands,

            spectrum_top_hz,
            band_buf: Vec::with_capacity(n),
            band_db: Vec::with_capacity(n),
            sum_mid_bins: vec![0.0; len / 2 + 1],
            sum_left: vec![0.0; n],
            sum_right: vec![0.0; n],
            sum_mid: vec![0.0; n],
            sum_side: vec![0.0; n],
            frames: 0,
            loud: 0,
            holes: 0,
            frame_cutoffs: Vec::new(),
            frame_rms: Vec::new(),
            map,
        }
    }

    fn push(&mut self, left: &[f64], right: &[f64]) {
        let Self {
            stft,
            powers,
            bands,
            band_buf,
            band_db,
            sum_mid_bins,
            sum_left,
            sum_right,
            sum_mid,
            sum_side,
            frames,
            loud,
            holes,
            frame_cutoffs,
            frame_rms,
            map,
            ..
        } = self;
        stft.push(left, right, powers, |frame| {
            let index = *frames;
            *frames += 1;
            frame_rms.push(frame.rms_dbfs);
            if let Some((rows, cells)) = map {
                rows.powers(frame.mid, band_buf);
                cells.extend(band_buf.iter().map(|&p| p as f32));
            }
            if frame.rms_dbfs < LOUD_FRAME_DBFS {
                return;
            }
            *loud += 1;
            for (sum, p) in sum_mid_bins.iter_mut().zip(frame.mid) {
                *sum += p;
            }
            for (sum, spectrum) in [
                (&mut *sum_left, frame.left),
                (&mut *sum_right, frame.right),
                (&mut *sum_side, frame.side),
                (&mut *sum_mid, frame.mid),
            ] {
                bands.powers(spectrum, band_buf);
                for (s, p) in sum.iter_mut().zip(band_buf.iter()) {
                    *s += p;
                }
            }
            // `band_buf` now holds the mid bands of this frame.
            band_db.clear();
            band_db.extend(band_buf.iter().map(|&p| to_db(p)));
            if let (Some(threshold), Some(cutoff)) =
                (content_threshold(band_db), cutoff_band(band_db))
            {
                frame_cutoffs.push((index, band_top_hz(cutoff)));
                // Holes are a property of lossy frames: only a frame whose top edge sits where
                // codecs cut is searched. Sparse treble (a solo piano) or hi-res ultrasonic
                // noise would otherwise read as holes.
                if band_top_hz(cutoff) <= crate::LOSSY_MAX_HZ
                    && has_hole(band_db, threshold, cutoff)
                {
                    *holes += 1;
                }
            }
        });
    }

    fn finish(self, source: SourceInfo, stereo: bool) -> Measured {
        let loud = self.loud;
        let mut m = Measured {
            source,
            loud_frames: loud,
            spectrum_top_hz: self.spectrum_top_hz,
            ..Measured::default()
        };
        m.quiet_level_dbfs = {
            let mut levels: Vec<f64> = self
                .frame_rms
                .iter()
                .copied()
                .filter(|&l| l > SILENCE_DBFS)
                .collect();
            levels.sort_by(f64::total_cmp);
            (!levels.is_empty()).then(|| levels[(levels.len() - 1) * 5 / 100])
        };
        let columns = self.frames.clamp(1, MAX_COLUMNS);
        m.cutoff_track = column_medians(&self.frame_cutoffs, self.frames, columns);
        m.spectrogram = self
            .map
            .as_ref()
            .map(|(_, cells)| spectrogram(cells, self.frames, columns, self.spectrum_top_hz));
        if loud == 0 {
            return m;
        }

        let avg_db =
            |sum: &[f64]| -> Vec<f64> { sum.iter().map(|&s| to_db(s / loud as f64)).collect() };
        let mid = avg_db(&self.sum_mid);
        let reference = median(&mut mid[20.min(mid.len())..80.min(mid.len())].to_vec());

        let cutoff = cutoff_band(&mid);
        m.cutoff_hz = cutoff.map(band_top_hz);
        m.slope_db = cutoff.and_then(|c| slope_db(&mid, c));

        if loud >= MIN_LOUD_FRAMES {
            let mut cutoffs: Vec<f64> = self.frame_cutoffs.iter().map(|&(_, hz)| hz).collect();
            m.stability_iqr_hz = interquartile_range(&mut cutoffs);
            m.hole_fraction = Some(self.holes as f64 / loud as f64);
        }

        if stereo {
            let left = cutoff_band(&avg_db(&self.sum_left));
            let right = cutoff_band(&avg_db(&self.sum_right));
            if let (Some(l), Some(r)) = (left, right) {
                m.channel_delta_hz = Some((band_top_hz(l) - band_top_hz(r)).abs());
            }
            m.stereo_collapse_db =
                cutoff.and_then(|c| stereo_collapse(&self.sum_mid, &self.sum_side, c));
        }

        let nyquist = f64::from(self.rate) / 2.0;
        if !source.dsd && source.rate >= 88_200 {
            let lo = (24_000.0 / crate::spectrum::BAND_HZ) as usize;
            let hi = ((0.9 * nyquist) / crate::spectrum::BAND_HZ) as usize;
            if let (Some(reference), true) = (reference, hi > lo && hi <= mid.len()) {
                m.ultrasonic_db = Some(mean(&mid[lo..hi]) - reference);
            }
        }
        if source.dsd && mid.len() >= 240 {
            m.dsd_drop_db = Some(mean(&mid[170..195]) - mean(&mid[225..240]));
        }

        m.spectrum = spectrum_points(
            &self.sum_mid_bins,
            loud,
            f64::from(self.rate) / self.stft.len() as f64,
            self.spectrum_top_hz,
        );
        m
    }
}

fn mean(values: &[f64]) -> f64 {
    values.iter().sum::<f64>() / values.len().max(1) as f64
}

/// Side-to-mid ratio at 2–8 kHz minus the same ratio from 14 kHz to just under the cutoff
/// (at most 20 kHz). `None` for near-mono material or too little treble to compare.
fn stereo_collapse(mid: &[f64], side: &[f64], cutoff: usize) -> Option<f64> {
    let ratio = |r: std::ops::Range<usize>| {
        to_db(side[r.clone()].iter().sum::<f64>()) - to_db(mid[r].iter().sum::<f64>())
    };
    let low = ratio(20..80.min(mid.len()));
    if low < -30.0 {
        return None;
    }
    let hi_end = cutoff.saturating_sub(3).min(200);
    if hi_end < 140 + 10 {
        return None;
    }
    Some(low - ratio(140..hi_end))
}

fn column_medians(cutoffs: &[(usize, f64)], frames: usize, columns: usize) -> Vec<f32> {
    let mut buckets: Vec<Vec<f64>> = vec![Vec::new(); columns];
    for &(index, hz) in cutoffs {
        let col = (index * columns / frames.max(1)).min(columns - 1);
        buckets[col].push(hz);
    }
    buckets
        .into_iter()
        .map(|mut b| median(&mut b).unwrap_or(0.0) as f32)
        .collect()
}

fn spectrogram(cells: &[f32], frames: usize, columns: usize, top_hz: f64) -> Spectrogram {
    let rows = SPECTROGRAM_ROWS;
    let mut grid = vec![0.0f64; rows * columns];
    let mut counts = vec![0usize; columns];
    for (index, frame) in cells.chunks_exact(rows).enumerate() {
        let col = (index * columns / frames.max(1)).min(columns - 1);
        counts[col] += 1;
        for (row, &p) in frame.iter().enumerate() {
            grid[row * columns + col] += f64::from(p);
        }
    }
    let db: Vec<f64> = grid
        .iter()
        .enumerate()
        .map(|(i, &p)| to_db(p / counts[i % columns].max(1) as f64))
        .collect();
    let peak = db.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    Spectrogram {
        cols: columns as u32,
        rows: rows as u32,
        top_hz,
        db: db
            .iter()
            .map(|&d| (((d - peak + 120.0) / 120.0).clamp(0.0, 1.0) * 255.0).round() as u8)
            .collect(),
    }
}

/// The average spectrum on a log frequency axis, relative to its loudest point.
fn spectrum_points(sum_bins: &[f64], loud: usize, bin_hz: f64, top_hz: f64) -> Vec<SpectrumPoint> {
    let lo_hz: f64 = 20.0;
    let step = (top_hz / lo_hz).powf(1.0 / SPECTRUM_POINTS as f64);
    let mut points: Vec<(f64, f64)> = Vec::with_capacity(SPECTRUM_POINTS);
    for i in 0..SPECTRUM_POINTS {
        let f_lo = lo_hz * step.powi(i as i32);
        let f_hi = f_lo * step;
        let a = ((f_lo / bin_hz).floor() as usize).min(sum_bins.len() - 1);
        let b = ((f_hi / bin_hz).ceil() as usize).clamp(a + 1, sum_bins.len());
        let p = sum_bins[a..b].iter().sum::<f64>() / ((b - a) * loud) as f64;
        points.push(((f_lo * f_hi).sqrt(), to_db(p)));
    }
    let peak = points.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
    points
        .into_iter()
        .map(|(hz, db)| SpectrumPoint {
            hz: hz as f32,
            db: (db - peak).max(-160.0) as f32,
        })
        .collect()
}
