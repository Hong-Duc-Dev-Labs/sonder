//! Convolution with a measured impulse response (room correction, headphone EQ).
//!
//! [`ImpulseResponse`] loads a WAV file (16/24/32-bit integer or 32-bit float), one channel
//! applied to every stream channel or one channel per stream channel. When the stream runs at
//! another rate, the response is resampled once with Sonder's linear-phase converter, its
//! filter delay removed and its level kept, and the result is cached per rate.
//!
//! [`Convolver`] is a uniformly partitioned overlap-save convolver. Input collects into a
//! partition of `B` frames; each full partition is transformed once and multiplied against
//! every partition of the response in a frequency-domain delay line. Output is the exact
//! convolution `y[n] = Σ h[k]·x[n−k]` from the first input frame on: the stage holds up to
//! `B − 1` frames until their partition completes, the way a resampler holds history, and
//! [`flush`](crate::Stage::flush) emits them plus the response's tail. No silence is added.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::SystemTime;

use realfft::num_complex::Complex;
use realfft::{ComplexToReal, RealFftPlanner, RealToComplex};

use crate::FilterVariant;
use crate::filter_source::{FilterPhase, FilterSourceKind, FilterSpec, FilterTable, gcd};
use crate::multistage::plan;
use crate::resampler::Resampler;
use crate::sonder_design::design_coefficients;
use crate::stage::{Stage, StreamFormat};

/// Longest response accepted, in seconds. Room-correction and headphone responses are well
/// under a second; the cap keeps a 768 kHz stream's partitions within ~100 MB.
pub const MAX_IR_SECONDS: f64 = 2.0;

/// Smallest and largest partition, in frames.
const MIN_PARTITION: usize = 1024;
const MAX_PARTITION: usize = 8192;

/// Why a response could not be used.
#[derive(Debug, thiserror::Error, Clone, PartialEq)]
pub enum IrError {
    #[error("cannot read {0}")]
    Read(String),
    #[error("{0} holds no samples")]
    Empty(String),
    #[error("{name} is {seconds:.1} s long; responses up to {max:.0} s are supported")]
    TooLong {
        name: String,
        seconds: f64,
        max: f64,
    },
    #[error("{name} runs at {rate} Hz, outside 8–768 kHz")]
    Rate { name: String, rate: u32 },
}

/// A loaded impulse response: per-channel samples at the file's rate.
pub struct ImpulseResponse {
    name: String,
    rate: u32,
    channels: Vec<Vec<f64>>,
    /// Responses resampled to other stream rates, built on first use.
    resampled: Mutex<HashMap<u32, Arc<Vec<Vec<f64>>>>>,
}

impl std::fmt::Debug for ImpulseResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImpulseResponse")
            .field("name", &self.name)
            .field("rate", &self.rate)
            .field("channels", &self.channels.len())
            .field("frames", &self.frames())
            .finish()
    }
}

impl PartialEq for ImpulseResponse {
    fn eq(&self, other: &Self) -> bool {
        self.rate == other.rate && self.channels == other.channels
    }
}

impl ImpulseResponse {
    /// Reads a WAV file.
    pub fn load(path: &Path) -> Result<Self, IrError> {
        let name = path.file_name().map_or_else(
            || path.display().to_string(),
            |n| n.to_string_lossy().into(),
        );
        let read_error = |err: hound::Error| IrError::Read(format!("{name}: {err}"));
        let mut reader = hound::WavReader::open(path).map_err(read_error)?;
        let spec = reader.spec();
        let count = usize::from(spec.channels.max(1));
        let interleaved: Vec<f64> = match spec.sample_format {
            hound::SampleFormat::Float => reader
                .samples::<f32>()
                .map(|s| s.map(f64::from))
                .collect::<Result<_, _>>()
                .map_err(read_error)?,
            hound::SampleFormat::Int => {
                let scale = 1.0 / f64::from(1u32 << (spec.bits_per_sample.clamp(8, 32) - 1));
                reader
                    .samples::<i32>()
                    .map(|s| s.map(|v| f64::from(v) * scale))
                    .collect::<Result<_, _>>()
                    .map_err(read_error)?
            }
        };
        let mut channels = vec![Vec::with_capacity(interleaved.len() / count); count];
        for frame in interleaved.chunks_exact(count) {
            for (channel, &sample) in channels.iter_mut().zip(frame) {
                channel.push(sample);
            }
        }
        Self::new(name, spec.sample_rate, channels)
    }

