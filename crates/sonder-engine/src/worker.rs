//! Turns decoded audio into the left-justified `i32` blocks the output ring carries.
//!
//! With no DSP stages the decoder's integers go straight through (no `f64` conversion), which
//! is what makes the default path bit-perfect. With stages, samples run through the
//! [`Pipeline`] in `f64` and are converted back at the end.

use sonder_decode::{DecodeError, Decoder, SourceFormat};

use crate::pipeline::Pipeline;
use crate::stage::{Stage, StreamFormat};

/// What the device must accept for the samples a track produces, after DSP.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutputSpec {
    pub rate: u32,
    /// Valid bits the device container must hold without loss.
    pub bits: u32,
    pub channels: u16,
}

/// Valid bits requested from the device when DSP is active: enough headroom that a volume
/// change does not throw away the source's resolution.
const DSP_OUTPUT_BITS: u32 = 24;

pub(crate) struct DecodeWorker {
    pipeline: Pipeline,
    float_in: Vec<f64>,
}

impl DecodeWorker {
    pub(crate) fn new(stages: Vec<Box<dyn Stage>>) -> Self {
        Self {
            pipeline: Pipeline::new(stages),
            float_in: Vec::new(),
        }
    }

    /// Replaces the DSP chain. History of the old chain is dropped without flushing.
    pub(crate) fn set_stages(&mut self, stages: Vec<Box<dyn Stage>>) {
        self.pipeline = Pipeline::new(stages);
    }

    /// Prepares for a track in `source` format and returns what the device has to play.
    ///
    /// Stages keep their history when the input format is unchanged (gapless across
    /// same-format tracks). On a change, the old chain's tail is appended to `tail` first so
    /// nothing buffered in a stage is lost.
    pub(crate) fn prepare(&mut self, source: SourceFormat, tail: &mut Vec<i32>) -> OutputSpec {
        let input = StreamFormat {
            rate: source.rate,
            channels: source.channels,
        };
        if self.pipeline.is_empty() {
            return OutputSpec {
                rate: source.rate,
                bits: source.bits,
                channels: source.channels,
            };
        }
        let output = match self.pipeline.input_format() {
            Some(current) if current == input => self.output_format(),
            Some(_) => {
                self.flush_tail(tail);
                self.pipeline.configure(input)
            }
            None => self.pipeline.configure(input),
        };
        OutputSpec {
            rate: output.rate,
            bits: source.bits.max(DSP_OUTPUT_BITS),
            channels: output.channels,
        }
    }

    fn output_format(&mut self) -> StreamFormat {
        // Re-deriving through `configure` would reset history; the input is unchanged, so the
        // output is whatever the last configuration produced.
        self.pipeline
            .output_format()
            .expect("configured pipeline has an output format")
    }

    /// Appends the next decoded block to `out`. Returns the source frames decoded; `0` means
    /// the track ended.
    pub(crate) fn decode(
        &mut self,
        decoder: &mut Decoder,
        out: &mut Vec<i32>,
    ) -> Result<usize, DecodeError> {
        if self.pipeline.is_empty() {
            return decoder.next_i32(out);
        }
        let frames = decoder.next_f64(&mut self.float_in)?;
        self.pipeline
            .process(&self.float_in, |block| append_converted(block, out));
        Ok(frames)
    }

    /// Appends whatever the stages still hold (end of queue or format change).
    pub(crate) fn flush_tail(&mut self, out: &mut Vec<i32>) {
        if self.pipeline.input_format().is_some() {
            self.pipeline.flush(|block| append_converted(block, out));
        }
    }

    /// Forgets stage history (seek, stop) without emitting it.
    pub(crate) fn reset(&mut self) {
        self.pipeline.reset();
    }
}

/// Converts `[-1.0, 1.0)` samples to left-justified `i32`, rounding to nearest and clipping.
/// Device packing then keeps the top bits; dither arrives with its own stage.
fn append_converted(block: &[f64], out: &mut Vec<i32>) {
    const SCALE: f64 = 2_147_483_648.0;
    out.extend(
        block
            .iter()
            .map(|&s| (s * SCALE).round().clamp(-SCALE, SCALE - 1.0) as i32),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversion_is_exact_for_integer_sources_and_clips() {
        let mut out = Vec::new();
        append_converted(&[0.5, -1.0, 0.0, 1.0, -2.0, 32_767.0 / 32_768.0], &mut out);
        assert_eq!(
            out,
            vec![1 << 30, i32::MIN, 0, i32::MAX, i32::MIN, 32_767 << 16]
        );
    }
}
