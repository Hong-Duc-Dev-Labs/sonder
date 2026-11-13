//! EBU R128 / ITU-R BS.1770 integrated loudness and sample peak.
//!
//! Used by the library's background analysis for files without ReplayGain tags: the job
//! decodes each track, feeds [`LoudnessMeter`], and stores the result so levelling has
//! something to work with. The meter is streaming and allocation-free while processing.
//!
//! Implementation notes: K-weighting follows the BS.1770 analog prototypes at the stream's
//! own rate (pre-filter high shelf plus the RLB high-pass), blocks are 400 ms with a 100 ms
//! hop, absolute gating is −70 LUFS and relative gating −10 LU below the gated mean. LFE is
//! excluded for 5.1 material. Sample peak is the largest absolute sample, not true peak.

use std::f64::consts::PI;
use std::path::{Path, PathBuf};

use rusqlite::{OptionalExtension, params};
use sonder_decode::Decoder;

use crate::tags::ReplayGain;
use crate::{Library, LibraryError};

/// One second-order section in transposed direct form II.
#[derive(Debug, Clone, Copy, Default)]
struct Section {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    z1: f64,
    z2: f64,
}

impl Section {
    fn process(&mut self, x: f64) -> f64 {
        let y = self.b0.mul_add(x, self.z1);
        self.z1 = self.b1.mul_add(x, self.z2) - self.a1 * y;
        self.z2 = self.b2.mul_add(x, -self.a2 * y);
        y
    }

    fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }
}

/// The two K-weighting sections, coefficients for one sample rate.
#[derive(Debug, Clone, Copy)]
struct KWeight {
    pre: Section,
    rlb: Section,
}

impl KWeight {
    fn new(rate: u32) -> Self {
        let fs = f64::from(rate.max(1));
        // BS.1770 analog prototypes (same constants libebur128 derives its filters from).
        let (shelf_f0, shelf_gain, shelf_q) = (
            1_681.974_450_955_533,
            3.999_843_853_973_347,
            0.707_175_236_955_419_6,
        );
        let k = (PI * shelf_f0 / fs).tan();
        let vh = 10f64.powf(shelf_gain / 20.0);
        let vb = vh.powf(0.499_666_774_154_541_6);
        let a0 = 1.0 + k / shelf_q + k * k;
        let pre = Section {
            b0: (vh + vb * k / shelf_q + k * k) / a0,
            b1: 2.0 * (k * k - vh) / a0,
            b2: (vh - vb * k / shelf_q + k * k) / a0,
            a1: 2.0 * (k * k - 1.0) / a0,
            a2: (1.0 - k / shelf_q + k * k) / a0,
            ..Section::default()
        };

        let (highpass_f0, highpass_q) = (38.135_470_876_024_44, 0.500_327_037_323_877_3);
        let k = (PI * highpass_f0 / fs).tan();
        let a0 = 1.0 + k / highpass_q + k * k;
        let rlb = Section {
            b0: 1.0 / a0,
            b1: -2.0 / a0,
            b2: 1.0 / a0,
            a1: 2.0 * (k * k - 1.0) / a0,
            a2: (1.0 - k / highpass_q + k * k) / a0,
            ..Section::default()
        };
        Self { pre, rlb }
    }

    fn process(&mut self, x: f64) -> f64 {
        self.rlb.process(self.pre.process(x))
    }

    fn reset(&mut self) {
        self.pre.reset();
        self.rlb.reset();
    }

    /// Power gain of the cascade at `freq_hz`, for tests.
    #[cfg(test)]
    fn power_at(&self, freq_hz: f64, rate: u32) -> f64 {
        let w = 2.0 * PI * freq_hz / f64::from(rate);
        let (sin1, cos1) = w.sin_cos();
        let (sin2, cos2) = (2.0 * w).sin_cos();
        let magnitude = |s: &Section| {
            let re = s.b0 + s.b1 * cos1 + s.b2 * cos2;
            let im = -(s.b1 * sin1 + s.b2 * sin2);
            let dre = 1.0 + s.a1 * cos1 + s.a2 * cos2;
            let dim = -(s.a1 * sin1 + s.a2 * sin2);
            (re.hypot(im) / dre.hypot(dim)).powi(2)
        };
        magnitude(&self.pre) * magnitude(&self.rlb)
    }
}

