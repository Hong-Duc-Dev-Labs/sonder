//! Sonder's own filter design, at runtime, with a disk cache.
//!
//! Linear phase is a Kaiser-windowed sinc; the minimum-phase variant is the same magnitude
//! converted through the real cepstrum (homomorphic method, `realfft`). Nothing is
//! committed: the first use designs a table (milliseconds for the usual ratios) and writes
//! it to the user's cache directory, so later runs load it back.
//!
//! Lengths follow Kaiser's estimate for the requested attenuation and transition width,
//! rounded up to a multiple of `L` so every polyphase branch has the same length. The
//! minimum-phase conversion keeps the first `2N` coefficients of an `8N`-point cepstral
//! transform: truncating at `N` leaves visible passband ripple. Larger ratios are handled by
//! the multistage planner instead of one giant filter.

use std::collections::HashMap;
use std::fs;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use realfft::RealFftPlanner;
use realfft::num_complex::Complex;

use crate::filter_source::{
    FilterPhase, FilterSource, FilterSourceKind, FilterSpec, FilterTable, FilterVariant,
};

/// Bumped when the design or the cache layout changes; old files are redesigned.
const CACHE_VERSION: u32 = 1;
const CACHE_MAGIC: &[u8; 8] = b"SFIR\x01\0\0\0";

/// Designs and caches Sonder's own tables. Cheap to call repeatedly: results live in memory
/// and on disk.
pub struct SonderDesigned {
    cache_dir: Option<PathBuf>,
    tables: Mutex<HashMap<FilterSpec, Arc<FilterTable>>>,
}

impl SonderDesigned {
    /// `cache_dir` is created on first write; `None` disables the disk cache (tests).
    pub fn new(cache_dir: Option<PathBuf>) -> Self {
        Self {
            cache_dir,
            tables: Mutex::new(HashMap::new()),
        }
    }

    /// Uses the per-user cache directory (or `SONDER_FILTER_CACHE` when set).
    pub fn with_default_cache() -> Self {
        Self::new(default_cache_dir())
    }

    fn cached(&self, spec: FilterSpec) -> Option<Arc<FilterTable>> {
        self.tables
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&spec)
            .cloned()
    }

    fn remember(&self, spec: FilterSpec, table: &Arc<FilterTable>) {
        self.tables
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(spec, table.clone());
    }

    fn load(&self, spec: FilterSpec) -> Option<FilterTable> {
        let path = self.cache_dir.as_ref()?.join(cache_name(spec));
        let file = fs::File::open(path).ok()?;
        let mut reader = BufReader::new(file);
        let mut magic = [0u8; 8];
        reader.read_exact(&mut magic).ok()?;
        if &magic != CACHE_MAGIC {
            return None;
        }
        if read_u32(&mut reader)? != CACHE_VERSION {
            return None;
        }
        let num = read_u32(&mut reader)?;
        let den = read_u32(&mut reader)?;
        let phase = read_u8(&mut reader)?;
        let variant = read_u8(&mut reader)?;
        if (num, den) != (spec.num, spec.den)
            || phase != phase_code(spec.phase)
            || variant != variant_code(spec.variant)
        {
            return None;
        }
        let taps = usize::try_from(read_u64(&mut reader)?).ok()?;
        if taps == 0 || taps > crate::filter_source::MAX_TAPS {
            return None;
        }
        let mut coeffs = Vec::with_capacity(taps);
        for _ in 0..taps {
            coeffs.push(read_f64(&mut reader)?);
        }
        FilterTable::from_prototype(spec, FilterSourceKind::Sonder, coeffs).ok()
    }

    fn store(&self, spec: FilterSpec, table: &FilterTable) {
        let Some(dir) = &self.cache_dir else {
            return;
        };
        if fs::create_dir_all(dir).is_err() {
            return;
        }
        let name = cache_name(spec);
        let path = dir.join(&name);
        let tmp = dir.join(format!("{name}.tmp"));
        let write = || -> std::io::Result<()> {
            let mut out = BufWriter::new(fs::File::create(&tmp)?);
            out.write_all(CACHE_MAGIC)?;
            out.write_all(&CACHE_VERSION.to_le_bytes())?;
            out.write_all(&spec.num.to_le_bytes())?;
            out.write_all(&spec.den.to_le_bytes())?;
            out.write_all(&[phase_code(spec.phase), variant_code(spec.variant)])?;
            out.write_all(&(table.coefficients().len() as u64).to_le_bytes())?;
            for &c in table.coefficients() {
                out.write_all(&c.to_le_bytes())?;
            }
            out.flush()?;
            drop(out);
            fs::rename(&tmp, &path)
        };
        if write().is_err() {
            let _ = fs::remove_file(&tmp);
        }
    }
}

