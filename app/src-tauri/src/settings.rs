//! User settings, stored as TOML in the OS config directory.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};
use sonder_output::DeviceId;
use specta::Type;

use crate::dto::{
    CoverStyle, CrossfeedPresetSetting, DsdMode, DsdPcmRate, GlassQuality, PcmToDsd,
    SdmOrderSetting, SortOrder,
};

/// Convolution with an impulse response file.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Type)]
#[serde(default, rename_all = "camelCase")]
pub struct ConvolutionSetting {
    pub enabled: bool,
    /// WAV file holding the response.
    pub path: Option<String>,
}

/// Equal-power crossfade between queue entries.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Type)]
#[serde(default, rename_all = "camelCase")]
pub struct CrossfadeSetting {
    pub enabled: bool,
    /// Fade length in seconds, within the engine's 1–10 s range.
    pub seconds: f64,
}

impl Default for CrossfadeSetting {
    fn default() -> Self {
        Self {
            enabled: false,
            seconds: 4.0,
        }
    }
}

/// Headphone crossfeed.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize, Type)]
#[serde(default, rename_all = "camelCase")]
pub struct CrossfeedSetting {
    pub enabled: bool,
    pub preset: CrossfeedPresetSetting,
}

/// One parametric EQ band as the settings file and the UI store it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Type)]
#[serde(default, rename_all = "camelCase")]
pub struct PeqBandSetting {
    pub kind: BiquadKindSetting,
    pub freq_hz: f64,
    pub gain_db: f64,
    pub q: f64,
    pub enabled: bool,
}

impl Default for PeqBandSetting {
    fn default() -> Self {
        Self {
            kind: BiquadKindSetting::Peak,
            freq_hz: 1_000.0,
            gain_db: 0.0,
            q: 0.707,
            enabled: true,
        }
    }
}

/// The RBJ filter shapes the EQ offers.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum BiquadKindSetting {
    #[default]
    Peak,
    LowShelf,
    HighShelf,
    LowPass,
    HighPass,
    BandPass,
    Notch,
}

/// A whole EQ preset: preamp plus bands.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, Type)]
#[serde(default, rename_all = "camelCase")]
pub struct PeqSetting {
    pub preamp_db: f64,
    pub bands: Vec<PeqBandSetting>,
}

/// Where levelling gets its gain.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum LevelingSetting {
    #[default]
    Off,
    Track,
    Album,
    Auto,
}

/// Filter phase choice for Sonder-designed tables.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum FilterPhaseSetting {
    #[default]
    Linear,
    Minimum,
}

/// Filter sharpness choice for Sonder-designed tables.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum FilterVariantSetting {
    #[default]
    Precise,
    Smooth,
}

/// Which tables serve a rate conversion.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum FilterSourceSetting {
    /// Sonder designs its own filters at runtime (shipped).
    #[default]
    Sonder,
    /// Roon's tables from a folder the user owns.
    Roon,
}

/// Where an enabled rate conversion sends the signal.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum SrcTargetSetting {
    /// Never resample unless the device refuses the source rate.
    #[default]
    Off,
    /// The device's highest rate.
    MaxDevice,
    /// The device's highest rate in the source's family.
    KeepFamily,
    /// A fixed rate chosen by the user.
    Fixed,
}

/// Rate-conversion settings (phase 6 policy, edited in the UI from phase 7 on).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(default, rename_all = "camelCase")]
pub struct SrcSetting {
    pub target: SrcTargetSetting,
    /// Target rate used when `target` is `fixed`.
    pub fixed_rate: u32,
    pub filter_source: FilterSourceSetting,
    /// Folder holding Roon's tables, only read when `filter_source` is `roon`.
    pub roon_folder: Option<String>,
    pub phase: FilterPhaseSetting,
    pub variant: FilterVariantSetting,
    pub force_upsample_below_44k: bool,
    /// Resample when the device refuses the source rate (the "not bit-perfect" fallback).
    pub auto_fallback: bool,
    /// Gain inserted before resampling; `None` applies none.
    pub headroom_db: Option<f64>,
}

