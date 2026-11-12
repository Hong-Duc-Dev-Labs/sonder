//! The DSP stage contract, owned here so every crate converts rates and shapes samples with
//! the same interface.

/// Rate and channel count of the interleaved `f64` signal between stages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StreamFormat {
    pub rate: u32,
    pub channels: u16,
}

/// One step of the DSP chain, working on interleaved `f64` samples in `[-1.0, 1.0)`.
///
/// Real-time rules: allocate only in [`configure`](Self::configure); `process`, `flush` and
/// `reset` must not allocate, lock or block. All counts are in frames.
pub trait Stage: Send {
    /// Prepares for `input` and returns the format this stage emits. Called whenever the
    /// input format changes; may allocate.
    fn configure(&mut self, input: StreamFormat) -> StreamFormat;

    /// Upper bound on the frames [`process`](Self::process) writes for `in_frames` input
    /// frames. `max_out(0)` bounds what [`flush`](Self::flush) writes.
    fn max_out(&self, in_frames: usize) -> usize;

    /// Consumes all of `input` and writes the frames it can produce into `out`, which holds at
    /// least `max_out(input frames)` frames. Returns the frames written.
    fn process(&mut self, input: &[f64], out: &mut [f64]) -> usize;

    /// Emits the tail still held by the stage (filter history, resampler phase) at the end
    /// of a stream. `out` holds at least `max_out(0)` frames. Returns the frames written.
    fn flush(&mut self, out: &mut [f64]) -> usize;

    /// Forgets all history, as after a seek.
    fn reset(&mut self);
}
