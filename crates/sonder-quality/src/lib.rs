//! Lossless quality check: is a FLAC/ALAC/WAV/AIFF/DSD file what its format claims?
//!
//! [`analyze`] decodes a whole track and measures the evidence a lossy source or a padded
//! master leaves behind:
//!
//! - **cutoff, slope, stability, holes, stereo field** — MP3/AAC encoders low-pass the
//!   signal at a fixed frequency (16–20.5 kHz), drop bands they cannot afford frame by frame,
//!   and fold the treble of joint stereo into mid. A transcode keeps that flat "lid" across
//!   the whole track; genuine lossless has a ragged top edge that moves with the music.
//! - **real bit depth** — the samples are read exactly, so a 24-bit file whose low byte is
//!   always zero is caught for certain.
//! - **hi-res content** — a 96/192 kHz file with a wall at 20–24 kHz was upsampled.
//! - **DSD source** — a DSD file with a CD-rate wall under its modulator noise.
//! - **FLAC MD5** — proves the file decodes to what was encoded, nothing about the source.
//!
//! Results are spectral estimates, never proof. Evidence carries ids and numbers only; the
//! UI words them.

mod analysis;
mod criteria;
mod spectrum;
mod verdict;

use std::path::Path;
use std::sync::atomic::AtomicBool;

use serde::{Deserialize, Serialize};

/// Bumped whenever measurements or thresholds change, so cached results are redone.
pub const ANALYZER_VERSION: u32 = 1;

/// The highest cutoff a common lossy encoder leaves (LAME 320 kbps, AAC 256 kbps ≈ 20–20.5 kHz).
pub const LOSSY_MAX_HZ: f64 = 20_500.0;

/// Content reaching this high is beyond every common lossy codec.
pub const GENUINE_REACH_HZ: f64 = 21_500.0;

/// Tracks shorter than this are reported as inconclusive.
pub const MIN_SECONDS: f64 = 3.0;

#[derive(Debug, Clone, Copy, Default)]
pub struct Options {
    /// Build the time × frequency map. Batch checks leave it off.
    pub spectrogram: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum QualityError {
    #[error(transparent)]
    Decode(#[from] sonder_decode::DecodeError),
    #[error("DSD rate {0} cannot be converted for analysis")]
    UnsupportedDsd(u32),
    #[error("analysis cancelled")]
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename = "QualityVerdict", rename_all = "camelCase")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum Verdict {
    /// Content reaches beyond every lossy codec and nothing else looks wrong.
    Genuine,
    /// Something is off (a wall just above lossy territory, little treble, a CD-rate DSD
    /// source), but not enough to call it a transcode.
    Suspect,
    /// The fingerprint of an MP3/AAC source re-wrapped as lossless.
    Transcoded,
    /// Declared 88.2 kHz or more, but nothing above 20–24 kHz: upsampled.
    FakeHiRes,
    /// Declared 24-bit (or more) but the samples carry 16 bits or fewer.
    FakeBitDepth,
    /// Too short, too quiet or too little treble to judge. Not a pass.
    Inconclusive,
}

impl Verdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Genuine => "genuine",
            Self::Suspect => "suspect",
            Self::Transcoded => "transcoded",
            Self::FakeHiRes => "fakeHiRes",
            Self::FakeBitDepth => "fakeBitDepth",
            Self::Inconclusive => "inconclusive",
        }
    }

    /// The inverse of [`as_str`](Self::as_str).
    pub fn parse(s: &str) -> Option<Self> {
        [
            Self::Genuine,
            Self::Suspect,
            Self::Transcoded,
            Self::FakeHiRes,
            Self::FakeBitDepth,
            Self::Inconclusive,
        ]
        .into_iter()
        .find(|v| v.as_str() == s)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename = "QualityConfidence", rename_all = "camelCase")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum Confidence {
    Low,
    Medium,
    High,
}