impl Default for SrcSetting {
    fn default() -> Self {
        Self {
            target: SrcTargetSetting::Off,
            fixed_rate: 192_000,
            filter_source: FilterSourceSetting::Sonder,
            roon_folder: None,
            phase: FilterPhaseSetting::Linear,
            variant: FilterVariantSetting::Precise,
            force_upsample_below_44k: false,
            auto_fallback: true,
            headroom_db: Some(-3.0),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
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
    /// Look up lyrics online (title, artist, album and length are sent) when a track has
    /// none of its own.
    pub lyrics_online: bool,
    /// DSD handling per output device id; devices without an entry use DoP.
    pub dsd_modes: BTreeMap<String, DsdMode>,
    /// Output rate when DSD is converted to PCM.
    pub dsd_pcm_rate: DsdPcmRate,
    /// Modulate PCM tracks to DSD for this zone.
    pub pcm_to_dsd: PcmToDsd,
    /// Sigma-delta modulator order for PCM→DSD.
    pub sdm_order: SdmOrderSetting,
    /// Convolution with an impulse response (runs before the EQ).
    pub convolution: ConvolutionSetting,
    /// Parametric EQ preset in use.
    pub peq: PeqSetting,
    /// Headphone crossfeed (runs after the EQ).
    pub crossfeed: CrossfeedSetting,
    /// Equal-power crossfade between entries; off keeps transitions gapless.
    pub crossfade: CrossfadeSetting,
    /// Saved EQ presets, by name.
    pub peq_presets: BTreeMap<String, PeqSetting>,
    /// Name of the preset `peq` came from, when it has not been edited since.
    pub peq_preset: Option<String>,
    /// Volume levelling source.
    pub leveling: LevelingSetting,
    /// Integrated loudness levelling aims for.
    pub leveling_target_lufs: f64,
    /// Add TPDF dither when the device container is narrower than the processed signal.
    pub dither: bool,
    /// Rate-conversion policy, filter source and headroom.
    pub src: SrcSetting,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            output_device: None,
            volume_db: None,
            album_sort: SortOrder::default(),
            cover_style: CoverStyle::default(),
            glass: GlassQuality::default(),
            lyrics_online: true,
            dsd_modes: BTreeMap::new(),
            dsd_pcm_rate: DsdPcmRate::default(),
            pcm_to_dsd: PcmToDsd::default(),
            sdm_order: SdmOrderSetting::default(),
            convolution: ConvolutionSetting::default(),
            peq: PeqSetting::default(),
            crossfeed: CrossfeedSetting::default(),
            crossfade: CrossfadeSetting::default(),
            peq_presets: BTreeMap::new(),
            peq_preset: None,
            leveling: LevelingSetting::default(),
            leveling_target_lufs: -18.0,
            dither: true,
            src: SrcSetting::default(),
        }
    }
}

impl Settings {
    /// Clamps values the UI or a hand-edited file could put out of range.
    pub fn sanitized(mut self) -> Self {
        self.volume_db = self
            .volume_db
            .filter(|db| db.is_finite())
            .map(|db| db.clamp(-90.0, 0.0));
        self.output_device = self.output_device.filter(|d| !d.is_empty());
        self.dsd_modes.retain(|device, _| !device.is_empty());
        self.leveling_target_lufs = self.leveling_target_lufs.clamp(
            sonder_engine::MIN_TARGET_LUFS,
            sonder_engine::MAX_TARGET_LUFS,
        );
        self.peq = sanitized_peq(self.peq);
        self.peq_presets.retain(|name, _| !name.is_empty());
        let presets = std::mem::take(&mut self.peq_presets);
        self.peq_presets = presets
            .into_iter()
            .map(|(name, preset)| (name, sanitized_peq(preset)))
            .collect();
        self.peq_preset = self
            .peq_preset
            .filter(|name| self.peq_presets.contains_key(name));
        self.src.fixed_rate = self.src.fixed_rate.clamp(8_000, 768_000);
        self.src.headroom_db = self
            .src
            .headroom_db
            .filter(|db| db.is_finite())
            .map(|db| db.clamp(-60.0, 0.0));
        self.src.roon_folder = self.src.roon_folder.filter(|dir| !dir.is_empty());
        self.convolution.path = self.convolution.path.filter(|path| !path.is_empty());
        self.crossfade.seconds = sonder_engine::CrossfadeConfig {
            enabled: self.crossfade.enabled,
            seconds: self.crossfade.seconds,
        }
        .sanitized()
        .seconds;
        self
    }

    /// The per-device DSD modes in engine terms.
    pub fn engine_dsd_modes(&self) -> HashMap<DeviceId, sonder_engine::DsdMode> {
        self.dsd_modes
            .iter()
            .map(|(device, mode)| (DeviceId(device.clone()), (*mode).into()))
            .collect()
    }

