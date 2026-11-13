//! PCM→DSD: 8× interpolation followed by a single-bit sigma-delta modulator.
//!
//! The chain is the one Roon and sox use for PCM→DSD: the PCM signal arrives at
//! `DSD rate / 8` (the engine's resampler plan produces that rate), an integer 8× polyphase
//! interpolator lifts it to the DSD bit rate, and a 5th- or 7th-order CIFF modulator turns it
//! into one bit per sample. Output is packed the way `sonder-decode` reads DSD: interleaved
//! MSB-first bytes, one byte per eight bit samples per channel.
//!
//! # Coefficients
//!
//! The tables in [`coeffs`] are the published Schreier-synthesised single-bit CIFF sets used
//! by sox's `sdm` effect (`sdm-5`/`sdm-7`), re-expressed for the topology below. They are
//! designed for these exact bit rates, so the same table serves both 44.1k- and 48k-family
//! sources (the input has already been resampled to `rate / 8`). `tools/design_sdm.py`
//! re-derives the loop response from these numbers, checks stability, and prints the tables.
//!
//! Every resonator `g` puts a pair of NTF zeros at `sqrt(g) · bit rate / 2π`, so a table
//! built for one rate places them at the same audio frequencies as its neighbours (a test
//! checks this). The DSD64 5th-order set is derived rather than copied: its zeros keep the
//! DSD128 set's audio frequencies, and its NTF denominator is extrapolated from DSD128 and
//! DSD256 (each halving of the rate moves every coefficient four times as far as the last),
//! then realised for this loop. The same procedure reproduces the published DSD64 7th-order
//! set to six significant digits.
//!
//! # Headroom and stability
//!
//! 0 dBFS PCM maps to 50 % modulation, the SACD convention both sox and Roon follow, so the
//! modulator sees at most ±0.5 and the DSD stream is 6 dB below full scale. The interpolated
//! signal is clamped to that range (loud inter-sample peaks are limited, not wrapped), and the state
//! is cleared when a pole grows past [`STATE_LIMIT`] — an unstable master degrades to a
//! click instead of a permanently stuck modulator.
//!
//! # Real-time rules
//!
//! Buffers are allocated in [`Sdm::new`] with room for one [`Sdm::MAX_FRAMES`] block;
//! [`Sdm::process`] reuses them and never allocates for blocks up to that size. The stage
//! runs on the decode worker, never on the render thread.

/// DSD bit rates this module can target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SdmRate {
    Dsd64,
    Dsd128,
    Dsd256,
}

impl SdmRate {
    /// Bits per second per channel.
    pub const fn bit_rate(self) -> u32 {
        match self {
            Self::Dsd64 => 2_822_400,
            Self::Dsd128 => 5_644_800,
            Self::Dsd256 => 11_289_600,
        }
    }

    /// Rate of the PCM the modulator expects: the bit rate divided by [`OVERSAMPLE`].
    pub const fn input_rate(self) -> u32 {
        self.bit_rate() / OVERSAMPLE as u32
    }

    /// Carrier rate of a DoP stream carrying this rate.
    pub const fn dop_rate(self) -> u32 {
        self.bit_rate() / 16
    }

    /// Word rate of an ALSA native-DSD stream carrying this rate.
    pub const fn native_rate(self) -> u32 {
        self.bit_rate() / 32
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Dsd64 => "DSD64",
            Self::Dsd128 => "DSD128",
            Self::Dsd256 => "DSD256",
        }
    }
}

/// Modulator order: the number of noise-shaping zeros and integrators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SdmOrder {
    #[default]
    Order5,
    Order7,
}

impl SdmOrder {
    pub const fn as_u8(self) -> u8 {
        match self {
            Self::Order5 => 5,
            Self::Order7 => 7,
        }
    }
}

/// Integer interpolation factor from the modulator input rate to the bit rate.
pub const OVERSAMPLE: usize = 8;

/// 0 dBFS PCM maps to this modulation depth (the SACD convention).
pub const INPUT_SCALE: f64 = 0.5;

/// A state larger than this means the loop left its stable region.
pub const STATE_LIMIT: f64 = 1.0e7;

/// Taps of the 8× interpolation prototype.
const INTERP_TAPS: usize = 64;
/// Taps each of the eight polyphase branches uses.
const PHASE_TAPS: usize = INTERP_TAPS / OVERSAMPLE;

/// One rate's modulator coefficients: CIFF feedforward `a` and resonator `g`.
struct Coeffs {
    a: &'static [f64],
    g: &'static [f64],
}

