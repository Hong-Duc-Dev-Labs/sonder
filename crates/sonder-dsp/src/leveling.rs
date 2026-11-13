//! Volume levelling from ReplayGain (or measured loudness) with peak limiting.
//!
//! The gain is decided off the audio thread — from tags or from the library's loudness
//! analysis — and published through an atomic so a new track's gain takes effect on its first
//! block without rebuilding the chain (which would break gapless playback). A boost is capped
//! by the track's peak so levelling never clips.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use crate::stage::{Stage, StreamFormat};

/// Quietest and loudest gain levelling may apply.
pub const MIN_GAIN_DB: f64 = -60.0;
pub const MAX_GAIN_DB: f64 = 24.0;

/// Where levelling gets its gain.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LevelingMode {
    /// Leave the samples alone.
    #[default]
    Off,
    /// Each track to the target loudness.
    Track,
    /// Each album to the target, keeping the relative loudness inside it.
    Album,
    /// Album while the queue stays inside one album, track otherwise.
    Auto,
}

impl LevelingMode {
    /// Whether the engine has to look up values and insert the stage.
    pub const fn is_on(self) -> bool {
        !matches!(self, Self::Off)
    }
}

/// ReplayGain-style values for one playable unit: a gain in dB and a linear peak.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Gain {
    pub gain_db: f64,
    /// Largest absolute sample, relative to full scale; `0` means unknown.
    pub peak: f64,
}

impl Gain {
    /// A gain measured as integrated loudness against `target_lufs`.
    pub fn from_loudness(lufs: f64, peak: f64, target_lufs: f64) -> Self {
        Self {
            gain_db: target_lufs - lufs,
            peak,
        }
    }

    /// The gain that may actually be applied: clipped to the range and to the headroom the
    /// peak leaves.
    pub fn limited(&self) -> f64 {
        if !self.gain_db.is_finite() {
            return 0.0;
        }
        let headroom = if self.peak.is_finite() && self.peak > 0.0 {
            -20.0 * self.peak.log10()
        } else {
            MAX_GAIN_DB
        };
        self.gain_db.min(headroom).clamp(MIN_GAIN_DB, MAX_GAIN_DB)
    }
}

/// Shared gain the stage reads once per block. The engine keeps a clone and updates it when a
/// track starts.
#[derive(Debug, Clone)]
pub struct GainHandle(Arc<AtomicU64>);

impl Default for GainHandle {
    fn default() -> Self {
        Self(Arc::new(AtomicU64::new(0.0f64.to_bits())))
    }
}

impl GainHandle {
    /// Publishes a gain in dB, limited by [`Gain::limited`] first.
    pub fn set_gain(&self, gain: Gain) {
        self.set_db(gain.limited());
    }

    pub fn set_db(&self, db: f64) {
        let db = if db.is_finite() {
            db.clamp(MIN_GAIN_DB, MAX_GAIN_DB)
        } else {
            0.0
        };
        self.0.store(db.to_bits(), Ordering::Relaxed);
    }

    /// The published gain in dB.
    pub fn db(&self) -> f64 {
        f64::from_bits(self.0.load(Ordering::Relaxed))
    }

    /// The published gain as a linear factor.
    pub fn linear(&self) -> f64 {
        10f64.powf(self.db() / 20.0)
    }
}

/// Applies the published gain to every channel.
pub struct LevelingStage {
    gain: GainHandle,
    channels: usize,
}

impl LevelingStage {
    pub fn new(gain: GainHandle) -> Self {
        Self { gain, channels: 1 }
    }

    pub fn handle(&self) -> &GainHandle {
        &self.gain
    }
}

impl Stage for LevelingStage {
    fn configure(&mut self, input: StreamFormat) -> StreamFormat {
        self.channels = usize::from(input.channels.max(1));
        input
    }

    fn max_out(&self, in_frames: usize) -> usize {
        in_frames
    }

    fn process(&mut self, input: &[f64], out: &mut [f64]) -> usize {
        // One atomic read per block keeps the real-time path free of surprises.
        let gain = self.gain.linear();
        for (o, i) in out.iter_mut().zip(input) {
            *o = i * gain;
        }
        input.len() / self.channels
    }

    fn flush(&mut self, _out: &mut [f64]) -> usize {
        0
    }

    fn reset(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    const FORMAT: StreamFormat = StreamFormat {
        rate: 44_100,
        channels: 2,
    };

    fn process(stage: &mut LevelingStage, samples: &[f64]) -> Vec<f64> {
        let mut out = vec![0.0; samples.len()];
        stage.process(samples, &mut out);
        out
    }

    #[test]
    fn a_boost_stops_before_the_peak_clips() {
        assert_eq!(
            Gain {
                gain_db: 6.0,
                peak: 1.0
            }
            .limited(),
            0.0
        );
        assert!(
            (Gain {
                gain_db: 6.0,
                peak: 0.5
            }
            .limited()
                - 6.0)
                .abs()
                < 1e-9
        );
        assert!(
            (Gain {
                gain_db: 6.0,
                peak: 0.9
            }
            .limited()
                - 0.915_149)
                .abs()
                < 1e-4
        );
        assert_eq!(
            Gain {
                gain_db: 40.0,
                peak: 0.1
            }
            .limited(),
            20.0
        );
        assert_eq!(
            Gain {
                gain_db: -90.0,
                peak: 1.0
            }
            .limited(),
            MIN_GAIN_DB
        );
        // An unknown peak must not block a cut, only an unlimited boost.
        assert_eq!(
            Gain {
                gain_db: -6.0,
                peak: 0.0
            }
            .limited(),
            -6.0
        );
    }

    #[test]
    fn the_published_gain_reaches_the_next_block() {
        let handle = GainHandle::default();
        let mut stage = LevelingStage::new(handle.clone());
        stage.configure(FORMAT);
        assert_eq!(process(&mut stage, &[0.5, -0.5]), [0.5, -0.5]);

        handle.set_gain(Gain {
            gain_db: -6.0,
            peak: 1.0,
        });
        let out = process(&mut stage, &[0.5, -0.5]);
        assert!((out[0] - 0.5 * 10f64.powf(-6.0 / 20.0)).abs() < 1e-12);
        assert!((out[1] + 0.5 * 10f64.powf(-6.0 / 20.0)).abs() < 1e-12);
        assert!((handle.db() + 6.0).abs() < 1e-12);
    }

    #[test]
    fn louder_tracks_move_less_than_quiet_ones() {
        // Album levelling keeps the 6 dB difference between two tracks: both are brought to
        // the target, so their applied gains differ by exactly the loudness difference.
        let target = -18.0;
        let quiet = Gain::from_loudness(-20.0, 0.5, target);
        let loud = Gain::from_loudness(-14.0, 0.5, target);
        assert!((quiet.limited() - 2.0).abs() < 1e-9);
        assert!((loud.limited() + 4.0).abs() < 1e-9);
        assert!((quiet.limited() - loud.limited() - 6.0).abs() < 1e-9);
    }

    #[test]
    fn modes_report_whether_they_are_on() {
        assert!(!LevelingMode::Off.is_on());
        assert!(LevelingMode::Auto.is_on());
        assert!(LevelingMode::Album.is_on());
    }
}