/// Largest channel count the meter weights. Channels past it still count towards the peak but
/// not the loudness, which BS.1770 does not define for wider layouts anyway.
const MAX_CHANNELS: usize = 8;
/// BS.1770 weight of the surround channels (Ls, Rs) in 5.1: +1.5 dB.
const SURROUND_WEIGHT: f64 = 1.41;
/// 400 ms analysis window, at 100 ms hops.
const BLOCKS_PER_WINDOW: usize = 4;
/// BS.1770 offset so a 997 Hz sine at 0 dBFS reads 0 LUFS.
const LOUDNESS_OFFSET: f64 = -0.691;
/// Absolute gate.
pub const ABSOLUTE_GATE_LUFS: f64 = -70.0;
/// Blocks more than this many LU below the gated mean are dropped.
pub const RELATIVE_GATE_LU: f64 = 10.0;
/// Reported instead of a measurement: just below the absolute gate.
const SILENT_LUFS: f64 = ABSOLUTE_GATE_LUFS - 1.0;
/// Loudness a ReplayGain 2.0 gain tag brings a track to (89 dB SPL reference).
pub const REPLAYGAIN_REFERENCE_LUFS: f64 = -18.0;

/// Integrated loudness in LUFS and linear sample peak.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Loudness {
    pub lufs: f64,
    pub peak: f64,
}

impl Loudness {
    /// Minutes of audio are decoded faster than real time, so an analysis that finds nothing
    /// (shorter than one 400 ms block, or only silent blocks) reports the absolute gate.
    pub fn silent() -> Self {
        Self {
            lufs: SILENT_LUFS,
            peak: 0.0,
        }
    }

    /// Whether `lufs` is a real measurement, not the silent placeholder.
    pub fn is_measured(&self) -> bool {
        self.lufs > ABSOLUTE_GATE_LUFS
    }
}

/// A block's mean square, summed over channels (the `z` of BS.1770).
#[derive(Debug, Clone, Copy)]
struct Block {
    energy: f64,
    lufs: f64,
}

/// Streaming BS.1770 loudness and peak meter.
pub struct LoudnessMeter {
    rate: u32,
    /// Interleaved channel count of the stream.
    channels: usize,
    /// Channels the loudness is measured on: `channels`, capped at [`MAX_CHANNELS`].
    weighted: usize,
    weights: [f64; MAX_CHANNELS],
    filters: [KWeight; MAX_CHANNELS],
    /// Frames accumulated in the current 100 ms sub-block.
    sub_frames: usize,
    sub_target: usize,
    sub_energy: [f64; MAX_CHANNELS],
    /// The last four sub-blocks, indexed modulo four.
    ring: [[f64; MAX_CHANNELS]; BLOCKS_PER_WINDOW],
    sub_seen: usize,
    blocks: Vec<Block>,
    peak: f64,
}

impl LoudnessMeter {
    pub fn new(rate: u32, channels: u16) -> Self {
        let channels = usize::from(channels.max(1));
        let weighted = channels.min(MAX_CHANNELS);
        let mut weights = [1.0; MAX_CHANNELS];
        // 5.1 order in Sonder is L R C LFE Ls Rs: the LFE is not part of the loudness and the
        // surrounds weigh +1.5 dB.
        if channels == 6 {
            weights[3] = 0.0;
            weights[4] = SURROUND_WEIGHT;
            weights[5] = SURROUND_WEIGHT;
        }
        for weight in weights.iter_mut().skip(weighted) {
            *weight = 0.0;
        }
        let k = KWeight::new(rate);
        Self {
            rate,
            channels,
            weighted,
            weights,
            filters: [k; MAX_CHANNELS],
            sub_frames: 0,
            sub_target: (rate as usize / 10).max(1),
            sub_energy: [0.0; MAX_CHANNELS],
            ring: [[0.0; MAX_CHANNELS]; BLOCKS_PER_WINDOW],
            sub_seen: 0,
            blocks: Vec::new(),
            peak: 0.0,
        }
    }

    pub fn rate(&self) -> u32 {
        self.rate
    }

    pub fn channels(&self) -> usize {
        self.channels
    }

    /// Forgets filter state and measurements, keeping the allocation.
    pub fn reset(&mut self) {
        for filter in &mut self.filters {
            filter.reset();
        }
        self.sub_frames = 0;
        self.sub_energy = [0.0; MAX_CHANNELS];
        self.ring = [[0.0; MAX_CHANNELS]; BLOCKS_PER_WINDOW];
        self.sub_seen = 0;
        self.blocks.clear();
        self.peak = 0.0;
    }

