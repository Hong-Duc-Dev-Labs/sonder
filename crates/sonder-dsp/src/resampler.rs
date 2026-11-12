//! One rational rate-conversion stage: `L/M` through a polyphase FIR.

use std::sync::Arc;

use crate::filter_source::{FilterSourceKind, FilterTable};
use crate::fir::FirCore;
use crate::stage::{Stage, StreamFormat};

/// A reduced rational ratio `num/den`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Ratio {
    pub num: u32,
    pub den: u32,
}

impl Ratio {
    pub fn new(num: u32, den: u32) -> Self {
        let g = crate::filter_source::gcd(num, den);
        Self {
            num: num / g,
            den: den / g,
        }
    }

    pub fn is_identity(self) -> bool {
        self.num == self.den
    }
}

/// A polyphase resampler stage backed by one [`FilterTable`].
pub struct Resampler {
    core: FirCore,
    source: FilterSourceKind,
}

impl Resampler {
    pub fn new(table: Arc<FilterTable>) -> Self {
        Self {
            source: table.source(),
            core: FirCore::new(table),
        }
    }

    pub fn ratio(&self) -> Ratio {
        let spec = self.core.spec();
        Ratio {
            num: spec.num,
            den: spec.den,
        }
    }

    pub fn filter_source(&self) -> FilterSourceKind {
        self.source
    }

    pub fn table(&self) -> &Arc<FilterTable> {
        self.core.table()
    }
}

impl Stage for Resampler {
    fn configure(&mut self, input: StreamFormat) -> StreamFormat {
        let ratio = self.ratio();
        let scaled = u64::from(input.rate) * u64::from(ratio.num);
        debug_assert_eq!(
            scaled % u64::from(ratio.den),
            0,
            "{} Hz is not a whole number after {}/{}",
            input.rate,
            ratio.num,
            ratio.den
        );
        let rate = u32::try_from(scaled / u64::from(ratio.den)).unwrap_or(u32::MAX);
        self.core.configure(usize::from(input.channels.max(1)));
        StreamFormat {
            rate,
            channels: input.channels,
        }
    }

    fn max_out(&self, in_frames: usize) -> usize {
        self.core.max_out(in_frames)
    }

    fn process(&mut self, input: &[f64], out: &mut [f64]) -> usize {
        self.core.process(input, out)
    }

    fn flush(&mut self, out: &mut [f64]) -> usize {
        self.core.flush(out)
    }

