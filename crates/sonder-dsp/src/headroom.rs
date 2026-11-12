//! Fixed gain inserted before rate conversion to keep interpolation overshoot below full
//! scale. It is a normal [`Stage`], so the signal path reports it as a non-bit-perfect step.

use crate::stage::{Stage, StreamFormat};

/// Linear gain in decibels, applied to every channel.
#[derive(Debug, Clone, Copy)]
pub struct Headroom {
    gain: f64,
    channels: usize,
}

impl Headroom {
    pub fn from_db(db: f64) -> Self {
        Self {
            gain: 10f64.powf(db / 20.0),
            channels: 1,
        }
    }

    pub fn gain(self) -> f64 {
        self.gain
    }
}

impl Stage for Headroom {
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
        let mut stage = Headroom::from_db(-20.0);
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
