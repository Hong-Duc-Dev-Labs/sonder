//! Multistage planning for large conversion ratios.
//!
//! A single 256x filter would need hundreds of thousands of taps. Above 8x the planner
//! splits the rational ratio into stages of at most 8x each, assigns the decimation factors
//! to the later stages, and keeps every intermediate rate exact. Each stage then gets its
//! own filter table, so a Roon table can serve one stage while Sonder designs another.

use crate::resampler::Ratio;

/// Longest single-stage ratio before the planner splits.
pub const MAX_STAGE_RATIO: u32 = 8;

/// Plans the ordered stages whose product is exactly `num/den`.
///
/// Ratios up to 8x stay a single stage. Larger ratios are factored into pieces of at most
/// 8x; any `M` factors are assigned to the last stages so the intermediate rate grows
/// smoothly instead of peaking at `L·fs`.
pub fn plan(num: u32, den: u32) -> Vec<Ratio> {
    let ratio = Ratio::new(num, den);
    if u64::from(ratio.num) <= u64::from(MAX_STAGE_RATIO) * u64::from(ratio.den) {
        return vec![ratio];
    }

    let ups = factor_groups(ratio.num);
    let downs = factor_groups(ratio.den);
    let stages = ups.len().max(downs.len());
    let mut nums = vec![1u32; stages];
    for (i, factor) in ups.iter().enumerate() {
        nums[i] = *factor;
    }
    let mut dens = vec![1u32; stages];
    let offset = stages - downs.len();
    for (i, factor) in downs.iter().enumerate() {
        dens[offset + i] = *factor;
    }
    nums.into_iter()
        .zip(dens)
        .map(|(num, den)| Ratio::new(num, den))
        .collect()
}

/// Factors `n` into groups no larger than [`MAX_STAGE_RATIO`] where possible; the product
/// is `n`. A prime above the limit (odd source rates such as 5512 or 11127 Hz) cannot be
/// split and becomes a group of its own.
fn factor_groups(mut n: u32) -> Vec<u32> {
    let mut out = Vec::new();
    while n > MAX_STAGE_RATIO {
        let factor = (2..=MAX_STAGE_RATIO)
            .rev()
            .find(|&f| n.is_multiple_of(f))
            .unwrap_or_else(|| smallest_prime_factor(n));
        out.push(factor);
        n /= factor;
    }
    if n > 1 || out.is_empty() {
        out.push(n.max(1));
    }
    out
}

fn smallest_prime_factor(n: u32) -> u32 {
    let mut f = 2u32;
    while u64::from(f) * u64::from(f) <= u64::from(n) {
        if n.is_multiple_of(f) {
            return f;
        }
        f += 1;
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    fn product(stages: &[Ratio]) -> Ratio {
        Ratio::new(
            stages.iter().map(|r| r.num).product(),
            stages.iter().map(|r| r.den).product(),
        )
    }

    #[test]
    fn small_ratios_stay_single_stage() {
        assert_eq!(plan(2, 1), vec![Ratio { num: 2, den: 1 }]);
        assert_eq!(plan(160, 147), vec![Ratio { num: 160, den: 147 }]);
        assert_eq!(plan(8, 1), vec![Ratio { num: 8, den: 1 }]);
    }

    #[test]
    fn large_ratios_split_with_exact_products_and_bounded_steps() {
        for (num, den) in [
            (16, 1),
            (64, 1),
            (256, 1),
            (1280, 1),
            (2560, 147),
            (1280, 147),
        ] {
            let stages = plan(num, den);
            assert_eq!(product(&stages).num, Ratio::new(num, den).num);
            assert_eq!(product(&stages).den, Ratio::new(num, den).den);
            for stage in &stages {
                assert!(stage.num <= MAX_STAGE_RATIO, "{stages:?}");
                assert!(stage.den <= MAX_STAGE_RATIO, "{stages:?}");
            }
        }
    }

    #[test]
    fn primes_above_the_stage_limit_get_their_own_stage() {
        // 5512 Hz → 44.1 kHz is 11025/1378 with 1378 = 2·13·53; 11127 Hz → 96 kHz is
        // 32000/3709 with 3709 prime.
        for (num, den) in [(11_025, 1_378), (32_000, 3_709), (11 * 13, 1)] {
            let stages = plan(num, den);
            assert_eq!(product(&stages), Ratio::new(num, den), "{stages:?}");
            assert!(stages.iter().all(|s| s.num > 0 && s.den > 0), "{stages:?}");
            assert!(
                stages.iter().all(|s| !s.is_identity()),
                "no pass-through stage: {stages:?}"
            );
        }
    }

    #[test]
    fn decimation_factors_land_on_the_late_stages() {
        let stages = plan(1280, 147);
        assert!(stages.len() > 1);
        assert_eq!(stages[0].den, 1, "the first stage only upsamples");
        assert!(stages.last().unwrap().den > 1, "{stages:?}");
    }
}