    /// Reads a WAV file, reusing the previous result while the file is unchanged. Settings are
    /// applied often (every slider move), and the engine compares responses by content.
    pub fn load_cached(path: &Path) -> Result<Arc<Self>, IrError> {
        /// The file a response was read from: path, modification time and length.
        type Key = (PathBuf, Option<SystemTime>, u64);
        type Last = Option<(Key, Arc<ImpulseResponse>)>;
        static LAST: LazyLock<Mutex<Last>> = LazyLock::new(|| Mutex::new(None));
        let meta = std::fs::metadata(path)
            .map_err(|err| IrError::Read(format!("{}: {err}", path.display())))?;
        let key = (path.to_path_buf(), meta.modified().ok(), meta.len());
        let mut last = LAST.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some((cached, ir)) = last.as_ref()
            && *cached == key
        {
            return Ok(ir.clone());
        }
        let ir = Arc::new(Self::load(path)?);
        *last = Some((key, ir.clone()));
        Ok(ir)
    }

    /// A response from samples already in memory (`channels[c][n]`).
    pub fn new(name: String, rate: u32, channels: Vec<Vec<f64>>) -> Result<Self, IrError> {
        if !(8_000..=768_000).contains(&rate) {
            return Err(IrError::Rate { name, rate });
        }
        let frames = channels.iter().map(Vec::len).max().unwrap_or(0);
        if frames == 0 {
            return Err(IrError::Empty(name));
        }
        let seconds = frames as f64 / f64::from(rate);
        if seconds > MAX_IR_SECONDS {
            return Err(IrError::TooLong {
                name,
                seconds,
                max: MAX_IR_SECONDS,
            });
        }
        Ok(Self {
            name,
            rate,
            channels,
            resampled: Mutex::new(HashMap::new()),
        })
    }

    /// File name, for the signal path.
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn rate(&self) -> u32 {
        self.rate
    }

    pub fn channel_count(&self) -> usize {
        self.channels.len()
    }

    /// Length in frames at the file's rate (the longest channel).
    pub fn frames(&self) -> usize {
        self.channels.iter().map(Vec::len).max().unwrap_or(0)
    }

    /// Whether the response can run on a stream with `channels` channels: one channel serves
    /// any count, otherwise the counts must match.
    pub fn fits(&self, channels: u16) -> bool {
        self.channel_count() == 1 || self.channel_count() == usize::from(channels)
    }

    /// The response at `rate`, resampled and cached on first use.
    pub fn at_rate(&self, rate: u32) -> Arc<Vec<Vec<f64>>> {
        let mut cache = self
            .resampled
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        cache
            .entry(rate)
            .or_insert_with(|| {
                Arc::new(if rate == self.rate {
                    self.channels.clone()
                } else {
                    self.channels
                        .iter()
                        .map(|channel| resample(channel, self.rate, rate))
                        .collect()
                })
            })
            .clone()
    }

    /// Highest gain of the frequency response over all channels, in dB. A positive value is
    /// how much headroom the response needs to stay clear of clipping.
    pub fn peak_gain_db(&self) -> f64 {
        let len = self.frames().next_power_of_two().max(8192);
        let mut planner = RealFftPlanner::<f64>::new();
        let fft = planner.plan_fft_forward(len);
        let mut spectrum = fft.make_output_vec();
        let mut peak: f64 = 0.0;
        for channel in &self.channels {
            let mut input = vec![0.0; len];
            input[..channel.len()].copy_from_slice(channel);
            fft.process(&mut input, &mut spectrum)
                .expect("buffers come from the plan");
            for bin in &spectrum {
                peak = peak.max(bin.norm());
            }
        }
        20.0 * peak.max(1e-12).log10()
    }
}