fn coeffs(rate: SdmRate, order: SdmOrder) -> Coeffs {
    match (rate, order) {
        (SdmRate::Dsd64, SdmOrder::Order5) => Coeffs {
            a: &[
                8.09008307688490e-01,
                3.14888846641351e-01,
                7.21231740641844e-02,
                9.61758957091257e-03,
                5.16717217197248e-04,
            ],
            g: &[0.0, 6.98490595355071e-04, 0.0, 1.97734345702526e-03],
        },
        (SdmRate::Dsd256, SdmOrder::Order5) => Coeffs {
            a: &[
                8.07768375734983e-01,
                3.16440095967511e-01,
                7.38231738259889e-02,
                1.01432044963374e-02,
                6.46658652275506e-04,
            ],
            g: &[0.0, 4.36651951230006e-05, 0.0, 1.23660417994961e-04],
        },
        (SdmRate::Dsd128, SdmOrder::Order5) => Coeffs {
            a: &[
                8.08016362125685e-01,
                3.16129639744972e-01,
                7.34835047943110e-02,
                1.00377576971692e-02,
                6.20309683440734e-04,
            ],
            g: &[0.0, 1.74653153894942e-04, 0.0, 4.94580504383930e-04],
        },
        (SdmRate::Dsd64, SdmOrder::Order7) => Coeffs {
            a: &[
                7.84599817960974e-01,
                2.99634346028983e-01,
                7.13049276218066e-02,
                1.13334107086916e-02,
                1.14497642818158e-03,
                7.33018502803093e-05,
                6.80633400002018e-07,
            ],
            g: &[
                0.0,
                3.96825873999969e-04,
                0.0,
                1.32436089566069e-03,
                0.0,
                2.16898568341885e-03,
            ],
        },
        (SdmRate::Dsd128, SdmOrder::Order7) => Coeffs {
            a: &[
                7.83148010097334e-01,
                3.01437231238902e-01,
                7.31891646224574e-02,
                1.20314098366875e-02,
                1.32077937193861e-03,
                9.11181979169687e-05,
                2.59895240306562e-06,
            ],
            g: &[
                0.0,
                9.92163123766340e-05,
                0.0,
                3.31199917300393e-04,
                0.0,
                5.42540771343282e-04,
            ],
        },
        (SdmRate::Dsd256, SdmOrder::Order7) => Coeffs {
            a: &[
                7.82785077952658e-01,
                3.01888671316811e-01,
                7.36594376027782e-02,
                1.22068270909817e-02,
                1.36572694403914e-03,
                9.57806082134936e-05,
                3.13239368043838e-06,
            ],
            g: &[
                0.0,
                2.48046933669715e-05,
                0.0,
                8.28068362972358e-05,
                0.0,
                1.35653594733585e-04,
            ],
        },
    }
}

/// A Kaiser-windowed sinc interpolation prototype, phase-major and normalised per phase.
///
/// The prototype is designed at the bit rate: cutoff at `1/32` of the output rate (half the
/// input Nyquist, still far above the audio band), 64 taps, β = 10. Each of the eight phases
/// sums to one, so an interpolated DC signal keeps its amplitude.
fn interp_phases() -> [[f64; PHASE_TAPS]; OVERSAMPLE] {
    let mut taps = [0.0; INTERP_TAPS];
    let mid = (INTERP_TAPS - 1) as f64 / 2.0;
    let beta = 10.0;
    let i0 = |x: f64| -> f64 {
        let mut sum = 1.0;
        let mut term = 1.0;
        for k in 1..25 {
            term *= (x / (2.0 * k as f64)).powi(2);
            sum += term;
        }
        sum
    };
    let denom = i0(beta);
    for (n, tap) in taps.iter_mut().enumerate() {
        let m = n as f64 - mid;
        // The centre tap only exists for an odd tap count; it is the limit of the branch below.
        let sinc = if m == 0.0 {
            1.0 / (OVERSAMPLE as f64 * 2.0)
        } else {
            (std::f64::consts::PI * m / (OVERSAMPLE as f64 * 2.0)).sin()
                / (std::f64::consts::PI * m)
        };
        let r = 2.0 * n as f64 / (INTERP_TAPS - 1) as f64 - 1.0;
        *tap = sinc * i0(beta * (1.0 - r * r).max(0.0).sqrt()) / denom;
    }
    let mut phases = [[0.0; PHASE_TAPS]; OVERSAMPLE];
    for (phase, branch) in phases.iter_mut().enumerate() {
        let mut sum = 0.0;
        for (k, tap) in branch.iter_mut().enumerate() {
            *tap = taps[phase + OVERSAMPLE * k];
            sum += *tap;
        }
        if sum != 0.0 {
            for tap in branch.iter_mut() {
                *tap /= sum;
            }
        }
    }
    phases
}

/// Highest modulator order; state and coefficient arrays are this long.
const MAX_ORDER: usize = 7;

