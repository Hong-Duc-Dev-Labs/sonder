//! Software volume: a plain linear gain from a decibel value.
//!
//! Any stage at all takes the output off the bit-perfect path, so the engine inserts this one
//! only when the user asks for software volume.

use crate::stage::{Stage, StreamFormat};

/// Linear gain applied to every channel.
#[derive(Debug, Clone, Copy)]
pub struct Volume {
    gain: f64,
    channels: usize,
}

impl Volume {
    /// Volume in decibels relative to full scale (0 dB = unity, negative attenuates).
    pub fn from_db(db: f64) -> Self {
        Self {
            gain: 10f64.powf(db / 20.0),
            channels: 1,
        }
    }

    pub fn gain(&self) -> f64 {
        self.gain
    }
}

impl Stage for Volume {
    fn configure(&mut self, input: StreamFormat) -> StreamFormat {
        self.channels = usize::from(input.channels.max(1));
        input
    }

    fn max_out(&self, in_frames: usize) -> usize {
        in_frames
    }

    fn process(&mut self, input: &[f64], out: &mut [f64]) -> usize {
        for (o, i) in out.iter_mut().zip(input) {
            *o = i * self.gain;
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

    #[test]
    fn scales_every_channel_by_the_db_gain() {
        let mut stage = Volume::from_db(-20.0);
        stage.configure(StreamFormat {
            rate: 44_100,
            channels: 2,
        });
        let mut out = [0.0; 4];
        let frames = stage.process(&[1.0, -1.0, 0.5, -0.5], &mut out);
        assert_eq!(frames, 2);
        for (o, e) in out.iter().zip([0.1, -0.1, 0.05, -0.05]) {
            assert!((o - e).abs() < 1e-12);
        }
    }
}
