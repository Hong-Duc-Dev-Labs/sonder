//! Turns decoded audio into the left-justified `i32` blocks the output ring carries.
//!
//! PCM with no DSP stages goes straight through the decoder's integers (no `f64`
//! conversion), which is what makes the default path bit-perfect. With stages, samples run
//! through the [`Pipeline`] in `f64` and are converted back at the end.
//!
//! DSD has three paths: packed payload for DoP, four-byte words for native DSD, or the
//! [`DsdToPcm`] decimator feeding the normal pipeline. DoP and native DSD never touch `f64`
//! and must run with the pipeline empty.

use sonder_decode::{DecodeError, Decoder, DsdFormat, SourceDecoder, SourceFormat};
use sonder_dsp::DsdToPcm;

use crate::dop::Packer;
use crate::pipeline::Pipeline;
use crate::stage::{Stage, StreamFormat};

/// What the device must accept for the samples a track produces, after DSP.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutputSpec {
    pub rate: u32,
    /// Valid bits the device container must hold without loss.
    pub bits: u32,
    pub channels: u16,
    pub mode: OutputMode,
}

/// How the samples of a track reach the device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    /// Integer PCM, possibly through the DSP pipeline.
    Pcm,
    /// DoP payload at the carrier rate; the renderer adds markers.
    Dop,
    /// ALSA `DSD_U32_BE` words at the word rate.
    NativeDsd,
}

/// Per-device DSD handling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DsdMode {
    /// DoP on a capable device, DSD→PCM otherwise.
    #[default]
    Dop,
    /// Native DSD when the backend exposes it, DoP next, DSD→PCM last.
    Native,
    /// Always convert to PCM.
    Pcm,
}

/// Valid bits requested when DSP is active, unless the device cannot carry them.
const DSP_OUTPUT_BITS: u32 = 24;
/// Bits the DSD→PCM converter claims, and the DoP payload depth.
const DSD_PCM_BITS: u32 = 24;

enum DsdPath {
    Dop(Packer),
    Native(Packer),
    Pcm {
        converter: Box<DsdToPcm>,
        dsd_rate: u32,
        out_rate: u32,
    },
}

pub(crate) struct DecodeWorker {
    pipeline: Pipeline,
    float_in: Vec<f64>,
    dsd_path: Option<DsdPath>,
    dsd_bytes: Vec<u8>,
    dsd_float: Vec<f64>,
}

impl DecodeWorker {
    pub(crate) fn new(stages: Vec<Box<dyn Stage>>) -> Self {
        Self {
            pipeline: Pipeline::new(stages),
            float_in: Vec::new(),
            dsd_path: None,
            dsd_bytes: Vec::new(),
            dsd_float: Vec::new(),
        }
    }

    /// Replaces the DSP chain. History of the old chain is dropped without flushing.
    pub(crate) fn set_stages(&mut self, stages: Vec<Box<dyn Stage>>) {
        self.pipeline = Pipeline::new(stages);
    }

