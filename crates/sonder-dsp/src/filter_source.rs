//! Filter tables shared by every rate converter.
//!
//! A [`FilterTable`] is one prototype low-pass FIR for a rational ratio `num/den`. The
//! table is stored twice: the prototype coefficients (summing to one) and a phase-major
//! layout scaled by `num`, where each branch has unity DC gain. [`FirCore`](crate::FirCore)
//! consumes the phase layout.
//!
//! Two real sources implement [`FilterSource`]: Sonder's runtime designer with a disk cache
//! ([`SonderDesigned`](crate::SonderDesigned)) and user-supplied Roon tables
//! ([`RoonTables`](crate::RoonTables)). [`FilterSourceChain`] prefers the first and falls
//! back to Sonder per ratio, which is what lets a missing Roon table fall back instead of
//! refusing playback.

use std::fmt;
use std::sync::Arc;

/// Largest table accepted from a user path, so a corrupt or hostile file cannot exhaust
/// memory before validation.
pub const MAX_TAPS: usize = 4_000_000;

/// Relative tolerance when validating that a table sums to one.
pub const SUM_TOLERANCE: f64 = 1e-3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FilterPhase {
    Linear,
    Minimum,
}

impl fmt::Display for FilterPhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Linear => "linear",
            Self::Minimum => "minimum",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FilterVariant {
    #[default]
    Precise,
    Smooth,
    /// Internal low-latency option; Roon calls its equivalent table `2s`.
    Short,
}

impl FilterVariant {
    /// Stopband attenuation the designer targets.
    pub const fn attenuation_db(self) -> f64 {
        match self {
            Self::Precise => 140.0,
            Self::Smooth => 100.0,
            Self::Short => 70.0,
        }
    }

    /// Transition width as a fraction of the narrower Nyquist frequency.
    pub const fn transition(self) -> f64 {
        match self {
            Self::Precise => 0.10,
            Self::Smooth => 0.20,
            Self::Short => 0.35,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Precise => "precise",
            Self::Smooth => "smooth",
            Self::Short => "2s",
        }
    }
}

impl fmt::Display for FilterVariant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Identifies one filter: a reduced rational ratio plus the user's phase and quality
/// choices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FilterSpec {
    /// Upsampling factor `L`.
    pub num: u32,
    /// Downsampling factor `M`.
    pub den: u32,
    pub phase: FilterPhase,
    pub variant: FilterVariant,
}

impl FilterSpec {
    /// Builds a spec with the ratio reduced, so every source names and caches it the same
    /// way (`160/147`, never `320/294`).
    pub fn new(num: u32, den: u32, phase: FilterPhase, variant: FilterVariant) -> Self {
        debug_assert!(num > 0 && den > 0, "a filter ratio is positive");
        let g = gcd(num.max(1), den.max(1));
        Self {
            num: num / g,
            den: den / g,
            phase,
            variant,
        }
    }

    /// Ratio as a float for sizing and logging.
    pub fn ratio(self) -> f64 {
        f64::from(self.num) / f64::from(self.den)
    }

    /// Roon's file stem for this ratio (`2x`, `3_2x`, `160_147x`).
    pub fn roon_stem(self) -> String {
        if self.den == 1 {
            format!("{}x", self.num)
        } else {
            format!("{}_{}x", self.num, self.den)
        }
    }
}

