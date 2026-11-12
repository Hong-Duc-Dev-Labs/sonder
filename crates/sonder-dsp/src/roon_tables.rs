//! Loads Roon's polyphase tables from a user-configured folder.
//!
//! The files are CSV (`index,coefficient`, one coefficient per line) named
//! `polyphase_<ratio>_<phase>_<variant>.csv`, with an optional `filters_manifest.json`
//! listing each table's tap count and sum. Tables arrive from the user, never from git or a
//! release bundle; this loader validates them, zero-pads to a multiple of `L` and scales
//! each branch by `L`. A missing folder or table is not an error: the resolver falls back to
//! Sonder's design.
//!
//! Coefficients are never logged or included in error messages, only the file name and the
//! reason a table was rejected.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use serde::Deserialize;

use crate::filter_source::{
    FilterError, FilterPhase, FilterSource, FilterSourceKind, FilterSpec, FilterTable, MAX_TAPS,
};

#[derive(Debug, Clone, Deserialize)]
struct ManifestEntry {
    #[allow(dead_code)]
    name: String,
    taps: usize,
    #[serde(default)]
    sum: Option<f64>,
}

/// Roon tables in `dir`, loaded lazily per ratio.
pub struct RoonTables {
    dir: PathBuf,
    manifest: OnceLock<HashMap<String, ManifestEntry>>,
}

impl RoonTables {
    /// Validates that `dir` exists and is a directory. A missing folder is reported to the
    /// caller, which warns the user and falls back to Sonder.
    pub fn new(dir: impl Into<PathBuf>) -> Result<Self, FilterError> {
        let dir = dir.into();
        if !dir.is_dir() {
            return Err(FilterError::BadTable(format!(
                "the Roon filter folder {} does not exist",
                dir.display()
            )));
        }
        Ok(Self {
            dir,
            manifest: OnceLock::new(),
        })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn manifest(&self) -> &HashMap<String, ManifestEntry> {
        self.manifest.get_or_init(|| {
            let path = self.dir.join("filters_manifest.json");
            let Ok(text) = fs::read_to_string(path) else {
                return HashMap::new();
            };
            let Ok(entries) = serde_json::from_str::<Vec<ManifestEntry>>(&text) else {
                return HashMap::new();
            };
            let mut map = HashMap::new();
            for entry in entries {
                let key = entry.name.to_ascii_lowercase();
                let csv = if key.ends_with(".csv") {
                    key.clone()
                } else {
                    format!("{key}.csv")
                };
                map.insert(key, entry.clone());
                map.insert(csv, entry);
            }
            map
        })
    }

    fn parse(&self, spec: FilterSpec, file: &str) -> Option<FilterTable> {
        let path = self.dir.join(file);
        // A coefficient line is far shorter than 48 bytes; reject oversized files before
        // reading them into memory.
        let size = fs::metadata(&path).ok()?.len();
        if size > (MAX_TAPS as u64).saturating_mul(48) {
            return None;
        }
        let text = fs::read_to_string(&path).ok()?;
        let mut coeffs = Vec::new();
        let mut next_index = 0usize;
        for (line_no, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if line_no == 0 && line.starts_with("index") {
                continue;
            }
            let (index, value) = line.split_once(',')?;
            let parsed_index: usize = index.trim().parse().ok()?;
            if parsed_index != next_index {
                return None;
            }
            next_index += 1;
            let value: f64 = value.trim().parse().ok()?;
            if !value.is_finite() {
                return None;
            }
            if coeffs.len() == MAX_TAPS {
                return None;
            }
            coeffs.push(value);
        }
        if coeffs.is_empty() {
            return None;
        }

        if let Some(entry) = self.manifest().get(&file.to_ascii_lowercase()) {
            if entry.taps != coeffs.len() {
                return None;
            }
            if let Some(sum) = entry.sum
                && (sum - 1.0).abs() > crate::filter_source::SUM_TOLERANCE
            {
                return None;
            }
        }
        FilterTable::from_prototype(spec, FilterSourceKind::Roon, coeffs).ok()
    }
}

impl FilterSource for RoonTables {
    fn kind(&self) -> FilterSourceKind {
        FilterSourceKind::Roon
    }

