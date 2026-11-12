//! Rate-conversion policy: where the target rate comes from, how a ratio becomes stages, and
//! which filter source serves each stage.
//!
//! The engine keeps the default path bit-perfect unless the user asks for rate conversion or
//! the device refuses the source rate. The latter is the automatic SRC fallback: the refusal
//! picks a rate the device accepts, and the signal path then reports "not bit-perfect".

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use sonder_dsp::filter_source::FilterTable;
use sonder_dsp::multistage::plan as plan_stages;
use sonder_dsp::{
    FilterPhase, FilterSource, FilterSourceChain, FilterSpec, FilterVariant, Headroom, Ratio,
    Resampler, RoonTables, SonderDesigned, Stage,
};

/// Where an explicitly enabled SRC should send the signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SrcTarget {
    /// Never resample unless the device refuses the source rate.
    #[default]
    Off,
    /// The device's highest rate.
    MaxDevice,
    /// The device's highest rate in the source's 44.1k or 48k family.
    KeepFamily,
    Fixed(u32),
}

/// Which filter tables to prefer. Roon remains per ratio: a missing table falls back to
/// Sonder instead of failing the track.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum FilterSourceConfig {
    #[default]
    Sonder,
    Roon {
        dir: PathBuf,
    },
}

/// Guest-facing SRC settings, carried by [`PlayerConfig`](crate::PlayerConfig).
#[derive(Debug, Clone, PartialEq)]
pub struct SrcConfig {
    pub target: SrcTarget,
    pub filter_source: FilterSourceConfig,
    pub phase: FilterPhase,
    pub variant: FilterVariant,
    /// Bring 44.1k-family rates below 44.1k up to 44.1k and 48k-family ones to 48k.
    pub force_upsample_below_44k: bool,
    /// Gain inserted before resampling. `None` applies none.
    pub headroom_db: Option<f64>,
    /// Resample when the device refuses the source rate (the "not bit-perfect" fallback).
    pub auto_fallback: bool,
}

impl Default for SrcConfig {
    fn default() -> Self {
        Self {
            target: SrcTarget::Off,
            filter_source: FilterSourceConfig::Sonder,
            phase: FilterPhase::Linear,
            variant: FilterVariant::Precise,
            force_upsample_below_44k: false,
            headroom_db: Some(-3.0),
            auto_fallback: true,
        }
    }
}

/// One stage's label for the signal path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignalStage {
    pub name: String,
    pub detail: String,
}

/// A built rate-conversion plan: exact ratios, resolved tables and the headroom setting.
#[derive(Clone)]
pub struct SrcPlan {
    pub source_rate: u32,
    pub target_rate: u32,
    pub channels: u16,
    pub headroom_db: Option<f64>,
    pub stages: Vec<(Ratio, Arc<FilterTable>)>,
}

impl SrcPlan {
    pub fn stages(&self) -> Vec<Box<dyn Stage>> {
        let mut stages: Vec<Box<dyn Stage>> = Vec::new();
        if let Some(db) = self.headroom_db {
            stages.push(Box::new(Headroom::from_db(db)));
        }
        for (_, table) in &self.stages {
            stages.push(Box::new(Resampler::new(table.clone())));
        }
        stages
    }

    pub fn describe(&self) -> Vec<SignalStage> {
        let mut out = Vec::new();
        if let Some(db) = self.headroom_db {
            out.push(SignalStage {
                name: "Headroom".into(),
                detail: format!("{db:+.1} dB"),
            });
        }
        let mut rate = self.source_rate;
        for (ratio, table) in &self.stages {
            let next = (u64::from(rate) * u64::from(ratio.num) / u64::from(ratio.den)) as u32;
            out.push(SignalStage {
                name: "Resampler".into(),
                detail: format!(
                    "{rate} → {next} Hz, {}/{} ({}x), {} {} {}",
                    ratio.num,
                    ratio.den,
                    ratio.num as f64 / ratio.den as f64,
                    table.source(),
                    table.spec().phase,
                    table.spec().variant,
                ),
            });
            rate = next;
        }
        out
    }
}

/// Resolves tables for a plan: Roon when configured and the ratio exists, Sonder otherwise.
pub struct FilterResolver {
    chain: FilterSourceChain,
}

impl FilterResolver {
    /// Builds the resolver and returns any warning to show the user (a missing Roon folder
    /// still falls back to Sonder).
    pub fn new(config: &SrcConfig, cache_dir: Option<PathBuf>) -> (Self, Vec<String>) {
        let sonder: Box<dyn FilterSource> = Box::new(SonderDesigned::new(cache_dir));
        let mut warnings = Vec::new();
        let preferred: Option<Box<dyn FilterSource>> = match &config.filter_source {
            FilterSourceConfig::Sonder => None,
            FilterSourceConfig::Roon { dir } => match RoonTables::new(dir) {
                Ok(roon) => Some(Box::new(roon)),
                Err(error) => {
                    warnings.push(format!(
                        "Roon filter folder {} is unavailable ({error}); using Sonder-designed filters",
                        dir.display()
                    ));
                    None
                }
            },
        };
        (
            Self {
                chain: FilterSourceChain::new(preferred, sonder),
            },
            warnings,
        )
    }

