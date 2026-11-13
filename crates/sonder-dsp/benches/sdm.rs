//! PCM→DSD cost per second of stereo audio, for the default-rate decision in the SDM plan.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use sonder_dsp::{Sdm, SdmOrder, SdmRate};

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
        let input: Vec<f64> = (0..frames)
            .flat_map(|frame| {
                let sample = 0.5
                    * (2.0 * std::f64::consts::PI * 1000.0 * frame as f64
                        / f64::from(rate.input_rate()))
                    .sin();
                [sample; 2]
            })
            .collect();
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
}

criterion_group!(benches, bench);
criterion_main!(benches);
