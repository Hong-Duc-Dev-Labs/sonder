//! The verdict rules, in priority order. A lossy source outranks every other finding: an MP3
//! upsampled to 96 kHz/24-bit is first of all a transcode.

use crate::analysis::{MIN_LOUD_FRAMES, Measured};
use crate::criteria::is_wall;
use crate::{
    Confidence, CriterionId, Evidence, EvidenceStatus, GENUINE_REACH_HZ, LOSSY_MAX_HZ, MIN_SECONDS,
    Verdict,
};

/// Below this the track has too little treble for any spectral judgement.
const MIN_CUTOFF_HZ: f64 = 8_000.0;

pub(crate) fn decide(m: &Measured, evidence: &[Evidence]) -> (Verdict, Confidence) {
    let status = |id| evidence.iter().find(|e| e.id == id).map(|e| e.status);
    let failed = |id| status(id) == Some(EvidenceStatus::Fail);

    let Some(cutoff) = m.cutoff_hz else {
        return (Verdict::Inconclusive, Confidence::Low);
    };
    if m.duration_secs < MIN_SECONDS || m.loud_frames < MIN_LOUD_FRAMES || cutoff < MIN_CUTOFF_HZ {
        return (Verdict::Inconclusive, Confidence::Low);
    }

    let wall = is_wall(m);
    if cutoff <= LOSSY_MAX_HZ && wall {
        let signs = [
            CriterionId::Stability,
            CriterionId::Holes,
            CriterionId::StereoField,
        ]
        .into_iter()
        .filter(|&id| failed(id))
        .count();
        return match signs {
            0 => (Verdict::Suspect, Confidence::Medium),
            1 => (Verdict::Transcoded, Confidence::Medium),
            _ => (Verdict::Transcoded, Confidence::High),
        };
    }
    if failed(CriterionId::HiRes) {
        return (Verdict::FakeHiRes, Confidence::High);
    }
    if failed(CriterionId::BitDepth) {
        return (Verdict::FakeBitDepth, Confidence::High);
    }
    if cutoff < GENUINE_REACH_HZ {
        // A wall just above lossy territory is most often a 48 → 44.1 kHz resample (still
        // lossless), but an encoder this check cannot fingerprint is not ruled out. Without
        // a wall, the music simply has little treble; a top edge that moves with it is the
        // sign of a real recording.
        let moving = status(CriterionId::Stability) == Some(EvidenceStatus::Pass);
        return if !wall && moving {
            (Verdict::Genuine, Confidence::Medium)
        } else {
            (Verdict::Suspect, Confidence::Low)
        };
    }
    if status(CriterionId::DsdSource) == Some(EvidenceStatus::Warn) {
        return (Verdict::Suspect, Confidence::Medium);
    }
    let doubts = evidence
        .iter()
        .any(|e| matches!(e.status, EvidenceStatus::Warn | EvidenceStatus::Fail));
    (
        Verdict::Genuine,
        if doubts {
            Confidence::Medium
        } else {
            Confidence::High
        },
    )
}