impl FilterSource for SonderDesigned {
    fn kind(&self) -> FilterSourceKind {
        FilterSourceKind::Sonder
    }

    fn get(&self, spec: FilterSpec) -> Option<Arc<FilterTable>> {
        if let Some(table) = self.cached(spec) {
            return Some(table);
        }
        let linear_taps = linear_taps(spec);
        if spec.phase == FilterPhase::Minimum && linear_taps > MAX_MINIMUM_PHASE_TAPS {
            // The cepstral transform would need a multi-gigabyte FFT. Serve the linear-phase
            // design instead; the table's spec says so, and so does the signal path.
            return self.get(FilterSpec {
                phase: FilterPhase::Linear,
                ..spec
            });
        }
        if linear_taps > crate::filter_source::MAX_TAPS {
            // Odd rates with a huge reduced ratio: refuse before designing megabytes of taps.
            return None;
        }
        let table = match self.load(spec) {
            Some(table) => table,
            None => {
                let coeffs = design_coefficients(spec);
                let table =
                    FilterTable::from_prototype(spec, FilterSourceKind::Sonder, coeffs).ok()?;
                self.store(spec, &table);
                table
            }
        };
        let table = Arc::new(table);
        self.remember(spec, &table);
        Some(table)
    }
}

/// Designs the prototype coefficients (sum one) for `spec`. Public for tests and
/// diagnostics; [`SonderDesigned`] adds the caches.
pub fn design_coefficients(spec: FilterSpec) -> Vec<f64> {
    let linear = design_linear(spec);
    match spec.phase {
        FilterPhase::Linear => linear,
        FilterPhase::Minimum => minimum_phase(&linear),
    }
}

/// Longest linear-phase prototype converted to minimum phase: its `8N`-point cepstral FFT
/// stays at or below 2^20 points. Longer specs are served linear phase.
pub const MAX_MINIMUM_PHASE_TAPS: usize = (1 << 20) / 8;

/// Narrowest Nyquist as a fraction of the intermediate (L·fs) Nyquist frequency.
fn narrow_nyquist(spec: FilterSpec) -> f64 {
    1.0 / f64::from(spec.num).max(f64::from(spec.den))
}

/// Length of the linear-phase prototype for `spec`: Kaiser's estimate rounded up to a
/// multiple of `L`. Cheap, so callers can refuse a spec before designing it.
pub fn linear_taps(spec: FilterSpec) -> usize {
    let transition = spec.variant.transition();
    let attenuation = spec.variant.attenuation_db();
    // Kaiser's estimate: N ≈ (A - 8) / (2.285 · Δω).
    let dw = std::f64::consts::PI * narrow_nyquist(spec) * transition;
    let estimate = ((attenuation - 8.0) / (2.285 * dw)).ceil() as usize + 1;
    estimate
        .max(3)
        .div_ceil(spec.num as usize)
        .saturating_mul(spec.num as usize)
}

