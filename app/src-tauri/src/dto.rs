//! Types shared with the web UI. They mirror the engine and library types in the shape the
//! UI needs and derive `specta::Type`, so `src/lib/bindings.ts` is generated from them.

use std::io::Read;
use std::path::Path;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde::{Deserialize, Serialize};
use specta::Type;

use sonder_decode::SourceFormat;
use sonder_library::{
    AlbumDetail, AlbumSort, AlbumSummary, ArtistDetail, ArtistSummary, PlaylistDetail,
    PlaylistSummary, Track,
};
use sonder_output::{DeviceFormat, DeviceInfo};

use crate::settings::PeqSetting;

/// Error returned by every command: a message the UI can show.
#[derive(Debug, Clone, Serialize, Type)]
pub struct AppError(pub String);

impl<E: std::fmt::Display> From<E> for AppError {
    fn from(e: E) -> Self {
        AppError(e.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum SortOrder {
    #[default]
    Artist,
    Title,
    Added,
    Year,
}

/// How Now Playing shows the cover.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum CoverStyle {
    /// The cover as the label of a spinning record, under a tonearm.
    #[default]
    Vinyl,
    /// The flat square cover.
    Square,
    /// Stage lyrics visualizer with Flow motion.
    Stage,
}

/// How much frosted glass the UI draws. Blur is the costliest effect in the UI; `Off` keeps the
/// same layout with opaque surfaces.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum GlassQuality {
    #[default]
    Full,
    /// Lighter blur and no grain, for slower graphics.
    Reduced,
    /// Opaque surfaces, no blur.
    Off,
}

/// DSD output for PCM sources.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum PcmToDsd {
    #[default]
    Off,
    Dsd64,
    Dsd128,
    Dsd256,
}

impl From<PcmToDsd> for sonder_engine::SdmMode {
    fn from(mode: PcmToDsd) -> Self {
        match mode {
            PcmToDsd::Off => sonder_engine::SdmMode::Off,
            PcmToDsd::Dsd64 => sonder_engine::SdmMode::Dsd64,
            PcmToDsd::Dsd128 => sonder_engine::SdmMode::Dsd128,
            PcmToDsd::Dsd256 => sonder_engine::SdmMode::Dsd256,
        }
    }
}

/// Sigma-delta modulator order for PCM→DSD.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum SdmOrderSetting {
    #[default]
    Order5,
    Order7,
}

impl From<SdmOrderSetting> for sonder_engine::SdmOrder {
    fn from(order: SdmOrderSetting) -> Self {
        match order {
            SdmOrderSetting::Order5 => sonder_engine::SdmOrder::Order5,
            SdmOrderSetting::Order7 => sonder_engine::SdmOrder::Order7,
        }
    }
}

/// Headphone crossfeed presets (libbs2b's).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum CrossfeedPresetSetting {
    /// 700 Hz, 4.5 dB.
    #[default]
    Default,
    /// 700 Hz, 6.0 dB.
    ChuMoy,
    /// 650 Hz, 9.5 dB.
    JanMeier,
}

impl From<CrossfeedPresetSetting> for sonder_engine::CrossfeedPreset {
    fn from(preset: CrossfeedPresetSetting) -> Self {
        match preset {
            CrossfeedPresetSetting::Default => sonder_engine::CrossfeedPreset::Default,
            CrossfeedPresetSetting::ChuMoy => sonder_engine::CrossfeedPreset::ChuMoy,
            CrossfeedPresetSetting::JanMeier => sonder_engine::CrossfeedPreset::JanMeier,
        }
    }
}

/// What Settings shows about an impulse response file.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ImpulseResponseInfo {
    pub path: String,
    pub name: String,
    pub rate: u32,
    pub channels: u32,
    pub seconds: f64,
    /// Highest gain of the response; a positive value is the headroom it needs.
    pub peak_gain_db: f64,
}

impl ImpulseResponseInfo {
    pub fn describe(path: &str) -> Result<Self, sonder_engine::IrError> {
        let ir = sonder_engine::ImpulseResponse::load_cached(std::path::Path::new(path))?;
        Ok(Self {
            path: path.to_owned(),
            name: ir.name().to_owned(),
            rate: ir.rate(),
            channels: u32::try_from(ir.channel_count()).unwrap_or(u32::MAX),
            seconds: ir.frames() as f64 / f64::from(ir.rate()),
            peak_gain_db: ir.peak_gain_db(),
        })
    }
}