    /// Adds interleaved `f64` frames. Must not allocate.
    pub fn push(&mut self, samples: &[f64]) {
        let channels = self.channels;
        for frame in samples.chunks_exact(channels) {
            for (ch, &x) in frame.iter().enumerate() {
                let a = x.abs();
                if a > self.peak {
                    self.peak = a;
                }
                if let Some(&weight) = self.weights.get(ch)
                    && weight > 0.0
                {
                    let y = self.filters[ch].process(x);
                    self.sub_energy[ch] += y * y;
                }
            }
            self.sub_frames += 1;
            if self.sub_frames == self.sub_target {
                self.finish_sub_block();
            }
        }
    }

    fn finish_sub_block(&mut self) {
        let slot = self.sub_seen % BLOCKS_PER_WINDOW;
        for ch in 0..self.weighted {
            self.ring[slot][ch] = self.sub_energy[ch] / self.sub_frames as f64;
            self.sub_energy[ch] = 0.0;
        }
        self.sub_frames = 0;
        self.sub_seen += 1;
        if self.sub_seen < BLOCKS_PER_WINDOW {
            return;
        }
        // Complete 400 ms window: average its four 100 ms sub-blocks.
        let mut energy = 0.0;
        for ch in 0..self.weighted {
            let sum: f64 = self.ring.iter().map(|sub| sub[ch]).sum();
            energy += (sum / BLOCKS_PER_WINDOW as f64) * self.weights[ch];
        }
        let lufs = loudness_of(energy);
        if lufs > ABSOLUTE_GATE_LUFS {
            self.blocks.push(Block { energy, lufs });
        }
    }

    /// Integrated loudness and peak of everything pushed since the last [`reset`](Self::reset).
    pub fn finish(&self) -> Loudness {
        Loudness {
            lufs: self.integrated_lufs(),
            peak: self.peak,
        }
    }

    /// Gated integrated loudness, [`SILENT_LUFS`] when no block survives the gates.
    fn integrated_lufs(&self) -> f64 {
        if self.blocks.is_empty() {
            return SILENT_LUFS;
        }
        let mean = self.blocks.iter().map(|b| b.energy).sum::<f64>() / self.blocks.len() as f64;
        let threshold = loudness_of(mean) - RELATIVE_GATE_LU;
        let mut energy = 0.0;
        let mut kept = 0;
        for block in &self.blocks {
            if block.lufs > threshold {
                energy += block.energy;
                kept += 1;
            }
        }
        if kept == 0 {
            SILENT_LUFS
        } else {
            loudness_of(energy / kept as f64)
        }
    }
}

/// BS.1770: `−0.691 + 10·log10(z)` over the channel-weighted mean square.
fn loudness_of(energy: f64) -> f64 {
    if energy > 0.0 {
        LOUDNESS_OFFSET + 10.0 * energy.log10()
    } else {
        f64::NEG_INFINITY
    }
}

/// ReplayGain tags plus whatever the background analysis measured for one track.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TrackLoudness {
    pub album_id: i64,
    pub replay_gain: ReplayGain,
    /// Measured integrated loudness and sample peak; `None` until analysed.
    pub loudness: Option<Loudness>,
    /// The measured values for the whole album, repeated on every track of it.
    pub album: Option<Loudness>,
}

impl TrackLoudness {
    /// Gain and peak for track mode, from tags first and the measurement second.
    ///
    /// A tag gain aims at the ReplayGain reference, so it is shifted by the distance between
    /// that reference and `target_lufs`; tagged and measured tracks then meet the same target.
    /// A placeholder measurement (silent, or shorter than one block) gives no gain at all.
    pub fn track_gain(&self, target_lufs: f64) -> Option<(f64, f64)> {
        if let Some(gain) = self.replay_gain.track_gain {
            return Some((
                gain + target_lufs - REPLAYGAIN_REFERENCE_LUFS,
                self.replay_gain.track_peak.unwrap_or(0.0),
            ));
        }
        let measured = self.loudness.filter(Loudness::is_measured)?;
        Some((target_lufs - measured.lufs, measured.peak))
    }