/// The 8× interpolator and one-bit modulator state of one channel.
#[derive(Clone, Copy)]
struct Channel {
    /// Interpolation history stored twice (`history[i] == history[i + PHASE_TAPS]`), so the
    /// newest-first window starting at `at` is contiguous and needs no wrap-around.
    history: [f64; 2 * PHASE_TAPS],
    /// Slot the next sample goes to.
    at: usize,
    /// Integrator states.
    state: [f64; MAX_ORDER],
    /// Quantiser output of the previous bit (delayed feedback).
    prev_y: f64,
}

impl Channel {
    fn new() -> Self {
        Self {
            history: [0.0; 2 * PHASE_TAPS],
            at: 0,
            state: [0.0; MAX_ORDER],
            prev_y: 1.0,
        }
    }

    fn reset(&mut self) {
        *self = Self::new();
    }

    /// Adds one input sample and returns the eight interpolated, limited bit-rate samples it
    /// produces.
    #[inline(always)]
    fn interpolate(
        &mut self,
        x: f64,
        phases: &[[f64; PHASE_TAPS]; OVERSAMPLE],
    ) -> [f64; OVERSAMPLE] {
        self.history[self.at] = x;
        self.history[self.at + PHASE_TAPS] = x;
        self.at = (self.at + 1) % PHASE_TAPS;
        // Newest first: window[k] is the sample k steps back.
        let mut window = [0.0; PHASE_TAPS];
        for (k, value) in window.iter_mut().enumerate() {
            *value = self.history[self.at + PHASE_TAPS - 1 - k];
        }
        let mut out = [0.0; OVERSAMPLE];
        for (value, branch) in out.iter_mut().zip(phases) {
            let mut sum = 0.0;
            for (&tap, &sample) in branch.iter().zip(&window) {
                sum += tap * sample;
            }
            // Limit after interpolation, so inter-sample overshoot never reaches the loop.
            *value = sum.clamp(-INPUT_SCALE, INPUT_SCALE);
        }
        out
    }
}

/// PCM→DSD for one or more channels.
pub struct Sdm {
    rate: SdmRate,
    order: SdmOrder,
    /// Feedforward and resonator coefficients, zero-padded to [`MAX_ORDER`].
    a: [f64; MAX_ORDER],
    g: [f64; MAX_ORDER],
    phases: [[f64; PHASE_TAPS]; OVERSAMPLE],
    channels: Vec<Channel>,
    /// Output scratch: one byte per eight bits per channel.
    scratch: Vec<u8>,
    /// Zero block used by [`Sdm::flush`].
    silence: Vec<f64>,
    /// Set when the last block put a channel at its state limit.
    limited: bool,
}

impl Sdm {
    /// Largest input block [`Sdm::process`] handles without allocating.
    pub const MAX_FRAMES: usize = 4096;

    pub fn new(rate: SdmRate, order: SdmOrder, channels: u16) -> Self {
        let coeffs = coeffs(rate, order);
        // Padding below would turn a short table into silent zeros instead of a failure.
        let order_len = usize::from(order.as_u8());
        debug_assert_eq!(
            coeffs.a.len(),
            order_len,
            "{rate:?} {order:?} feedforward table"
        );
        debug_assert_eq!(
            coeffs.g.len(),
            order_len - 1,
            "{rate:?} {order:?} resonator table"
        );
        let channels = usize::from(channels.max(1));
        let pad = |values: &[f64]| {
            let mut padded = [0.0; MAX_ORDER];
            padded[..values.len()].copy_from_slice(values);
            padded
        };
        Self {
            rate,
            order,
            a: pad(coeffs.a),
            g: pad(coeffs.g),
            phases: interp_phases(),
            channels: (0..channels).map(|_| Channel::new()).collect(),
            scratch: vec![0; Self::MAX_FRAMES * OVERSAMPLE / 8 * channels],
            silence: vec![0.0; Self::MAX_FRAMES * channels],
            limited: false,
        }
    }

    pub fn rate(&self) -> SdmRate {
        self.rate
    }

    pub fn order(&self) -> SdmOrder {
        self.order
    }

    /// Whether a channel hit its state limit since the last call.
    pub fn take_limited(&mut self) -> bool {
        std::mem::take(&mut self.limited)
    }

    /// Forgets interpolation and integrator history (seek, stop).
    pub fn reset(&mut self) {
        for channel in &mut self.channels {
            channel.reset();
        }
        self.limited = false;
    }