/// What an imported Equalizer APO parametric file yields: a named EQ and what had to change.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PeqImport {
    /// Preset name from the file name: its stem without a trailing ` ParametricEQ`.
    pub name: String,
    pub peq: PeqSetting,
    /// Lines the parser skipped and values it clamped.
    pub warnings: Vec<String>,
}

/// Largest file read: an APO parametric file is a few dozen lines.
const MAX_APO_BYTES: u64 = 256 * 1024;

/// Name AutoEQ appends to a headphone's parametric file.
const APO_FILE_SUFFIX: &str = " ParametricEQ";

impl PeqImport {
    /// Reads and parses `path`; every failure is a message the UI can show.
    pub fn read(path: &Path) -> AppResult<Self> {
        // Read one byte past the cap, so the cap bounds the allocation rather than describing it
        // after the fact (a file can grow between a metadata check and the read).
        let file = std::fs::File::open(path)
            .map_err(|e| AppError(format!("cannot read {}: {e}", path.display())))?;
        let mut bytes = Vec::new();
        file.take(MAX_APO_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| AppError(format!("cannot read {}: {e}", path.display())))?;
        if bytes.len() as u64 > MAX_APO_BYTES {
            return Err(too_large(path));
        }
        // A NUL byte is valid UTF-8 once encoded, so UTF-16 from a Windows editor would
        // otherwise fail later as "not ON/OFF" instead of naming the real problem.
        if bytes.contains(&0) {
            return Err(AppError(format!(
                "{} is not UTF-8 text (a NUL byte suggests UTF-16)",
                path.display()
            )));
        }
        let text = String::from_utf8(bytes)
            .map_err(|_| AppError(format!("{} is not UTF-8 text", path.display())))?;
        Self::parse(preset_name(path), &text)
    }

    /// Parses Equalizer APO parametric `text` into a preset called `name`.
    pub fn parse(name: String, text: &str) -> AppResult<Self> {
        let import = sonder_dsp::Peq::from_apo_text(text)?;
        Ok(Self {
            name,
            peq: import.peq.into(),
            warnings: import.warnings,
        })
    }
}

/// One AutoEQ result: a headphone as one source measured it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AutoEqResult {
    pub name: String,
    /// Who measured it (oratory1990, crinacle, Rtings…).
    pub source: String,
    /// The measurement rig, when the source used several.
    pub rig: Option<String>,
    /// Folder of the result in the AutoEQ repository; passed back to `autoeq_import`.
    pub path: String,
}

impl From<sonder_autoeq::Entry> for AutoEqResult {
    fn from(e: sonder_autoeq::Entry) -> Self {
        Self {
            name: e.name,
            source: e.source,
            rig: e.rig,
            path: e.path,
        }
    }
}

impl From<AutoEqResult> for sonder_autoeq::Entry {
    fn from(r: AutoEqResult) -> Self {
        Self {
            name: r.name,
            source: r.source,
            rig: r.rig,
            path: r.path,
        }
    }
}

/// The message for a file past [`MAX_APO_BYTES`].
fn too_large(path: &Path) -> AppError {
    AppError(format!(
        "{} is larger than {} KB, so it is not an Equalizer APO parametric file",
        path.display(),
        MAX_APO_BYTES / 1024
    ))
}

/// Longest preset name kept in the suggested file name.
const MAX_NAME_CHARS: usize = 100;

/// The file name the export dialog starts with: `<preset> ParametricEQ.txt`, with the characters
/// Windows forbids in a file name replaced. A name the shell rejects makes the dialog report
/// nothing at all (`rfd` folds the error into "no file chosen"), so `Export…` would look dead.
pub fn export_file_name(preset: &str) -> String {
    format!("{}{APO_FILE_SUFFIX}.txt", safe_file_stem(preset))
}

