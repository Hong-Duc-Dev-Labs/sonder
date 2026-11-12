//! DSP stage contract and the stages that ship with the engine.
//!
//! The contract itself lives in `sonder-dsp` so filters, resamplers and later PEQ/levelling
//! stages share one type; the software volume stage stays here because it is engine policy.

pub use sonder_dsp::stage::{Stage, StreamFormat};

/// Software volume. Not part of the default chain: any stage at all means the output is no
/// longer bit-perfect.
#[derive(Debug, Clone)]
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
