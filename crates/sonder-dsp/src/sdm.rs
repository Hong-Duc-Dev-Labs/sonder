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

/// The 8× interpolator and one-bit modulator state of one channel.
struct Channel {
    /// Interpolation history, newest at `history[(at + len - 1) % len]`.
    history: [f64; PHASE_TAPS],
    at: usize,
    /// Integrator states.
    state: [f64; 7],
    /// Quantiser output of the previous bit (delayed feedback).
    prev_y: f64,
}

impl Channel {
    fn new() -> Self {
        Self {
            history: [0.0; PHASE_TAPS],
            at: 0,
            state: [0.0; 7],
            prev_y: 1.0,
        }
    }

    fn reset(&mut self) {
        self.history = [0.0; PHASE_TAPS];
        self.at = 0;
        self.state = [0.0; 7];
        self.prev_y = 1.0;
    }
}

/// PCM→DSD for one or more channels.
pub struct Sdm {
    rate: SdmRate,
    order: SdmOrder,
    a: &'static [f64],
    g: &'static [f64],
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
        let channels = usize::from(channels.max(1));
        Self {
            rate,
            order,
            a: coeffs.a,
            g: coeffs.g,
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
        self.scratch[..bytes].fill(0);
        let order = usize::from(self.order.as_u8());
        let (a, g) = (self.a, self.g);
        for frame in 0..frames {
            for index in 0..channel_count {
                let channel = &mut self.channels[index];
                let x = input[frame * channel_count + index] * INPUT_SCALE;
                channel.history[channel.at] = x;
                channel.at = (channel.at + 1) % PHASE_TAPS;
                for phase in 0..OVERSAMPLE {
                    let branch = self.phases[phase];
                    let mut interpolated = 0.0;
                    for (k, &tap) in branch.iter().enumerate() {
                        let pos = (channel.at + PHASE_TAPS - 1 - k) % PHASE_TAPS;
                        interpolated += tap * channel.history[pos];
                    }
                    // Limit after interpolation, so inter-sample overshoot never reaches the loop.
                    let interpolated = interpolated.clamp(-INPUT_SCALE, INPUT_SCALE);
                    let (bit, limited) = step(channel, interpolated, a, g, order);
                    self.limited |= limited;
                    let byte = &mut self.scratch
                        [((frame * OVERSAMPLE + phase) / 8) * channel_count + index];
                    *byte = (*byte << 1) | bit;
                }
            }
        }
        out.extend_from_slice(&self.scratch[..bytes]);
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

/// Runs one bit of the modulator; also reports whether the loop left its stable region.
fn step(channel: &mut Channel, x: f64, a: &[f64], g: &[f64], order: usize) -> (u8, bool) {
    let old = channel.state;
    let y_prev = channel.prev_y;
    // sox's CIFF loop: the integrators see the previous quantiser output. The last
    // integrator has no resonator, so the tables carry only `order - 1` g values.
    channel.state[0] = if order > 1 {
        old[0] - g[0] * old[1] + x - y_prev
    } else {
        old[0] + x - y_prev
    };
    for i in 1..order.saturating_sub(1) {
        channel.state[i] = old[i] + old[i - 1] - g[i] * old[i + 1];
    }
    if order > 1 {
        channel.state[order - 1] = old[order - 1] + old[order - 2];
    }
    let mut v = x + a[0] * channel.state[0];
    for (value, &state) in a.iter().zip(&channel.state).skip(1) {
        v += value * state;
    }
    let y = if v >= 0.0 { 1.0 } else { -1.0 };
    channel.prev_y = y;
    let limited = channel.state[..order]
        .iter()
        .any(|s| !s.is_finite() || s.abs() > STATE_LIMIT);
    if limited {
        channel.reset();
    }
    (u8::from(y > 0.0), limited)
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
}