    /// Gain and peak for album mode, from tags first and the measurement second, with the
    /// same reference shift as [`track_gain`](Self::track_gain).
    pub fn album_gain(&self, target_lufs: f64) -> Option<(f64, f64)> {
        if let Some(gain) = self.replay_gain.album_gain {
            return Some((
                gain + target_lufs - REPLAYGAIN_REFERENCE_LUFS,
                self.replay_gain.album_peak.unwrap_or(0.0),
            ));
        }
        let measured = self.album.filter(Loudness::is_measured)?;
        Some((target_lufs - measured.lufs, measured.peak))
    }
}

/// What an analysis pass changed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LoudnessReport {
    pub tracks: usize,
    pub albums: usize,
    pub failed: usize,
}

/// How far the background analysis is, reported after every album.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoudnessProgress {
    pub done: usize,
    pub total: usize,
    pub failed: usize,
}

impl Library {
    /// Tracks still waiting for an analysis: PCM files with neither ReplayGain tags nor a
    /// measurement. DSD is skipped because it is not a PCM signal.
    pub fn loudness_pending(&self) -> Result<usize, LibraryError> {
        let count: i64 = self.conn.query_row(
            "SELECT count(*) FROM tracks
             WHERE loudness_lufs IS NULL AND rg_track_gain IS NULL
               AND codec NOT IN ('dsf', 'dff')",
            [],
            |row| row.get(0),
        )?;
        Ok(usize::try_from(count).unwrap_or(usize::MAX))
    }

    /// Analyses every pending track, album by album.
    pub fn analyze_loudness(&mut self) -> Result<LoudnessReport, LibraryError> {
        self.analyze_loudness_with_progress(&|_| {})
    }

    /// [`analyze_loudness`](Self::analyze_loudness), reporting progress after every album.
    ///
    /// Album values are a duration-weighted energy average of the measured tracks, so an
    /// album's relative track loudness survives album-mode levelling. A file that cannot be
    /// decoded is counted and retried on the next pass; the rest of the album is still used.
    pub fn analyze_loudness_with_progress(
        &mut self,
        progress: &(dyn Fn(LoudnessProgress) + Sync),
    ) -> Result<LoudnessReport, LibraryError> {
        let pending: Vec<PendingTrack> = self
            .conn
            .prepare(
                "SELECT id, album_id, path, mtime, size FROM tracks
                 WHERE loudness_lufs IS NULL AND rg_track_gain IS NULL
                   AND codec NOT IN ('dsf', 'dff')
                 ORDER BY album_id, coalesce(disc_no, 1), track_no IS NULL, track_no, path",
            )?
            .query_map([], |row| {
                Ok(PendingTrack {
                    id: row.get(0)?,
                    album_id: row.get(1)?,
                    path: PathBuf::from(row.get::<_, String>(2)?),
                    mtime: row.get(3)?,
                    size: row.get(4)?,
                })
            })?
            .collect::<Result<_, _>>()?;

        let mut report = LoudnessReport::default();
        let total = pending.len();
        // The query orders by album, so each run of equal album ids is one album.
        for group in pending.chunk_by(|a, b| a.album_id == b.album_id) {
            let album_id = group[0].album_id;
            // Decode the whole album before writing: decoding takes seconds per track, and an
            // open write transaction would hold the database lock (and the scanner) that long.
            let measured: Vec<(&PendingTrack, Loudness)> = group
                .iter()
                .filter_map(|track| match analyse_file_isolated(&track.path) {
                    Some(loudness) => Some((track, loudness)),
                    None => {
                        report.failed += 1;
                        None
                    }
                })
                .collect();
            let tx = self.conn.transaction()?;
            for (track, loudness) in &measured {
                // Another connection (the folder watcher) may have re-indexed the file while it
                // was decoding; then this measurement describes the old contents and the row
                // stays pending for the next pass instead.
                let written = tx.execute(
                    "UPDATE tracks SET loudness_lufs = ?1, loudness_peak = ?2
                     WHERE id = ?3 AND loudness_lufs IS NULL AND mtime = ?4 AND size = ?5",
                    params![
                        loudness.lufs,
                        loudness.peak,
                        track.id,
                        track.mtime,
                        track.size
                    ],
                )?;
                report.tracks += written;
            }
            if write_album_loudness(&tx, album_id)? {
                report.albums += 1;
            }
            tx.commit()?;
            progress(LoudnessProgress {
                done: report.tracks + report.failed,
                total,
                failed: report.failed,
            });
        }

        // Albums a rescan changed (a track added, replaced, moved or removed) lost their album
        // values; rebuild them from the measurements that are still stored.
        let stale: Vec<i64> = self
            .conn
            .prepare(
                "SELECT DISTINCT album_id FROM tracks
                 WHERE loudness_lufs IS NOT NULL AND album_lufs IS NULL",
            )?
            .query_map([], |row| row.get(0))?
            .collect::<Result<_, _>>()?;
        for album_id in stale {
            let tx = self.conn.transaction()?;
            if write_album_loudness(&tx, album_id)? {
                report.albums += 1;
            }
            tx.commit()?;
        }
        Ok(report)
    }

