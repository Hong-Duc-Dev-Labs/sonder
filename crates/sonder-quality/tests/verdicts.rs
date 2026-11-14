//! End-to-end verdicts on synthesized files.
//!
//! Signals are built in the frequency domain (random-phase noise with a chosen magnitude per
//! bin, overlap-added with a Hann window), so a "transcode" has exactly the lid, holes and
//! stereo collapse a lossy encoder leaves, and a "genuine" file has content to Nyquist. No
//! real encoder is involved; real-world thresholds still need checking on real transcodes.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use md5::{Digest, Md5};
use realfft::RealFftPlanner;
use realfft::num_complex::Complex;
use sonder_quality::{
    Confidence, CriterionId, EvidenceStatus, Options, QualityError, Report, Verdict, analyze,
};
use sonder_verify::fixtures::{write_flac, write_flac_with_md5};

const BLOCK: usize = 4096;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// Mono noise whose magnitude at `hz` in block `b` is `amp(b, hz)`, at `rms_dbfs`.
fn noise(
    rate: u32,
    seconds: f64,
    rms_dbfs: f64,
    seed: u64,
    amp: impl Fn(usize, f64) -> f64,
) -> Vec<f64> {
    let frames = (f64::from(rate) * seconds) as usize;
    let hop = BLOCK / 2;
    let mut planner = RealFftPlanner::<f64>::new();
    let inverse = planner.plan_fft_inverse(BLOCK);
    let mut rng = Rng(seed);
    let window: Vec<f64> = (0..BLOCK)
        .map(|n| 0.5 - 0.5 * (std::f64::consts::TAU * n as f64 / BLOCK as f64).cos())
        .collect();
    let mut out = vec![0.0; frames + BLOCK];
    let mut spectrum = inverse.make_input_vec();
    let mut block = inverse.make_output_vec();
    let bin_hz = f64::from(rate) / BLOCK as f64;
    for b in 0..frames.div_ceil(hop) {
        for (k, x) in spectrum.iter_mut().enumerate() {
            let a = if k == 0 || k == BLOCK / 2 {
                0.0
            } else {
                amp(b, k as f64 * bin_hz)
            };
            let phase = rng.next() * std::f64::consts::TAU;
            *x = Complex::from_polar(a, phase);
        }
        inverse.process(&mut spectrum, &mut block).unwrap();
        let start = b * hop;
        for (i, (s, w)) in block.iter().zip(&window).enumerate() {
            out[start + i] += s * w;
        }
    }
    out.truncate(frames);
    let rms = (out.iter().map(|s| s * s).sum::<f64>() / out.len() as f64).sqrt();
    let gain = 10f64.powf(rms_dbfs / 20.0) / rms.max(1e-30);
    out.iter_mut().for_each(|s| *s *= gain);
    out
}

fn pink(hz: f64) -> f64 {
    1.0 / hz.max(20.0).sqrt()
}

/// Interleaved stereo from mid and side.
fn stereo(mid: &[f64], side: &[f64]) -> Vec<f64> {
    mid.iter()
        .zip(side)
        .flat_map(|(m, s)| [m + s, m - s])
        .collect()
}

fn quantize(samples: &[f64], bits: u32) -> Vec<i32> {
    let full = f64::from((1i32 << (bits - 1)) - 1);
    samples
        .iter()
        .map(|s| (s * full).round().clamp(-full - 1.0, full) as i32)
        .collect()
}

fn write_wav(path: &Path, rate: u32, bits: u16, samples: &[i32]) {
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: rate,
        bits_per_sample: bits,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    for &s in samples {
        w.write_sample(s).unwrap();
    }
    w.finalize().unwrap();
}

fn flac_md5(samples: &[i32], bits: u32) -> [u8; 16] {
    let bytes = (bits / 8) as usize;
    let mut hasher = Md5::new();
    for s in samples {
        hasher.update(&s.to_le_bytes()[..bytes]);
    }
    hasher.finalize().into()
}

/// A stereo track: `cutoff` limits mid and side, `side_top` limits the side only, and
/// `holes` drops 13–14 kHz in alternating 370 ms stretches.
struct Spec {
    rate: u32,
    seconds: f64,
    rms_dbfs: f64,
    cutoff: f64,
    side_top: f64,
    holes: bool,
}