pub fn gcd(mut a: u32, mut b: u32) -> u32 {
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a.max(1)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FilterSourceKind {
    Sonder,
    Roon,
}

impl fmt::Display for FilterSourceKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Sonder => "Sonder",
            Self::Roon => "Roon",
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum FilterError {
    #[error("{0}")]
    BadTable(String),
    #[error("cannot read {path}: {error}")]
    Io {
        path: std::path::PathBuf,
        error: std::io::Error,
    },
}

/// A validated prototype filter and its polyphase layout.
#[derive(Debug)]
pub struct FilterTable {
    spec: FilterSpec,
    source: FilterSourceKind,
    /// Prototype coefficients after zero-padding, summing to one.
    coeffs: Vec<f64>,
    /// Phase-major coefficients scaled by `num`, so each branch has unity DC gain.
    phases: Vec<f64>,
    taps_per_phase: usize,
}

impl FilterTable {
    /// Validates `coeffs` (finite, non-empty, summing to one), zero-pads the length to a
    /// multiple of `num`, and builds the scaled phase layout.
    pub fn from_prototype(
        spec: FilterSpec,
        source: FilterSourceKind,
        mut coeffs: Vec<f64>,
    ) -> Result<Self, FilterError> {
        if coeffs.is_empty() {
            return Err(FilterError::BadTable("the table is empty".into()));
        }
        if coeffs.len() > MAX_TAPS {
            return Err(FilterError::BadTable(format!(
                "{} taps exceed the {MAX_TAPS} tap limit",
                coeffs.len()
            )));
        }
        if let Some((at, value)) = coeffs.iter().enumerate().find(|(_, v)| !v.is_finite()) {
            return Err(FilterError::BadTable(format!(
                "coefficient {at} is not finite ({value})"
            )));
        }
        let sum: f64 = coeffs.iter().sum();
        if (sum - 1.0).abs() > SUM_TOLERANCE {
            return Err(FilterError::BadTable(format!(
                "the table sums to {sum}, not 1 within {SUM_TOLERANCE}"
            )));
        }

        let l = spec.num as usize;
        let padded = coeffs.len().div_ceil(l) * l;
        coeffs.resize(padded, 0.0);
        let taps_per_phase = padded / l;
        let gain = l as f64;
        let mut phases = vec![0.0; padded];
        for p in 0..l {
            for i in 0..taps_per_phase {
                phases[p * taps_per_phase + i] = coeffs[p + i * l] * gain;
            }
        }
        Ok(Self {
            spec,
            source,
            coeffs,
            phases,
            taps_per_phase,
        })
    }

    pub fn spec(&self) -> FilterSpec {
        self.spec
    }

    pub fn source(&self) -> FilterSourceKind {
        self.source
    }

    /// Prototype coefficients (sum one) including the zero padding.
    pub fn coefficients(&self) -> &[f64] {
        &self.coeffs
    }

    /// Phase-major coefficients, scaled so every branch has unity DC gain.
    pub fn phases(&self) -> &[f64] {
        &self.phases
    }

    pub fn taps_per_phase(&self) -> usize {
        self.taps_per_phase
    }

    /// Magnitude response at `f` in cycles per sample of the filtered (upsampled) stream.
    ///
    /// For diagnostics and tests; not on the audio path.
    pub fn magnitude_at(&self, f: f64) -> f64 {
        let (mut re, mut im) = (0.0, 0.0);
        for (n, &h) in self.coeffs.iter().enumerate() {
            let angle = -2.0 * std::f64::consts::PI * f * n as f64;
            re += h * angle.cos();
            im += h * angle.sin();
        }
        re.hypot(im)
    }

    /// DC gain of one branch, which must be one for unity level.
    pub fn branch_gain(&self, phase: usize) -> f64 {
        let start = phase * self.taps_per_phase;
        self.phases[start..start + self.taps_per_phase].iter().sum()
    }
}

/// Supplies filter tables for a ratio, or `None` when this source cannot serve it.
pub trait FilterSource: Send + Sync {
    fn kind(&self) -> FilterSourceKind;

    fn get(&self, spec: FilterSpec) -> Option<Arc<FilterTable>>;
}

/// Tries the preferred source (Roon, when enabled) and falls back to Sonder per ratio.
///
/// A ratio without a Roon table is served by Sonder, and the signal path says so (a
/// 48k→44.1k stream with Roon enabled). [`resolve`](Self::resolve) returns `None` only
/// when no source can serve the ratio at all, such as an odd source rate whose reduced
/// ratio would need more than [`MAX_TAPS`] taps.
pub struct FilterSourceChain {
    preferred: Option<Box<dyn FilterSource>>,
    fallback: Box<dyn FilterSource>,
}

impl FilterSourceChain {
    pub fn new(preferred: Option<Box<dyn FilterSource>>, fallback: Box<dyn FilterSource>) -> Self {
        Self {
            preferred,
            fallback,
        }
    }

    pub fn resolve(&self, spec: FilterSpec) -> Option<Arc<FilterTable>> {
        self.preferred
            .as_ref()
            .and_then(|s| s.get(spec))
            .or_else(|| self.fallback.get(spec))
    }
}

impl FilterSource for FilterSourceChain {
    fn kind(&self) -> FilterSourceKind {
        self.preferred
            .as_ref()
            .map_or(FilterSourceKind::Sonder, |s| s.kind())
    }

    fn get(&self, spec: FilterSpec) -> Option<Arc<FilterTable>> {
        self.resolve(spec)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(num: u32, den: u32) -> FilterSpec {
        FilterSpec::new(num, den, FilterPhase::Linear, FilterVariant::Precise)
    }

    #[test]
    fn ratios_are_reduced_for_names_and_cache_keys() {
        assert_eq!(spec(320, 294).num, 160);
        assert_eq!(spec(320, 294).den, 147);
        assert_eq!(spec(2, 1).roon_stem(), "2x");
        assert_eq!(spec(3, 2).roon_stem(), "3_2x");
        assert_eq!(spec(160, 147).roon_stem(), "160_147x");
    }

    #[test]
    fn padding_keeps_the_length_a_multiple_of_l_and_phases_unity() {
        // Ten coefficients, sum one, for a 4x ratio: pad to twelve.
        let coeffs = vec![0.1; 10];
        let sum: f64 = coeffs.iter().sum();
        let coeffs: Vec<f64> = coeffs.into_iter().map(|c| c / sum).collect();
        let table =
            FilterTable::from_prototype(spec(4, 1), FilterSourceKind::Sonder, coeffs).unwrap();
        assert_eq!(table.coefficients().len(), 12);
        assert_eq!(table.taps_per_phase(), 3);
        // The phase layout is the prototype scaled by L: four branches sum to L.
        let total: f64 = (0..4).map(|p| table.branch_gain(p)).sum();
        assert!((total - 4.0).abs() < 1e-12, "{total}");
        assert!((table.phases()[0] - 0.1 * 4.0).abs() < 1e-12);
        assert!((table.phases()[1] - 0.1 * 4.0).abs() < 1e-12);
    }

    #[test]
    fn bad_tables_are_rejected() {
        let mut bad = vec![0.5; 4];
        bad[2] = f64::NAN;
        assert!(FilterTable::from_prototype(spec(2, 1), FilterSourceKind::Roon, bad).is_err());
        assert!(
            FilterTable::from_prototype(spec(2, 1), FilterSourceKind::Roon, vec![0.5; 10]).is_err()
        );
    }
}