/// Kaiser-windowed sinc, normalised to unity DC gain.
fn design_linear(spec: FilterSpec) -> Vec<f64> {
    let nyquist = narrow_nyquist(spec);
    let transition = spec.variant.transition();
    let attenuation = spec.variant.attenuation_db();
    let taps = linear_taps(spec);

    // Cutoff at the middle of the transition band, in cycles per intermediate sample.
    let cutoff = 0.5 * nyquist * (1.0 - transition / 2.0);
    let beta = kaiser_beta(attenuation);
    let centre = (taps - 1) as f64 / 2.0;
    let i0_beta = bessel_i0(beta);

    let mut h = Vec::with_capacity(taps);
    for n in 0..taps {
        let x = n as f64 - centre;
        let sinc = sinc(2.0 * cutoff * x);
        let r = x / centre;
        let window = bessel_i0(beta * (1.0 - r * r).max(0.0).sqrt()) / i0_beta;
        h.push(2.0 * cutoff * sinc * window);
    }
    normalise(&mut h);
    h
}

/// Converts a zero-phase FIR to its minimum-phase equivalent through the real cepstrum.
fn minimum_phase(h: &[f64]) -> Vec<f64> {
    let n = h.len();
    let fft_len = (8 * n).next_power_of_two();
    let mut planner = RealFftPlanner::<f64>::new();
    let forward = planner.plan_fft_forward(fft_len);
    let inverse = planner.plan_fft_inverse(fft_len);
    let scale = 1.0 / fft_len as f64;

    let mut time = forward.make_input_vec();
    time[..n].copy_from_slice(h);
    let mut spectrum = forward.make_output_vec();
    forward
        .process(&mut time, &mut spectrum)
        .expect("matching FFT lengths");

    // Real cepstrum of the log magnitude.
    let mut log_spectrum = inverse.make_input_vec();
    for (out, bin) in log_spectrum.iter_mut().zip(&spectrum) {
        let magnitude = bin.norm().max(1e-300);
        *out = Complex::new(magnitude.ln(), 0.0);
    }
    let mut cepstrum = inverse.make_output_vec();
    inverse
        .process(&mut log_spectrum, &mut cepstrum)
        .expect("matching FFT lengths");
    for c in &mut cepstrum {
        *c *= scale;
    }

    // Double the positive quefrencies, keep DC and Nyquist: the minimum-phase cepstrum.
    let half = fft_len / 2;
    let mut folded = vec![0.0; fft_len];
    folded[0] = cepstrum[0];
    for k in 1..half {
        folded[k] = 2.0 * cepstrum[k];
    }
    folded[half] = cepstrum[half];

    let mut folded_time = forward.make_input_vec();
    folded_time.copy_from_slice(&folded);
    let mut folded_spectrum = forward.make_output_vec();
    forward
        .process(&mut folded_time, &mut folded_spectrum)
        .expect("matching FFT lengths");
    let mut exponentiated = inverse.make_input_vec();
    for (out, bin) in exponentiated.iter_mut().zip(&folded_spectrum) {
        let magnitude = bin.re.exp();
        *out = Complex::new(magnitude * bin.im.cos(), magnitude * bin.im.sin());
    }
    let mut response = inverse.make_output_vec();
    inverse
        .process(&mut exponentiated, &mut response)
        .expect("matching FFT lengths");
    for x in &mut response {
        *x *= scale;
    }
    response.truncate(2 * n);
    normalise(&mut response);
    response
}

fn normalise(h: &mut [f64]) {
    let sum: f64 = h.iter().sum();
    if sum.is_finite() && sum.abs() > 1e-30 {
        for c in h {
            *c /= sum;
        }
    }
}

fn sinc(x: f64) -> f64 {
    if x.abs() < 1e-12 {
        1.0
    } else {
        let px = std::f64::consts::PI * x;
        px.sin() / px
    }
}

fn kaiser_beta(attenuation_db: f64) -> f64 {
    if attenuation_db > 50.0 {
        0.1102 * (attenuation_db - 8.7)
    } else if attenuation_db > 21.0 {
        0.5842 * (attenuation_db - 21.0).powf(0.4) + 0.07886 * (attenuation_db - 21.0)
    } else {
        0.0
    }
}

/// Modified Bessel function of the first kind, order zero, by its power series.
fn bessel_i0(x: f64) -> f64 {
    let y = x * x / 4.0;
    let mut term = 1.0;
    let mut sum = 1.0;
    for k in 1..200 {
        term *= y / (k * k) as f64;
        sum += term;
        if term < sum * 1e-17 {
            break;
        }
    }
    sum
}

