//! PCM→DSD cost per second of stereo audio: the modulator alone, and the whole chain the
//! engine runs for a 44.1 kHz source (resampling to `DSD rate / 8`, then the modulator).

use std::hint::black_box;
use std::sync::Arc;

use criterion::{Criterion, criterion_group, criterion_main};
use sonder_dsp::filter_source::{FilterPhase, FilterSourceKind, FilterSpec, FilterTable, gcd};
use sonder_dsp::multistage::plan;
use sonder_dsp::resampler::Resampler;
use sonder_dsp::sonder_design::design_coefficients;
use sonder_dsp::stage::Stage;
use sonder_dsp::{FilterVariant, Sdm, SdmOrder, SdmRate, StreamFormat};

fn sine(rate: u32, frames: usize) -> Vec<f64> {
    (0..frames)
        .flat_map(|frame| {
            let sample =
                0.5 * (2.0 * std::f64::consts::PI * 1000.0 * frame as f64 / f64::from(rate)).sin();
            [sample; 2]
        })
        .collect()
}

/// The engine's resampler shape: one precise stage, cheap later stages.
fn resamplers(src: u32, dst: u32) -> Vec<Resampler> {
    let g = gcd(src, dst);
    plan(dst / g, src / g)
        .into_iter()
        .enumerate()
        .map(|(index, ratio)| {
            let variant = if index == 0 {
                FilterVariant::Precise
            } else {
                FilterVariant::Short
            };
            let spec = FilterSpec::new(ratio.num, ratio.den, FilterPhase::Linear, variant);
            let table = FilterTable::from_prototype(
                spec,
                FilterSourceKind::Sonder,
                design_coefficients(spec),
            )
            .unwrap();
            Resampler::new(Arc::new(table))
        })
        .collect()
}

fn bench(c: &mut Criterion) {
    let mut group = c.benchmark_group("sdm");
    // Criterion's default sample count is plenty; one iteration is one second of audio.
    group.sample_size(10);
    for (rate, order) in [
        (SdmRate::Dsd64, SdmOrder::Order5),
        (SdmRate::Dsd128, SdmOrder::Order5),
        (SdmRate::Dsd256, SdmOrder::Order5),
        (SdmRate::Dsd256, SdmOrder::Order7),
    ] {
        let frames = rate.input_rate() as usize;
        let input = sine(rate.input_rate(), frames);
        let mut sdm = Sdm::new(rate, order, 2);
        let mut out = Vec::with_capacity(frames * sonder_dsp::OVERSAMPLE / 8 * 2);
        group.bench_function(format!("{}-{order:?}", rate.as_str()), |b| {
            b.iter(|| {
                out.clear();
                sdm.process(black_box(&input), &mut out);
                black_box(out.len());
            })
        });
    }
    group.finish();

    // One second of 44.1 kHz stereo through the resamplers and the modulator, in engine-sized
    // blocks, so the number is the realtime factor of the whole PCM→DSD path.
    let mut group = c.benchmark_group("pcm_to_dsd");
    group.sample_size(10);
    for (rate, order) in [
        (SdmRate::Dsd128, SdmOrder::Order5),
        (SdmRate::Dsd256, SdmOrder::Order7),
    ] {
        const BLOCK: usize = 1024;
        let source = 44_100;
        let input = sine(source, source as usize);
        let mut stages = resamplers(source, rate.input_rate());
        let mut format = StreamFormat {
            rate: source,
            channels: 2,
        };
        let mut buffers = Vec::new();
        let mut frames = BLOCK;
        for stage in &mut stages {
            format = stage.configure(format);
            frames = stage.max_out(frames);
            buffers.push(vec![0.0; frames * 2]);
        }
        let mut sdm = Sdm::new(rate, order, 2);
        let mut out = Vec::with_capacity(frames * 2);
        group.bench_function(format!("44k-{}-{order:?}", rate.as_str()), |b| {
            b.iter(|| {
                for block in input.chunks(BLOCK * 2) {
                    let mut current = block;
                    let mut written = 0;
                    for (stage, buffer) in stages.iter_mut().zip(&mut buffers) {
                        written = stage.process(current, buffer);
                        current = &buffer[..written * 2];
                    }
                    out.clear();
                    sdm.process(black_box(current), &mut out);
                    black_box(written);
                }
            })
        });
    }
    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