    /// Converts interleaved `f64` PCM at [`SdmRate::input_rate`] into interleaved DSD bytes.
    ///
    /// One frame becomes one byte per channel (eight bit samples). `out` is appended to, so
    /// the caller can reuse one buffer across blocks.
    pub fn process(&mut self, input: &[f64], out: &mut Vec<u8>) {
        let channel_count = self.channels.len();
        let frames = input.len() / channel_count;
        if frames == 0 {
            return;
        }
        let bytes = frames * OVERSAMPLE / 8 * channel_count;
        if out.capacity() < out.len() + bytes || self.scratch.len() < bytes {
            // Larger than the documented block: grow once, then stay allocation-free.
            self.scratch.resize(bytes, 0);
            out.reserve(bytes);
        }
        let input = &input[..frames * channel_count];
        let limited = match self.order {
            SdmOrder::Order5 => self.run::<5>(input, bytes),
            SdmOrder::Order7 => self.run::<7>(input, bytes),
        };
        self.limited |= limited;
        out.extend_from_slice(&self.scratch[..bytes]);
    }

    /// Runs the channels in pairs: two independent loops interleaved bit by bit keep the CPU
    /// busy while each one waits on its own previous bit. Returns whether any channel hit its
    /// state limit.
    fn run<const ORDER: usize>(&mut self, input: &[f64], bytes: usize) -> bool {
        let stride = self.channels.len();
        let out = &mut self.scratch[..bytes];
        let mut limited = false;
        let mut first = 0;
        while first < stride {
            let lanes = (stride - first).min(2);
            limited |= if lanes == 2 {
                modulate::<ORDER, 2>(
                    &mut self.channels,
                    first,
                    input,
                    out,
                    &self.phases,
                    &self.a,
                    &self.g,
                )
            } else {
                modulate::<ORDER, 1>(
                    &mut self.channels,
                    first,
                    input,
                    out,
                    &self.phases,
                    &self.a,
                    &self.g,
                )
            };
            first += lanes;
        }
        limited
    }

    /// Emits the tail still held by the interpolator, feeding it zeros.
    pub fn flush(&mut self, out: &mut Vec<u8>) {
        let channels = self.channels.len();
        let frames = PHASE_TAPS;
        let silence = std::mem::take(&mut self.silence);
        self.process(&silence[..frames * channels], out);
        self.silence = silence;
    }
}

/// Modulates `LANES` adjacent channels starting at `first`, reading and writing interleaved
/// data with the full channel stride. The channel state lives in locals for the whole block.
#[inline(always)]
fn modulate<const ORDER: usize, const LANES: usize>(
    channels: &mut [Channel],
    first: usize,
    input: &[f64],
    out: &mut [u8],
    phases: &[[f64; PHASE_TAPS]; OVERSAMPLE],
    a: &[f64; MAX_ORDER],
    g: &[f64; MAX_ORDER],
) -> bool {
    let stride = channels.len();
    let mut lanes: [Channel; LANES] = std::array::from_fn(|lane| channels[first + lane]);
    let mut limited = false;
    for (frame, bytes) in input.chunks_exact(stride).zip(out.chunks_exact_mut(stride)) {
        let mut x: [[f64; OVERSAMPLE]; LANES] = std::array::from_fn(|lane| {
            lanes[lane].interpolate(frame[first + lane] * INPUT_SCALE, phases)
        });
        let mut byte = [0u8; LANES];
        for phase in 0..OVERSAMPLE {
            for lane in 0..LANES {
                let (bit, unstable) = step::<ORDER>(&mut lanes[lane], x[lane][phase], a, g);
                byte[lane] = (byte[lane] << 1) | bit;
                if unstable {
                    limited = true;
                    lanes[lane].reset();
                    // The rest of this frame interpolates from the cleared history.
                    x[lane] = [0.0; OVERSAMPLE];
                }
            }
        }
        bytes[first..first + LANES].copy_from_slice(&byte);
    }
    channels[first..first + LANES].copy_from_slice(&lanes);
    limited
}