/// Per-user filter cache, or `SONDER_FILTER_CACHE` when set.
pub fn default_cache_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("SONDER_FILTER_CACHE") {
        return Some(PathBuf::from(dir));
    }
    base_cache_dir().map(|dir| dir.join("filters"))
}

fn base_cache_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("LOCALAPPDATA").map(|p| PathBuf::from(p).join("Sonder"))
    }
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME").map(|p| PathBuf::from(p).join("Library/Caches/Sonder"))
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".cache")))
            .map(|p| p.join("sonder"))
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", unix)))]
    {
        None
    }
}

fn cache_name(spec: FilterSpec) -> String {
    format!(
        "v{CACHE_VERSION}-{}_{}_{}_{}.fir",
        spec.num, spec.den, spec.phase, spec.variant
    )
}

const fn phase_code(phase: FilterPhase) -> u8 {
    match phase {
        FilterPhase::Linear => 0,
        FilterPhase::Minimum => 1,
    }
}

const fn variant_code(variant: FilterVariant) -> u8 {
    match variant {
        FilterVariant::Precise => 0,
        FilterVariant::Smooth => 1,
        FilterVariant::Short => 2,
    }
}

fn read_u8(r: &mut impl Read) -> Option<u8> {
    let mut b = [0u8; 1];
    r.read_exact(&mut b).ok()?;
    Some(b[0])
}

fn read_u32(r: &mut impl Read) -> Option<u32> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b).ok()?;
    Some(u32::from_le_bytes(b))
}

fn read_u64(r: &mut impl Read) -> Option<u64> {
    let mut b = [0u8; 8];
    r.read_exact(&mut b).ok()?;
    Some(u64::from_le_bytes(b))
}