/// Resamples a response from `from` to `to` Hz with Sonder's precise linear-phase filters,
/// removing the filters' delay and keeping the response's gain.
fn resample(samples: &[f64], from: u32, to: u32) -> Vec<f64> {
    let g = gcd(from, to);
    let ratios = plan(to / g, from / g);
    let run = |input: &[f64]| -> Vec<f64> {
        let mut signal = input.to_vec();
        let mut rate = from;
        for ratio in &ratios {
            let spec = FilterSpec::new(
                ratio.num,
                ratio.den,
                FilterPhase::Linear,
                FilterVariant::Precise,
            );
            let table = FilterTable::from_prototype(
                spec,
                FilterSourceKind::Sonder,
                design_coefficients(spec),
            )
            .expect("Sonder designs a table for every planned ratio");
            let mut stage = Resampler::new(Arc::new(table));
            let format = stage.configure(StreamFormat { rate, channels: 1 });
            let mut out = vec![0.0; stage.max_out(signal.len()) + stage.max_out(0)];
            let mut written = stage.process(&signal, &mut out);
            written += stage.flush(&mut out[written..]);
            out.truncate(written);
            signal = out;
            rate = format.rate;
        }
        signal
    };
    // The chain's delay, read off a unit impulse, is removed so the response keeps its timing.
    let probe = run(&[1.0]);
    let delay = probe
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
        .map_or(0, |(index, _)| index);
    let frames = (samples.len() as u128 * u128::from(to)).div_ceil(u128::from(from)) as usize;
    // Interpolating a response multiplies its sum (its DC gain) by `to / from`.
    let scale = f64::from(from) / f64::from(to);
    run(samples)
        .into_iter()
        .skip(delay)
        .take(frames)
        .map(|s| s * scale)
        .collect()
}

/// Partition length for a response of `frames` frames: long enough that the delay line stays
/// near 128 partitions, short enough to keep each burst of output small.
fn partition_for(frames: usize) -> usize {
    frames
        .div_ceil(128)
        .next_power_of_two()
        .clamp(MIN_PARTITION, MAX_PARTITION)
}

/// Per-channel convolution state.
struct Lane {
    /// Previous partition then the one being filled: the overlap-save input window.
    window: Vec<f64>,
    /// Spectra of the last `parts` input partitions, a ring indexed by `Convolver::head`.
    history: Vec<Vec<Complex<f64>>>,
    /// The last computed output partition.
    output: Vec<f64>,
}

/// The convolution [`Stage`]. Channels the response does not fit pass through untouched;
/// the engine reports that in the signal path.
pub struct Convolver {
    ir: Arc<ImpulseResponse>,
    channels: usize,
    /// Partition length `B`.
    partition: usize,
    /// Response length at the stream rate.
    ir_frames: usize,
    /// Response spectra per response channel, `parts` partitions of `B + 1` bins each.
    filters: Vec<Vec<Vec<Complex<f64>>>>,
    lanes: Vec<Lane>,
    /// Newest slot of every lane's `history`.
    head: usize,
    /// Frames collected in the current partition.
    fill: usize,
    forward: Option<Arc<dyn RealToComplex<f64>>>,
    inverse: Option<Arc<dyn ComplexToReal<f64>>>,
    time: Vec<f64>,
    spectrum: Vec<Complex<f64>>,
    sum: Vec<Complex<f64>>,
    scratch: Vec<Complex<f64>>,
    /// The response does not fit the stream's channels.
    bypass: bool,
}

impl Convolver {
    pub fn new(ir: Arc<ImpulseResponse>) -> Self {
        Self {
            ir,
            channels: 1,
            partition: MIN_PARTITION,
            ir_frames: 0,
            filters: Vec::new(),
            lanes: Vec::new(),
            head: 0,
            fill: 0,
            forward: None,
            inverse: None,
            time: Vec::new(),
            spectrum: Vec::new(),
            sum: Vec::new(),
            scratch: Vec::new(),
            bypass: true,
        }
    }

    pub fn impulse_response(&self) -> &Arc<ImpulseResponse> {
        &self.ir
    }

    /// Transforms the current partition of every lane and writes one output partition per
    /// lane into `Lane::output`.
    fn run_partition(&mut self) {
        let b = self.partition;
        let parts = self.filters.first().map_or(0, Vec::len);
        let (Some(forward), Some(inverse)) = (self.forward.clone(), self.inverse.clone()) else {
            return;
        };
        let scale = 1.0 / (2 * b) as f64;
        for (index, lane) in self.lanes.iter_mut().enumerate() {
            self.time.copy_from_slice(&lane.window);
            forward
                .process_with_scratch(
                    &mut self.time,
                    &mut lane.history[self.head],
                    &mut self.scratch,
                )
                .expect("buffers come from the plan");
            let filters = &self.filters[index.min(self.filters.len() - 1)];
            self.sum.fill(Complex::new(0.0, 0.0));
            for (p, filter) in filters.iter().enumerate().take(parts) {
                let input = &lane.history[(self.head + parts - p) % parts];
                for ((acc, x), h) in self.sum.iter_mut().zip(input).zip(filter) {
                    *acc += x * h;
                }
            }
            // Products of real signals' spectra are real at DC and Nyquist; rounding is not.
            self.sum[0].im = 0.0;
            self.sum[b].im = 0.0;
            self.spectrum.copy_from_slice(&self.sum);
            inverse
                .process_with_scratch(&mut self.spectrum, &mut self.time, &mut self.scratch)
                .expect("buffers come from the plan");
            for (out, &value) in lane.output.iter_mut().zip(&self.time[b..]) {
                *out = value * scale;
            }
            lane.window.copy_within(b.., 0);
        }
        self.head = (self.head + 1) % parts.max(1);
    }