/// Runs one bit of the modulator; also reports whether the loop left its stable region.
#[inline(always)]
fn step<const ORDER: usize>(
    channel: &mut Channel,
    x: f64,
    a: &[f64; MAX_ORDER],
    g: &[f64; MAX_ORDER],
) -> (u8, bool) {
    let old = channel.state;
    let s = &mut channel.state;
    // sox's CIFF loop: the integrators see the previous quantiser output. The last
    // integrator has no resonator, so the tables carry only `ORDER - 1` g values.
    s[0] = old[0] - g[0] * old[1] + x - channel.prev_y;
    for i in 1..ORDER - 1 {
        s[i] = old[i] + old[i - 1] - g[i] * old[i + 1];
    }
    s[ORDER - 1] = old[ORDER - 1] + old[ORDER - 2];
    let mut v = x + a[0] * s[0];
    for i in 1..ORDER {
        v += a[i] * s[i];
    }
    let bit = v >= 0.0;
    channel.prev_y = if bit { 1.0 } else { -1.0 };
    // One comparison per state catches both overflow and NaN (which compares false); the
    // negation is the point, so `partial_cmp` would only hide it.
    let mut unstable = false;
    for value in &s[..ORDER] {
        #[allow(clippy::neg_cmp_op_on_partial_ord)]
        let out_of_range = !(value.abs() <= STATE_LIMIT);
        unstable |= out_of_range;
    }
    (u8::from(bit), unstable)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsd2pcm::DsdToPcm;

    /// Interleaved DSD bytes → `f64` PCM through the production decimator.
    fn decode(bytes: &[u8], rate: SdmRate, channels: u16) -> Vec<f64> {
        let mut converter =
            DsdToPcm::new(rate.bit_rate(), 176_400, channels).expect("supported DSD pair");
        let mut out = Vec::new();
        converter.process(bytes, &mut out);
        out
    }

    /// Amplitude of `freq` in channel 0 of an interleaved signal (Goertzel).
    fn tone_amplitude(signal: &[f64], channels: usize, rate: u32, freq: f64) -> f64 {
        let frames = signal.len() / channels;
        let mut re = 0.0;
        let mut im = 0.0;
        for frame in 0..frames {
            let sample = signal[frame * channels];
            let angle = 2.0 * std::f64::consts::PI * freq * frame as f64 / f64::from(rate);
            re += sample * angle.cos();
            im -= sample * angle.sin();
        }
        2.0 * (re * re + im * im).sqrt() / frames as f64
    }

    /// Unpacks MSB-first DSD bytes into ±1 symbols.
    fn symbols(bytes: &[u8]) -> Vec<f64> {
        let mut out = Vec::with_capacity(bytes.len() * 8);
        for byte in bytes {
            for shift in (0..8).rev() {
                out.push(if (byte >> shift) & 1 == 1 { 1.0 } else { -1.0 });
            }
        }
        out
    }

    /// In-place radix-2 FFT.
    fn fft(re: &mut [f64], im: &mut [f64]) {
        let n = re.len();
        let mut j = 0;
        for i in 1..n {
            let mut bit = n >> 1;
            while j & bit != 0 {
                j ^= bit;
                bit >>= 1;
            }
            j ^= bit;
            if i < j {
                re.swap(i, j);
                im.swap(i, j);
            }
        }
        let mut len = 2;
        while len <= n {
            let angle = -2.0 * std::f64::consts::PI / len as f64;
            let (wr, wi) = (angle.cos(), angle.sin());
            for start in (0..n).step_by(len) {
                let (mut cr, mut ci) = (1.0, 0.0);
                for k in 0..len / 2 {
                    let (ar, ai) = (re[start + k], im[start + k]);
                    let (br, bi) = (
                        re[start + k + len / 2] * cr - im[start + k + len / 2] * ci,
                        re[start + k + len / 2] * ci + im[start + k + len / 2] * cr,
                    );
                    re[start + k] = ar + br;
                    im[start + k] = ai + bi;
                    re[start + k + len / 2] = ar - br;
                    im[start + k + len / 2] = ai - bi;
                    let nc = cr * wr - ci * wi;
                    ci = cr * wi + ci * wr;
                    cr = nc;
                }
            }
            len <<= 1;
        }
    }

    /// In-band SNR of a one-channel bitstream against a `freq` tone: a four-term
    /// Blackman-Harris window keeps the tone's leakage out of the audio band, and the bins
    /// around each harmonic of the tone are excluded so the number reports noise, not THD.
    ///
    /// This works on the bitstream, not on decoded PCM: a 120 dB decimation filter still
    /// leaks a little of the (very loud) out-of-band DSD noise, which would hide a noise
    /// floor better than about 50 dB.
    fn snr_db(bytes: &[u8], rate: SdmRate, freq: f64) -> f64 {
        band_snr_db(bytes, rate, freq, 0.0, 20_000.0)
    }

    /// [`snr_db`] with the noise counted only between `low` and `high` Hz.
    fn band_snr_db(bytes: &[u8], rate: SdmRate, freq: f64, low: f64, high: f64) -> f64 {
        const N: usize = 262_144;
        let symbols = symbols(&bytes[..N / 8]);
        let mut re = vec![0.0; N];
        let mut im = vec![0.0; N];
        for (i, sample) in symbols.iter().enumerate() {
            let t = 2.0 * std::f64::consts::PI * i as f64 / (N - 1) as f64;
            let window =
                0.35875 - 0.48829 * t.cos() + 0.14128 * (2.0 * t).cos() - 0.01168 * (3.0 * t).cos();
            re[i] = sample * window;
        }
        fft(&mut re, &mut im);
        let power = |bin: usize| re[bin] * re[bin] + im[bin] * im[bin];
        let signal_bin = (freq * N as f64 / f64::from(rate.bit_rate())).round() as usize;
        let band_start = ((low * N as f64 / f64::from(rate.bit_rate())) as usize).max(2);
        let band_end = (high * N as f64 / f64::from(rate.bit_rate())) as usize;
        let mut signal = 0.0;
        for bin in signal_bin.saturating_sub(2)..=signal_bin + 2 {
            signal += power(bin);
        }
        let mut noise = 0.0;
        for bin in band_start..band_end {
            let nearest_harmonic =
                ((bin as f64 / signal_bin as f64).round() as usize).max(1) * signal_bin;
            if bin.abs_diff(nearest_harmonic) <= 6 {
                continue;
            }
            noise += power(bin);
        }
        10.0 * (signal / noise.max(f64::MIN_POSITIVE)).log10()
    }

    fn sine(rate: SdmRate, freq: f64, amplitude: f64, seconds: f64) -> Vec<f64> {
        let frames = (f64::from(rate.input_rate()) * seconds) as usize;
        (0..frames)
            .flat_map(|frame| {
                let sample = amplitude
                    * (2.0 * std::f64::consts::PI * freq * frame as f64
                        / f64::from(rate.input_rate()))
                    .sin();
                [sample; 2]
            })
            .collect()
    }

    #[test]
    fn a_tone_survives_the_round_trip() {
        let rate = SdmRate::Dsd64;
        let mut sdm = Sdm::new(rate, SdmOrder::Order5, 2);
        let input = sine(rate, 1000.0, 0.8, 0.1);
        let mut bytes = Vec::new();
        sdm.process(&input, &mut bytes);
        assert_eq!(
            bytes.len(),
            input.len() / 2 * OVERSAMPLE / 8 * 2,
            "one byte per eight bits per channel"
        );
        let pcm = decode(&bytes, rate, 2);
        let amplitude = tone_amplitude(&pcm, 2, 176_400, 1000.0);
        assert!(
            (amplitude - 0.4).abs() < 0.05,
            "tone amplitude {amplitude} should be about 0.4 (0 dBFS = 50 % modulation)"
        );
    }

    #[test]
    fn noise_shaping_meets_the_target() {
        let rate = SdmRate::Dsd64;
        let mut sdm = Sdm::new(rate, SdmOrder::Order5, 1);
        let frames = 262_144 * 8 / OVERSAMPLE;
        let mut input = Vec::with_capacity(frames);
        for frame in 0..frames {
            input.push(
                0.7 * (2.0 * std::f64::consts::PI * 1000.0 * frame as f64
                    / f64::from(rate.input_rate()))
                .sin(),
            );
        }
        let mut bytes = Vec::new();
        sdm.process(&input, &mut bytes);
        let snr = snr_db(&bytes, rate, 1000.0);
        assert!(
            snr >= 90.0,
            "in-band SNR {snr:.1} dB is below the 90 dB target"
        );
    }

    #[test]
    fn the_seventh_order_shapes_harder() {
        let rate = SdmRate::Dsd64;
        let frames = 262_144 * 8 / OVERSAMPLE;
        let input: Vec<f64> = (0..frames)
            .map(|frame| {
                0.7 * (2.0 * std::f64::consts::PI * 1000.0 * frame as f64
                    / f64::from(rate.input_rate()))
                .sin()
            })
            .collect();
        let snr = |order| {
            let mut sdm = Sdm::new(rate, order, 1);
            let mut bytes = Vec::new();
            sdm.process(&input, &mut bytes);
            snr_db(&bytes, rate, 1000.0)
        };
        let (fifth, seventh) = (snr(SdmOrder::Order5), snr(SdmOrder::Order7));
        assert!(
            seventh >= fifth,
            "order 7 SNR {seventh:.1} dB is below order 5 {fifth:.1} dB"
        );
    }

    #[test]
    fn a_full_scale_signal_is_limited_not_wrapped() {
        let rate = SdmRate::Dsd64;
        let mut sdm = Sdm::new(rate, SdmOrder::Order7, 1);
        // A ±1 square wave at 1 kHz: loud inter-sample peaks are the risk.
        let frames = rate.input_rate() as usize / 10;
        let input: Vec<f64> = (0..frames)
            .map(|frame| {
                if (frame * 1000 / rate.input_rate() as usize).is_multiple_of(2) {
                    1.0
                } else {
                    -1.0
                }
            })
            .collect();
        let mut bytes = Vec::new();
        sdm.process(&input, &mut bytes);
        let ones: u32 = bytes.iter().map(|b| b.count_ones()).sum();
        let bits = bytes.len() as u32 * 8;
        assert!(
            ones > bits / 4 && ones < 3 * bits / 4,
            "{ones} of {bits} bits set: the loop latched"
        );

        // A normal tone afterwards still modulates cleanly.
        let mut sdm = Sdm::new(rate, SdmOrder::Order5, 1);
        let input: Vec<f64> = (0..frames)
            .map(|frame| {
                0.25 * (2.0 * std::f64::consts::PI * 1000.0 * frame as f64
                    / f64::from(rate.input_rate()))
                .sin()
            })
            .collect();
        let mut bytes = Vec::new();
        sdm.process(&input, &mut bytes);
        assert!(!sdm.take_limited());
    }

    /// Frequencies (Hz) of a table's NTF zeros: each resonator `g` puts a pair at
    /// `sqrt(g) · bit rate / 2π`.
    fn zero_frequencies(rate: SdmRate, order: SdmOrder) -> Vec<f64> {
        coeffs(rate, order)
            .g
            .iter()
            .filter(|&&g| g > 0.0)
            .map(|g| g.sqrt() * f64::from(rate.bit_rate()) / (2.0 * std::f64::consts::PI))
            .collect()
    }

    #[test]
    fn every_rate_puts_its_noise_zeros_at_the_same_audio_frequencies() {
        for order in [SdmOrder::Order5, SdmOrder::Order7] {
            let reference = zero_frequencies(SdmRate::Dsd128, order);
            for rate in [SdmRate::Dsd64, SdmRate::Dsd256] {
                for (zero, expected) in zero_frequencies(rate, order).iter().zip(&reference) {
                    assert!(
                        (zero / expected - 1.0).abs() < 0.01,
                        "{} order {}: zero at {zero:.0} Hz, DSD128 has it at {expected:.0} Hz",
                        rate.as_str(),
                        order.as_u8()
                    );
                }
            }
        }
    }

    #[test]
    fn the_fifth_order_keeps_high_audio_frequencies_quiet_at_dsd64() {
        // A tone near the top of the band: noise between the NTF zeros and 20 kHz dominates.
        let rate = SdmRate::Dsd64;
        let mut sdm = Sdm::new(rate, SdmOrder::Order5, 1);
        let frames = 262_144 * 8 / OVERSAMPLE;
        let input: Vec<f64> = (0..frames)
            .map(|frame| {
                0.7 * (2.0 * std::f64::consts::PI * 1000.0 * frame as f64
                    / f64::from(rate.input_rate()))
                .sin()
            })
            .collect();
        let mut bytes = Vec::new();
        sdm.process(&input, &mut bytes);
        assert!(!sdm.take_limited());
        let snr = band_snr_db(&bytes, rate, 1000.0, 10_000.0, 20_000.0);
        assert!(snr >= 110.0, "10-20 kHz SNR {snr:.1} dB is below 110 dB");
    }

    #[test]
    fn every_rate_and_order_reports_its_carrier_formats() {
        assert_eq!(SdmRate::Dsd64.input_rate(), 352_800);
        assert_eq!(SdmRate::Dsd64.dop_rate(), 176_400);
        assert_eq!(SdmRate::Dsd64.native_rate(), 88_200);
        assert_eq!(SdmRate::Dsd256.input_rate(), 1_411_200);
        assert_eq!(SdmRate::Dsd256.dop_rate(), 705_600);
        assert_eq!(SdmRate::Dsd256.native_rate(), 352_800);
        for rate in [SdmRate::Dsd64, SdmRate::Dsd128, SdmRate::Dsd256] {
            for order in [SdmOrder::Order5, SdmOrder::Order7] {
                let sdm = Sdm::new(rate, order, 2);
                assert_eq!(sdm.rate(), rate);
                assert_eq!(sdm.order(), order);
            }
        }
    }

    #[test]
    fn flush_emits_a_whole_byte_per_channel() {
        let mut sdm = Sdm::new(SdmRate::Dsd64, SdmOrder::Order5, 2);
        let mut out = Vec::new();
        sdm.flush(&mut out);
        assert_eq!(out.len(), PHASE_TAPS * 2);
    }

    /// The straightforward form of the modulator: one channel after another, the order read
    /// at run time and the stability check on every bit. The fast path must match it byte for
    /// byte.
    struct Reference {
        a: &'static [f64],
        g: &'static [f64],
        order: usize,
        phases: [[f64; PHASE_TAPS]; OVERSAMPLE],
        history: Vec<[f64; PHASE_TAPS]>,
        at: Vec<usize>,
        state: Vec<[f64; 7]>,
        prev_y: Vec<f64>,
        limited: bool,
    }

    impl Reference {
        fn new(rate: SdmRate, order: SdmOrder, channels: usize) -> Self {
            let coeffs = coeffs(rate, order);
            Self {
                a: coeffs.a,
                g: coeffs.g,
                order: usize::from(order.as_u8()),
                phases: interp_phases(),
                history: vec![[0.0; PHASE_TAPS]; channels],
                at: vec![0; channels],
                state: vec![[0.0; 7]; channels],
                prev_y: vec![1.0; channels],
                limited: false,
            }
        }

        // Indexes mirror the loop equations, which is what makes this the reference.
        #[allow(clippy::needless_range_loop)]
        fn process(&mut self, input: &[f64]) -> Vec<u8> {
            let channels = self.state.len();
            let frames = input.len() / channels;
            let mut out = vec![0u8; frames * channels];
            let order = self.order;
            for frame in 0..frames {
                for ch in 0..channels {
                    let x = input[frame * channels + ch] * INPUT_SCALE;
                    self.history[ch][self.at[ch]] = x;
                    self.at[ch] = (self.at[ch] + 1) % PHASE_TAPS;
                    for phase in 0..OVERSAMPLE {
                        let mut v = 0.0;
                        for (k, &tap) in self.phases[phase].iter().enumerate() {
                            v += tap
                                * self.history[ch][(self.at[ch] + PHASE_TAPS - 1 - k) % PHASE_TAPS];
                        }
                        let x = v.clamp(-INPUT_SCALE, INPUT_SCALE);
                        let old = self.state[ch];
                        let s = &mut self.state[ch];
                        s[0] = old[0] - self.g[0] * old[1] + x - self.prev_y[ch];
                        for i in 1..order - 1 {
                            s[i] = old[i] + old[i - 1] - self.g[i] * old[i + 1];
                        }
                        s[order - 1] = old[order - 1] + old[order - 2];
                        let mut v = x + self.a[0] * s[0];
                        for i in 1..order {
                            v += self.a[i] * s[i];
                        }
                        let y = if v >= 0.0 { 1.0 } else { -1.0 };
                        self.prev_y[ch] = y;
                        let byte = &mut out[frame * channels + ch];
                        *byte = (*byte << 1) | u8::from(y > 0.0);
                        if s[..order]
                            .iter()
                            .any(|s| !s.is_finite() || s.abs() > STATE_LIMIT)
                        {
                            self.limited = true;
                            self.history[ch] = [0.0; PHASE_TAPS];
                            self.at[ch] = 0;
                            self.state[ch] = [0.0; 7];
                            self.prev_y[ch] = 1.0;
                        }
                    }
                }
            }
            out
        }
    }

    /// A deterministic mix of music-like content, silence and an overload that trips the
    /// stability reset.
    fn torture(channels: usize, frames: usize) -> Vec<f64> {
        let mut seed = 0x9E37_79B9_7F4A_7C15_u64;
        let mut noise = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0
        };
        (0..frames)
            .flat_map(|frame| {
                let section = frame * 4 / frames;
                (0..channels)
                    .map(|ch| match section {
                        0 => 0.7 * (frame as f64 * 0.013 * (ch + 1) as f64).sin() + 0.2 * noise(),
                        1 => 0.0,
                        // Full-scale square wave: drives the loop out of its stable region.
                        2 => {
                            if (frame / 40) % 2 == 0 {
                                1.0
                            } else {
                                -1.0
                            }
                        }
                        _ => noise(),
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    #[test]
    fn the_fast_path_matches_the_reference_modulator_byte_for_byte() {
        let mut resets = 0;
        for rate in [SdmRate::Dsd64, SdmRate::Dsd128, SdmRate::Dsd256] {
            for order in [SdmOrder::Order5, SdmOrder::Order7] {
                for channels in [1usize, 2, 3] {
                    let input = torture(channels, 20_000);
                    let mut reference = Reference::new(rate, order, channels);
                    let mut sdm = Sdm::new(rate, order, channels as u16);
                    let mut out = Vec::new();
                    // Odd block sizes cross every block boundary case.
                    let blocks: Vec<&[f64]> = input.chunks(777 * channels).collect();
                    let (head, tail) = blocks.split_at(blocks.len() / 2);
                    for block in head {
                        sdm.process(block, &mut out);
                    }
                    // Push the loop out of its stable region the same way in both: a NaN
                    // resets on the next bit, a state just under the limit a few bits later,
                    // in the middle of a frame.
                    let split = head.iter().map(|b| b.len()).sum::<usize>();
                    let mut expected = reference.process(&input[..split]);
                    let last = channels - 1;
                    sdm.channels[0].state[2] = f64::NAN;
                    reference.state[0][2] = f64::NAN;
                    sdm.channels[last].state[1] = 0.999 * STATE_LIMIT;
                    reference.state[last][1] = 0.999 * STATE_LIMIT;
                    for block in tail {
                        sdm.process(block, &mut out);
                    }
                    expected.extend(reference.process(&input[split..]));
                    let first_difference = out.iter().zip(&expected).position(|(a, b)| a != b);
                    assert_eq!(
                        first_difference, None,
                        "{rate:?} {order:?} {channels} ch differs from the reference"
                    );
                    assert_eq!(out.len(), expected.len());
                    assert_eq!(sdm.take_limited(), reference.limited, "{rate:?} {order:?}");
                    resets += usize::from(reference.limited);
                }
            }
        }
        // Every case must have gone through the reset path, or it compared nothing there.
        assert_eq!(resets, 3 * 2 * 3);
    }
}