impl Confidence {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }

    /// The inverse of [`as_str`](Self::as_str).
    pub fn parse(s: &str) -> Option<Self> {
        [Self::Low, Self::Medium, Self::High]
            .into_iter()
            .find(|c| c.as_str() == s)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename = "QualityCriterion", rename_all = "camelCase")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum CriterionId {
    /// Highest frequency with content (Hz).
    Cutoff,
    /// Drop across the cutoff (dB).
    Slope,
    /// Interquartile range of the per-frame cutoff (Hz).
    Stability,
    /// Share of loud frames with a hole above 12 kHz (0–1).
    Holes,
    /// How much more the side channel collapses above 14 kHz than at 2–8 kHz (dB).
    StereoField,
    /// Left/right cutoff difference (Hz).
    ChannelSymmetry,
    /// Bits the samples actually use.
    BitDepth,
    /// RMS level of the quietest 5 % of the track (dBFS), for reading only.
    QuietLevel,
    /// Level above 24 kHz relative to 2–8 kHz (dB).
    HiRes,
    /// Drop from 17–19.5 kHz to 22.5–24 kHz in a DSD file (dB).
    DsdSource,
    /// FLAC MD5 signature check.
    Md5,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename = "QualityEvidenceStatus", rename_all = "camelCase")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum EvidenceStatus {
    Pass,
    Warn,
    Fail,
    /// A measurement shown for reading, not judged.
    Info,
    /// The check could not run on this file. Not a pass.
    NotMeasured,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename = "QualityEvidence", rename_all = "camelCase")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Evidence {
    pub id: CriterionId,
    pub status: EvidenceStatus,
    pub value: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename = "QualitySpectrumPoint", rename_all = "camelCase")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct SpectrumPoint {
    pub hz: f32,
    /// Relative to the loudest point of the average spectrum.
    pub db: f32,
}

/// Time × frequency map. `db` is row-major, row 0 the lowest frequency; each byte maps
/// 0..=255 onto −120..0 dB relative to the map's loudest cell.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename = "QualitySpectrogram", rename_all = "camelCase")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Spectrogram {
    pub cols: u32,
    pub rows: u32,
    pub top_hz: f64,
    pub db: Vec<u8>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename = "QualitySource", rename_all = "camelCase")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct SourceInfo {
    /// PCM sample rate, or the DSD bit rate per channel.
    pub rate: u32,
    /// Declared bits per sample; 1 for DSD.
    pub bits: u32,
    pub channels: u16,
    pub dsd: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename = "QualityReport", rename_all = "camelCase")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Report {
    pub analyzer_version: u32,
    pub source: SourceInfo,
    pub duration_secs: f64,
    pub verdict: Verdict,
    pub confidence: Confidence,
    pub evidence: Vec<Evidence>,
    /// Average spectrum, log-spaced from 20 Hz to `spectrum_top_hz`.
    pub spectrum: Vec<SpectrumPoint>,
    pub spectrum_top_hz: f64,
    pub cutoff_hz: Option<f64>,
    /// Median cutoff of each spectrogram column (Hz; 0 where no loud frame fell).
    pub cutoff_track: Vec<f32>,
    pub spectrogram: Option<Spectrogram>,
}

impl Report {
    pub fn evidence(&self, id: CriterionId) -> Option<&Evidence> {
        self.evidence.iter().find(|e| e.id == id)
    }
}

/// Decodes and analyses the whole track at `path`.
///
/// `progress` receives the decoded fraction (0–1) after each block; `cancel` is checked as
/// often and ends the analysis with [`QualityError::Cancelled`].
pub fn analyze(
    path: &Path,
    options: &Options,
    cancel: &AtomicBool,
    progress: impl FnMut(f32),
) -> Result<Report, QualityError> {
    let measured = analysis::measure(path, options, cancel, progress)?;
    let evidence = criteria::evaluate(&measured);
    let (verdict, confidence) = verdict::decide(&measured, &evidence);
    Ok(Report {
        analyzer_version: ANALYZER_VERSION,
        source: measured.source,
        duration_secs: measured.duration_secs,
        verdict,
        confidence,
        evidence,
        spectrum: measured.spectrum,
        spectrum_top_hz: measured.spectrum_top_hz,
        cutoff_hz: measured.cutoff_hz,
        cutoff_track: measured.cutoff_track,
        spectrogram: measured.spectrogram,
    })
}