fn read_f64(r: &mut impl Read) -> Option<f64> {
    let mut b = [0u8; 8];
    r.read_exact(&mut b).ok()?;
    Some(f64::from_le_bytes(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(num: u32, den: u32, phase: FilterPhase, variant: FilterVariant) -> FilterSpec {
        FilterSpec::new(num, den, phase, variant)
    }

    /// Response in dB relative to DC, at `f` cycles per intermediate sample.
    fn db(table: &FilterTable, f: f64) -> f64 {
        20.0 * table.magnitude_at(f).log10()
    }

    fn read_cached(dir: &std::path::Path, spec: FilterSpec) -> Option<Vec<f64>> {
        let file = fs::File::open(dir.join(cache_name(spec))).ok()?;
        let mut reader = BufReader::new(file);
        let mut magic = [0u8; 8];
        reader.read_exact(&mut magic).ok()?;
        assert_eq!(&magic, CACHE_MAGIC);
        assert_eq!(read_u32(&mut reader)?, CACHE_VERSION);
        let _ = (read_u32(&mut reader)?, read_u32(&mut reader)?);
        let _ = (read_u8(&mut reader)?, read_u8(&mut reader)?);
        let taps = read_u64(&mut reader)? as usize;
        (0..taps).map(|_| read_f64(&mut reader)).collect()
    }

    #[test]
    fn precise_linear_design_meets_passband_and_stopband() {
        let spec = spec(2, 1, FilterPhase::Linear, FilterVariant::Precise);
        let coeffs = design_coefficients(spec);
        let table = FilterTable::from_prototype(spec, FilterSourceKind::Sonder, coeffs).unwrap();
        // 2x: input Nyquist is a quarter of the intermediate sample rate.
        let passband_edge = 0.25 * (1.0 - FilterVariant::Precise.transition());
        let mut worst_ripple: f64 = 0.0;
        for i in 0..=90 {
            let f = passband_edge * f64::from(i) / 90.0;
            worst_ripple = worst_ripple.max(db(&table, f).abs());
        }
        assert!(
            worst_ripple <= 0.001,
            "passband ripple {worst_ripple} dB exceeds 0.001 dB"
        );
        let mut worst_stop = f64::NEG_INFINITY;
        for i in 0..=100 {
            let f = 0.25 + 0.25 * f64::from(i) / 100.0;
            worst_stop = worst_stop.max(db(&table, f));
        }
        assert!(
            worst_stop <= -120.0,
            "stopband leakage {worst_stop} dB is above -120 dB"
        );
    }

    #[test]
    fn minimum_phase_keeps_unity_dc_and_moves_energy_to_the_front() {
        let linear_spec = spec(4, 1, FilterPhase::Linear, FilterVariant::Precise);
        let min_spec = spec(4, 1, FilterPhase::Minimum, FilterVariant::Precise);
        let linear = design_coefficients(linear_spec);
        let minimum = design_coefficients(min_spec);
        assert_eq!(minimum.len(), 2 * linear.len());
        let sum: f64 = minimum.iter().sum();
        assert!((sum - 1.0).abs() < 1e-12, "minimum-phase DC gain {sum}");
        let energy = |h: &[f64]| -> f64 {
            let total: f64 = h.iter().map(|c| c * c).sum();
            let first: f64 = h[..h.len() / 4].iter().map(|c| c * c).sum();
            first / total
        };
        assert!(
            energy(&minimum) > energy(&linear) + 0.1,
            "minimum phase did not front-load energy"
        );
        // Same magnitude response within the cepstral truncation error.
        let linear =
            FilterTable::from_prototype(linear_spec, FilterSourceKind::Sonder, linear).unwrap();
        let minimum =
            FilterTable::from_prototype(min_spec, FilterSourceKind::Sonder, minimum).unwrap();
        // 4x passband ends at 0.1125 cycles/sample; beyond it both responses are stopband.
        for i in 0..=50 {
            let f = 0.11 * f64::from(i) / 50.0;
            let delta = db(&linear, f) - db(&minimum, f);
            assert!(delta.abs() < 0.1, "response differs by {delta} dB at {f}");
        }
    }

    #[test]
    fn long_minimum_phase_specs_are_served_linear_and_huge_ratios_are_refused() {
        let source = SonderDesigned::new(None);
        // 11127 Hz → 44.1 kHz reduces to 14700/3709: millions of taps.
        let huge = spec(14_700, 3_709, FilterPhase::Minimum, FilterVariant::Precise);
        assert!(linear_taps(huge) > MAX_MINIMUM_PHASE_TAPS);
        // A short-variant 6001x stage is too long for the cepstral FFT but cheap to design.
        let long = spec(6_001, 1, FilterPhase::Minimum, FilterVariant::Short);
        let taps = linear_taps(long);
        assert!(taps > MAX_MINIMUM_PHASE_TAPS && taps < 1_000_000, "{taps}");
        let table = source.get(long).expect("linear-phase fallback");
        assert_eq!(table.spec().phase, FilterPhase::Linear);
        assert_eq!(table.coefficients().len(), taps);

        // 44.1 kHz → 44.101 kHz reduces to 44101/44100: beyond the tap limit.
        let refused = spec(44_101, 44_100, FilterPhase::Linear, FilterVariant::Precise);
        assert!(linear_taps(refused) > crate::filter_source::MAX_TAPS);
        assert!(source.get(refused).is_none());
    }

    #[test]
    fn disk_cache_round_trips_coefficients() {
        let dir = std::env::temp_dir().join(format!("sonder-filter-cache-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let source = SonderDesigned::new(Some(dir.clone()));
        let spec = spec(3, 2, FilterPhase::Linear, FilterVariant::Smooth);
        let designed = source.get(spec).unwrap();
        let cached = read_cached(&dir, spec).expect("cache file written");
        assert_eq!(cached, designed.coefficients());

        // A fresh source reads the same table from disk.
        let reopened = SonderDesigned::new(Some(dir.clone()));
        let loaded = reopened.get(spec).unwrap();
        assert_eq!(loaded.coefficients(), designed.coefficients());
        let _ = fs::remove_dir_all(&dir);
    }
}