    /// Writes the first `frames` frames of the last output partition, interleaved.
    fn emit(&self, frames: usize, out: &mut [f64]) {
        for (frame, samples) in out.chunks_exact_mut(self.channels).take(frames).enumerate() {
            for (sample, lane) in samples.iter_mut().zip(&self.lanes) {
                *sample = lane.output[frame];
            }
        }
    }
}

impl Stage for Convolver {
    fn configure(&mut self, input: StreamFormat) -> StreamFormat {
        self.channels = usize::from(input.channels.max(1));
        self.bypass = !self.ir.fits(input.channels.max(1));
        if self.bypass {
            self.filters.clear();
            self.lanes.clear();
            return input;
        }
        let response = self.ir.at_rate(input.rate);
        self.ir_frames = response.iter().map(Vec::len).max().unwrap_or(0);
        let b = partition_for(self.ir_frames);
        self.partition = b;
        let parts = self.ir_frames.div_ceil(b).max(1);
        let mut planner = RealFftPlanner::<f64>::new();
        let forward = planner.plan_fft_forward(2 * b);
        let inverse = planner.plan_fft_inverse(2 * b);
        self.time = vec![0.0; 2 * b];
        self.spectrum = forward.make_output_vec();
        self.sum = forward.make_output_vec();
        self.scratch =
            vec![Complex::new(0.0, 0.0); forward.get_scratch_len().max(inverse.get_scratch_len())];
        self.filters = response
            .iter()
            .map(|channel| {
                (0..parts)
                    .map(|p| {
                        self.time.fill(0.0);
                        let start = p * b;
                        let start = start.min(channel.len());
                        let end = (start + b).min(channel.len());
                        self.time[..end - start].copy_from_slice(&channel[start..end]);
                        let mut spectrum = forward.make_output_vec();
                        forward
                            .process_with_scratch(&mut self.time, &mut spectrum, &mut self.scratch)
                            .expect("buffers come from the plan");
                        spectrum
                    })
                    .collect()
            })
            .collect();
        self.lanes = (0..self.channels)
            .map(|_| Lane {
                window: vec![0.0; 2 * b],
                history: vec![forward.make_output_vec(); parts],
                output: vec![0.0; b],
            })
            .collect();
        self.forward = Some(forward);
        self.inverse = Some(inverse);
        self.head = 0;
        self.fill = 0;
        input
    }

    fn max_out(&self, in_frames: usize) -> usize {
        if self.bypass {
            return in_frames;
        }
        if in_frames == 0 {
            // Held frames plus the response's tail.
            return self.partition + self.ir_frames;
        }
        in_frames + self.partition
    }

    fn process(&mut self, input: &[f64], out: &mut [f64]) -> usize {
        let channels = self.channels;
        if self.bypass {
            out[..input.len()].copy_from_slice(input);
            return input.len() / channels;
        }
        let b = self.partition;
        let mut written = 0;
        for frame in input.chunks_exact(channels) {
            for (lane, &sample) in self.lanes.iter_mut().zip(frame) {
                lane.window[b + self.fill] = sample;
            }
            self.fill += 1;
            if self.fill == b {
                self.run_partition();
                self.emit(b, &mut out[written * channels..]);
                written += b;
                self.fill = 0;
            }
        }
        written
    }