impl Spec {
    fn genuine(rate: u32) -> Self {
        Self {
            rate,
            seconds: 8.0,
            rms_dbfs: -20.0,
            cutoff: f64::INFINITY,
            side_top: f64::INFINITY,
            holes: false,
        }
    }

    fn lid(cutoff: f64) -> Self {
        Self {
            cutoff,
            ..Self::genuine(44_100)
        }
    }

    fn signal(&self) -> Vec<f64> {
        let shape = |b: usize, hz: f64, top: f64| {
            let in_hole =
                self.holes && (b / 8).is_multiple_of(2) && (13_000.0..14_000.0).contains(&hz);
            if hz >= top.min(self.cutoff) || in_hole {
                0.0
            } else {
                pink(hz)
            }
        };
        let mid = noise(
            self.rate,
            self.seconds,
            self.rms_dbfs,
            0x9E37_79B9,
            |b, hz| shape(b, hz, f64::INFINITY),
        );
        let side = noise(
            self.rate,
            self.seconds,
            self.rms_dbfs - 6.0,
            0x2545_F491,
            |b, hz| shape(b, hz, self.side_top),
        );
        stereo(&mid, &side)
    }
}

struct Dir(tempfile::TempDir);

impl Dir {
    fn new() -> Self {
        Self(tempfile::tempdir().unwrap())
    }

    fn wav(&self, name: &str, spec: &Spec, bits: u16) -> PathBuf {
        let path = self.0.path().join(name);
        write_wav(
            &path,
            spec.rate,
            bits,
            &quantize(&spec.signal(), u32::from(bits)),
        );
        path
    }
}

fn check(path: &Path) -> Report {
    analyze(path, &Options::default(), &AtomicBool::new(false), |_| {}).unwrap()
}

fn status(report: &Report, id: CriterionId) -> EvidenceStatus {
    report
        .evidence(id)
        .unwrap_or_else(|| panic!("{id:?} missing"))
        .status
}

#[test]
fn full_band_noise_is_genuine() {
    let dir = Dir::new();
    let report = check(&dir.wav("genuine.wav", &Spec::genuine(44_100), 16));
    assert_eq!(report.verdict, Verdict::Genuine, "{report:#?}");
    assert_eq!(
        report.confidence,
        Confidence::High,
        "{:#?}",
        report.evidence
    );
    assert_eq!(status(&report, CriterionId::BitDepth), EvidenceStatus::Pass);
    assert!(report.cutoff_hz.unwrap() >= 21_500.0);
}

#[test]
fn a_lossy_lid_with_holes_and_folded_stereo_is_a_confident_transcode() {
    let dir = Dir::new();
    let spec = Spec {
        side_top: 14_000.0,
        holes: true,
        ..Spec::lid(16_000.0)
    };
    let report = check(&dir.wav("mp3-128.wav", &spec, 16));
    assert_eq!(
        report.verdict,
        Verdict::Transcoded,
        "{:#?}",
        report.evidence
    );
    assert_eq!(
        report.confidence,
        Confidence::High,
        "{:#?}",
        report.evidence
    );
    let cutoff = report.cutoff_hz.unwrap();
    assert!((15_800.0..=16_200.0).contains(&cutoff), "{cutoff}");
    for id in [
        CriterionId::Cutoff,
        CriterionId::Slope,
        CriterionId::Stability,
        CriterionId::Holes,
        CriterionId::StereoField,
    ] {
        assert_eq!(
            status(&report, id),
            EvidenceStatus::Fail,
            "{id:?}: {:#?}",
            report.evidence
        );
    }
}

#[test]
fn plain_lids_at_common_lossy_cutoffs_are_transcodes() {
    let dir = Dir::new();
    for cutoff in [16_000.0, 19_500.0, 20_000.0] {
        let report = check(&dir.wav(&format!("lid-{cutoff}.wav"), &Spec::lid(cutoff), 16));
        assert_eq!(
            report.verdict,
            Verdict::Transcoded,
            "{cutoff}: {:#?}",
            report.evidence
        );
        let found = report.cutoff_hz.unwrap();
        assert!((found - cutoff).abs() <= 200.0, "{cutoff} -> {found}");
    }
}