/// `name` as a file name stem every OS accepts: forbidden characters replaced, trailing dots
/// and spaces dropped, at most [`MAX_NAME_CHARS`] characters, "Sonder" when nothing is left.
pub fn safe_file_stem(name: &str) -> String {
    let mut base = String::with_capacity(name.len());
    for c in name.trim().chars().take(MAX_NAME_CHARS) {
        base.push(match c {
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
            c if c.is_control() => '-',
            c => c,
        });
    }
    let base = base.trim_end_matches(['.', ' ']);
    if base.is_empty() { "Sonder" } else { base }.to_owned()
}

/// What an M3U import created.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct M3uImported {
    pub playlist: Playlist,
    /// Entries added, missing ones included.
    pub added: u32,
    /// Added entries whose file is not in the library (yet).
    pub missing: u32,
    /// Web streams and other entries that are not local files.
    pub skipped: u32,
}

/// The preset name a file implies: `Sennheiser HD 600 ParametricEQ.txt` → `Sennheiser HD 600`.
fn preset_name(path: &Path) -> String {
    let stem = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let name = stem.trim();
    let keep = name.len().saturating_sub(APO_FILE_SUFFIX.len());
    let trimmed = if name.len() > APO_FILE_SUFFIX.len()
        && name.is_char_boundary(keep)
        && name[keep..].eq_ignore_ascii_case(APO_FILE_SUFFIX)
    {
        name[..keep].trim_end()
    } else {
        name
    };
    if trimmed.is_empty() {
        "Imported EQ".to_owned()
    } else {
        trimmed.to_owned()
    }
}

/// What plays after a track ends.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum RepeatMode {
    #[default]
    Off,
    /// After the last entry, continue at the first.
    All,
    /// Play the same entry again.
    One,
}

impl From<RepeatMode> for sonder_engine::RepeatMode {
    fn from(mode: RepeatMode) -> Self {
        match mode {
            RepeatMode::Off => sonder_engine::RepeatMode::Off,
            RepeatMode::All => sonder_engine::RepeatMode::All,
            RepeatMode::One => sonder_engine::RepeatMode::One,
        }
    }
}

/// How DSD files reach an output device.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum DsdMode {
    /// DSD over PCM, bit-perfect; converted to PCM when the device does not take DoP.
    #[default]
    Dop,
    /// Native DSD where the driver exposes it (ALSA), else DoP, else PCM.
    Native,
    /// Always converted to PCM.
    Pcm,
}

impl From<DsdMode> for sonder_engine::DsdMode {
    fn from(mode: DsdMode) -> Self {
        match mode {
            DsdMode::Dop => sonder_engine::DsdMode::Dop,
            DsdMode::Native => sonder_engine::DsdMode::Native,
            DsdMode::Pcm => sonder_engine::DsdMode::Pcm,
        }
    }
}

/// Output rate of DSD→PCM conversion.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum DsdPcmRate {
    #[default]
    Hz176400,
    Hz352800,
}

impl From<DsdPcmRate> for sonder_engine::DsdPcmRate {
    fn from(rate: DsdPcmRate) -> Self {
        match rate {
            DsdPcmRate::Hz176400 => sonder_engine::DsdPcmRate::R176_4,
            DsdPcmRate::Hz352800 => sonder_engine::DsdPcmRate::R352_8,
        }
    }
}