    /// Applies the DSD settings to the engine; the engine ignores unchanged values.
    pub fn apply_dsd(&self, player: &sonder_engine::Player) {
        player.set_dsd(
            DsdMode::default().into(),
            self.engine_dsd_modes(),
            self.dsd_pcm_rate.into(),
        );
    }

    /// The PCM→DSD settings in engine terms.
    pub fn engine_sdm(&self) -> sonder_engine::SdmConfig {
        sonder_engine::SdmConfig {
            mode: self.pcm_to_dsd.into(),
            order: self.sdm_order.into(),
        }
    }

    /// Applies the PCM→DSD output mode to the engine.
    pub fn apply_sdm(&self, player: &sonder_engine::Player) {
        player.set_sdm(self.engine_sdm());
    }

    /// The crossfade settings in engine terms.
    pub fn engine_crossfade(&self) -> sonder_engine::CrossfadeConfig {
        sonder_engine::CrossfadeConfig {
            enabled: self.crossfade.enabled,
            seconds: self.crossfade.seconds,
        }
        .sanitized()
    }

    /// Applies the crossfade settings to the running engine.
    pub fn apply_crossfade(&self, player: &sonder_engine::Player) {
        player.set_crossfade(self.engine_crossfade());
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
            lyrics_online: false,
            dsd_modes: BTreeMap::from([("hw:1,0".to_string(), DsdMode::Native)]),
            dsd_pcm_rate: DsdPcmRate::Hz352800,
            ..Settings::default()
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

        fs::write(
            store.path(),
            "volumeDb = 12.0\noutputDevice = \"\"\n[dsdModes]\n\"\" = \"pcm\"\n",
        )
        .unwrap();
        let loaded = store.load();
        assert_eq!(loaded.volume_db, Some(0.0));
        assert_eq!(loaded.output_device, None);
        assert!(loaded.dsd_modes.is_empty());
        let _ = fs::remove_dir_all(dir);
    }
}

// ----- engine mappings -----

impl From<BiquadKindSetting> for sonder_dsp::BiquadKind {
    fn from(kind: BiquadKindSetting) -> Self {
        match kind {
            BiquadKindSetting::Peak => Self::Peak,
            BiquadKindSetting::LowShelf => Self::LowShelf,
            BiquadKindSetting::HighShelf => Self::HighShelf,
            BiquadKindSetting::LowPass => Self::LowPass,
            BiquadKindSetting::HighPass => Self::HighPass,
            BiquadKindSetting::BandPass => Self::BandPass,
            BiquadKindSetting::Notch => Self::Notch,
        }
    }
}

impl From<sonder_dsp::BiquadKind> for BiquadKindSetting {
    fn from(kind: sonder_dsp::BiquadKind) -> Self {
        match kind {
            sonder_dsp::BiquadKind::Peak => Self::Peak,
            sonder_dsp::BiquadKind::LowShelf => Self::LowShelf,
            sonder_dsp::BiquadKind::HighShelf => Self::HighShelf,
            sonder_dsp::BiquadKind::LowPass => Self::LowPass,
            sonder_dsp::BiquadKind::HighPass => Self::HighPass,
            sonder_dsp::BiquadKind::BandPass => Self::BandPass,
            sonder_dsp::BiquadKind::Notch => Self::Notch,
        }
    }
}

impl From<PeqBandSetting> for sonder_dsp::PeqBand {
    fn from(band: PeqBandSetting) -> Self {
        Self {
            kind: band.kind.into(),
            freq_hz: band.freq_hz,
            gain_db: band.gain_db,
            q: band.q,
            enabled: band.enabled,
        }
    }
}

impl From<sonder_dsp::PeqBand> for PeqBandSetting {
    fn from(band: sonder_dsp::PeqBand) -> Self {
        Self {
            kind: band.kind.into(),
            freq_hz: band.freq_hz,
            gain_db: band.gain_db,
            q: band.q,
            enabled: band.enabled,
        }
    }
}