    fn get(&self, spec: FilterSpec) -> Option<Arc<FilterTable>> {
        let phase = match spec.phase {
            FilterPhase::Linear => "linear_phase",
            FilterPhase::Minimum => "minimum_phase",
        };
        let file = format!(
            "polyphase_{}_{}_{}.csv",
            spec.roon_stem(),
            phase,
            spec.variant.as_str()
        );
        self.parse(spec, &file).map(Arc::new)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter_source::FilterVariant;

    fn spec(num: u32, den: u32) -> FilterSpec {
        FilterSpec::new(num, den, FilterPhase::Linear, FilterVariant::Precise)
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sonder-roon-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn missing_folder_is_an_error_so_the_caller_can_warn_and_fall_back() {
        let dir = std::env::temp_dir().join("sonder-roon-does-not-exist");
        assert!(RoonTables::new(dir).is_err());
    }

    #[test]
    fn loads_csv_pads_and_scales_by_l() {
        let dir = temp_dir("load");
        // Five coefficients summing to one, for 2x: pad to six and scale each branch by 2.
        let coeffs = [0.1, 0.2, 0.4, 0.2, 0.1];
        let mut text = String::from("index,coefficient\n");
        for (i, c) in coeffs.iter().enumerate() {
            text.push_str(&format!("{i},{c}\n"));
        }
        fs::write(dir.join("polyphase_2x_linear_phase_precise.csv"), text).unwrap();
        let roon = RoonTables::new(&dir).unwrap();
        let table = roon.get(spec(2, 1)).expect("table");
        assert_eq!(table.coefficients().len(), 6);
        assert_eq!(table.taps_per_phase(), 3);
        // Every phase is the matching prototype coefficient scaled by L = 2.
        let k = table.taps_per_phase();
        assert!((table.phases()[0] - 0.1 * 2.0).abs() < 1e-12);
        assert!((table.phases()[k] - 0.2 * 2.0).abs() < 1e-12);
        assert!((table.branch_gain(0) + table.branch_gain(1) - 2.0).abs() < 1e-12);
        assert_eq!(table.source(), FilterSourceKind::Roon);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn manifest_tap_mismatch_and_non_finite_values_are_rejected() {
        let dir = temp_dir("validate");
        let file = "polyphase_16_3x_minimum_phase_smooth.csv";
        fs::write(dir.join(file), "index,coefficient\n0,0.5\n1,0.5\n").unwrap();
        fs::write(
            dir.join("filters_manifest.json"),
            r#"[{"name":"polyphase_16_3x_minimum_phase_smooth","taps":3,"sum":1.0}]"#,
        )
        .unwrap();
        let roon = RoonTables::new(&dir).unwrap();
        let spec = FilterSpec::new(16, 3, FilterPhase::Minimum, FilterVariant::Smooth);
        assert!(roon.get(spec).is_none(), "manifest tap count must match");

        fs::write(dir.join(file), "index,coefficient\n0,0.5\n1,NaN\n").unwrap();
        assert!(
            roon.get(spec).is_none(),
            "non-finite coefficients are invalid"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// Needs the real tables; CI never has them.
    #[test]
    #[ignore = "reads the user's Roon tables from SONDER_ROON_FILTERS"]
    fn real_roon_tables_load_and_have_unity_branches() {
        let Ok(dir) = std::env::var("SONDER_ROON_FILTERS") else {
            return;
        };
        let roon = RoonTables::new(&dir).expect("Roon filter folder");
        for (num, den) in [(2, 1), (3, 2), (8, 1), (160, 147), (147, 20)] {
            let table = roon
                .get(FilterSpec::new(
                    num,
                    den,
                    FilterPhase::Linear,
                    FilterVariant::Precise,
                ))
                .unwrap_or_else(|| panic!("missing table for {num}/{den}"));
            for phase in 0..table.spec().num {
                assert!(
                    (table.branch_gain(phase as usize) - 1.0).abs() < 1e-3,
                    "{num}/{den} branch {phase}: {}",
                    table.branch_gain(phase as usize)
                );
            }
        }
    }
}