    /// `None` when a stage's ratio cannot be served (an odd source rate whose reduced ratio
    /// needs more taps than any filter may have); the caller keeps the source rate.
    pub fn plan(
        &self,
        source_rate: u32,
        target_rate: u32,
        channels: u16,
        config: &SrcConfig,
    ) -> Option<SrcPlan> {
        let g = sonder_dsp::filter_source::gcd(source_rate, target_rate);
        let ratios = plan_stages(target_rate / g, source_rate / g);
        let multistage = ratios.len() > 1;
        let stages = ratios
            .into_iter()
            .enumerate()
            .map(|(index, ratio)| {
                // One sharp stage does the anti-imaging/anti-aliasing work; later stages
                // only remove images far above the audible band, so `Short` keeps the
                // high-ratio chain (PCM→DSD256) affordable.
                let variant = if multistage && index > 0 {
                    FilterVariant::Short
                } else {
                    config.variant
                };
                let spec = FilterSpec::new(ratio.num, ratio.den, config.phase, variant);
                Some((ratio, self.chain.resolve(spec)?))
            })
            .collect::<Option<Vec<_>>>()?;
        Some(SrcPlan {
            source_rate,
            target_rate,
            channels,
            headroom_db: config.headroom_db,
            stages,
        })
    }
}

/// The explicit target, or force-upsampling for sub-44.1k rates. `None` means "leave it".
pub fn choose_target(config: &SrcConfig, source: u32, supported: &[u32]) -> Option<u32> {
    let explicit = match config.target {
        SrcTarget::Off => None,
        SrcTarget::MaxDevice => supported.iter().copied().max(),
        SrcTarget::KeepFamily => family_max(source, supported),
        SrcTarget::Fixed(rate) => Some(rate),
    };
    let target = explicit.or_else(|| {
        config
            .force_upsample_below_44k
            .then(|| force_target(source, supported))
            .flatten()
    })?;
    (target != source && target > 0).then_some(target)
}

/// A rate the device accepts when it refuses `source`; `None` when it accepts the source
/// (the caller should not fall back) or lists nothing usable.
pub fn fallback_target(source: u32, supported: &[u32]) -> Option<u32> {
    let mut rates: Vec<u32> = supported.iter().copied().filter(|&r| r > 0).collect();
    rates.sort_unstable();
    rates.dedup();
    if rates.is_empty() || rates.binary_search(&source).is_ok() {
        return None;
    }
    let family = rate_family(source);
    let candidate = rates
        .iter()
        .copied()
        .filter(|r| rate_family(*r) == family && *r <= source)
        .max()
        .or_else(|| family_max(source, &rates))
        .or_else(|| rates.iter().copied().filter(|r| *r <= source).max())
        .or_else(|| rates.iter().copied().min());
    candidate.filter(|r| *r != source)
}

fn force_target(source: u32, supported: &[u32]) -> Option<u32> {
    if source >= 44_100 {
        return None;
    }
    let preferred = if rate_family(source) == 44_100 {
        44_100
    } else {
        48_000
    };
    if supported.contains(&preferred) {
        return Some(preferred);
    }
    supported.iter().copied().filter(|r| *r > source).min()
}

fn family_max(source: u32, supported: &[u32]) -> Option<u32> {
    let family = rate_family(source);
    supported
        .iter()
        .copied()
        .filter(|r| family != 0 && rate_family(*r) == family)
        .max()
}

/// 44.1k or 48k for the standard rate families, 0 otherwise.
fn rate_family(rate: u32) -> u32 {
    if rate > 0 && (rate.is_multiple_of(44_100) || 44_100u32.is_multiple_of(rate)) {
        44_100
    } else if rate > 0 && (rate.is_multiple_of(48_000) || 48_000u32.is_multiple_of(rate)) {
        48_000
    } else {
        0
    }
}