impl From<PeqSetting> for sonder_dsp::Peq {
    fn from(peq: PeqSetting) -> Self {
        Self {
            preamp_db: peq.preamp_db,
            bands: peq.bands.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<sonder_dsp::Peq> for PeqSetting {
    fn from(peq: sonder_dsp::Peq) -> Self {
        Self {
            preamp_db: peq.preamp_db,
            bands: peq.bands.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<LevelingSetting> for sonder_dsp::LevelingMode {
    fn from(mode: LevelingSetting) -> Self {
        match mode {
            LevelingSetting::Off => Self::Off,
            LevelingSetting::Track => Self::Track,
            LevelingSetting::Album => Self::Album,
            LevelingSetting::Auto => Self::Auto,
        }
    }
}

impl From<FilterPhaseSetting> for sonder_dsp::FilterPhase {
    fn from(phase: FilterPhaseSetting) -> Self {
        match phase {
            FilterPhaseSetting::Linear => Self::Linear,
            FilterPhaseSetting::Minimum => Self::Minimum,
        }
    }
}

impl From<FilterVariantSetting> for sonder_dsp::FilterVariant {
    fn from(variant: FilterVariantSetting) -> Self {
        match variant {
            FilterVariantSetting::Precise => Self::Precise,
            FilterVariantSetting::Smooth => Self::Smooth,
        }
    }
}

impl From<SrcSetting> for sonder_engine::SrcConfig {
    fn from(src: SrcSetting) -> Self {
        Self {
            target: match src.target {
                SrcTargetSetting::Off => sonder_engine::SrcTarget::Off,
                SrcTargetSetting::MaxDevice => sonder_engine::SrcTarget::MaxDevice,
                SrcTargetSetting::KeepFamily => sonder_engine::SrcTarget::KeepFamily,
                SrcTargetSetting::Fixed => sonder_engine::SrcTarget::Fixed(src.fixed_rate),
            },
            filter_source: match (src.filter_source, src.roon_folder) {
                (FilterSourceSetting::Roon, Some(dir)) if !dir.is_empty() => {
                    sonder_engine::FilterSourceConfig::Roon { dir: dir.into() }
                }
                _ => sonder_engine::FilterSourceConfig::Sonder,
            },
            phase: src.phase.into(),
            variant: src.variant.into(),
            force_upsample_below_44k: src.force_upsample_below_44k,
            headroom_db: src.headroom_db,
            auto_fallback: src.auto_fallback,
        }
    }
}

impl Settings {
    /// The engine's view of the EQ, levelling and dither settings.
    pub fn engine_dsp(&self) -> sonder_engine::DspConfig {
        sonder_engine::DspConfig {
            // The file is read here (and cached while unchanged); a failure reaches the
            // engine, which turns the stage off and says why.
            convolution: self
                .convolution
                .path
                .as_deref()
                .filter(|_| self.convolution.enabled)
                .map(|path| sonder_engine::ImpulseResponse::load_cached(Path::new(path))),
            peq: Some(self.peq.clone().into()).filter(|peq: &sonder_dsp::Peq| peq.active()),
            crossfeed: self.crossfeed.enabled.then(|| self.crossfeed.preset.into()),
            leveling: self.leveling.into(),
            leveling_target_lufs: self.leveling_target_lufs,
            dither: self.dither,
        }
        .sanitized()
    }

    /// Applies EQ, levelling and dither to the running engine.
    pub fn apply_dsp(&self, player: &sonder_engine::Player) {
        player.set_dsp(self.engine_dsp());
    }

    /// Applies the rate-conversion settings to the running engine.
    pub fn apply_src(&self, player: &sonder_engine::Player) {
        player.set_src(self.src.clone().into());
    }

    /// The engine's view of the rate-conversion settings.
    pub fn engine_src(&self) -> sonder_engine::SrcConfig {
        self.src.clone().into()
    }
}

/// Clamps a preset the settings file or the UI could put out of range.
fn sanitized_peq(peq: PeqSetting) -> PeqSetting {
    sonder_dsp::Peq::from(peq).sanitized().into()
}

#[cfg(test)]
mod dsp_settings_tests {
    use super::*;

    #[test]
    fn engine_dsp_drops_an_inactive_preset_and_clamps_the_target() {
        let settings = Settings {
            leveling_target_lufs: -99.0,
            peq: PeqSetting {
                preamp_db: 0.0,
                bands: vec![PeqBandSetting {
                    enabled: false,
                    ..PeqBandSetting::default()
                }],
            },
            ..Settings::default()
        }
        .sanitized();
        let dsp = settings.engine_dsp();
        assert!(dsp.peq.is_none());
        assert_eq!(dsp.leveling_target_lufs, sonder_engine::MIN_TARGET_LUFS);
    }

    #[test]
    fn convolution_and_crossfeed_reach_the_engine_only_when_switched_on() {
        let mut settings = Settings {
            convolution: ConvolutionSetting {
                enabled: false,
                path: Some("no/such/response.wav".into()),
            },
            crossfeed: CrossfeedSetting {
                enabled: false,
                preset: CrossfeedPresetSetting::ChuMoy,
            },
            ..Settings::default()
        };
        let dsp = settings.engine_dsp();
        assert!(dsp.convolution.is_none());
        assert!(dsp.crossfeed.is_none());

        settings.convolution.enabled = true;
        settings.crossfeed.enabled = true;
        let dsp = settings.engine_dsp();
        // A missing file reaches the engine as a reason, not as a silent "off".
        assert!(matches!(dsp.convolution, Some(Err(_))));
        assert_eq!(dsp.crossfeed, Some(sonder_engine::CrossfeedPreset::ChuMoy));
    }

    #[test]
    fn crossfade_defaults_off_and_is_clamped_to_what_the_engine_accepts() {
        let settings: Settings = toml::from_str("dither = true\n").unwrap();
        assert_eq!(settings.crossfade, CrossfadeSetting::default());
        assert_eq!(settings.engine_crossfade().seconds, 4.0);

        let edited = Settings {
            crossfade: CrossfadeSetting {
                enabled: true,
                seconds: 30.0,
            },
            ..Settings::default()
        }
        .sanitized();
        assert_eq!(
            edited.engine_crossfade().seconds,
            sonder_engine::MAX_CROSSFADE_SECONDS
        );
        assert!(edited.engine_crossfade().is_on());

        let short = Settings {
            crossfade: CrossfadeSetting {
                enabled: true,
                seconds: 0.2,
            },
            ..Settings::default()
        }
        .sanitized();
        assert_eq!(
            short.engine_crossfade().seconds,
            sonder_engine::MIN_CROSSFADE_SECONDS
        );
        assert!(!settings.crossfade.enabled);
    }

    #[test]
    fn a_settings_file_from_before_convolution_loads_with_both_off() {
        let settings: Settings = toml::from_str("dither = false\n").unwrap();
        assert_eq!(settings.convolution, ConvolutionSetting::default());
        assert_eq!(settings.crossfeed, CrossfeedSetting::default());
        assert!(!settings.dither);
        // An empty path from a hand-edited file means no file.
        let edited: Settings =
            toml::from_str("[convolution]\nenabled = true\npath = \"\"\n").unwrap();
        assert_eq!(edited.sanitized().convolution.path, None);
    }

    #[test]
    fn src_settings_map_every_policy_onto_the_engine() {
        let src = SrcSetting {
            target: SrcTargetSetting::Fixed,
            fixed_rate: 96_000,
            filter_source: FilterSourceSetting::Roon,
            roon_folder: Some("D:/filters".into()),
            phase: FilterPhaseSetting::Minimum,
            variant: FilterVariantSetting::Smooth,
            force_upsample_below_44k: true,
            auto_fallback: false,
            headroom_db: Some(-4.0),
        };
        let engine: sonder_engine::SrcConfig = src.into();
        assert_eq!(engine.target, sonder_engine::SrcTarget::Fixed(96_000));
        assert_eq!(engine.phase, sonder_dsp::FilterPhase::Minimum);
        assert_eq!(engine.variant, sonder_dsp::FilterVariant::Smooth);
        assert!(engine.force_upsample_below_44k);
        assert!(!engine.auto_fallback);
        assert_eq!(engine.headroom_db, Some(-4.0));
        assert!(matches!(
            engine.filter_source,
            sonder_engine::FilterSourceConfig::Roon { .. }
        ));

        // A Roon choice without a folder falls back to Sonder instead of failing.
        let no_folder: sonder_engine::SrcConfig = SrcSetting::default().into();
        assert!(matches!(
            no_folder.filter_source,
            sonder_engine::FilterSourceConfig::Sonder
        ));
    }

    #[test]
    fn a_preset_round_trips_through_the_engine_type() {
        let preset = PeqSetting {
            preamp_db: -6.0,
            bands: vec![PeqBandSetting {
                kind: BiquadKindSetting::HighShelf,
                freq_hz: 8_000.0,
                gain_db: 3.5,
                q: 0.7,
                enabled: true,
            }],
        };
        let back = PeqSetting::from(sonder_dsp::Peq::from(preset.clone()));
        assert_eq!(back, preset);
        assert_eq!(sanitized_peq(preset).preamp_db, -6.0);
    }
}