    /// The stored values for the track at `path`, if the library knows it.
    pub fn track_loudness(&self, path: &Path) -> Result<Option<TrackLoudness>, LibraryError> {
        let key = path
            .to_str()
            .ok_or_else(|| LibraryError::NonUtf8Path(PathBuf::from(path)))?;
        let row = self
            .conn
            .query_row(
                "SELECT album_id, rg_track_gain, rg_track_peak, rg_album_gain, rg_album_peak,
                    loudness_lufs, loudness_peak, album_lufs, album_peak
             FROM tracks WHERE path = ?1",
                [key],
                |row| {
                    Ok(TrackLoudness {
                        album_id: row.get(0)?,
                        replay_gain: ReplayGain {
                            track_gain: row.get(1)?,
                            track_peak: row.get(2)?,
                            album_gain: row.get(3)?,
                            album_peak: row.get(4)?,
                        },
                        loudness: row_loudness(row, 5, 6)?,
                        album: row_loudness(row, 7, 8)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }
}

/// The LUFS and peak columns of one row; `None` while the track has no measurement.
fn row_loudness(
    row: &rusqlite::Row<'_>,
    lufs_col: usize,
    peak_col: usize,
) -> rusqlite::Result<Option<Loudness>> {
    let lufs = row.get::<_, Option<f64>>(lufs_col)?;
    let peak = row.get::<_, Option<f64>>(peak_col)?;
    Ok(lufs.map(|value| Loudness {
        lufs: value,
        peak: peak.unwrap_or(0.0),
    }))
}

/// Stores the album value on every track of `album_id`; `false` when it has none yet.
fn write_album_loudness(conn: &rusqlite::Connection, album_id: i64) -> Result<bool, LibraryError> {
    let Some(album) = album_loudness(conn, album_id)? else {
        return Ok(false);
    };
    conn.execute(
        "UPDATE tracks SET album_lufs = ?1, album_peak = ?2 WHERE album_id = ?3",
        params![album.lufs, album.peak, album_id],
    )?;
    Ok(true)
}

/// Clears the album values of `albums`, so the next analysis pass rebuilds them from the
/// tracks those albums hold now. The scanner calls this for every album it changed.
pub(crate) fn invalidate_album_loudness(
    conn: &rusqlite::Connection,
    albums: &std::collections::BTreeSet<i64>,
) -> Result<(), LibraryError> {
    let mut stmt = conn.prepare_cached(
        "UPDATE tracks SET album_lufs = NULL, album_peak = NULL WHERE album_id = ?1",
    )?;
    for album_id in albums {
        stmt.execute([album_id])?;
    }
    Ok(())
}

/// Duration-weighted energy average of the album's measured tracks, and their loudest peak.
///
/// Tracks whose stored value is the silent placeholder still contribute their peak but not
/// their (non-)loudness: counting them as silence would drag the album down and boost it.
/// `None` when no track of the album has a real measurement.
fn album_loudness(
    conn: &rusqlite::Connection,
    album_id: i64,
) -> Result<Option<Loudness>, LibraryError> {
    let mut stmt = conn.prepare(
        "SELECT loudness_lufs, coalesce(duration_ms, 0), loudness_peak FROM tracks
         WHERE album_id = ?1 AND loudness_lufs IS NOT NULL",
    )?;
    let rows: Vec<(f64, i64, Option<f64>)> = stmt
        .query_map([album_id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?
        .collect::<Result<_, _>>()?;
    let mut energy = 0.0;
    let mut weight = 0.0;
    let mut peak = 0.0f64;
    for (lufs, duration_ms, sample_peak) in rows {
        peak = peak.max(sample_peak.unwrap_or(0.0));
        if lufs <= ABSOLUTE_GATE_LUFS {
            continue;
        }
        // Tracks shorter than 1 s still count; everything else is weighted by length.
        let w = duration_ms.max(1) as f64;
        energy += 10f64.powf((lufs - LOUDNESS_OFFSET) / 10.0) * w;
        weight += w;
    }
    if weight == 0.0 {
        return Ok(None);
    }
    Ok(Some(Loudness {
        lufs: loudness_of(energy / weight),
        peak,
    }))
}

/// A track waiting for analysis, with the file identity it was indexed with.
struct PendingTrack {
    id: i64,
    album_id: i64,
    path: PathBuf,
    mtime: i64,
    size: i64,
}

/// [`analyse_file`] with a panic in a decoder treated like a decode error, so one malformed
/// file cannot take down the thread that runs the whole pass.
fn analyse_file_isolated(path: &Path) -> Option<Loudness> {
    std::panic::catch_unwind(|| analyse_file(path)).ok()?.ok()
}

/// Decodes one PCM file through the meter.
fn analyse_file(path: &Path) -> Result<Loudness, sonder_decode::DecodeError> {
    let mut decoder = Decoder::open(path)?;
    let format = decoder.format();
    let mut meter = LoudnessMeter::new(format.rate, format.channels);
    let mut block = Vec::new();
    while decoder.next_f64(&mut block)? > 0 {
        meter.push(&block);
    }
    Ok(meter.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    fn tone(rate: u32, channels: u16, freq: f64, amplitude: f64, seconds: f64) -> Vec<f64> {
        let frames = (f64::from(rate) * seconds) as usize;
        (0..frames)
            .flat_map(|i| {
                let s = amplitude * (2.0 * PI * freq * i as f64 / f64::from(rate)).sin();
                std::iter::repeat_n(s, usize::from(channels))
            })
            .collect()
    }

    fn measure(rate: u32, channels: u16, samples: &[f64]) -> Loudness {
        let mut meter = LoudnessMeter::new(rate, channels);
        meter.push(samples);
        meter.finish()
    }

    #[test]
    fn a_steady_sine_reads_its_k_weighted_level() {
        let amplitude = 0.25;
        let freq = 997.0;
        let samples = tone(RATE, 2, freq, amplitude, 5.0);
        let measured = measure(RATE, 2, &samples);

        // Identical stereo sine: each channel contributes A²/2, so z = A²·|K|².
        let k = KWeight::new(RATE);
        let expected =
            LOUDNESS_OFFSET + 20.0 * amplitude.log10() + 10.0 * k.power_at(freq, RATE).log10();
        assert!(
            (measured.lufs - expected).abs() < 0.05,
            "measured {} LUFS, expected {expected}",
            measured.lufs
        );
        assert!((measured.peak - amplitude).abs() < 1e-6);
    }

    #[test]
    fn silence_is_gated_out_and_leaves_the_tone_alone() {
        let tone = tone(RATE, 2, 1_000.0, 0.1, 3.0);
        let mut samples = vec![0.0; RATE as usize * 3 * 2];
        samples.extend_from_slice(&tone);
        let with_silence = measure(RATE, 2, &samples);
        let alone = measure(RATE, 2, &tone);
        assert!(
            (with_silence.lufs - alone.lufs).abs() < 0.5,
            "{} vs {}",
            with_silence.lufs,
            alone.lufs
        );
        assert!(alone.is_measured());
    }

    #[test]
    fn a_quiet_passage_below_the_relative_gate_is_dropped() {
        // −20 dB for 3 s then −50 dB for 3 s: the quiet half is more than 10 LU down and
        // must not drag the integrated value towards it.
        let loud = tone(RATE, 2, 1_000.0, 0.1, 3.0);
        let quiet = tone(RATE, 2, 1_000.0, 0.003_162_277_660_168_379_5, 3.0);
        let mut samples = loud.clone();
        samples.extend_from_slice(&quiet);
        let integrated = measure(RATE, 2, &samples);
        let loud_only = measure(RATE, 2, &loud);
        assert!(
            (integrated.lufs - loud_only.lufs).abs() < 0.5,
            "{} vs {}",
            integrated.lufs,
            loud_only.lufs
        );
    }

    #[test]
    fn shorter_than_one_block_has_no_integrated_value_but_keeps_the_peak() {
        let samples = tone(RATE, 2, 1_000.0, 0.5, 0.2);
        let measured = measure(RATE, 2, &samples);
        assert!(!measured.is_measured(), "{measured:?}");
        assert!((measured.peak - 0.5).abs() < 0.01);
        assert_eq!(
            Loudness::silent(),
            Loudness {
                lufs: ABSOLUTE_GATE_LUFS - 1.0,
                peak: 0.0
            }
        );
    }

    #[test]
    fn other_rates_and_channel_counts_are_measured_too() {
        for (rate, channels) in [(44_100u32, 1u16), (48_000, 2), (96_000, 2), (192_000, 2)] {
            let samples = tone(rate, channels, 1_000.0, 0.1, 2.0);
            let measured = measure(rate, channels, &samples);
            assert!(measured.is_measured(), "{rate} Hz: {measured:?}");
            assert!(
                (-24.0..-17.0).contains(&measured.lufs),
                "{rate} Hz, {channels} ch: {} LUFS",
                measured.lufs
            );
        }
    }

    #[test]
    fn wider_than_eight_channels_is_measured_on_the_first_eight() {
        // 7.1.4: twelve channels of the same tone. Only the first eight weigh in, so the
        // reading is the eight-channel one, and the peak still covers every channel.
        let twelve = measure(RATE, 12, &tone(RATE, 12, 1_000.0, 0.1, 2.0));
        let eight = measure(RATE, 8, &tone(RATE, 8, 1_000.0, 0.1, 2.0));
        assert!(twelve.is_measured());
        assert!(
            (twelve.lufs - eight.lufs).abs() < 1e-9,
            "{twelve:?} vs {eight:?}"
        );
        assert!((twelve.peak - 0.1).abs() < 1e-6);
    }

    #[test]
    fn surround_channels_weigh_more_and_the_lfe_not_at_all() {
        // A tone only in one channel of a 5.1 stream.
        let only = |channel: usize| -> Vec<f64> {
            tone(RATE, 1, 1_000.0, 0.1, 2.0)
                .into_iter()
                .flat_map(|s| (0..6).map(move |ch| if ch == channel { s } else { 0.0 }))
                .collect()
        };
        let left = measure(RATE, 6, &only(0));
        let surround = measure(RATE, 6, &only(4));
        let lfe = measure(RATE, 6, &only(3));
        let boost = 10.0 * SURROUND_WEIGHT.log10();
        assert!(
            (surround.lufs - left.lufs - boost).abs() < 1e-6,
            "{surround:?} vs {left:?}"
        );
        assert!(!lfe.is_measured(), "{lfe:?}");
    }

    #[test]
    fn tag_gains_follow_the_target_like_measured_ones() {
        let tagged = TrackLoudness {
            replay_gain: ReplayGain {
                track_gain: Some(-4.0),
                track_peak: Some(0.9),
                album_gain: Some(-5.0),
                album_peak: Some(0.95),
            },
            ..TrackLoudness::default()
        };
        // At the ReplayGain reference the tag is used as written.
        assert_eq!(
            tagged.track_gain(REPLAYGAIN_REFERENCE_LUFS),
            Some((-4.0, 0.9))
        );
        // A target 4 dB louder moves the tag gain by the same 4 dB a measurement would move.
        assert_eq!(tagged.track_gain(-14.0), Some((0.0, 0.9)));
        assert_eq!(tagged.album_gain(-14.0), Some((-1.0, 0.95)));

        let measured = TrackLoudness {
            loudness: Some(Loudness {
                lufs: -14.0,
                peak: 0.5,
            }),
            ..TrackLoudness::default()
        };
        assert_eq!(measured.track_gain(-14.0), Some((0.0, 0.5)));
    }

    #[test]
    fn a_placeholder_measurement_gives_no_gain() {
        let short = TrackLoudness {
            loudness: Some(Loudness {
                lufs: SILENT_LUFS,
                peak: 0.5,
            }),
            album: Some(Loudness::silent()),
            ..TrackLoudness::default()
        };
        assert_eq!(short.track_gain(-18.0), None);
        assert_eq!(short.album_gain(-18.0), None);
    }

    #[test]
    fn a_moving_measurement_resets_cleanly() {
        let mut meter = LoudnessMeter::new(RATE, 2);
        meter.push(&tone(RATE, 2, 1_000.0, 0.1, 1.0));
        assert!(meter.finish().is_measured());
        meter.reset();
        assert!(!meter.finish().is_measured());
        meter.push(&tone(RATE, 2, 1_000.0, 0.1, 1.0));
        assert!(meter.finish().is_measured());
    }
}
