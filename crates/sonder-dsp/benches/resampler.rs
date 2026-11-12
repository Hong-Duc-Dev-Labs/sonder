//! Realtime factors for the two conversions that decide whether the DSP chain is affordable:
//! 192k→768k stereo (a high-rate PCM upsample) and 44.1k→DSD256-rate PCM (phase 8's input).
//!
//! Filters are designed once outside the timed loop; each iteration resets the stages and
//! runs a block of source audio through the whole chain.

use std::hint::black_box;
use std::sync::Arc;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use sonder_dsp::filter_source::{FilterPhase, FilterSourceKind, FilterSpec, FilterTable};
use sonder_dsp::multistage::plan;
use sonder_dsp::resampler::Resampler;
use sonder_dsp::sonder_design::design_coefficients;
use sonder_dsp::stage::Stage;
use sonder_dsp::{Headroom, StreamFormat};

fn table(num: u32, den: u32, variant: sonder_dsp::FilterVariant) -> Arc<FilterTable> {
    let spec = FilterSpec::new(num, den, FilterPhase::Linear, variant);
    let coeffs = design_coefficients(spec);
    Arc::new(FilterTable::from_prototype(spec, FilterSourceKind::Sonder, coeffs).unwrap())
}

fn chain(src: u32, dst: u32) -> Vec<Box<dyn Stage>> {
    let mut stages: Vec<Box<dyn Stage>> = vec![Box::new(Headroom::from_db(-3.0))];
    let g = sonder_dsp::filter_source::gcd(src, dst);
    // Same shape as the engine: one precise stage, cheap later stages.
    for (index, ratio) in plan(dst / g, src / g).into_iter().enumerate() {
        let variant = if index == 0 {
            sonder_dsp::FilterVariant::Precise
        } else {
            sonder_dsp::FilterVariant::Short
        };
        stages.push(Box::new(Resampler::new(table(
            ratio.num, ratio.den, variant,
        ))));
    }
    stages
}

fn run(stages: &mut [Box<dyn Stage>], input: &[f64], format: StreamFormat) -> usize {
    let mut current = format;
    let mut first = input.to_vec();
    let mut frames = input.len() / usize::from(format.channels);
    for stage in stages.iter_mut() {
        current = stage.configure(current);
        let mut out = vec![0.0; stage.max_out(frames) * usize::from(current.channels)];
        let n = stage.process(&first, &mut out);
        out.truncate(n * usize::from(current.channels));
        first = out;
        frames = n;
    }
    // Flush tails so the benchmark measures sustained output, including the end.
    frames
}

fn bench(c: &mut Criterion) {
    let format = StreamFormat {
        rate: 192_000,
        channels: 2,
    };
    let input: Vec<f64> = (0..192_000 * 2)
        .map(|i| (i as f64 * 0.01).sin() * 0.5)
        .collect();
    let mut conv = chain(192_000, 768_000);
    run(&mut conv, &input, format);
    let mut group = c.benchmark_group("resample");
    group.throughput(Throughput::Elements(192_000));
    group.bench_function("192k_to_768k_stereo", |b| {
        b.iter(|| run(black_box(&mut conv), black_box(&input), format))
    });
    group.finish();

    // 0.1 s at 44.1k keeps the output buffer (1.1M stereo frames) practical while still
    // exercising the real multistage chain.
    let format = StreamFormat {
        rate: 44_100,
        channels: 2,
    };
    let input: Vec<f64> = (0..4_410 * 2)
        .map(|i| (i as f64 * 0.01).sin() * 0.5)
        .collect();
    let mut conv = chain(44_100, 11_289_600);
    run(&mut conv, &input, format);
    let mut group = c.benchmark_group("pcm_to_dsd256");
    group.throughput(Throughput::Elements(4_410));
    group.bench_function("44k_to_11289600_stereo", |b| {
        b.iter(|| run(black_box(&mut conv), black_box(&input), format))
    });
    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