#[test]
fn a_wall_just_above_lossy_territory_is_only_suspect() {
    let dir = Dir::new();
    let report = check(&dir.wav("resampled.wav", &Spec::lid(21_100.0), 16));
    assert_eq!(report.verdict, Verdict::Suspect, "{:#?}", report.evidence);
    assert_eq!(report.confidence, Confidence::Low);
}

#[test]
fn padded_24_bit_is_fake_bit_depth() {
    let dir = Dir::new();
    let path = dir.0.path().join("padded.wav");
    let samples: Vec<i32> = quantize(&Spec::genuine(44_100).signal(), 16)
        .into_iter()
        .map(|s| s << 8)
        .collect();
    write_wav(&path, 44_100, 24, &samples);
    let report = check(&path);
    assert_eq!(
        report.verdict,
        Verdict::FakeBitDepth,
        "{:#?}",
        report.evidence
    );
    assert_eq!(
        report.evidence(CriterionId::BitDepth).unwrap().value,
        Some(16.0)
    );
}

#[test]
fn real_24_bit_passes_the_bit_depth_check() {
    let dir = Dir::new();
    let report = check(&dir.wav("real24.wav", &Spec::genuine(44_100), 24));
    assert_eq!(status(&report, CriterionId::BitDepth), EvidenceStatus::Pass);
    assert_eq!(report.verdict, Verdict::Genuine, "{:#?}", report.evidence);
}

#[test]
fn a_96_khz_file_walled_at_22_khz_is_fake_hi_res() {
    let dir = Dir::new();
    let spec = Spec {
        rate: 96_000,
        cutoff: 22_000.0,
        ..Spec::genuine(96_000)
    };
    let report = check(&dir.wav("upsampled.wav", &spec, 24));
    assert_eq!(report.verdict, Verdict::FakeHiRes, "{:#?}", report.evidence);
    assert_eq!(status(&report, CriterionId::HiRes), EvidenceStatus::Fail);
}

#[test]
fn genuine_hi_res_passes() {
    let dir = Dir::new();
    let report = check(&dir.wav("hires.wav", &Spec::genuine(96_000), 24));
    assert_eq!(status(&report, CriterionId::HiRes), EvidenceStatus::Pass);
    assert_eq!(report.verdict, Verdict::Genuine, "{:#?}", report.evidence);
}

#[test]
fn near_silence_and_short_clips_are_inconclusive() {
    let dir = Dir::new();
    let quiet = Spec {
        rms_dbfs: -80.0,
        ..Spec::genuine(44_100)
    };
    let short = Spec {
        seconds: 2.0,
        ..Spec::genuine(44_100)
    };
    for (name, spec) in [("quiet.wav", quiet), ("short.wav", short)] {
        let report = check(&dir.wav(name, &spec, 16));
        assert_eq!(
            report.verdict,
            Verdict::Inconclusive,
            "{name}: {:#?}",
            report.evidence
        );
    }
}

#[test]
fn flac_md5_is_checked_but_never_proves_the_source() {
    let dir = Dir::new();
    let write = |name: &str, spec: &Spec, md5: Option<bool>| {
        let path = dir.0.path().join(name);
        let samples = quantize(&spec.signal(), 16);
        match md5 {
            None => write_flac(&path, spec.rate, 16, &samples).unwrap(),
            Some(_) => {
                write_flac_with_md5(&path, spec.rate, 16, &samples, flac_md5(&samples, 16)).unwrap()
            }
        }
        path
    };

    let good = check(&write("good.flac", &Spec::genuine(44_100), Some(true)));
    assert_eq!(status(&good, CriterionId::Md5), EvidenceStatus::Pass);
    assert_eq!(good.verdict, Verdict::Genuine);

    let unsigned = check(&write("unsigned.flac", &Spec::genuine(44_100), None));
    assert_eq!(
        status(&unsigned, CriterionId::Md5),
        EvidenceStatus::NotMeasured
    );

    let transcode = check(&write("transcode.flac", &Spec::lid(16_000.0), Some(true)));
    assert_eq!(status(&transcode, CriterionId::Md5), EvidenceStatus::Pass);
    assert_eq!(transcode.verdict, Verdict::Transcoded);

    // Flip one sample byte in the middle of the file (inside a frame's payload).
    let path = write("corrupt.flac", &Spec::genuine(44_100), Some(true));
    let mut bytes = std::fs::read(&path).unwrap();
    let at = bytes.len() / 2;
    bytes[at] ^= 0x40;
    std::fs::write(&path, bytes).unwrap();
    let corrupt = check(&path);
    assert_eq!(
        status(&corrupt, CriterionId::Md5),
        EvidenceStatus::Fail,
        "{:#?}",
        corrupt.evidence
    );
    assert_ne!(corrupt.confidence, Confidence::High);
}