impl From<SortOrder> for AlbumSort {
    fn from(s: SortOrder) -> Self {
        match s {
            SortOrder::Artist => AlbumSort::Artist,
            SortOrder::Title => AlbumSort::Title,
            SortOrder::Added => AlbumSort::Added,
            SortOrder::Year => AlbumSort::Year,
        }
    }
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Album {
    pub id: i64,
    pub title: String,
    pub artist: Option<String>,
    pub artist_id: Option<i64>,
    pub year: Option<i32>,
    pub track_count: u32,
    pub duration_ms: u64,
    /// Cover hash for the `sonder-art` scheme.
    pub art: Option<String>,
    /// The cover's dominant vivid colours (`#rrggbb`), most dominant first; may be empty.
    pub art_colors: Vec<String>,
}

impl From<AlbumSummary> for Album {
    fn from(a: AlbumSummary) -> Self {
        Album {
            id: a.id,
            title: a.title,
            artist: a.artist,
            artist_id: a.artist_id,
            year: a.year,
            track_count: a.track_count,
            duration_ms: a.duration_ms,
            art: a.art,
            art_colors: a.art_colors,
        }
    }
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AlbumPage {
    /// Albums in the whole library, for sizing the grid before every page is loaded.
    pub total: u32,
    pub offset: u32,
    pub albums: Vec<Album>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TrackItem {
    pub id: i64,
    pub path: String,
    pub title: String,
    pub artist: Option<String>,
    pub album_artist: Option<String>,
    pub album_id: i64,
    pub album: String,
    pub track_no: Option<u32>,
    pub disc_no: Option<u32>,
    pub year: Option<i32>,
    pub genre: Option<String>,
    pub codec: String,
    pub rate: u32,
    pub bits: Option<u32>,
    pub channels: u16,
    pub duration_ms: u64,
    pub art: Option<String>,
    pub art_colors: Vec<String>,
    /// The cached quality check, when the file has not changed since it ran.
    pub quality: Option<QualityBadge>,
    /// Whether this track is marked as favorite.
    pub favorite: bool,
}

/// A track's cached quality verdict, for the badge in track lists.
#[derive(Debug, Clone, Copy, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct QualityBadge {
    pub verdict: sonder_quality::Verdict,
    pub confidence: sonder_quality::Confidence,
}

impl From<Track> for TrackItem {
    fn from(t: Track) -> Self {
        TrackItem {
            id: t.id,
            path: t.path.to_string_lossy().into_owned(),
            title: t.title,
            artist: t.artist,
            album_artist: t.album_artist,
            album_id: t.album_id,
            album: t.album,
            track_no: t.track_no,
            disc_no: t.disc_no,
            year: t.year,
            genre: t.genre,
            codec: t.audio.codec.as_str().to_owned(),
            rate: t.audio.rate,
            bits: t.audio.bits,
            channels: t.audio.channels,
            duration_ms: t.audio.duration_ms,
            art: t.art,
            art_colors: t.art_colors,
            quality: t.quality.and_then(|q| {
                Some(QualityBadge {
                    verdict: sonder_quality::Verdict::parse(&q.verdict)?,
                    confidence: sonder_quality::Confidence::parse(&q.confidence)?,
                })
            }),
            favorite: t.favorite,
        }
    }
}

/// What the metadata editor shows for a track: the tags in its file and the cover embedded
/// there. A cover the library found beside the file (`folder.jpg`) is not part of it.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TrackTagsInfo {
    pub tags: TrackEdit,
    pub cover: Option<CoverImage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TrackEdit {
    pub title: String,
    pub artists: Vec<String>,
    pub album: String,
    pub album_artists: Vec<String>,
    pub track_no: Option<u32>,
    pub disc_no: Option<u32>,
    pub year: Option<i32>,
    pub genre: String,
    /// The file's lyrics (plain or LRC text). When saving, `null` keeps the file's lyrics as
    /// they are and blank text removes them.
    pub lyrics: Option<String>,
    /// What Save does to the embedded cover; reading always gives `keep`.
    pub cover: CoverChange,
}

/// A change to the cover embedded in a file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum CoverChange {
    Keep,
    /// Removes every embedded picture.
    Remove,
    /// Makes this JPEG or PNG (base64) the front cover.
    Set {
        data: String,
    },
}

/// An image for the UI to show: a JPEG or PNG, base64-encoded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CoverImage {
    pub mime: String,
    pub data: String,
}

impl CoverImage {
    /// `None` when the bytes are not a JPEG or PNG.
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let mime = sonder_library::cover_mime(bytes)?;
        Some(CoverImage {
            mime: mime.to_string(),
            data: BASE64.encode(bytes),
        })
    }
}

impl From<sonder_library::TrackTags> for TrackEdit {
    fn from(tags: sonder_library::TrackTags) -> Self {
        TrackEdit {
            title: tags.title.unwrap_or_default(),
            artists: tags.artists,
            album: tags.album.unwrap_or_default(),
            album_artists: tags.album_artists,
            track_no: tags.track_no,
            disc_no: tags.disc_no,
            year: tags.year,
            genre: tags.genre.unwrap_or_default(),
            lyrics: tags.lyrics,
            cover: CoverChange::Keep,
        }
    }
}