    fn flush(&mut self, out: &mut [f64]) -> usize {
        if self.bypass || self.lanes.is_empty() {
            return 0;
        }
        let b = self.partition;
        let mut remaining = self.fill + self.ir_frames - 1;
        let mut written = 0;
        while remaining > 0 {
            for lane in &mut self.lanes {
                lane.window[b + self.fill..].fill(0.0);
            }
            self.run_partition();
            let frames = remaining.min(b);
            self.emit(frames, &mut out[written * self.channels..]);
            written += frames;
            remaining -= frames;
            self.fill = 0;
        }
        self.reset();
        written
    }

    fn reset(&mut self) {
        for lane in &mut self.lanes {
            lane.window.fill(0.0);
            for spectrum in &mut lane.history {
                spectrum.fill(Complex::new(0.0, 0.0));
            }
        }
        self.head = 0;
        self.fill = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noise(len: usize, seed: u64) -> Vec<f64> {
        let mut state = seed | 1;
        (0..len)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                (state >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0
            })
            .collect()
    }

    fn direct(x: &[f64], h: &[f64]) -> Vec<f64> {
        let mut y = vec![0.0; x.len() + h.len() - 1];
        for (n, &xn) in x.iter().enumerate() {
            for (k, &hk) in h.iter().enumerate() {
                y[n + k] += xn * hk;
            }
        }
        y
    }

    /// Runs mono or interleaved `input` through a convolver in `block`-frame pieces, then
    /// flushes.
    fn convolve(ir: ImpulseResponse, input: &[f64], channels: u16, block: usize) -> Vec<f64> {
        let mut stage = Convolver::new(Arc::new(ir));
        stage.configure(StreamFormat {
            rate: 48_000,
            channels,
        });
        let ch = usize::from(channels);
        let mut out = Vec::new();
        for piece in input.chunks(block * ch) {
            let mut buf = vec![0.0; stage.max_out(piece.len() / ch) * ch];
            let n = stage.process(piece, &mut buf);
            out.extend_from_slice(&buf[..n * ch]);
        }
        let mut buf = vec![0.0; stage.max_out(0) * ch];
        let n = stage.flush(&mut buf);
        out.extend_from_slice(&buf[..n * ch]);
        out
    }

    fn mono(h: Vec<f64>) -> ImpulseResponse {
        ImpulseResponse::new("test".into(), 48_000, vec![h]).unwrap()
    }

    #[test]
    fn a_dirac_response_is_identity() {
        let x = noise(10_000, 1);
        let mut h = vec![0.0; 300];
        h[0] = 1.0;
        let y = convolve(mono(h), &x, 1, 512);
        assert_eq!(y.len(), x.len() + 299);
        for (a, b) in y.iter().zip(&x) {
            assert!((a - b).abs() < 1e-12, "{a} vs {b}");
        }
        assert!(y[x.len()..].iter().all(|v| v.abs() < 1e-12));
    }

