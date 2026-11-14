//! Turning measurements into evidence rows. Only criteria that apply to the file's format
//! are listed; one that applies but could not be measured is `NotMeasured`.

use crate::analysis::Measured;
use crate::spectrum::WALL_DB;
use crate::{CriterionId, Evidence, EvidenceStatus, GENUINE_REACH_HZ, LOSSY_MAX_HZ};

/// A per-frame cutoff that moves less than this (interquartile range) is a fixed lid.
pub(crate) const STABLE_IQR_HZ: f64 = 250.0;
/// Share of loud frames with a hole above which holes are a lossy fingerprint.
pub(crate) const HOLES_FAIL: f64 = 0.2;
const HOLES_WARN: f64 = 0.05;
/// Side-channel collapse above 14 kHz (dB) typical of joint-stereo encoding.
const STEREO_FAIL_DB: f64 = 20.0;
const STEREO_WARN_DB: f64 = 12.0;
const CHANNEL_DELTA_WARN_HZ: f64 = 1_000.0;
/// A hi-res file whose content stops at or below this was made from a 44.1/48 kHz master.
pub(crate) const HI_RES_MAX_BASE_HZ: f64 = 24_500.0;
/// Drop across 20–22 kHz in a DSD file that marks a CD-rate source.
const DSD_WALL_DB: f64 = 30.0;

pub(crate) fn is_wall(m: &Measured) -> bool {
    m.slope_db.is_some_and(|s| s >= WALL_DB)
}

pub(crate) fn evaluate(m: &Measured) -> Vec<Evidence> {
    use EvidenceStatus::{Fail, Info, NotMeasured, Pass, Warn};
    let row = |id, status, value| Evidence { id, status, value };
    let wall = is_wall(m);
    let below_reach = m.cutoff_hz.is_some_and(|c| c < GENUINE_REACH_HZ);
    let mut out = Vec::new();

    out.push(match m.cutoff_hz {
        None => row(CriterionId::Cutoff, NotMeasured, None),
        Some(c) if c >= GENUINE_REACH_HZ => row(CriterionId::Cutoff, Pass, Some(c)),
        Some(c) if c <= LOSSY_MAX_HZ && wall => row(CriterionId::Cutoff, Fail, Some(c)),
        Some(c) => row(CriterionId::Cutoff, Warn, Some(c)),
    });

    out.push(match (m.cutoff_hz, m.slope_db) {
        (None, _) => row(CriterionId::Slope, NotMeasured, None),
        (Some(_), slope) if wall && below_reach => row(CriterionId::Slope, Fail, slope),
        (Some(_), slope) => row(CriterionId::Slope, Pass, slope),
    });

    out.push(match m.stability_iqr_hz {
        None => row(CriterionId::Stability, NotMeasured, None),
        Some(iqr) if below_reach && iqr < STABLE_IQR_HZ => {
            row(CriterionId::Stability, Fail, Some(iqr))
        }
        Some(iqr) => row(CriterionId::Stability, Pass, Some(iqr)),
    });

    out.push(match m.hole_fraction {
        None => row(CriterionId::Holes, NotMeasured, None),
        Some(f) if below_reach && f > HOLES_FAIL => row(CriterionId::Holes, Fail, Some(f)),
        Some(f) if below_reach && f > HOLES_WARN => row(CriterionId::Holes, Warn, Some(f)),
        Some(f) => row(CriterionId::Holes, Pass, Some(f)),
    });

    if m.source.channels > 1 {
        out.push(match m.stereo_collapse_db {
            None => row(CriterionId::StereoField, NotMeasured, None),
            Some(d) if d > STEREO_FAIL_DB => row(CriterionId::StereoField, Fail, Some(d)),
            Some(d) if d > STEREO_WARN_DB => row(CriterionId::StereoField, Warn, Some(d)),
            Some(d) => row(CriterionId::StereoField, Pass, Some(d)),
        });
        out.push(match m.channel_delta_hz {
            None => row(CriterionId::ChannelSymmetry, NotMeasured, None),
            Some(d) if d > CHANNEL_DELTA_WARN_HZ => {
                row(CriterionId::ChannelSymmetry, Warn, Some(d))
            }
            Some(d) => row(CriterionId::ChannelSymmetry, Pass, Some(d)),
        });
    }

    if !m.source.dsd {
        let declared = m.source.bits;
        out.push(match m.real_bits {
            None => row(CriterionId::BitDepth, NotMeasured, None),
            Some(real) if declared >= 24 && real <= 16 => {
                row(CriterionId::BitDepth, Fail, Some(f64::from(real)))
            }
            Some(real) if real < declared => {
                row(CriterionId::BitDepth, Warn, Some(f64::from(real)))
            }
            Some(real) => row(CriterionId::BitDepth, Pass, Some(f64::from(real))),
        });
    }

    out.push(match m.quiet_level_dbfs {
        None => row(CriterionId::QuietLevel, NotMeasured, None),
        Some(f) => row(CriterionId::QuietLevel, Info, Some(f)),
    });

    if !m.source.dsd && m.source.rate >= 88_200 {
        out.push(match m.cutoff_hz {
            None => row(CriterionId::HiRes, NotMeasured, m.ultrasonic_db),
            Some(c) if c <= HI_RES_MAX_BASE_HZ && wall => {
                row(CriterionId::HiRes, Fail, m.ultrasonic_db)
            }
            Some(c) if c <= HI_RES_MAX_BASE_HZ => row(CriterionId::HiRes, Warn, m.ultrasonic_db),
            Some(_) => row(CriterionId::HiRes, Pass, m.ultrasonic_db),
        });
    }

    if m.source.dsd {
        out.push(match m.dsd_drop_db {
            None => row(CriterionId::DsdSource, NotMeasured, None),
            Some(d) if d > DSD_WALL_DB => row(CriterionId::DsdSource, Warn, Some(d)),
            Some(d) => row(CriterionId::DsdSource, Pass, Some(d)),
        });
    }

    if m.flac {
        out.push(match m.md5 {
            Some(true) => row(CriterionId::Md5, Pass, None),
            Some(false) => row(CriterionId::Md5, Fail, None),
            None => row(CriterionId::Md5, NotMeasured, None),
        });
    }

    out
}
