//! Fixed-order chain of [`Stage`]s.
//!
//! Input is cut into blocks of at most [`BLOCK_FRAMES`] frames so every stage buffer has a
//! known size; all buffers are allocated in [`Pipeline::configure`] and reused afterwards.

use crate::stage::{Stage, StreamFormat};

/// Largest number of input frames pushed through the chain at once.
pub const BLOCK_FRAMES: usize = 4096;

#[derive(Default)]
pub struct Pipeline {
    stages: Vec<Box<dyn Stage>>,
    input: Option<StreamFormat>,
    output: Option<StreamFormat>,
    /// `bufs[i]` is the input of stage `i`; `bufs[i + 1]` its output.
    bufs: Vec<Vec<f64>>,
    /// Channel count at each buffer boundary, matching `bufs`.
    channels: Vec<usize>,
}

impl Pipeline {
    pub fn new(stages: Vec<Box<dyn Stage>>) -> Self {
        Self {
            stages,
            ..Self::default()
        }
    }

    /// No stages: the signal path is identity and the engine skips `f64` entirely.
    pub fn is_empty(&self) -> bool {
        self.stages.is_empty()
    }

    /// The format currently configured on the input side, if any.
    pub fn input_format(&self) -> Option<StreamFormat> {
        self.input
    }

    /// The format the chain emits for the configured input, if configured.
    pub fn output_format(&self) -> Option<StreamFormat> {
        self.output
    }

    /// Configures every stage for `input` and returns the chain's output format.
    ///
    /// Stage history is lost; call it only when the input format actually changes.
    pub fn configure(&mut self, input: StreamFormat) -> StreamFormat {
        let mut format = input;
        let mut frames = BLOCK_FRAMES;
        self.channels = vec![usize::from(input.channels)];
        self.bufs = vec![vec![0.0; BLOCK_FRAMES * usize::from(input.channels)]];
        for stage in &mut self.stages {
            format = stage.configure(format);
            frames = stage.max_out(frames).max(stage.max_out(0));
            let channels = usize::from(format.channels);
            self.channels.push(channels);
            self.bufs.push(vec![0.0; frames * channels]);
        }
        self.input = Some(input);
        self.output = Some(format);
        format
    }

    /// Runs `input` (interleaved, in the configured input format) through the chain and hands
    /// every produced block to `sink`.
    ///
    /// # Panics
    /// Panics if the pipeline was never configured.
    pub fn process(&mut self, input: &[f64], mut sink: impl FnMut(&[f64])) {
        let channels = self.channels[0];
        for chunk in input.chunks(BLOCK_FRAMES * channels) {
            self.bufs[0][..chunk.len()].copy_from_slice(chunk);
            self.run_from(0, chunk.len() / channels, &mut sink);
        }
    }

    /// Drains every stage's tail through the stages after it, in order.
    pub fn flush(&mut self, mut sink: impl FnMut(&[f64])) {
        for i in 0..self.stages.len() {
            let frames = self.stages[i].flush(&mut self.bufs[i + 1]);
            if frames > 0 {
                self.run_from(i + 1, frames, &mut sink);
            }
        }
    }

    /// Forgets every stage's history (seek, stop).
    pub fn reset(&mut self) {
        for stage in &mut self.stages {
            stage.reset();
        }
    }

    /// Runs the `frames` frames sitting in `bufs[first]` through stages `first..` to `sink`.
    fn run_from(&mut self, first: usize, mut frames: usize, sink: &mut impl FnMut(&[f64])) {
        for i in first..self.stages.len() {
            let (head, tail) = self.bufs.split_at_mut(i + 1);
            let input = &head[i][..frames * self.channels[i]];
            frames = self.stages[i].process(input, &mut tail[0]);
        }
        let last = self.stages.len();
        if frames > 0 {
            sink(&self.bufs[last][..frames * self.channels[last]]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stage::Volume;

    const STEREO_44K: StreamFormat = StreamFormat {
        rate: 44_100,
        channels: 2,
    };

    /// Delays the signal by `n` frames, so it has a tail to flush.
    struct Delay {
        n: usize,
        channels: usize,
        history: Vec<f64>,
    }

    impl Stage for Delay {
        fn configure(&mut self, input: StreamFormat) -> StreamFormat {
            self.channels = usize::from(input.channels);
            self.history = vec![0.0; self.n * self.channels];
            input
        }
        fn max_out(&self, in_frames: usize) -> usize {
            in_frames.max(self.n)
        }
        fn process(&mut self, input: &[f64], out: &mut [f64]) -> usize {
            let delay = self.history.len();
            let total = delay + input.len();
            for (k, o) in out[..input.len()].iter_mut().enumerate() {
                *o = if k < delay {
                    self.history[k]
                } else {
                    input[k - delay]
                };
            }
            // Keep the last `n` frames of history ++ input.
            for k in 0..delay {
                let src = total - delay + k;
                self.history[k] = if src < delay {
                    self.history[src]
                } else {
                    input[src - delay]
                };
            }
            input.len() / self.channels
        }
        fn flush(&mut self, out: &mut [f64]) -> usize {
            out[..self.history.len()].copy_from_slice(&self.history);
            self.history.fill(0.0);
            self.n
        }
        fn reset(&mut self) {
            self.history.fill(0.0);
        }
    }

    fn collect(pipeline: &mut Pipeline, input: &[f64]) -> Vec<f64> {
        let mut out = Vec::new();
        pipeline.process(input, |b| out.extend_from_slice(b));
        out
    }

    #[test]
    fn empty_pipeline_is_identity() {
        let mut p = Pipeline::new(Vec::new());
        assert!(p.is_empty());
        assert_eq!(p.configure(STEREO_44K), STEREO_44K);
        let input: Vec<f64> = (0..10_000).map(|i| f64::from(i) / 10_000.0).collect();
        assert_eq!(collect(&mut p, &input), input);
    }

    #[test]
    fn volume_scales_every_sample() {
        let mut p = Pipeline::new(vec![Box::new(Volume::from_db(-20.0))]);
        p.configure(STEREO_44K);
        let out = collect(&mut p, &[0.5, -0.5, 1.0, 0.0]);
        for (o, e) in out.iter().zip([0.05, -0.05, 0.1, 0.0]) {
            assert!((o - e).abs() < 1e-12, "{o} vs {e}");
        }
    }

    #[test]
    fn tails_flow_through_later_stages_in_order() {
        let delay = || {
            Box::new(Delay {
                n: 3,
                channels: 0,
                history: Vec::new(),
            }) as Box<dyn Stage>
        };
        let mut p = Pipeline::new(vec![delay(), Box::new(Volume::from_db(0.0)), delay()]);
        p.configure(StreamFormat {
            rate: 48_000,
            channels: 1,
        });
        // Longer than one block so chunking is exercised too.
        let input: Vec<f64> = (1..=9_000).map(f64::from).collect();
        let mut out = collect(&mut p, &input);
        p.flush(|b| out.extend_from_slice(b));

        // Two 3-frame delays: six leading zeros, then the whole input, nothing lost.
        assert_eq!(out.len(), input.len() + 6);
        assert!(out[..6].iter().all(|&s| s == 0.0));
        assert_eq!(out[6..], input[..]);
    }
}
