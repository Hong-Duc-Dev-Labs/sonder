//! Types shared with the web UI. They mirror the engine and library types in the shape the
//! UI needs and derive `specta::Type`, so `src/lib/bindings.ts` is generated from them.

use serde::{Deserialize, Serialize};
use specta::Type;

use sonder_decode::SourceFormat;
use sonder_library::{AlbumDetail, AlbumSort, AlbumSummary, ArtistDetail, ArtistSummary, Track};
use sonder_output::{DeviceFormat, DeviceInfo};

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
            title: t.title.clone(),
            artist: t.artist.clone().or_else(|| t.album_artist.clone()),
            album: t.album.clone(),
            album_id: t.album_id,
            art: t.art.clone(),
            art_colors: t.art_colors.clone(),
            duration_ms: t.duration_ms,
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
