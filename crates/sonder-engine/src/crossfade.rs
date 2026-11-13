//! Crossfade between two queue entries: the equal-power mixer and the rules that decide when
//! a transition fades instead of joining gaplessly.
//!
//! The mixer runs on the engine thread, before the device ring, so it never touches the render
//! callback. It works on a chain's `f64` output (after every DSP stage, before the integer
//! conversion or the PCM→DSD modulator), which is what lets a modulated pair mix before the
//! modulator.

use crate::worker::OutputSpec;

/// Shortest fade the settings accept, in seconds.
pub const MIN_SECONDS: f64 = 1.0;
/// Longest fade the settings accept, in seconds.
pub const MAX_SECONDS: f64 = 10.0;
/// Fade length before the user or a track length says otherwise.
pub const DEFAULT_SECONDS: f64 = 4.0;

/// Crossfade settings, as the player carries them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CrossfadeConfig {
    pub enabled: bool,
    /// Fade length in seconds, kept within [`MIN_SECONDS`]..=[`MAX_SECONDS`].
    pub seconds: f64,
}

impl Default for CrossfadeConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            seconds: DEFAULT_SECONDS,
        }
    }
}

impl CrossfadeConfig {
    /// The values as the engine applies them: an out-of-range or non-finite length becomes
    /// the default and a short one [`MIN_SECONDS`].
    pub fn sanitized(self) -> Self {
        let seconds = if self.seconds.is_finite() {
            self.seconds.clamp(MIN_SECONDS, MAX_SECONDS)
        } else {
            DEFAULT_SECONDS
        };
        Self {
            enabled: self.enabled,
            seconds,
        }
    }

    pub const fn is_on(&self) -> bool {
        self.enabled
    }
}

/// Why a transition joined gaplessly instead of crossfading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Skip {
    /// Repeat-one repeats the same track; a track never fades into itself.
    RepeatOne,
    /// The queue repeats a single entry: same track again.
    SameTrack,
    /// The two entries are consecutive tracks of one album.
    SameAlbum,
    /// A DSD file on either side: its bits cannot be mixed at PCM resolution.
    Dsd,
    /// The two tracks do not reach the same output format.
    DifferentFormat,
    /// A container without a duration; the fade point cannot be placed.
    UnknownLength,
}

impl Skip {
    /// The reason as the signal path shows it.
    pub const fn reason(self) -> &'static str {
        match self {
            Self::RepeatOne => "repeat one",
            Self::SameTrack => "same track",
            Self::SameAlbum => "same album",
            Self::Dsd => "DSD",
            Self::DifferentFormat => "different output format",
            Self::UnknownLength => "unknown length",
        }
    }
}

/// What a transition needs from one side.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Side {
    /// The format the track's chain produces.
    pub spec: OutputSpec,
    /// The source is PCM. A DSD file is `false` even when it is converted to PCM or packed as
    /// DoP: its samples are one-bit.
    pub pcm: bool,
    /// The track's length in seconds, when the container states it.
    pub seconds: Option<f64>,
}

/// The two entries a crossfade would join, and how the queue sees them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transition {
    pub outgoing: Side,
    pub incoming: Side,
    /// Repeat mode one: the "next" entry is the same track.
    pub repeat_one: bool,
    /// Both sides are the same queue entry (repeat-all over one entry).
    pub same_entry: bool,
    /// Both entries carry the same album identity.
    pub same_album: bool,
}

/// The fade length in seconds for `transition`, or why it stays gapless.
///
/// Call only when crossfade is switched on; the length is clamped to half of the shorter
/// track so a short track is never wholly fade.
pub fn decide(config: CrossfadeConfig, transition: Transition) -> Result<f64, Skip> {
    if transition.repeat_one {
        return Err(Skip::RepeatOne);
    }
    if transition.same_entry {
        return Err(Skip::SameTrack);
    }
    if transition.same_album {
        return Err(Skip::SameAlbum);
    }
    if !transition.outgoing.pcm || !transition.incoming.pcm {
        return Err(Skip::Dsd);
    }
    if transition.outgoing.spec != transition.incoming.spec {
        return Err(Skip::DifferentFormat);
    }
    let (Some(outgoing), Some(incoming)) =
        (transition.outgoing.seconds, transition.incoming.seconds)
    else {
        return Err(Skip::UnknownLength);
    };
    let shortest = outgoing.min(incoming);
    let seconds = if shortest.is_finite() {
        config.seconds.min(shortest / 2.0)
    } else {
        config.seconds
    };
    if seconds.is_finite() && seconds > 0.0 {
        Ok(seconds)
    } else {
        Err(Skip::UnknownLength)
    }
}

/// An equal-power (cosine/sine) crossfade over a fixed number of frames.
#[derive(Debug, Clone, Copy)]
pub struct Fade {
    frames: u64,
    mixed: u64,
}

impl Fade {
    pub const fn new(frames: u64) -> Self {
        Self { frames, mixed: 0 }
    }

    /// Frames the fade spans in total.
    pub const fn frames(&self) -> u64 {
        self.frames
    }

