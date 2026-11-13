//! Convolution cost per second of stereo audio at 192 kHz, for a room-correction-sized
//! response (65 536 taps) and a long one (1 s).

use std::hint::black_box;
use std::sync::Arc;

use criterion::{Criterion, criterion_group, criterion_main};
use sonder_dsp::{Convolver, ImpulseResponse, Stage, StreamFormat};

const RATE: u32 = 192_000;
const BLOCK: usize = 4096;

fn response(frames: usize) -> Arc<ImpulseResponse> {
    // A decaying noise burst: every tap non-zero, like a measured response.
    let mut state = 0x2545_F491_4F6C_DD1D_u64;
    let channel = |decay: f64, state: &mut u64| -> Vec<f64> {
        (0..frames)
            .map(|n| {
                *state ^= *state << 13;
                *state ^= *state >> 7;
                *state ^= *state << 17;
                let noise = (*state >> 11) as f64 / (1u64 << 53) as f64 - 0.5;
                noise * (-(n as f64) / decay).exp()
            })
            .collect()
    };
    let left = channel(frames as f64 / 6.0, &mut state);
    let right = channel(frames as f64 / 6.0, &mut state);
    Arc::new(ImpulseResponse::new("bench".into(), RATE, vec![left, right]).unwrap())
}

fn bench(c: &mut Criterion) {
    let mut group = c.benchmark_group("convolver");
    group.sample_size(10);
    let input: Vec<f64> = (0..RATE as usize)
        .flat_map(|n| {
            let s = 0.5 * (2.0 * std::f64::consts::PI * 1000.0 * n as f64 / f64::from(RATE)).sin();
            [s, s]
        })
        .collect();
    for (label, frames) in [("65536-taps", 65_536), ("1s", RATE as usize)] {
        let mut stage = Convolver::new(response(frames));
        stage.configure(StreamFormat {
            rate: RATE,
            channels: 2,
        });
        let mut out = vec![0.0; stage.max_out(BLOCK) * 2];
        group.bench_function(format!("192k-stereo-{label}"), |b| {
            b.iter(|| {
                for block in input.chunks(BLOCK * 2) {
                    black_box(stage.process(black_box(block), &mut out));
                }
            })
        });
    }
    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
