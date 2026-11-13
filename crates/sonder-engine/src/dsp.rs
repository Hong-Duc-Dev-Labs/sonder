//! The user-facing PCM DSP settings and the levelling lookup the engine needs.
//!
//! [`DspConfig`] is the settings half of the chain (PEQ, levelling, dither); the rate
//! conversion half lives in [`SrcConfig`](crate::SrcConfig) and software volume stays a
//! separate command. [`LoudnessSource`] is how the engine reads ReplayGain or measured
//! loudness without depending on the library's database.

use std::path::Path;
use std::sync::Arc;

use sonder_dsp::{CrossfeedPreset, Gain, ImpulseResponse, IrError, LevelingMode, Peq};

/// Values levelling needs for one playable unit.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TrackGain {
    pub track: Gain,
    pub album: Gain,
    /// Album identity, used to decide whether a queue run is an album in order.
    pub album_id: Option<i64>,
}

/// Where the engine reads levelling values from.
pub trait LoudnessSource: Send + Sync {
    /// Gains for `path` targeting `target_lufs`; `None` when nothing is known about it.
    fn gain(&self, path: &Path, target_lufs: f64) -> Option<TrackGain>;
}

/// A source that knows nothing: levelling then does nothing.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoLoudness;

impl LoudnessSource for NoLoudness {
    fn gain(&self, _path: &Path, _target_lufs: f64) -> Option<TrackGain> {
        None
    }
}

/// Loudest and quietest levelling target the engine accepts.
pub const MIN_TARGET_LUFS: f64 = -30.0;
pub const MAX_TARGET_LUFS: f64 = -5.0;

/// PCM shaping settings: convolution, parametric EQ, crossfeed, levelling and dither.
#[derive(Debug, Clone, PartialEq)]
pub struct DspConfig {
    /// Impulse response to convolve with. `Err` keeps the stage off; the engine reports the
    /// reason once.
    pub convolution: Option<Result<Arc<ImpulseResponse>, IrError>>,
    /// Parametric EQ; `None` or an inactive preset leaves the samples alone.
    pub peq: Option<Peq>,
    /// Headphone crossfeed for stereo tracks.
    pub crossfeed: Option<CrossfeedPreset>,
    pub leveling: LevelingMode,
    /// Integrated loudness levelling aims for.
    pub leveling_target_lufs: f64,
    /// Add TPDF dither when the device container is narrower than the processed signal.
    pub dither: bool,
}

impl Default for DspConfig {
    fn default() -> Self {
        Self {
            convolution: None,
            peq: None,
            crossfeed: None,
            leveling: LevelingMode::Off,
            leveling_target_lufs: -18.0,
            dither: true,
        }
    }
}

impl DspConfig {
    /// Clamped copy: a preset cannot push the preamp past its own limits, and the target
    /// stays in a range where levelling is audible and safe.
    pub fn sanitized(mut self) -> Self {
        self.leveling_target_lufs = self
            .leveling_target_lufs
            .clamp(MIN_TARGET_LUFS, MAX_TARGET_LUFS);
        self.peq = self.peq.map(Peq::sanitized);
        self
    }

    /// Whether the EQ changes the signal.
    pub fn peq_active(&self) -> bool {
        self.peq.as_ref().is_some_and(Peq::active)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sonder_dsp::PeqBand;

    #[test]
    fn sanitize_clamps_the_target_and_the_preset() {
        let config = DspConfig {
            leveling_target_lufs: -100.0,
            peq: Some(Peq {
                preamp_db: 99.0,
                bands: vec![PeqBand {
                    freq_hz: 1e9,
                    gain_db: 100.0,
                    q: 0.0,
                    ..PeqBand::default()
                }],
            }),
            ..DspConfig::default()
        }
        .sanitized();
        assert_eq!(config.leveling_target_lufs, MIN_TARGET_LUFS);
        assert!(config.peq_active());
        let peq = config.peq.clone().unwrap();
        assert_eq!(peq.preamp_db, 12.0);
        assert_eq!(peq.bands[0].freq_hz, 24_000.0);
        assert!(config.peq_active());
    }

    #[test]
    fn an_empty_preset_is_not_an_active_eq() {
        let config = DspConfig {
            peq: Some(Peq::default()),
            ..DspConfig::default()
        };
        assert!(!config.peq_active());
    }
}