/// Formats accepted by the current device, cached per channel count.
pub type DeviceFormats = HashMap<(String, u16), Vec<sonder_output::DeviceFormat>>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_targets_follow_the_policy() {
        let mut config = SrcConfig::default();
        let supported = [44_100, 48_000, 96_000, 192_000];
        assert_eq!(choose_target(&config, 44_100, &supported), None);

        config.target = SrcTarget::MaxDevice;
        assert_eq!(choose_target(&config, 44_100, &supported), Some(192_000));

        config.target = SrcTarget::KeepFamily;
        assert_eq!(choose_target(&config, 44_100, &supported), None);
        assert_eq!(choose_target(&config, 88_200, &supported), Some(44_100));
        config.target = SrcTarget::Fixed(48_000);
        assert_eq!(choose_target(&config, 44_100, &supported), Some(48_000));
    }

    #[test]
    fn sub_44k_force_upsampling_picks_the_family() {
        let config = SrcConfig {
            target: SrcTarget::Off,
            force_upsample_below_44k: true,
            ..SrcConfig::default()
        };
        assert_eq!(
            choose_target(&config, 22_050, &[44_100, 48_000]),
            Some(44_100)
        );
        assert_eq!(
            choose_target(&config, 32_000, &[44_100, 48_000]),
            Some(48_000)
        );
        assert_eq!(choose_target(&config, 44_100, &[44_100, 48_000]), None);
    }

    #[test]
    fn fallback_prefers_the_same_family_then_the_nearest_rate() {
        assert_eq!(fallback_target(192_000, &[44_100, 96_000]), Some(96_000));
        assert_eq!(fallback_target(48_000, &[44_100]), Some(44_100));
        assert_eq!(fallback_target(96_000, &[44_100, 88_200]), Some(88_200));
        assert_eq!(fallback_target(32_000, &[44_100, 48_000]), Some(44_100));
        assert_eq!(fallback_target(48_000, &[44_100, 48_000]), None);
        assert_eq!(fallback_target(192_000, &[]), None);
    }

    #[test]
    fn plan_splits_large_ratios_and_names_each_source() {
        let (resolver, warnings) = FilterResolver::new(&SrcConfig::default(), None);
        assert!(warnings.is_empty());
        let plan = resolver
            .plan(44_100, 11_289_600, 2, &SrcConfig::default())
            .unwrap();
        assert!(plan.stages.len() > 1);
        assert_eq!(plan.stages.iter().map(|(r, _)| r.num).product::<u32>(), 256);
        assert_eq!(plan.stages.iter().map(|(r, _)| r.den).product::<u32>(), 1);
        let described = plan.describe();
        assert_eq!(described[0].name, "Headroom");
        for stage in &described[1..] {
            assert!(stage.detail.contains("Sonder"), "{}", stage.detail);
        }
        assert_eq!(described.len(), plan.stages.len() + 1);
    }

    #[test]
    fn missing_roon_folder_warns_and_still_serves_every_ratio() {
        let dir = std::env::temp_dir().join(format!("sonder-missing-roon-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let config = SrcConfig {
            filter_source: FilterSourceConfig::Roon { dir },
            ..SrcConfig::default()
        };
        let (resolver, warnings) = FilterResolver::new(&config, None);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("using Sonder-designed filters"));
        let plan = resolver.plan(48_000, 44_100, 2, &config).unwrap();
        for (ratio, table) in &plan.stages {
            assert_eq!(table.source(), sonder_dsp::FilterSourceKind::Sonder);
            assert_eq!((ratio.num, ratio.den), (147, 160));
        }
    }

    #[test]
    fn roon_without_the_ratio_table_falls_back_to_sonder() {
        let dir = std::env::temp_dir().join(format!("sonder-empty-roon-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let config = SrcConfig {
            filter_source: FilterSourceConfig::Roon { dir: dir.clone() },
            ..SrcConfig::default()
        };
        let (resolver, warnings) = FilterResolver::new(&config, None);
        assert!(warnings.is_empty(), "{warnings:?}");
        // There is no 48k→44.1k Roon table, so Sonder serves it and says so.
        let plan = resolver.plan(48_000, 44_100, 2, &config).unwrap();
        assert!(
            plan.describe()
                .iter()
                .any(|stage| { stage.detail.contains("Sonder") && stage.detail.contains("48000") })
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn odd_source_rates_plan_or_refuse_without_panicking() {
        let (resolver, _) = FilterResolver::new(&SrcConfig::default(), None);
        // 5512 Hz → 44.1 kHz: 11025/1378, where 1378 = 2·13·53 cannot split into ≤8x steps.
        let plan = resolver
            .plan(5_512, 44_100, 2, &SrcConfig::default())
            .expect("odd rate plans");
        assert_eq!(
            plan.stages.iter().map(|(r, _)| r.num).product::<u32>(),
            11_025
        );
        assert_eq!(
            plan.stages.iter().map(|(r, _)| r.den).product::<u32>(),
            1_378
        );
        // 44.1 kHz → 44.101 kHz needs a filter longer than any table may be.
        assert!(
            resolver
                .plan(44_100, 44_101, 2, &SrcConfig::default())
                .is_none()
        );
    }
}