    /// Frames mixed so far.
    pub const fn mixed(&self) -> u64 {
        self.mixed
    }

    /// Frames still to mix.
    pub const fn remaining(&self) -> u64 {
        self.frames.saturating_sub(self.mixed)
    }

    pub const fn is_done(&self) -> bool {
        self.mixed >= self.frames
    }

    /// Mixes the next block at the current position, interleaved in `channels`.
    ///
    /// A side that is `None` (or shorter) counts as silence, so a track that ended inside the
    /// fade still fades out cleanly. `out` is cleared and receives at most the frames left in
    /// the fade; the returned frame count is what was written. The first frame weights the
    /// outgoing side fully, the last the incoming side fully, so the samples on either side of
    /// the fade join without a step.
    pub fn mix(
        &mut self,
        outgoing: Option<&[f64]>,
        incoming: Option<&[f64]>,
        channels: usize,
        out: &mut Vec<f64>,
    ) -> usize {
        out.clear();
        let channels = channels.max(1);
        let provided = [outgoing, incoming]
            .into_iter()
            .flatten()
            .map(|side| side.len() / channels)
            .min()
            .unwrap_or(0) as u64;
        let frames = provided.min(self.remaining());
        if frames == 0 {
            return 0;
        }
        // t runs 0..=1 across the whole fade, not the block.
        let span = self.frames.saturating_sub(1).max(1) as f64;
        let base = self.mixed;
        for frame in 0..frames {
            let t = (((base + frame) as f64) / span).min(1.0);
            let (sine, cosine) = (std::f64::consts::FRAC_PI_2 * t).sin_cos();
            let start = usize::try_from(frame).unwrap_or(usize::MAX) * channels;
            for channel in 0..channels {
                let index = start + channel;
                let outgoing = outgoing.map_or(0.0, |s| s[index]);
                let incoming = incoming.map_or(0.0, |s| s[index]);
                out.push(outgoing * cosine + incoming * sine);
            }
        }
        self.mixed += frames;
        usize::try_from(frames).unwrap_or(usize::MAX)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worker::OutputMode;

    const RATE: f64 = 44_100.0;

    fn spec(rate: u32) -> OutputSpec {
        OutputSpec {
            rate,
            bits: 16,
            channels: 2,
            mode: OutputMode::Pcm,
        }
    }

    fn side(rate: u32, seconds: f64) -> Side {
        Side {
            spec: spec(rate),
            pcm: true,
            seconds: Some(seconds),
        }
    }

    /// Interleaved stereo sine, the same sample in both channels.
    fn tone(hz: f64, seconds: f64, amplitude: f64) -> Vec<f64> {
        let frames = (seconds * RATE) as usize;
        (0..frames)
            .flat_map(|frame| {
                let sample =
                    amplitude * (2.0 * std::f64::consts::PI * hz * frame as f64 / RATE).sin();
                [sample, sample]
            })
            .collect()
    }

    fn rms(samples: &[f64]) -> f64 {
        (samples.iter().map(|s| s * s).sum::<f64>() / samples.len() as f64).sqrt()
    }

    /// Magnitude of `hz` in an interleaved stereo slice, from one channel.
    fn magnitude(samples: &[f64], hz: f64) -> f64 {
        let frames = samples.len() / 2;
        let (mut re, mut im) = (0.0, 0.0);
        for frame in 0..frames {
            let angle = 2.0 * std::f64::consts::PI * hz * frame as f64 / RATE;
            re += samples[frame * 2] * angle.cos();
            im -= samples[frame * 2] * angle.sin();
        }
        (re * re + im * im).sqrt() / frames as f64
    }

    #[test]
    fn equal_power_keeps_two_tones_at_a_constant_level() {
        let fade_seconds = 1.0;
        let outgoing = tone(500.0, fade_seconds, 0.5);
        let incoming = tone(2_000.0, fade_seconds, 0.5);
        let frames = outgoing.len() / 2;
        let mut fade = Fade::new(frames as u64);
        let mut mixed = Vec::new();
        fade.mix(Some(&outgoing), Some(&incoming), 2, &mut mixed);
        assert_eq!(fade.mixed(), frames as u64);
        assert!(fade.is_done());
        assert_eq!(mixed.len(), outgoing.len());

        let reference = rms(&outgoing);
        let reference_db = 20.0 * (reference / 0.5 / std::f64::consts::SQRT_2).log10();
        // Every fifth of the fade, including its edges, stays within ±0.5 dB.
        let step = frames / 5;
        for start in (0..frames - step).step_by(step) {
            let window = &mixed[start * 2..(start + step) * 2];
            let level = rms(window);
            let db = 20.0 * (level / 0.5 / std::f64::consts::SQRT_2).log10();
            assert!(
                (db - reference_db).abs() < 0.5,
                "window at {start}: {db:.2} dB vs {reference_db:.2} dB"
            );
        }

        // The middle of the fade holds both tones.
        let middle = &mixed[(frames / 2 - step / 2) * 2..(frames / 2 + step / 2) * 2];
        assert!(
            magnitude(middle, 500.0) > 0.01,
            "outgoing tone missing: {}",
            magnitude(middle, 500.0)
        );
        assert!(
            magnitude(middle, 2_000.0) > 0.01,
            "incoming tone missing: {}",
            magnitude(middle, 2_000.0)
        );
    }

    #[test]
    fn the_fade_starts_and_ends_without_a_step() {
        let outgoing: Vec<f64> = (0..800).map(|i| f64::from(i) / 800.0).collect();
        let incoming: Vec<f64> = (0..800).map(|i| 1.0 + f64::from(i) / 800.0).collect();
        let mut fade = Fade::new(300);
        let mut mixed = Vec::new();
        // A short block mixes only what it carries, and keeps its place in the curve.
        let first = fade.mix(
            Some(&outgoing[..200]),
            Some(&incoming[..200]),
            2,
            &mut mixed,
        );
        assert_eq!(first, 100);
        assert_eq!(fade.mixed(), 100);
        assert!(!fade.is_done());
        assert!(
            (mixed[0] - outgoing[0]).abs() < 1e-12,
            "the incoming gain is zero first"
        );
        assert!((mixed[1] - outgoing[1]).abs() < 1e-12);
        // The next block continues where the first stopped and is cut at the fade's end.
        let mut next = Vec::new();
        let second = fade.mix(Some(&outgoing[200..]), Some(&incoming[200..]), 2, &mut next);
        assert_eq!(second, 200);
        assert!(fade.is_done());
        assert_eq!(next.len(), 200 * 2);
        // The last frame is the incoming side at the end of the fade.
        assert!((next[next.len() - 1] - incoming[599]).abs() < 1e-9);
        assert!(
            next[next.len() - 2] > 1.0,
            "incoming only: {}",
            next[next.len() - 2]
        );
        assert_eq!(
            fade.mix(Some(&outgoing[600..]), Some(&incoming[600..]), 2, &mut next),
            0,
            "nothing is left to mix"
        );
    }

    #[test]
    fn a_missing_side_fades_out_or_in_over_silence() {
        let incoming = tone(1_000.0, 0.1, 0.5);
        let frames = incoming.len() / 2;
        let mut fade = Fade::new(frames as u64);
        let mut mixed = Vec::new();
        fade.mix(Some(&incoming), None, 2, &mut mixed);
        assert_eq!(mixed.len() / 2, frames);
        // The outgoing side only, so the level falls from its own to silence.
        assert!(rms(&mixed[..200]) > rms(&mixed[mixed.len() - 200..]));
        assert!(rms(&mixed[mixed.len() - 20..]) < 0.01);

        let outgoing = tone(1_000.0, 0.1, 0.5);
        let mut fade = Fade::new(frames as u64);
        fade.mix(None, Some(&outgoing), 2, &mut mixed);
        assert!(rms(&mixed[..20]) < 0.01);
        assert!(rms(&mixed[mixed.len() - 200..]) > 0.01);
    }

    #[test]
    fn decision_table() {
        let config = CrossfadeConfig {
            enabled: true,
            seconds: 4.0,
        };
        let base = Transition {
            outgoing: side(44_100, 300.0),
            incoming: side(44_100, 300.0),
            repeat_one: false,
            same_entry: false,
            same_album: false,
        };
        assert_eq!(decide(config, base), Ok(4.0));
        assert_eq!(
            decide(
                config,
                Transition {
                    repeat_one: true,
                    ..base
                }
            ),
            Err(Skip::RepeatOne)
        );
        assert_eq!(
            decide(
                config,
                Transition {
                    same_entry: true,
                    ..base
                }
            ),
            Err(Skip::SameTrack)
        );
        assert_eq!(
            decide(
                config,
                Transition {
                    same_album: true,
                    ..base
                }
            ),
            Err(Skip::SameAlbum)
        );
        assert_eq!(
            decide(
                config,
                Transition {
                    incoming: Side {
                        pcm: false,
                        ..base.incoming
                    },
                    ..base
                }
            ),
            Err(Skip::Dsd)
        );
        assert_eq!(
            decide(
                config,
                Transition {
                    incoming: side(48_000, 300.0),
                    ..base
                }
            ),
            Err(Skip::DifferentFormat)
        );
        assert_eq!(
            decide(
                config,
                Transition {
                    incoming: Side {
                        seconds: None,
                        ..base.incoming
                    },
                    ..base
                }
            ),
            Err(Skip::UnknownLength)
        );
        // Half of the shorter track caps the fade.
        assert_eq!(
            decide(
                config,
                Transition {
                    outgoing: side(44_100, 3.0),
                    ..base
                }
            ),
            Ok(1.5)
        );
        // The rules trust the sanitized settings; the 1–10 s clamp lives in `CrossfadeConfig`.
        assert_eq!(
            decide(
                CrossfadeConfig {
                    enabled: true,
                    seconds: 30.0
                },
                base
            ),
            Ok(30.0),
            "the settings clamp before the engine sees them"
        );
        assert_eq!(
            CrossfadeConfig {
                enabled: true,
                seconds: 30.0
            }
            .sanitized()
            .seconds,
            MAX_SECONDS
        );
    }
}
