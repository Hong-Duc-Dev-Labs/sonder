//! User settings, stored as TOML in the OS config directory.

use std::fs;
use std::io::ErrorKind;
#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::dto::{CoverStyle, GlassQuality, SortOrder};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, Type)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    /// Backend id of the output device; `None` uses the system default device.
    pub output_device: Option<String>,
    /// Software volume in dB; `None` keeps output bit-perfect.
    pub volume_db: Option<f64>,
    pub album_sort: SortOrder,
    /// How Now Playing shows the cover.
    pub cover_style: CoverStyle,
    /// How much frosted glass the UI draws.
    pub glass: GlassQuality,
}

impl Settings {
    /// Clamps values the UI or a hand-edited file could put out of range.
    pub fn sanitized(mut self) -> Self {
        self.volume_db = self
            .volume_db
            .filter(|db| db.is_finite())
            .map(|db| db.clamp(-90.0, 0.0));
        self.output_device = self.output_device.filter(|d| !d.is_empty());
        self
    }
}

pub struct SettingsStore {
    path: PathBuf,
}

impl SettingsStore {
    pub fn new(path: PathBuf) -> Self {
        SettingsStore { path }
    }

    /// Reads the settings; a missing file gives the defaults. A file that does not parse is
    /// moved aside (so the next save does not destroy it) and the defaults are used.
    pub fn load(&self) -> Settings {
        match fs::read_to_string(&self.path) {
            Ok(text) => match toml::from_str::<Settings>(&text) {
                Ok(settings) => settings.sanitized(),
                Err(err) => {
                    eprintln!("sonder: ignoring unreadable {}: {err}", self.path.display());
                    let _ = fs::rename(&self.path, self.path.with_extension("toml.broken"));
                    Settings::default()
                }
            },
            Err(err) if err.kind() == ErrorKind::NotFound => Settings::default(),
            Err(err) => {
                eprintln!("sonder: cannot read {}: {err}", self.path.display());
                Settings::default()
            }
        }
    }

    /// Writes atomically: a crash mid-write leaves the previous file intact.
    pub fn save(&self, settings: &Settings) -> std::io::Result<()> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        let text = toml::to_string_pretty(settings).map_err(std::io::Error::other)?;
        let tmp = self.path.with_extension("toml.tmp");
        fs::write(&tmp, text)?;
        fs::rename(&tmp, &self.path)
    }

    #[cfg(test)]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_defaults_missing_fields() {
        let dir = std::env::temp_dir().join(format!("sonder-settings-{}", std::process::id()));
        let store = SettingsStore::new(dir.join("settings.toml"));
        let _ = fs::remove_file(store.path());
        assert_eq!(store.load(), Settings::default());

        let settings = Settings {
            output_device: Some("{0.0.0.00000000}.{abc}".into()),
            volume_db: Some(-6.5),
            album_sort: SortOrder::Added,
            cover_style: CoverStyle::Square,
            glass: GlassQuality::Off,
        };
        store.save(&settings).unwrap();
        assert_eq!(store.load(), settings);

        fs::write(store.path(), "albumSort = \"year\"\n").unwrap();
        assert_eq!(
            store.load(),
            Settings {
                album_sort: SortOrder::Year,
                ..Settings::default()
            }
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn broken_file_is_kept_aside_and_out_of_range_values_are_clamped() {
        let dir = std::env::temp_dir().join(format!("sonder-settings-bad-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let store = SettingsStore::new(dir.join("settings.toml"));
        fs::write(store.path(), "volumeDb = [").unwrap();
        assert_eq!(store.load(), Settings::default());
        assert!(dir.join("settings.toml.broken").is_file());

        fs::write(store.path(), "volumeDb = 12.0\noutputDevice = \"\"\n").unwrap();
        let loaded = store.load();
        assert_eq!(loaded.volume_db, Some(0.0));
        assert_eq!(loaded.output_device, None);
        let _ = fs::remove_dir_all(dir);
    }
}