impl TryFrom<TrackEdit> for sonder_library::TagEdit {
    type Error = AppError;

    fn try_from(edit: TrackEdit) -> AppResult<Self> {
        let cover = match edit.cover {
            CoverChange::Keep => sonder_library::CoverEdit::Keep,
            CoverChange::Remove => sonder_library::CoverEdit::Remove,
            CoverChange::Set { data } => sonder_library::CoverEdit::Set(
                BASE64
                    .decode(data.as_bytes())
                    .map_err(|e| AppError(format!("the cover image is not readable: {e}")))?,
            ),
        };
        Ok(sonder_library::TagEdit {
            title: edit.title,
            artists: edit.artists,
            album: edit.album,
            album_artists: edit.album_artists,
            track_no: edit.track_no,
            disc_no: edit.disc_no,
            year: edit.year,
            genre: edit.genre,
            lyrics: edit.lyrics,
            cover,
        })
    }
}

/// What the editor asks online catalogues and lyrics services with: the fields as typed.
#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LookupQuery {
    pub title: String,
    pub artist: String,
    pub album: String,
}

/// One online catalogue's idea of the track, for the editor to fill its fields from.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MetadataCandidate {
    /// "iTunes", "Deezer" or "MusicBrainz".
    pub source: String,
    pub title: String,
    pub artists: Vec<String>,
    pub album: String,
    pub album_artists: Vec<String>,
    pub track_no: Option<u32>,
    pub disc_no: Option<u32>,
    pub year: Option<i32>,
    pub genre: String,
    pub duration_ms: Option<u64>,
    /// Its length is the track's, give or take a few seconds: most likely the same recording.
    pub length_matches: bool,
    /// For `cover_download`.
    pub cover_url: Option<String>,
    pub thumb_url: Option<String>,
}

impl MetadataCandidate {
    pub fn new(c: sonder_metadata::Candidate, duration_ms: u64) -> Self {
        MetadataCandidate {
            length_matches: c.matches_length(duration_ms),
            source: c.source,
            title: c.title,
            artists: c.artists,
            album: c.album.unwrap_or_default(),
            album_artists: c.album_artists,
            track_no: c.track_no,
            disc_no: c.disc_no,
            year: c.year,
            genre: c.genre.unwrap_or_default(),
            duration_ms: c.duration_ms,
            cover_url: c.cover_url,
            thumb_url: c.thumb_url,
        }
    }
}

/// Lyrics found online for the editor, as the text Save would store in the file.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FoundLyrics {
    /// The service's name.
    pub source: String,
    /// The text is LRC, with a time on every line.
    pub synced: bool,
    /// The service says the track has no vocals; the text is empty.
    pub instrumental: bool,
    pub text: String,
}