    /// Whether the DSP chain is empty, which DoP and native DSD require.
    pub(crate) fn pipeline_is_empty(&self) -> bool {
        self.pipeline.is_empty()
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
                mode: OutputMode::Pcm,
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
            mode: OutputMode::Pcm,
        }
    }

    /// Prepares for a DSD track in `mode`, converting to `pcm_rate` when needed.
    ///
    /// The caller must have checked that `mode` is not DoP/native while the pipeline has
    /// stages: those paths are bit-perfect and would be corrupted by any DSP.
    pub(crate) fn prepare_dsd(
        &mut self,
        dsd: DsdFormat,
        mode: DsdMode,
        pcm_rate: u32,
        tail: &mut Vec<i32>,
    ) -> OutputSpec {
        let channels = dsd.channels;
        match mode {
            DsdMode::Dop => {
                debug_assert!(self.pipeline.is_empty());
                self.dsd_path = Some(DsdPath::Dop(Packer::dop(channels)));
                OutputSpec {
                    rate: dsd.rate / 16,
                    bits: DSD_PCM_BITS,
                    channels,
                    mode: OutputMode::Dop,
                }
            }
            DsdMode::Native => {
                debug_assert!(self.pipeline.is_empty());
                self.dsd_path = Some(DsdPath::Native(Packer::native(channels)));
                OutputSpec {
                    rate: dsd.rate / 32,
                    bits: 32,
                    channels,
                    mode: OutputMode::NativeDsd,
                }
            }
            DsdMode::Pcm => {
                let input = StreamFormat {
                    rate: pcm_rate,
                    channels,
                };
                let output = if self.pipeline.is_empty() {
                    input
                } else {
                    match self.pipeline.input_format() {
                        Some(current) if current == input => self.output_format(),
                        Some(_) => {
                            self.flush_tail(tail);
                            self.pipeline.configure(input)
                        }
                        None => self.pipeline.configure(input),
                    }
                };
                let reuse = matches!(
                    &self.dsd_path,
                    Some(DsdPath::Pcm { dsd_rate, out_rate, .. })
                        if *dsd_rate == dsd.rate && *out_rate == pcm_rate
                );
                if !reuse {
                    let converter = DsdToPcm::new(dsd.rate, pcm_rate, channels)
                        .expect("DSD rates are validated by the reader");
                    self.dsd_path = Some(DsdPath::Pcm {
                        converter: Box::new(converter),
                        dsd_rate: dsd.rate,
                        out_rate: pcm_rate,
                    });
                }
                OutputSpec {
                    rate: output.rate,
                    bits: DSD_PCM_BITS,
                    channels: output.channels,
                    mode: OutputMode::Pcm,
                }
            }
        }
    }

    fn output_format(&mut self) -> StreamFormat {
        // Re-deriving through `configure` would reset history; the input is unchanged, so the
        // output is whatever the last configuration produced.
        self.pipeline
            .output_format()
            .expect("configured pipeline has an output format")
    }

    /// Appends the next decoded block to `out`. Returns the source units decoded; `0` means
    /// the track ended. DSD packers may still append their final padded frame on `0`.
    pub(crate) fn decode(
        &mut self,
        decoder: &mut SourceDecoder,
        out: &mut Vec<i32>,
    ) -> Result<usize, DecodeError> {
        match decoder {
            SourceDecoder::Pcm(decoder) => self.decode_pcm(decoder, out),
            SourceDecoder::Dsd(decoder) => {
                let frames = decoder.next_bytes(&mut self.dsd_bytes)?;
                let ended = frames == 0;
                let pcm = match &mut self.dsd_path {
                    Some(DsdPath::Dop(packer)) | Some(DsdPath::Native(packer)) => {
                        if ended {
                            packer.finish(out);
                        } else {
                            packer.push(&self.dsd_bytes, out);
                        }
                        false
                    }
                    Some(DsdPath::Pcm { converter, .. }) => {
                        if !ended {
                            self.dsd_float.clear();
                            converter.process(&self.dsd_bytes, &mut self.dsd_float);
                        }
                        true
                    }
                    None => {
                        return Err(DecodeError::Decode("DSD path not configured".into()));
                    }
                };
                if pcm && !ended {
                    if self.pipeline.is_empty() {
                        append_converted(&self.dsd_float, out);
                    } else {
                        self.pipeline
                            .process(&self.dsd_float, |block| append_converted(block, out));
                    }
                }
                Ok(frames)
            }
        }
    }

    fn decode_pcm(
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

    /// Forgets stage and converter history (seek, stop) without emitting it.
    pub(crate) fn reset(&mut self) {
        self.pipeline.reset();
        match &mut self.dsd_path {
            Some(DsdPath::Dop(packer)) | Some(DsdPath::Native(packer)) => packer.reset(),
            Some(DsdPath::Pcm { converter, .. }) => converter.reset(),
            None => {}
        }
        self.dsd_bytes.clear();
        self.dsd_float.clear();
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

    #[test]
    fn dsd_output_specs_land_on_the_right_rates() {
        let dsd = DsdFormat {
            rate: 2_822_400,
            channels: 2,
            bytes: Some(1_000),
        };
        let mut worker = DecodeWorker::new(Vec::new());
        let mut tail = Vec::new();
        let dop = worker.prepare_dsd(dsd, DsdMode::Dop, 176_400, &mut tail);
        assert_eq!(
            dop,
            OutputSpec {
                rate: 176_400,
                bits: 24,
                channels: 2,
                mode: OutputMode::Dop
            }
        );
        let native = worker.prepare_dsd(dsd, DsdMode::Native, 176_400, &mut tail);
        assert_eq!(
            native,
            OutputSpec {
                rate: 88_200,
                bits: 32,
                channels: 2,
                mode: OutputMode::NativeDsd
            }
        );
        let pcm = worker.prepare_dsd(dsd, DsdMode::Pcm, 176_400, &mut tail);
        assert_eq!(
            pcm,
            OutputSpec {
                rate: 176_400,
                bits: 24,
                channels: 2,
                mode: OutputMode::Pcm
            }
        );
    }
}