    #[test]
    fn matches_direct_convolution_across_several_partitions() {
        let x = noise(9_000, 2);
        let h: Vec<f64> = noise(5_000, 3).iter().map(|v| v * 0.01).collect();
        let expected = direct(&x, &h);
        let y = convolve(mono(h), &x, 1, 1000);
        assert_eq!(y.len(), expected.len());
        let worst = y
            .iter()
            .zip(&expected)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0, f64::max);
        assert!(worst < 1e-9, "largest difference {worst}");
    }

    #[test]
    fn block_size_does_not_change_the_output() {
        let x = noise(12_000, 4);
        let h: Vec<f64> = noise(2_500, 5).iter().map(|v| v * 0.02).collect();
        let reference = convolve(mono(h.clone()), &x, 1, 4096);
        for block in [1, 333, 1024] {
            let y = convolve(mono(h.clone()), &x, 1, block);
            assert_eq!(y.len(), reference.len());
            let worst = y
                .iter()
                .zip(&reference)
                .map(|(a, b)| (a - b).abs())
                .fold(0.0, f64::max);
            assert!(worst < 1e-12, "block {block}: {worst}");
        }
    }

    #[test]
    fn a_stereo_response_filters_each_channel_with_its_own_channel() {
        let frames = 4_000;
        let x = noise(frames * 2, 6);
        let left_h: Vec<f64> = noise(700, 7).iter().map(|v| v * 0.05).collect();
        let right_h = vec![0.5];
        let ir =
            ImpulseResponse::new("stereo".into(), 48_000, vec![left_h.clone(), right_h]).unwrap();
        let y = convolve(ir, &x, 2, 777);
        let left: Vec<f64> = x.iter().step_by(2).copied().collect();
        let expected_left = direct(&left, &left_h);
        // Both channels flush to the longer response's tail.
        assert_eq!(y.len() / 2, frames + 699);
        for (n, expected) in expected_left.iter().enumerate() {
            assert!((y[2 * n] - expected).abs() < 1e-9);
        }
        for n in 0..frames {
            assert!((y[2 * n + 1] - 0.5 * x[2 * n + 1]).abs() < 1e-12);
        }
    }

    #[test]
    fn a_response_that_does_not_fit_the_channels_passes_the_signal_through() {
        let ir = ImpulseResponse::new("stereo".into(), 48_000, vec![vec![0.5], vec![0.5]]).unwrap();
        assert!(!ir.fits(3));
        let x = noise(3 * 500, 8);
        let y = convolve(ir, &x, 3, 100);
        assert_eq!(y, x);
    }

    #[test]
    fn a_resampled_response_keeps_its_timing_and_gain() {
        // A Dirac at 48 kHz, delayed by 10 samples, played on a 96 kHz stream.
        let mut h = vec![0.0; 2_000];
        h[10] = 1.0;
        let ir = mono(h);
        let resampled = ir.at_rate(96_000);
        let r = &resampled[0];
        assert_eq!(r.len(), 4_000);
        let peak = r
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
            .unwrap()
            .0;
        assert_eq!(peak, 20, "the delay doubles in samples, it does not grow");
        // 1 kHz gain stays at 0 dB.
        let w = 2.0 * std::f64::consts::PI * 1_000.0 / 96_000.0;
        let (re, im) = r.iter().enumerate().fold((0.0, 0.0), |(re, im), (n, &v)| {
            (re + v * (w * n as f64).cos(), im - v * (w * n as f64).sin())
        });
        let gain_db = 20.0 * (re * re + im * im).sqrt().log10();
        assert!(gain_db.abs() < 0.05, "{gain_db} dB");
        // Cached: the same allocation comes back.
        assert!(Arc::ptr_eq(&resampled, &ir.at_rate(96_000)));
    }

    #[test]
    fn peak_gain_reports_a_boost() {
        assert!(mono(vec![1.0]).peak_gain_db().abs() < 1e-9);
        assert!((mono(vec![2.0]).peak_gain_db() - 6.0206).abs() < 1e-3);
    }

    #[test]
    fn rejects_empty_long_and_out_of_range_responses() {
        assert!(matches!(
            ImpulseResponse::new("e".into(), 48_000, vec![vec![]]),
            Err(IrError::Empty(_))
        ));
        assert!(matches!(
            ImpulseResponse::new("l".into(), 8_000, vec![vec![0.0; 16_001]]),
            Err(IrError::TooLong { .. })
        ));
        assert!(matches!(
            ImpulseResponse::new("r".into(), 1_000, vec![vec![1.0]]),
            Err(IrError::Rate { .. })
        ));
    }

    #[test]
    fn loads_integer_and_float_wav_files() {
        let dir = tempfile::tempdir().unwrap();
        let int_path = dir.path().join("int.wav");
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 44_100,
            bits_per_sample: 24,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&int_path, spec).unwrap();
        for v in [4_194_304i32, -4_194_304, 0, 8_388_607] {
            writer.write_sample(v).unwrap();
        }
        writer.finalize().unwrap();
        let ir = ImpulseResponse::load(&int_path).unwrap();
        assert_eq!((ir.rate(), ir.channel_count(), ir.frames()), (44_100, 2, 2));
        assert_eq!(ir.channels[0], vec![0.5, 0.0]);
        assert_eq!(ir.channels[1][0], -0.5);
        assert_eq!(ir.name(), "int.wav");

        let float_path = dir.path().join("float.wav");
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 96_000,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let mut writer = hound::WavWriter::create(&float_path, spec).unwrap();
        for v in [1.0f32, 0.25] {
            writer.write_sample(v).unwrap();
        }
        writer.finalize().unwrap();
        let ir = ImpulseResponse::load_cached(&float_path).unwrap();
        assert_eq!(ir.channels[0], vec![1.0, 0.25]);
        assert!(Arc::ptr_eq(
            &ir,
            &ImpulseResponse::load_cached(&float_path).unwrap()
        ));
        assert!(matches!(
            ImpulseResponse::load(&dir.path().join("missing.wav")),
            Err(IrError::Read(_))
        ));
    }
}