impl From<sonder_lyrics::Lyrics> for FoundLyrics {
    fn from(l: sonder_lyrics::Lyrics) -> Self {
        FoundLyrics {
            text: sonder_lyrics::lrc::format(&l),
            source: l.source,
            synced: l.synced,
            instrumental: l.instrumental,
        }
    }
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AlbumInfo {
    pub album: Album,
    pub tracks: Vec<TrackItem>,
}

impl From<AlbumDetail> for AlbumInfo {
    fn from(d: AlbumDetail) -> Self {
        AlbumInfo {
            album: d.album.into(),
            tracks: d.tracks.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Playlist {
    pub id: i64,
    pub name: String,
    /// Every entry, including those whose file is not in the library.
    pub item_count: u32,
    /// Total length of the entries whose file is in the library.
    pub duration_ms: u64,
    /// Cover hash of the first entry in the library.
    pub art: Option<String>,
}

impl From<PlaylistSummary> for Playlist {
    fn from(p: PlaylistSummary) -> Self {
        Playlist {
            id: p.id,
            name: p.name,
            item_count: p.item_count,
            duration_ms: p.duration_ms,
            art: p.art,
        }
    }
}

/// A playlist entry whose file is in the library.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistEntry {
    /// Index among all entries, missing ones included; what remove and move take.
    pub position: u32,
    pub track: TrackItem,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistInfo {
    pub playlist: Playlist,
    pub entries: Vec<PlaylistEntry>,
    /// Entries whose file is not in the library (moved, deleted or not yet rescanned).
    pub missing: u32,
}

impl From<PlaylistDetail> for PlaylistInfo {
    fn from(d: PlaylistDetail) -> Self {
        PlaylistInfo {
            playlist: d.playlist.into(),
            entries: d
                .items
                .into_iter()
                .map(|i| PlaylistEntry {
                    position: i.position,
                    track: i.track.into(),
                })
                .collect(),
            missing: d.missing,
        }
    }
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Artist {
    pub id: i64,
    pub name: String,
    pub album_count: u32,
    pub track_count: u32,
    /// A cover to show for the artist (their newest album's), if any.
    pub art: Option<String>,
}

impl From<ArtistSummary> for Artist {
    fn from(a: ArtistSummary) -> Self {
        Artist {
            id: a.id,
            name: a.name,
            album_count: a.album_count,
            track_count: a.track_count,
            art: a.art,
        }
    }
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ArtistInfo {
    pub artist: Artist,
    pub albums: Vec<Album>,
    pub appears_on: Vec<Album>,
}

impl From<ArtistDetail> for ArtistInfo {
    fn from(d: ArtistDetail) -> Self {
        ArtistInfo {
            artist: d.artist.into(),
            albums: d.albums.into_iter().map(Into::into).collect(),
            appears_on: d.appears_on.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RootFolder {
    pub path: String,
    pub follow_symlinks: bool,
}

/// What a file contains.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SourceInfo {
    pub rate: u32,
    pub bits: u32,
    pub channels: u16,
}

impl From<SourceFormat> for SourceInfo {
    fn from(f: SourceFormat) -> Self {
        SourceInfo {
            rate: f.rate,
            bits: f.bits,
            channels: f.channels,
        }
    }
}

/// What the device was opened with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OutputInfo {
    pub rate: u32,
    /// Bits that carry audio in each sample.
    pub bits: u32,
    /// Bits each sample occupies on the wire (24 for packed, 32 for 24-in-32).
    pub container_bits: u32,
    pub channels: u16,
    pub label: String,
}

impl From<DeviceFormat> for OutputInfo {
    fn from(f: DeviceFormat) -> Self {
        OutputInfo {
            rate: f.rate,
            bits: f.container.valid_bits(),
            container_bits: (f.container.bytes() * 8) as u32,
            channels: f.channels,
            label: f.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub id: String,
    pub name: String,
    pub is_default: bool,
    pub selected: bool,
    /// Stereo formats the device accepts in exclusive mode (empty when it could not be
    /// probed, for example while another app holds it).
    pub formats: Vec<OutputInfo>,
}

impl Device {
    pub fn new(info: DeviceInfo, selected: bool) -> Self {
        Device {
            id: info.id.0,
            name: info.name,
            is_default: info.is_default,
            selected,
            formats: info.formats.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum PlayState {
    #[default]
    Stopped,
    Playing,
    Paused,
}

/// One entry of the play queue.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct QueueItem {
    /// Stable for the life of this entry, unlike its position: a track queued twice gets two.
    pub entry_id: u32,
    pub track_id: i64,
    pub path: String,
    pub title: String,
    pub artist: Option<String>,
    pub album: String,
    pub album_id: i64,
    pub art: Option<String>,
    pub art_colors: Vec<String>,
    pub duration_ms: u64,
}

impl QueueItem {
    pub fn new(entry_id: u32, t: &TrackItem) -> Self {
        QueueItem {
            entry_id,
            track_id: t.id,
            path: t.path.clone(),
            title: t.title.clone(),
            artist: t.artist.clone().or_else(|| t.album_artist.clone()),
            album: t.album.clone(),
            album_id: t.album_id,
            art: t.art.clone(),
            art_colors: t.art_colors.clone(),
            duration_ms: t.duration_ms,
        }
    }

    /// Builds a queue entry for an audio file outside the library index (`track_id = 0`).
    pub fn from_external(entry_id: u32, path: &Path) -> Self {
        let path_str = path.to_string_lossy().into_owned();
        let fallback_title = path
            .file_stem()
            .or_else(|| path.file_name())
            .map(|s| s.to_string_lossy().into_owned())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| path_str.clone());
        let meta = sonder_library::read_file(path).ok();
        let title = meta
            .as_ref()
            .and_then(|m| m.tags.title.clone())
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(fallback_title);
        let artist = meta.as_ref().and_then(|m| {
            if m.tags.artists.is_empty() {
                (!m.tags.album_artists.is_empty()).then(|| m.tags.album_artists.join("; "))
            } else {
                Some(m.tags.artists.join("; "))
            }
        });
        let album = meta
            .as_ref()
            .and_then(|m| m.tags.album.clone())
            .unwrap_or_default();
        let duration_ms = meta.as_ref().map_or(0, |m| m.audio.duration_ms);
        QueueItem {
            entry_id,
            track_id: 0,
            path: path_str,
            title,
            artist,
            album,
            album_id: 0,
            art: None,
            art_colors: Vec::new(),
            duration_ms,
        }
    }
}

/// One stage of the signal path, as the engine labels it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SignalPathStage {
    pub name: String,
    pub detail: String,
}

/// What the device receives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum SignalPathMode {
    Pcm,
    Dop,
    NativeDsd,
}

/// The stages a track passes through, in processing order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SignalPathInfo {
    pub source: SourceInfo,
    pub output_rate: u32,
    pub output_bits: u32,
    pub output_channels: u16,
    pub mode: SignalPathMode,
    pub stages: Vec<SignalPathStage>,
    /// `true` only when nothing touched the samples and the rate is unchanged.
    pub bit_perfect: bool,
    /// `true` when playing through the OS shared mixer rather than exclusive mode.
    pub shared: bool,
    /// How the track joined the previous one: the crossfade length, or why it stayed gapless.
    pub transition: Option<String>,
}

impl From<sonder_engine::SignalPath> for SignalPathInfo {
    fn from(path: sonder_engine::SignalPath) -> Self {
        use sonder_engine::OutputMode;
        SignalPathInfo {
            source: path.source.into(),
            output_rate: path.output_rate,
            output_bits: path.output_bits,
            output_channels: path.output_channels,
            mode: match path.mode {
                OutputMode::Pcm => SignalPathMode::Pcm,
                OutputMode::Dop => SignalPathMode::Dop,
                OutputMode::NativeDsd => SignalPathMode::NativeDsd,
            },
            stages: path
                .stages
                .into_iter()
                .map(|stage| SignalPathStage {
                    name: stage.name,
                    detail: stage.detail,
                })
                .collect(),
            bit_perfect: path.bit_perfect,
            shared: path.share_mode == sonder_engine::ShareMode::Shared,
            transition: path.transition,
        }
    }
}

/// Everything the transport bar and Now Playing show.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PlayerSnapshot {
    pub state: PlayState,
    /// Queue entry playing, paused, or selected to play next.
    pub index: Option<u32>,
    pub elapsed_ms: u64,
    pub duration_ms: Option<u64>,
    pub source: Option<SourceInfo>,
    pub output: Option<OutputInfo>,
    pub underruns: u64,
    /// Software volume in dB; `None` is bit-perfect output.
    pub volume_db: Option<f64>,
    /// What happens to the signal for the announced track; `None` before the first one.
    pub signal_path: Option<SignalPathInfo>,
    /// What plays after the current track ends.
    pub repeat: RepeatMode,
}

/// One line of lyrics; `start_ms` is 0 on every line of unsynced lyrics.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LyricsLine {
    pub start_ms: u32,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Lyrics {
    /// "Embedded", "LRC file" or the online service's name.
    pub source: String,
    pub synced: bool,
    /// The track has no vocals; `lines` is empty.
    pub instrumental: bool,
    pub lines: Vec<LyricsLine>,
}

impl From<sonder_lyrics::Lyrics> for Lyrics {
    fn from(l: sonder_lyrics::Lyrics) -> Self {
        Lyrics {
            source: l.source,
            synced: l.synced,
            instrumental: l.instrumental,
            lines: l
                .lines
                .into_iter()
                .map(|x| LyricsLine {
                    start_ms: x.start_ms,
                    text: x.text,
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An AutoEQ file for one headphone, in the shape the project publishes.
    const AUTOEQ: &str = "Preamp: -6.2 dB\nFilter 1: ON PK Fc 105 Hz Gain 3.4 dB Q 0.70\n";

    /// Writes `name` into its own temporary folder and returns the path.
    fn temp_file(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("sonder-peq-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    fn read_and_clean(path: &std::path::Path) -> AppResult<PeqImport> {
        let result = PeqImport::read(path);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
        result
    }

    #[test]
    fn autoeq_text_becomes_a_preset_named_after_the_headphone() {
        let import = PeqImport::parse("Sennheiser HD 600".into(), AUTOEQ).unwrap();
        assert_eq!(import.name, "Sennheiser HD 600");
        assert_eq!(import.peq.preamp_db, -6.2);
        assert_eq!(import.peq.bands.len(), 1);
        assert!(PeqImport::parse("x".into(), "<html>not found</html>").is_err());
    }

    #[test]
    fn import_names_the_preset_after_the_file() {
        let autoeq = temp_file("Sennheiser HD 600 ParametricEQ.txt", AUTOEQ.as_bytes());
        let import = read_and_clean(&autoeq).unwrap();
        assert_eq!(import.name, "Sennheiser HD 600");
        assert_eq!(import.peq.preamp_db, -6.2);
        assert_eq!(import.peq.bands.len(), 1);
        assert!(import.warnings.is_empty());

        let plain = temp_file("My EQ.txt", AUTOEQ.as_bytes());
        assert_eq!(read_and_clean(&plain).unwrap().name, "My EQ");
    }

    #[test]
    fn export_suggests_a_file_name_the_shell_accepts() {
        assert_eq!(
            export_file_name("Sennheiser HD 600"),
            "Sennheiser HD 600 ParametricEQ.txt"
        );
        assert_eq!(
            export_file_name("Bass/Treble: v2?"),
            "Bass-Treble- v2- ParametricEQ.txt"
        );
        // A trailing dot or space makes Windows reject the name; both go.
        assert_eq!(export_file_name("  My EQ.  "), "My EQ ParametricEQ.txt");
        assert_eq!(export_file_name("   "), "Sonder ParametricEQ.txt");
        assert_eq!(export_file_name("a\u{7}b"), "a-b ParametricEQ.txt");
        assert!(export_file_name(&"x".repeat(300)).len() < 130);
    }

    #[test]
    fn import_reports_why_a_file_cannot_be_used() {
        let graphic = temp_file("GraphicEQ.txt", b"GraphicEQ: 20 -3.0; 21 -2.9\n");
        let error = read_and_clean(&graphic).unwrap_err();
        assert!(
            error.0.contains("not an Equalizer APO parametric file"),
            "{}",
            error.0
        );

        let binary = temp_file("Binary.txt", &[0xff, 0xfe, 0x00, 0x41]);
        let error = read_and_clean(&binary).unwrap_err();
        assert!(error.0.contains("not UTF-8"), "{}", error.0);

        // UTF-16 from a Windows editor is full of NULs; say so rather than complain about the
        // filter lines it decodes to.
        let utf16 = temp_file("UTF16.txt", &[b'P', 0, b'r', 0, b'e', 0, b'a', 0, b'm', 0]);
        let error = read_and_clean(&utf16).unwrap_err();
        assert!(error.0.contains("UTF-16"), "{}", error.0);

        let huge = temp_file("Huge.txt", &vec![b'#'; MAX_APO_BYTES as usize + 1]);
        let error = read_and_clean(&huge).unwrap_err();
        assert!(error.0.contains("larger than 256 KB"), "{}", error.0);
    }

    #[test]
    fn a_stem_with_a_lowercase_suffix_still_names_the_headphone() {
        let path = temp_file("HD 600 parametricEq.txt", AUTOEQ.as_bytes());
        assert_eq!(read_and_clean(&path).unwrap().name, "HD 600");
    }
}