    fn reset(&mut self) {
        self.core.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter_source::FilterPhase;
    use crate::sonder_design::design_coefficients;

    fn table(num: u32, den: u32) -> Arc<FilterTable> {
        let spec = crate::filter_source::FilterSpec::new(
            num,
            den,
            FilterPhase::Linear,
            crate::filter_source::FilterVariant::Precise,
        );
        let coeffs = design_coefficients(spec);
        Arc::new(FilterTable::from_prototype(spec, FilterSourceKind::Sonder, coeffs).unwrap())
    }

    fn run(
        stage: &mut Resampler,
        source_rate: u32,
        source_frames: usize,
    ) -> (Vec<f64>, StreamFormat) {
        let format = StreamFormat {
            rate: source_rate,
            channels: 1,
        };
        let out_format = stage.configure(format);
        let input: Vec<f64> = (0..source_frames)
            .map(|n| (2.0 * std::f64::consts::PI * 1_000.0 * n as f64 / source_rate as f64).sin())
            .collect();
        let mut out = vec![0.0; stage.max_out(source_frames) + stage.max_out(0)];
        let n = stage.process(&input, &mut out);
        stage.flush(&mut out[n..]);
        (out, out_format)
    }

    /// Fits a 1 kHz sine to `out[skip..end]` and returns amplitude, THD+N and sample count.
    fn fit(out: &[f64], out_format: StreamFormat, skip: usize, end: usize) -> (f64, f64, f64) {
        let (mut ss, mut cc, mut sc) = (0.0, 0.0, 0.0);
        let (mut ys, mut yc, mut yy) = (0.0, 0.0, 0.0);
        let mut count = 0.0;
        for (i, y) in out[skip..end]
            .iter()
            .enumerate()
            .map(|(k, &y)| (k + skip, y))
        {
            let w = 2.0 * std::f64::consts::PI * 1_000.0 * i as f64 / out_format.rate as f64;
            let (s, c) = (w.sin(), w.cos());
            ss += s * s;
            cc += c * c;
            sc += s * c;
            ys += y * s;
            yc += y * c;
            yy += y * y;
            count += 1.0;
        }
        let det = ss * cc - sc * sc;
        let a = (ys * cc - yc * sc) / det;
        let b = (yc * ss - ys * sc) / det;
        let fitted_energy = a * a * ss + 2.0 * a * b * sc + b * b * cc;
        let thd_n = ((yy - fitted_energy) / yy).max(0.0).sqrt();
        ((a * a + b * b).sqrt(), thd_n, count)
    }

    /// Magnitude of `out[skip..end]` at `hz`, with a Hann window to suppress leakage.
    fn tone_db(out: &[f64], out_format: StreamFormat, skip: usize, end: usize, hz: f64) -> f64 {
        let span = (end - skip) as f64;
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for (k, y) in out[skip..end].iter().enumerate() {
            let hann = 0.5 - 0.5 * (2.0 * std::f64::consts::PI * k as f64 / (span - 1.0)).cos();
            let w = 2.0 * std::f64::consts::PI * hz * (k + skip) as f64 / out_format.rate as f64;
            re += y * hann * w.cos();
            im += y * hann * w.sin();
        }
        20.0 * (re.hypot(im) / (span / 4.0)).log10()
    }

    #[test]
    fn output_rate_is_exact_for_common_ratios() {
        for (src, num, den, want) in [
            (44_100, 160, 147, 48_000),
            (48_000, 147, 160, 44_100),
            (44_100, 4, 1, 176_400),
            (44_100, 256, 1, 11_289_600),
        ] {
            let mut stage = Resampler::new(table(num, den));
            let out = stage.configure(StreamFormat {
                rate: src,
                channels: 2,
            });
            assert_eq!(out.rate, want, "{src} {num}/{den}");
        }
    }

    #[test]
    fn integer_upsampling_passes_unity_and_rejects_images() {
        for num in [2u32, 4, 8] {
            let mut stage = Resampler::new(table(num, 1));
            let source_frames = 4_410; // 0.1 s
            let (out, out_format) = run(&mut stage, 44_100, source_frames);
            let end = source_frames * num as usize;
            // Upper bound on the filter's group delay in output samples.
            let skip = stage.table().coefficients().len() * num as usize;
            assert!(end > skip, "{num}x: {end} <= {skip}");
            let (amplitude, thd_n, count) = fit(&out, out_format, skip, end);
            assert!(
                (amplitude - 1.0).abs() < 1e-3,
                "{num}x amplitude {amplitude}"
            );
            assert!(thd_n < 1e-4, "{num}x THD+N {thd_n}");
            assert!(count > 1_000.0);

            // The first image sits at the source rate minus the tone.
            let image_db = tone_db(&out, out_format, skip, end, 44_100.0 - 1_000.0);
            assert!(image_db < -100.0, "{num}x image at {image_db} dB");
        }
    }

    #[test]
    fn rate_family_conversions_keep_unity_and_low_thd() {
        for (source, num, den) in [(44_100u32, 160u32, 147u32), (48_000, 147, 160)] {
            let mut stage = Resampler::new(table(num, den));
            let source_frames = source as usize; // 1 s
            let (out, out_format) = run(&mut stage, source, source_frames);
            let end = source_frames * num as usize / den as usize;
            let skip = (stage.table().coefficients().len() * num as usize) / den as usize;
            assert!(end > skip, "{source}: {end} <= {skip}");
            let (amplitude, thd_n, count) = fit(&out, out_format, skip, end);
            assert!(
                (amplitude - 1.0).abs() < 1e-3,
                "{source} {num}/{den} amplitude {amplitude}"
            );
            assert!(thd_n < 1e-4, "{source} {num}/{den} THD+N {thd_n}");
            assert!(count > 10_000.0);
        }
    }
}