#[test]
fn the_spectrogram_and_cutoff_track_line_up() {
    let dir = Dir::new();
    let path = dir.wav("map.wav", &Spec::lid(16_000.0), 16);
    let report = analyze(
        &path,
        &Options { spectrogram: true },
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    let map = report.spectrogram.as_ref().unwrap();
    assert_eq!(map.rows, 256);
    assert_eq!(map.db.len(), (map.rows * map.cols) as usize);
    assert_eq!(report.cutoff_track.len(), map.cols as usize);
    assert!(
        report
            .cutoff_track
            .iter()
            .filter(|&&c| c > 0.0)
            .all(|&c| (15_500.0..=16_200.0).contains(&c))
    );
    assert!(report.spectrum.len() > 100);
    assert!(check(&path).spectrogram.is_none());
}

#[test]
fn reports_round_trip_through_json() {
    let dir = Dir::new();
    let report = check(&dir.wav("json.wav", &Spec::lid(19_500.0), 16));
    let text = serde_json::to_string(&report).unwrap();
    assert_eq!(serde_json::from_str::<Report>(&text).unwrap(), report);
}

#[test]
fn a_cancelled_check_stops() {
    let dir = Dir::new();
    let path = dir.wav("cancel.wav", &Spec::genuine(44_100), 16);
    let result = analyze(&path, &Options::default(), &AtomicBool::new(true), |_| {});
    assert!(matches!(result, Err(QualityError::Cancelled)));
}

/// A DSF file of DSD silence (the 0x69 idle pattern), `seconds` long at DSD64.
fn write_dsf_silence(path: &Path, seconds: u64) {
    let (rate, channels, block) = (2_822_400u64, 2u64, 4096u64);
    let bytes_per_channel = rate / 8 * seconds;
    let blocks = bytes_per_channel.div_ceil(block);
    let data_len = blocks * block * channels;
    let mut f = Vec::new();
    f.extend_from_slice(b"DSD ");
    f.extend_from_slice(&28u64.to_le_bytes());
    f.extend_from_slice(&(28 + 52 + 12 + data_len).to_le_bytes());
    f.extend_from_slice(&0u64.to_le_bytes());
    f.extend_from_slice(b"fmt ");
    f.extend_from_slice(&52u64.to_le_bytes());
    for v in [1u32, 0, 2, channels as u32, rate as u32, 1] {
        f.extend_from_slice(&v.to_le_bytes());
    }
    f.extend_from_slice(&(rate * seconds).to_le_bytes());
    f.extend_from_slice(&(block as u32).to_le_bytes());
    f.extend_from_slice(&0u32.to_le_bytes());
    f.extend_from_slice(b"data");
    f.extend_from_slice(&(12 + data_len).to_le_bytes());
    f.resize(f.len() + data_len as usize, 0x69);
    std::fs::write(path, f).unwrap();
}

#[test]
fn dsd_files_are_analysed_through_pcm_conversion() {
    let dir = Dir::new();
    let path = dir.0.path().join("silence.dsf");
    write_dsf_silence(&path, 4);
    let report = check(&path);
    assert!(report.source.dsd);
    assert!(
        (report.duration_secs - 4.0).abs() < 0.1,
        "{}",
        report.duration_secs
    );
    assert!(report.evidence(CriterionId::DsdSource).is_some());
    assert!(report.evidence(CriterionId::BitDepth).is_none());
    assert!(report.evidence(CriterionId::Md5).is_none());
    assert_eq!(report.verdict, Verdict::Inconclusive);
}
