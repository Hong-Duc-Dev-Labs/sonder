//! Tag and technical-format reading.
//!
//! FLAC, ALAC (MP4), WAV and AIFF go through lofty. lofty has no DSD support, so DSF and DFF
//! are parsed here: their headers give the format, and their ID3v2 blob is handed to lofty's
//! ID3v2 reader. A DFF without ID3 falls back to its native `DIIN` title/artist chunks, and any
//! file without a title falls back to its file name.

use std::fs::File;
use std::io::{self, Cursor, Read, Seek, SeekFrom, Write};
use std::path::Path;

use lofty::config::{ParseOptions, WriteOptions};
use lofty::file::{AudioFile, FileType, TaggedFile, TaggedFileExt};
use lofty::id3::v2::Id3v2Tag;
use lofty::mp4::{Mp4Codec, Mp4File};
use lofty::mpeg::MpegFile;
use lofty::picture::{MimeType, Picture, PictureType};
use lofty::probe::Probe;
use lofty::tag::items::Timestamp;
use lofty::tag::{Accessor, ItemKey, ItemValue, MergeTag, SplitTag, Tag, TagExt, TagItem, TagType};

/// Largest ID3v2 blob read from a DSD file (covers can be large, tags are not).
const MAX_ID3_BYTES: u64 = 64 * 1024 * 1024;

/// File extensions the scanner indexes (lowercase).
pub const AUDIO_EXTENSIONS: &[&str] = &[
    "flac", "m4a", "mp4", "wav", "aif", "aiff", "aifc", "dsf", "dff",
];

/// Whether `path` has an extension the scanner indexes.
pub fn is_audio_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| AUDIO_EXTENSIONS.iter().any(|x| e.eq_ignore_ascii_case(x)))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Codec {
    Flac,
    Alac,
    Wav,
    Aiff,
    Dsf,
    Dff,
}

impl Codec {
    pub fn as_str(self) -> &'static str {
        match self {
            Codec::Flac => "flac",
            Codec::Alac => "alac",
            Codec::Wav => "wav",
            Codec::Aiff => "aiff",
            Codec::Dsf => "dsf",
            Codec::Dff => "dff",
        }
    }

    pub fn parse(s: &str) -> Option<Codec> {
        Some(match s {
            "flac" => Codec::Flac,
            "alac" => Codec::Alac,
            "wav" => Codec::Wav,
            "aiff" => Codec::Aiff,
            "dsf" => Codec::Dsf,
            "dff" => Codec::Dff,
            _ => return None,
        })
    }

    /// DSD streams: rate is the 1-bit sample rate (2 822 400 for DSD64).
    pub fn is_dsd(self) -> bool {
        matches!(self, Codec::Dsf | Codec::Dff)
    }
}

/// Technical format of the audio stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioInfo {
    pub codec: Codec,
    pub rate: u32,
    /// Bits per sample; 1 for DSD, `None` when the container does not say.
    pub bits: Option<u32>,
    pub channels: u16,
    pub duration_ms: u64,
}

/// ReplayGain values in dB (gain) and linear full scale (peak).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ReplayGain {
    pub track_gain: Option<f64>,
    pub track_peak: Option<f64>,
    pub album_gain: Option<f64>,
    pub album_peak: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TrackTags {
    pub title: Option<String>,
    pub artists: Vec<String>,
    pub album: Option<String>,
    pub album_artists: Vec<String>,
    pub track_no: Option<u32>,
    pub disc_no: Option<u32>,
    pub year: Option<i32>,
    pub genre: Option<String>,
    pub compilation: bool,
    pub mb_album_id: Option<String>,
    pub replay_gain: ReplayGain,
    /// Embedded front cover (or first picture) as stored in the file.
    pub cover: Option<Vec<u8>>,
    /// Embedded lyrics (plain or LRC text), as stored in the file.
    pub lyrics: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FileMeta {
    pub tags: TrackTags,
    pub audio: AudioInfo,
}

/// The metadata fields the editor can change. Text and number values are the complete new
/// state: an empty string or list clears the field. Lyrics and cover art change only when the
/// edit says so. Everything else in the file — ReplayGain, comments and format-specific
/// frames — is left as it was.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TagEdit {
    /// Never empty: a file with no title has nothing to show in the library.
    pub title: String,
    pub artists: Vec<String>,
    pub album: String,
    pub album_artists: Vec<String>,
    pub track_no: Option<u32>,
    pub disc_no: Option<u32>,
    pub year: Option<i32>,
    pub genre: String,
    /// `None` keeps the file's lyrics; blank text removes them.
    pub lyrics: Option<String>,
    pub cover: CoverEdit,
}

/// What happens to the file's embedded pictures.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum CoverEdit {
    /// The pictures stay as they are.
    #[default]
    Keep,
    /// Every picture goes: the reader falls back to any picture, so leaving one would keep
    /// showing it as the cover.
    Remove,
    /// The front cover becomes this JPEG or PNG; other pictures (a back cover, a booklet
    /// page) stay.
    Set(Vec<u8>),
}

/// The media type of a cover image the tag formats all accept, from its first bytes.
pub fn cover_mime(data: &[u8]) -> Option<&'static str> {
    if data.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else {
        None
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TagError {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error("cannot read tags: {0}")]
    Parse(String),
    #[error("unsupported file: {0}")]
    Unsupported(String),
}

impl From<lofty::error::FileParseError> for TagError {
    fn from(err: lofty::error::FileParseError) -> Self {
        TagError::Parse(err.to_string())
    }
}

/// Reads tags and technical format from one audio file.
pub fn read_file(path: &Path) -> Result<FileMeta, TagError> {
    let mut meta = read_stored(path)?;
    fill_from_file_name(&mut meta.tags, path);
    Ok(meta)
}

/// Reads tags and technical format from any seekable byte stream (such as a Google Drive
/// Range-cached reader) without requiring a local file on disk.
///
/// `virtual_path` is used only for its file extension hint and fallback title/track-number
/// parsing.
pub fn read_stream<R: Read + Seek + Send + Sync + 'static>(
    mut reader: R,
    byte_len: u64,
    virtual_path: &Path,
) -> Result<FileMeta, TagError> {
    let ext = virtual_path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    let mut meta = match ext.as_str() {
        "dsf" => dsd::read_dsf_from(&mut reader, byte_len)?,
        "dff" => dsd::read_dff_from(&mut reader, byte_len)?,
        "m4a" | "mp4" => read_mp4_stream(reader, byte_len)?,
        _ => read_lofty_stream(reader, &ext)?,
    };
    fill_from_file_name(&mut meta.tags, virtual_path);
    Ok(meta)
}

/// Builds a minimal [`FileMeta`] from a filename and optional parent folder name when remote
/// tag probing fails. Returns `None` if the extension is not a supported audio format.
pub fn fallback_meta(virtual_path: &Path, folder_hint: Option<&str>) -> Option<FileMeta> {
    let ext = virtual_path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)?;
    let (codec, rate, bits) = match ext.as_str() {
        "flac" => (Codec::Flac, 44_100, Some(16)),
        "m4a" | "mp4" => (Codec::Alac, 44_100, Some(16)),
        "wav" => (Codec::Wav, 44_100, Some(16)),
        "aif" | "aiff" | "aifc" => (Codec::Aiff, 44_100, Some(16)),
        "dsf" => (Codec::Dsf, 2_822_400, Some(1)),
        "dff" => (Codec::Dff, 2_822_400, Some(1)),
        _ => return None,
    };
    let mut tags = TrackTags::default();
    fill_from_file_name(&mut tags, virtual_path);
    if tags.album.is_none() {
        tags.album = folder_hint.and_then(clean);
    }
    Some(FileMeta {
        tags,
        audio: AudioInfo {
            codec,
            rate,
            bits,
            channels: 2,
            duration_ms: 0,
        },
    })
}

/// What the file itself holds, without values guessed from its name.
fn read_stored(path: &Path) -> Result<FileMeta, TagError> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    match ext.as_str() {
        "dsf" => dsd::read_dsf(path),
        "dff" => dsd::read_dff(path),
        "m4a" | "mp4" => read_mp4(path),
        _ => read_lofty(path),
    }
}

/// The tags stored in the file, for the metadata editor, embedded cover included. Unlike
/// [`read_file`], a missing track number is not guessed from the file name, because the
/// editor writes back what it shows. A missing title still is: a track needs one, and the
/// field shows what Save writes.
pub fn read_file_tags(path: &Path) -> Result<TrackTags, TagError> {
    let mut tags = read_stored(path)?.tags;
    if tags.title.is_none() {
        let mut guess = TrackTags::default();
        fill_from_file_name(&mut guess, path);
        tags.title = guess.title;
    }
    Ok(tags)
}

/// Writes `edit` into the file at `path`, keeping every tag the editor does not own.
///
/// The file is rebuilt in a temporary file beside it and moved over the original, so a
/// failure (a full disk, a read-only folder) leaves the original untouched. A file the user
/// cannot write to is refused. FLAC, ALAC, WAV and AIFF go through lofty; DSF and DFF carry
/// an ID3v2 tag that is replaced in the container.
pub fn write_file(path: &Path, edit: &TagEdit) -> Result<(), TagError> {
    if edit.title.trim().is_empty() {
        return Err(TagError::Parse("a track needs a title".into()));
    }
    if let CoverEdit::Set(data) = &edit.cover
        && cover_mime(data).is_none()
    {
        return Err(TagError::Unsupported(
            "the cover must be a JPEG or PNG image".into(),
        ));
    }
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    match ext.as_str() {
        "dsf" => dsd::write_dsf(path, edit),
        "dff" => dsd::write_dff(path, edit),
        _ => write_lofty(path, edit),
    }
}

/// Writes the tags lofty owns (FLAC, ALAC/MP4, WAV, AIFF).
fn write_lofty(path: &Path, edit: &TagEdit) -> Result<(), TagError> {
    let mut tagged = Probe::open(path)?.guess_file_type()?.read()?;
    let tag_type = tagged.primary_tag_type();
    if tagged.tag_mut(tag_type).is_none() {
        if !tagged.file_type().tag_support(tag_type).is_writable() {
            return Err(TagError::Unsupported(format!(
                "{:?} files cannot hold tags",
                tagged.file_type()
            )));
        }
        tagged.insert_tag(Tag::new(tag_type));
    }
    // Every tag gets the edit, not only the primary one: the reader falls back to the others
    // (a WAV's RIFF INFO, an AIFF's text chunks), so a value cleared in one would come back.
    let file_type = tagged.file_type();
    let types: Vec<TagType> = tagged
        .tags()
        .iter()
        .map(Tag::tag_type)
        .filter(|t| file_type.tag_support(*t).is_writable())
        .collect();
    for tag_type in types {
        if let Some(tag) = tagged.tag_mut(tag_type) {
            apply_edit(tag, edit);
        }
    }
    // lofty writes in place; it gets a copy of the file instead, which then replaces it.
    rewrite(path, |source, tmp| {
        io::copy(source, tmp)?;
        tmp.seek(SeekFrom::Start(0))?;
        tagged
            .save_to(tmp, WriteOptions::new())
            .map_err(|e| TagError::Parse(format!("cannot write tags: {e}")))?;
        // lofty 0.25 cuts a FLAC short when its shrunken metadata ends the file (no audio
        // frames after it). The copy must read back before it replaces the original.
        tmp.seek(SeekFrom::Start(0))?;
        Probe::with_file_type(io::BufReader::new(&mut *tmp), file_type)
            .read()
            .map(drop)
            .map_err(|e| TagError::Parse(format!("the rewritten file does not read back: {e}")))
    })
}

/// Rebuilds the file at `path` through `build`, which reads the original from its first
/// argument and writes the new file to the second, then moves the result over the original,
/// so a failed write never leaves a half-written track. The original is opened for writing
/// first so that a file the user may not change is refused instead of being replaced (a
/// rename only needs the folder to be writable), and the new file takes its permissions.
fn rewrite(
    path: &Path,
    build: impl FnOnce(&mut File, &mut File) -> Result<(), TagError>,
) -> Result<(), TagError> {
    let mut source = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)?;
    let permissions = source.metadata()?.permissions();
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    build(&mut source, tmp.as_file_mut())?;
    tmp.as_file().set_permissions(permissions)?;
    tmp.as_file_mut().sync_all()?;
    // Windows refuses to replace a file that is still open.
    drop(source);
    tmp.persist(path)
        .map(|_| ())
        .map_err(|err| TagError::Io(err.error))
}

/// Copies `from..to` of `source` to `tmp`.
fn copy_range(source: &mut File, tmp: &mut File, from: u64, to: u64) -> Result<(), TagError> {
    if to <= from {
        return Ok(());
    }
    source.seek(SeekFrom::Start(from))?;
    io::copy(&mut source.take(to - from), tmp)?;
    Ok(())
}

/// Replaces the fields the editor owns, leaving every other item (and every picture) alone.
fn apply_edit(tag: &mut Tag, edit: &TagEdit) {
    let title = edit.title.trim();
    tag.set_title(title.to_string());
    match clean(&edit.album) {
        Some(album) => tag.set_album(album),
        None => tag.remove_album(),
    }
    match clean(&edit.genre) {
        Some(genre) => tag.set_genre(genre),
        None => tag.remove_genre(),
    }
    write_names(
        tag,
        &[ItemKey::TrackArtist, ItemKey::TrackArtists],
        &edit.artists,
    );
    write_names(tag, &[ItemKey::AlbumArtist], &edit.album_artists);
    match edit.track_no {
        Some(number) => tag.set_track(number),
        None => tag.remove_track(),
    }
    match edit.disc_no {
        Some(number) => tag.set_disk(number),
        None => tag.remove_disk(),
    }
    match edit.year {
        Some(year) => tag.set_date(Timestamp {
            year: year.clamp(0, 9999) as u16,
            month: None,
            day: None,
            hour: None,
            minute: None,
            second: None,
        }),
        None => tag.remove_date(),
    }
    if let Some(lyrics) = &edit.lyrics {
        // Not `clean`: line breaks and indentation inside lyrics are meaningful.
        let text = lyrics.trim_matches(|c: char| c.is_whitespace() || c == '\0');
        tag.remove_key(ItemKey::Lyrics);
        tag.remove_key(ItemKey::UnsyncLyrics);
        // ID3v2 has only its unsynced-lyrics frame (USLT) for text. Formats with no lyrics
        // field at all (RIFF INFO, AIFF text) refuse both; the file's other tag carries them.
        if !text.is_empty() && !tag.insert_text(ItemKey::Lyrics, text.to_string()) {
            tag.insert_text(ItemKey::UnsyncLyrics, text.to_string());
        }
    }
    match &edit.cover {
        CoverEdit::Keep => {}
        CoverEdit::Remove => {
            while !tag.pictures().is_empty() {
                tag.remove_picture(0);
            }
        }
        CoverEdit::Set(data) => {
            tag.remove_picture_type(PictureType::CoverFront);
            let mime = if cover_mime(data) == Some("image/png") {
                MimeType::Png
            } else {
                MimeType::Jpeg
            };
            // First, so a reader that takes the first picture also finds it.
            let mut pictures: Vec<Picture> = Vec::new();
            while !tag.pictures().is_empty() {
                pictures.push(tag.remove_picture(0));
            }
            tag.push_picture(
                Picture::unchecked(data.clone())
                    .pic_type(PictureType::CoverFront)
                    .mime_type(mime)
                    .build(),
            );
            for picture in pictures {
                tag.push_picture(picture);
            }
        }
    }
}

/// Writes an ordered list of names. Every listed key is cleared first: a file can carry the
/// same credit under two spellings (ID3v2 `TPE1` and a `TXXX:ARTISTS` frame, for example), and
/// the reader prefers one of them. Each name is its own value under the first key: Vorbis
/// comments repeat the field, MP4 repeats the data atom and ID3v2.4 separates them with NUL,
/// and the reader splits all three back into names. Only the formats that hold a single text
/// (RIFF INFO, AIFF text chunks, ID3v1) get them joined, the way the library displays them.
fn write_names(tag: &mut Tag, keys: &[ItemKey], names: &[String]) {
    let Some(&key) = keys.first() else {
        return;
    };
    for other in keys {
        tag.remove_key(*other);
    }
    let names: Vec<String> = names.iter().filter_map(|n| clean(n)).collect();
    let single = matches!(
        tag.tag_type(),
        TagType::RiffInfo | TagType::AiffText | TagType::Id3v1
    );
    if !single {
        for name in names {
            tag.push(TagItem::new(key, ItemValue::Text(name)));
        }
    } else if !names.is_empty() {
        tag.insert_text(key, names.join("; "));
    }
}

fn read_mp4(path: &Path) -> Result<FileMeta, TagError> {
    let mut file = File::open(path)?;
    let mp4 = Mp4File::read_from(&mut file, ParseOptions::new())?;
    let codec = mp4.properties().codec();
    let tagged: TaggedFile = mp4.into();
    let audio = match codec {
        Some(Mp4Codec::ALAC) => audio_info(&tagged, Codec::Alac)?,
        // lofty names the codec only when the ALAC sample entry has exactly the standard
        // size; encoders that pad it (a `free` box after the `alac` config) leave it unknown.
        // The player's decoder decides then: what it can open is what the player can play.
        None => decoder_audio_info(path, tagged.properties().duration())?,
        Some(other) => {
            return Err(TagError::Unsupported(format!(
                "MP4 audio is {other:?}, not ALAC"
            )));
        }
    };
    Ok(FileMeta {
        tags: extract(&tagged),
        audio,
    })
}

/// Format of an MP4 whose codec lofty could not name, from the playback decoder. The decoder
/// also opens PCM and FLAC in MP4, so it must confirm the stream is ALAC. `container_duration`
/// (lofty's, from the track header) is used when the decoder reports no frame count.
fn decoder_audio_info(
    path: &Path,
    container_duration: std::time::Duration,
) -> Result<AudioInfo, TagError> {
    let decoder = sonder_decode::Decoder::open(path)
        .map_err(|e| TagError::Unsupported(format!("cannot open MP4 audio: {e}")))?;
    if !decoder.is_alac() {
        return Err(TagError::Unsupported("MP4 audio is not ALAC".into()));
    }
    let format = decoder.format();
    Ok(AudioInfo {
        codec: Codec::Alac,
        rate: format.rate,
        bits: Some(format.bits),
        channels: format.channels,
        duration_ms: match decoder.total_frames().filter(|frames| *frames > 0) {
            Some(frames) => frames.saturating_mul(1000) / u64::from(format.rate),
            None => container_duration.as_millis() as u64,
        },
    })
}

fn read_mp4_stream<R: Read + Seek + Send + Sync + 'static>(
    mut reader: R,
    byte_len: u64,
) -> Result<FileMeta, TagError> {
    let mp4 = Mp4File::read_from(&mut reader, ParseOptions::new())?;
    let codec = mp4.properties().codec();
    let tagged: TaggedFile = mp4.into();
    let audio = match codec {
        Some(Mp4Codec::ALAC) => audio_info(&tagged, Codec::Alac)?,
        None => {
            reader.seek(SeekFrom::Start(0))?;
            let decoder = sonder_decode::Decoder::open_source(
                Box::new(reader),
                Some(byte_len),
                Some("m4a"),
            )
            .map_err(|e| TagError::Unsupported(format!("cannot open MP4 audio: {e}")))?;
            if !decoder.is_alac() {
                return Err(TagError::Unsupported("MP4 audio is not ALAC".into()));
            }
            let format = decoder.format();
            let container_duration = tagged.properties().duration();
            AudioInfo {
                codec: Codec::Alac,
                rate: format.rate,
                bits: Some(format.bits),
                channels: format.channels,
                duration_ms: match decoder.total_frames().filter(|frames| *frames > 0) {
                    Some(frames) => frames.saturating_mul(1000) / u64::from(format.rate),
                    None => container_duration.as_millis() as u64,
                },
            }
        }
        Some(other) => {
            return Err(TagError::Unsupported(format!(
                "MP4 audio is {other:?}, not ALAC"
            )));
        }
    };
    Ok(FileMeta {
        tags: extract(&tagged),
        audio,
    })
}

fn read_lofty_stream<R: Read + Seek>(reader: R, ext: &str) -> Result<FileMeta, TagError> {
    let buffered = io::BufReader::new(reader);
    let probe = match ext {
        "flac" => Probe::with_file_type(buffered, FileType::Flac),
        "wav" => Probe::with_file_type(buffered, FileType::Wav),
        "aif" | "aiff" | "aifc" => Probe::with_file_type(buffered, FileType::Aiff),
        _ => Probe::new(buffered).guess_file_type()?,
    };
    let tagged = probe.read()?;
    let codec = match tagged.file_type() {
        FileType::Flac => Codec::Flac,
        FileType::Wav => Codec::Wav,
        FileType::Aiff => Codec::Aiff,
        other => {
            return Err(TagError::Unsupported(format!(
                "{other:?} files are not indexed"
            )));
        }
    };
    Ok(FileMeta {
        tags: extract(&tagged),
        audio: audio_info(&tagged, codec)?,
    })
}

fn read_lofty(path: &Path) -> Result<FileMeta, TagError> {
    let tagged = Probe::open(path)?.guess_file_type()?.read()?;
    let codec = match tagged.file_type() {
        FileType::Flac => Codec::Flac,
        FileType::Wav => Codec::Wav,
        FileType::Aiff => Codec::Aiff,
        other => {
            return Err(TagError::Unsupported(format!(
                "{other:?} files are not indexed"
            )));
        }
    };
    Ok(FileMeta {
        tags: extract(&tagged),
        audio: audio_info(&tagged, codec)?,
    })
}

fn audio_info(tagged: &TaggedFile, codec: Codec) -> Result<AudioInfo, TagError> {
    let props = tagged.properties();
    let rate = props
        .sample_rate()
        .filter(|r| *r > 0)
        .ok_or_else(|| TagError::Parse("no sample rate".into()))?;
    let channels = props
        .channels()
        .filter(|c| *c > 0)
        .ok_or_else(|| TagError::Parse("no channel count".into()))?;
    Ok(AudioInfo {
        codec,
        rate,
        bits: props.bit_depth().map(u32::from),
        channels: u16::from(channels),
        duration_ms: props.duration().as_millis() as u64,
    })
}

/// Merges all tags of a file, taking each field from the primary tag first.
fn extract(tagged: &TaggedFile) -> TrackTags {
    let mut tags: Vec<&Tag> = Vec::new();
    if let Some(primary) = tagged.primary_tag() {
        tags.push(primary);
    }
    for tag in tagged.tags() {
        if !tags.iter().any(|t| std::ptr::eq(*t, tag)) {
            tags.push(tag);
        }
    }
    extract_tags(&tags)
}

fn extract_tags(tags: &[&Tag]) -> TrackTags {
    fn first<T>(tags: &[&Tag], f: impl Fn(&Tag) -> Option<T>) -> Option<T> {
        tags.iter().find_map(|t| f(t))
    }
    fn text(tag: &Tag, key: ItemKey) -> Option<String> {
        tag.get_string(key).and_then(clean)
    }
    fn list(tag: &Tag, key: ItemKey) -> Option<Vec<String>> {
        let mut out: Vec<String> = Vec::new();
        for value in tag.get_strings(key) {
            // ID3v2.4 multi-value frames may arrive NUL-separated.
            for part in value.split('\0') {
                if let Some(v) = clean(part)
                    && !out.contains(&v)
                {
                    out.push(v);
                }
            }
        }
        (!out.is_empty()).then_some(out)
    }

    let artists = first(tags, |t| list(t, ItemKey::TrackArtists))
        .or_else(|| first(tags, |t| list(t, ItemKey::TrackArtist)))
        .unwrap_or_default();
    let year = first(tags, |t| t.date().map(|d| i32::from(d.year)))
        .or_else(|| {
            first(tags, |t| {
                text(t, ItemKey::Year).and_then(|s| parse_year(&s))
            })
        })
        .or_else(|| {
            first(tags, |t| {
                text(t, ItemKey::RecordingDate).and_then(|s| parse_year(&s))
            })
        });
    let cover = first(tags, |t| {
        let pics = t.pictures();
        pics.iter()
            .find(|p| p.pic_type() == PictureType::CoverFront)
            .or_else(|| pics.first())
            .map(|p| p.data().to_vec())
    });

    TrackTags {
        title: first(tags, |t| t.title().and_then(|s| clean(&s))),
        artists,
        album: first(tags, |t| t.album().and_then(|s| clean(&s))),
        album_artists: first(tags, |t| list(t, ItemKey::AlbumArtist)).unwrap_or_default(),
        track_no: first(tags, |t| t.track().filter(|n| *n > 0)),
        disc_no: first(tags, |t| t.disk().filter(|n| *n > 0)),
        year,
        genre: first(tags, |t| t.genre().and_then(|s| clean(&s))),
        compilation: first(tags, |t| text(t, ItemKey::FlagCompilation))
            .is_some_and(|v| parse_flag(&v)),
        mb_album_id: first(tags, |t| text(t, ItemKey::MusicBrainzReleaseId)),
        replay_gain: ReplayGain {
            track_gain: first(tags, |t| {
                text(t, ItemKey::ReplayGainTrackGain).and_then(|s| parse_gain(&s))
            }),
            track_peak: first(tags, |t| {
                text(t, ItemKey::ReplayGainTrackPeak).and_then(|s| parse_number(&s))
            }),
            album_gain: first(tags, |t| {
                text(t, ItemKey::ReplayGainAlbumGain).and_then(|s| parse_gain(&s))
            }),
            album_peak: first(tags, |t| {
                text(t, ItemKey::ReplayGainAlbumPeak).and_then(|s| parse_number(&s))
            }),
        },
        cover,
        // Not `clean`: line breaks and indentation inside lyrics are meaningful.
        // ID3v2 keeps lyrics only as unsynced text (USLT), even when they are LRC.
        lyrics: first(tags, |t| {
            t.get_string(ItemKey::Lyrics)
                .or_else(|| t.get_string(ItemKey::UnsyncLyrics))
                .map(|s| s.trim_matches(|c: char| c.is_whitespace() || c == '\0'))
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        }),
    }
}

fn clean(s: &str) -> Option<String> {
    let t = s.trim_matches(|c: char| c.is_whitespace() || c == '\0');
    (!t.is_empty()).then(|| t.to_string())
}

fn parse_flag(s: &str) -> bool {
    matches!(s.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes")
}

/// First four-digit run: "1959", "1959-08-17", "(P) 1997".
fn parse_year(s: &str) -> Option<i32> {
    let bytes = s.as_bytes();
    (0..bytes.len().saturating_sub(3)).find_map(|i| {
        let w = &bytes[i..i + 4];
        let bounded = (i == 0 || !bytes[i - 1].is_ascii_digit())
            && bytes.get(i + 4).is_none_or(|b| !b.is_ascii_digit());
        (bounded && w.iter().all(u8::is_ascii_digit))
            .then(|| std::str::from_utf8(w).ok()?.parse().ok())
            .flatten()
    })
}

/// "-6.54 dB", "+1.2dB", "-3.0".
fn parse_gain(s: &str) -> Option<f64> {
    let t = s.trim();
    let t = t
        .strip_suffix("dB")
        .or_else(|| t.strip_suffix("db"))
        .or_else(|| t.strip_suffix("DB"))
        .unwrap_or(t);
    parse_number(t)
}

fn parse_number(s: &str) -> Option<f64> {
    s.trim()
        .trim_start_matches('+')
        .replace(',', ".")
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
}

/// Fills a missing title and track number from names like "03 - Song.flac" or "1-03 Song".
fn fill_from_file_name(tags: &mut TrackTags, path: &Path) {
    if tags.title.is_some() && tags.track_no.is_some() {
        return;
    }
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let (number, rest) = split_track_number(&stem);
    if tags.track_no.is_none() {
        tags.track_no = number;
    }
    if tags.title.is_none() {
        tags.title = Some(if rest.is_empty() {
            stem.clone()
        } else {
            rest.to_string()
        });
    }
}

fn split_track_number(stem: &str) -> (Option<u32>, &str) {
    let digits = stem.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 || digits > 3 {
        return (None, stem.trim());
    }
    let mut number: Option<u32> = stem[..digits].parse().ok();
    let mut rest = &stem[digits..];
    // "1-03 Title": disc-track; keep the track part.
    if let Some(after) = rest.strip_prefix('-') {
        let d2 = after.bytes().take_while(u8::is_ascii_digit).count();
        if (1..=3).contains(&d2) {
            number = after[..d2].parse().ok();
            rest = &after[d2..];
        }
    }
    // A number must be followed by a separator to count ("1999 Song" stays a title).
    match rest.chars().next() {
        Some(c) if c == ' ' || c == '.' || c == '-' || c == '_' || c == ')' => {
            let title = rest.trim_start_matches([' ', '.', '-', '_', ')']).trim();
            if title.is_empty() {
                (number, stem.trim())
            } else {
                (number, title)
            }
        }
        None => (number, stem.trim()),
        _ => (None, stem.trim()),
    }
}

/// Parses a standalone ID3v2 tag with every frame it holds, including the ones lofty's generic
/// [`Tag`] has no item for (private, binary, chapter frames), so a rewrite can keep them.
fn parse_id3_blob(blob: Vec<u8>) -> Result<Option<Id3v2Tag>, TagError> {
    // lofty's MPEG reader accepts a stream that is only an ID3v2 tag when it is not asked
    // for audio properties; that is the only public entry point to its ID3v2 parser.
    let file = MpegFile::read_from(
        &mut Cursor::new(blob),
        ParseOptions::new().read_properties(false),
    )?;
    Ok(file.id3v2().cloned())
}

fn read_id3_at<R: Read + Seek>(
    r: &mut R,
    offset: u64,
    len: u64,
) -> Result<Option<TrackTags>, TagError> {
    Ok(read_tag_at(r, offset, len)?.as_ref().map(id3_tags))
}

/// Reads the ID3v2 tag at `offset`, if the bytes there are one.
fn read_tag_at<R: Read + Seek>(
    r: &mut R,
    offset: u64,
    len: u64,
) -> Result<Option<Id3v2Tag>, TagError> {
    if len < 10 {
        return Ok(None);
    }
    r.seek(SeekFrom::Start(offset))?;
    let mut blob = Vec::new();
    r.take(len.min(MAX_ID3_BYTES)).read_to_end(&mut blob)?;
    if !blob.starts_with(b"ID3") {
        return Ok(None);
    }
    parse_id3_blob(blob)
}

/// The fields of an ID3v2 tag as the reader sees them.
fn id3_tags(tag: &Id3v2Tag) -> TrackTags {
    extract_tags(&[&Tag::from(tag.clone())])
}

/// Applies `edit` to `tag` (or to a new tag) and serializes the result as a standalone ID3v2
/// tag, ready to be embedded in a DSD file. Frames outside the editor's fields stay as they
/// were, down to the ones lofty cannot convert.
fn edited_id3(tag: Option<Id3v2Tag>, edit: &TagEdit) -> Result<Vec<u8>, TagError> {
    let (rest, mut fields) = tag.unwrap_or_default().split_tag();
    apply_edit(&mut fields, edit);
    dump_id3(&rest.merge_tag(fields))
}

/// Serializes `tag` as a standalone ID3v2 tag.
fn dump_id3(tag: &Id3v2Tag) -> Result<Vec<u8>, TagError> {
    let mut blob = Vec::new();
    tag.dump_to(&mut blob, WriteOptions::new())
        .map_err(|e| TagError::Parse(format!("cannot write an ID3 tag: {e}")))?;
    Ok(blob)
}

mod dsd {
    //! DSF (Sony) and DSDIFF (Philips) headers.

    use super::*;

    fn le_u32(b: &[u8]) -> u32 {
        u32::from_le_bytes(b[..4].try_into().unwrap())
    }
    fn le_u64(b: &[u8]) -> u64 {
        u64::from_le_bytes(b[..8].try_into().unwrap())
    }

    fn bad(msg: &str) -> TagError {
        TagError::Parse(msg.to_string())
    }

    /// Offset of the chunk after one whose body starts at `body` and holds `len` bytes (plus
    /// a pad byte when odd). `None` when that overflows or runs past `end`, which ends the walk.
    /// Sizes come from the file, so this is the only way chunk loops advance: the result is
    /// always past `body`, so a corrupt size can never send a walk backwards or in a circle.
    pub(super) fn next_chunk(body: u64, len: u64, end: u64) -> Option<u64> {
        body.checked_add(len)?
            .checked_add(len & 1)
            .filter(|next| *next <= end)
    }

    pub(super) fn read_dsf(path: &Path) -> Result<FileMeta, TagError> {
        let mut f = File::open(path)?;
        let file_len = f.metadata()?.len();
        read_dsf_from(&mut f, file_len)
    }

    pub(super) fn read_dsf_from<R: Read + Seek>(
        f: &mut R,
        file_len: u64,
    ) -> Result<FileMeta, TagError> {
        let mut head = [0u8; 28 + 52];
        f.read_exact(&mut head)
            .map_err(|_| bad("DSF header truncated"))?;
        if &head[0..4] != b"DSD " || &head[28..32] != b"fmt " {
            return Err(bad("not a DSF file"));
        }
        let metadata_offset = le_u64(&head[20..28]);
        let fmt = &head[28..];
        let channels = le_u32(&fmt[24..28]);
        let rate = le_u32(&fmt[28..32]);
        let bits = le_u32(&fmt[32..36]);
        let samples = le_u64(&fmt[36..44]);
        if rate == 0 || channels == 0 || channels > 6 {
            return Err(bad("DSF format chunk is invalid"));
        }
        let tags = if metadata_offset > 0 && metadata_offset < file_len {
            read_id3_at(f, metadata_offset, file_len - metadata_offset)?.unwrap_or_default()
        } else {
            TrackTags::default()
        };
        Ok(FileMeta {
            tags,
            audio: AudioInfo {
                codec: Codec::Dsf,
                rate,
                // DSF stores 1 (LSB first) or 8 (MSB first) here; both are 1-bit DSD.
                bits: Some(if bits == 8 { 1 } else { bits }),
                channels: channels as u16,
                duration_ms: samples.saturating_mul(1000) / u64::from(rate),
            },
        })
    }

    pub(super) fn read_dff(path: &Path) -> Result<FileMeta, TagError> {
        let mut f = File::open(path)?;
        let file_len = f.metadata()?.len();
        read_dff_from(&mut f, file_len)
    }

    pub(super) fn read_dff_from<R: Read + Seek>(
        f: &mut R,
        file_len: u64,
    ) -> Result<FileMeta, TagError> {
        let mut head = [0u8; 16];
        f.read_exact(&mut head)
            .map_err(|_| bad("DFF header truncated"))?;
        if &head[0..4] != b"FRM8" || &head[12..16] != b"DSD " {
            return Err(bad("not a DSDIFF file"));
        }
        let form_end = u64::from_be_bytes(head[4..12].try_into().unwrap())
            .saturating_add(12)
            .min(file_len);

        let mut rate = 0u32;
        let mut channels = 0u16;
        let mut sound_bytes: Option<u64> = None;
        let mut dst_frames: Option<(u32, u16)> = None;
        let mut id3: Option<Id3v2Tag> = None;
        let mut native = TrackTags::default();

        let mut pos = 16u64;
        while pos + 12 <= form_end {
            let (id, size) = chunk_header(f, pos)?;
            let data = pos + 12;
            // A size past the end of the form is clamped: the last chunk of a truncated
            // download is still worth reading.
            let len = size.min(form_end.saturating_sub(data));
            match &id {
                b"PROP" => read_prop(f, data, len, &mut rate, &mut channels)?,
                b"DSD " => sound_bytes = Some(len),
                b"DST " => dst_frames = read_dst_info(f, data, len)?,
                b"DIIN" => read_diin(f, data, len, &mut native)?,
                b"ID3 " => id3 = read_tag_at(f, data, len)?,
                _ => {}
            }
            match next_chunk(data, size, form_end) {
                Some(next) => pos = next,
                None => break,
            }
        }
        // Files written by sacd_extract carry the tag after the form instead of inside it.
        if id3.is_none()
            && let Some(spot) = tail_spot(f, form_end, file_len)?
        {
            id3 = read_tag_at(f, spot.body, spot.len)?;
        }
        if rate == 0 || channels == 0 {
            return Err(bad("DSDIFF property chunk is missing or invalid"));
        }
        let duration_ms = match (sound_bytes, dst_frames) {
            (Some(bytes), _) => {
                bytes.saturating_mul(8 * 1000) / u64::from(rate) / u64::from(channels)
            }
            (None, Some((frames, fps))) if fps > 0 => u64::from(frames) * 1000 / u64::from(fps),
            _ => 0,
        };
        let mut tags = id3.as_ref().map(id3_tags).unwrap_or_default();
        if tags.title.is_none() {
            tags.title = native.title;
        }
        if tags.artists.is_empty() {
            tags.artists = native.artists;
        }
        Ok(FileMeta {
            tags,
            audio: AudioInfo {
                codec: Codec::Dff,
                rate,
                bits: Some(1),
                channels,
                duration_ms,
            },
        })
    }

    fn chunk_header<R: Read + Seek>(f: &mut R, pos: u64) -> Result<([u8; 4], u64), TagError> {
        let mut h = [0u8; 12];
        f.seek(SeekFrom::Start(pos))?;
        f.read_exact(&mut h)
            .map_err(|_| bad("DSDIFF chunk truncated"))?;
        Ok((
            h[0..4].try_into().unwrap(),
            u64::from_be_bytes(h[4..12].try_into().unwrap()),
        ))
    }

    fn read_prop<R: Read + Seek>(
        f: &mut R,
        data: u64,
        size: u64,
        rate: &mut u32,
        channels: &mut u16,
    ) -> Result<(), TagError> {
        let mut kind = [0u8; 4];
        f.seek(SeekFrom::Start(data))?;
        f.read_exact(&mut kind)?;
        if &kind != b"SND " {
            return Ok(());
        }
        let end = data + size;
        let mut pos = data + 4;
        while pos + 12 <= end {
            let (id, len) = chunk_header(f, pos)?;
            let body = pos + 12;
            let mut buf = [0u8; 4];
            match &id {
                b"FS  " if len >= 4 => {
                    f.read_exact(&mut buf)?;
                    *rate = u32::from_be_bytes(buf);
                }
                b"CHNL" if len >= 2 => {
                    f.read_exact(&mut buf[..2])?;
                    *channels = u16::from_be_bytes([buf[0], buf[1]]);
                }
                _ => {}
            }
            match next_chunk(body, len, end) {
                Some(next) => pos = next,
                None => break,
            }
        }
        Ok(())
    }

    /// DST-compressed sound: the `FRTE` chunk gives frame count and frames per second.
    fn read_dst_info<R: Read + Seek>(
        f: &mut R,
        data: u64,
        size: u64,
    ) -> Result<Option<(u32, u16)>, TagError> {
        if size < 12 + 6 {
            return Ok(None);
        }
        let (id, _) = chunk_header(f, data)?;
        if &id != b"FRTE" {
            return Ok(None);
        }
        let mut buf = [0u8; 6];
        f.read_exact(&mut buf)?;
        Ok(Some((
            u32::from_be_bytes(buf[0..4].try_into().unwrap()),
            u16::from_be_bytes([buf[4], buf[5]]),
        )))
    }

    /// `DIIN` edited-master info: `DITI` title and `DIAR` artist (u32 length + text).
    fn read_diin<R: Read + Seek>(
        f: &mut R,
        data: u64,
        size: u64,
        tags: &mut TrackTags,
    ) -> Result<(), TagError> {
        let end = data + size;
        let mut pos = data;
        while pos + 12 <= end {
            let (id, len) = chunk_header(f, pos)?;
            if (&id == b"DITI" || &id == b"DIAR") && (4..=64 * 1024).contains(&len) {
                let mut body = vec![0u8; len as usize];
                f.read_exact(&mut body)?;
                let n = (u32::from_be_bytes(body[0..4].try_into().unwrap()) as usize)
                    .min(body.len() - 4);
                if let Some(text) = clean(&String::from_utf8_lossy(&body[4..4 + n])) {
                    if &id == b"DITI" {
                        tags.title = Some(text);
                    } else {
                        tags.artists = vec![text];
                    }
                }
            }
            match next_chunk(pos + 12, len, end) {
                Some(next) => pos = next,
                None => break,
            }
        }
        Ok(())
    }

    /// An ID3 tag in a DSDIFF file and where its bytes sit. `chunked` records whether it is
    /// wrapped in an `ID3 ` chunk or is the bare tag, as each writer of these files does it.
    #[derive(Debug, Clone, Copy)]
    pub(super) struct TagSpot {
        /// Offset of the tag or of its chunk header.
        start: u64,
        /// Offset of the ID3v2 blob.
        body: u64,
        /// Length of the ID3v2 blob.
        len: u64,
        /// End of the tag or chunk, including the pad byte of an odd chunk.
        end: u64,
        chunked: bool,
    }

    /// The tag a DDF file appends after its form, if the bytes there are one.
    pub(super) fn tail_spot<R: Read + Seek>(
        f: &mut R,
        start: u64,
        file_len: u64,
    ) -> Result<Option<TagSpot>, TagError> {
        if file_len.saturating_sub(start) < 10 {
            return Ok(None);
        }
        let mut head = [0u8; 12];
        f.seek(SeekFrom::Start(start))?;
        let mut n = 0usize;
        while n < head.len() {
            match f.read(&mut head[n..])? {
                0 => break,
                got => n += got,
            }
        }
        let head = &head[..n];
        if n >= 12 && head.starts_with(b"ID3 ") {
            let size = u64::from_be_bytes(head[4..12].try_into().unwrap());
            return Ok(next_chunk(start + 12, size, file_len).map(|end| TagSpot {
                start,
                body: start + 12,
                len: size,
                end,
                chunked: true,
            }));
        }
        if head.starts_with(b"ID3") && n >= 10 {
            // A bare ID3v2 tag: its ten-byte header ends with a syncsafe size.
            let size = head[6..10]
                .iter()
                .fold(0u64, |size, byte| (size << 7) | u64::from(byte & 0x7f));
            let len = 10 + size;
            let end = start + len;
            if end <= file_len {
                return Ok(Some(TagSpot {
                    start,
                    body: start,
                    len,
                    end,
                    chunked: false,
                }));
            }
        }
        Ok(None)
    }

    /// Replaces the ID3v2 tag of a DSF file, keeping its audio untouched.
    pub(super) fn write_dsf(path: &Path, edit: &TagEdit) -> Result<(), TagError> {
        let mut source = File::open(path)?;
        let file_len = source.metadata()?.len();
        let mut header = [0u8; 28];
        source
            .read_exact(&mut header)
            .map_err(|_| bad("DSF header truncated"))?;
        if &header[0..4] != b"DSD " {
            return Err(bad("not a DSF file"));
        }
        // The metadata pointer is zero or past the end when the file has no tag yet.
        let metadata_offset = le_u64(&header[20..28]);
        let tagged = metadata_offset >= 28 && metadata_offset < file_len;
        let sound_end = if tagged { metadata_offset } else { file_len };
        let tag = if tagged {
            read_tag_at(&mut source, metadata_offset, file_len - metadata_offset)?
        } else {
            None
        };
        let blob = edited_id3(tag, edit)?;
        drop(source);

        header[12..20].copy_from_slice(&(sound_end + blob.len() as u64).to_le_bytes());
        header[20..28].copy_from_slice(&sound_end.to_le_bytes());
        rewrite(path, |source, tmp| {
            tmp.write_all(&header)?;
            copy_range(source, tmp, 28, sound_end)?;
            tmp.write_all(&blob)?;
            Ok(())
        })
    }

    /// Replaces the ID3v2 tag of a DSDIFF file, keeping its audio and its non-tag chunks.
    pub(super) fn write_dff(path: &Path, edit: &TagEdit) -> Result<(), TagError> {
        let mut source = File::open(path)?;
        let file_len = source.metadata()?.len();
        let mut header = [0u8; 16];
        source
            .read_exact(&mut header)
            .map_err(|_| bad("DFF header truncated"))?;
        if &header[0..4] != b"FRM8" || &header[12..16] != b"DSD " {
            return Err(bad("not a DSDIFF file"));
        }
        let form_end = u64::from_be_bytes(header[4..12].try_into().unwrap())
            .saturating_add(12)
            .min(file_len);

        // The tag the form carries, if any.
        let mut inside: Option<TagSpot> = None;
        let mut pos = 16u64;
        while pos + 12 <= form_end {
            let (id, size) = chunk_header(&mut source, pos)?;
            // An odd last chunk may lack its pad byte; the reader still reads it, so the
            // writer must find it too, or the edit would land in a second tag nobody reads.
            let Some(end) = next_chunk(pos + 12, size, form_end)
                .or_else(|| (pos + 12).checked_add(size).filter(|end| *end == form_end))
            else {
                break;
            };
            if &id == b"ID3 " {
                inside = Some(TagSpot {
                    start: pos,
                    body: pos + 12,
                    len: size,
                    end,
                    chunked: true,
                });
                break;
            }
            pos = end;
        }
        let tail = tail_spot(&mut source, form_end, file_len)?;

        // The tag in force is the one inside the form; a file with both keeps them in sync,
        // and the rewrite leaves only the inside one.
        let tag = match inside.or(tail) {
            Some(spot) => read_tag_at(&mut source, spot.body, spot.len)?,
            None => None,
        };
        let chunk = id3_chunk(&edited_id3(tag, edit)?);
        drop(source);

        rewrite(path, |source, tmp| match (inside, tail) {
            (Some(spot), tail) => {
                let form_len = 4
                    + (form_end - 16)
                        .saturating_sub(spot.end - spot.start)
                        .saturating_add(chunk.len() as u64);
                tmp.write_all(b"FRM8")?;
                tmp.write_all(&form_len.to_be_bytes())?;
                tmp.write_all(b"DSD ")?;
                copy_range(source, tmp, 16, spot.start)?;
                tmp.write_all(&chunk)?;
                copy_range(source, tmp, spot.end, form_end)?;
                copy_range(source, tmp, tail.map_or(form_end, |t| t.end), file_len)?;
                Ok(())
            }
            (None, Some(spot)) => {
                copy_range(source, tmp, 0, spot.start)?;
                if spot.chunked {
                    tmp.write_all(&chunk)?;
                } else {
                    tmp.write_all(&chunk[12..chunk.len() - (chunk.len() & 1)])?;
                }
                copy_range(source, tmp, spot.end, file_len)?;
                Ok(())
            }
            // No tag anywhere: append an `ID3 ` chunk after the form, the way these files
            // usually carry one. Bytes after the form that are not a tag stay where they are,
            // with the chunk put inside the form so the reader still finds it.
            (None, None) if form_end == file_len => {
                copy_range(source, tmp, 0, file_len)?;
                tmp.write_all(&chunk)?;
                Ok(())
            }
            (None, None) => {
                let form_len = 4 + (form_end - 16).saturating_add(chunk.len() as u64);
                tmp.write_all(b"FRM8")?;
                tmp.write_all(&form_len.to_be_bytes())?;
                tmp.write_all(b"DSD ")?;
                copy_range(source, tmp, 16, form_end)?;
                tmp.write_all(&chunk)?;
                copy_range(source, tmp, form_end, file_len)?;
                Ok(())
            }
        })
    }

    /// An `ID3 ` chunk holding `blob`, padded to an even length as DSDIFF requires.
    pub(super) fn id3_chunk(blob: &[u8]) -> Vec<u8> {
        let mut chunk = Vec::with_capacity(12 + blob.len() + 1);
        chunk.extend_from_slice(b"ID3 ");
        chunk.extend_from_slice(&(blob.len() as u64).to_be_bytes());
        chunk.extend_from_slice(blob);
        if blob.len() & 1 == 1 {
            chunk.push(0);
        }
        chunk
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gains_and_numbers() {
        assert_eq!(parse_gain("-6.54 dB"), Some(-6.54));
        assert_eq!(parse_gain("+1.20dB"), Some(1.2));
        assert_eq!(parse_gain("-3,5 dB"), Some(-3.5));
        assert_eq!(parse_gain("loud"), None);
        assert_eq!(parse_number("0.988525"), Some(0.988525));
    }

    #[test]
    fn years() {
        assert_eq!(parse_year("1959"), Some(1959));
        assert_eq!(parse_year("1959-08-17"), Some(1959));
        assert_eq!(parse_year("(P) 1997 Label"), Some(1997));
        assert_eq!(parse_year("12345"), None);
        assert_eq!(parse_year("n/a"), None);
    }

    #[test]
    fn file_name_fallback() {
        let cases = [
            ("03 - So What", Some(3), "So What"),
            ("03. So What", Some(3), "So What"),
            ("1-03 So What", Some(3), "So What"),
            ("07_Freddie", Some(7), "Freddie"),
            ("So What", None, "So What"),
            ("1999 Prince", None, "1999 Prince"),
            ("2112", None, "2112"),
            ("05", Some(5), "05"),
        ];
        for (stem, number, title) in cases {
            assert_eq!(split_track_number(stem), (number, title), "{stem}");
        }
    }

    #[test]
    fn extensions() {
        assert!(is_audio_file(Path::new("a/b.FLAC")));
        assert!(is_audio_file(Path::new("a/b.dsf")));
        assert!(!is_audio_file(Path::new("a/b.mp3")));
        assert!(!is_audio_file(Path::new("a/cover.jpg")));
    }

    fn chunk(id: &[u8; 4], size: u64, body: &[u8]) -> Vec<u8> {
        let mut out = id.to_vec();
        out.extend_from_slice(&size.to_be_bytes());
        out.extend_from_slice(body);
        out
    }

    fn form(chunks: &[Vec<u8>]) -> Vec<u8> {
        let body: Vec<u8> = chunks.concat();
        let mut out = b"FRM8".to_vec();
        out.extend_from_slice(&(4 + body.len() as u64).to_be_bytes());
        out.extend_from_slice(b"DSD ");
        out.extend_from_slice(&body);
        out
    }

    fn snd_prop(rate: u32) -> Vec<u8> {
        let mut prop = b"SND ".to_vec();
        prop.extend(chunk(b"FS  ", 4, &rate.to_be_bytes()));
        prop.extend(chunk(b"CHNL", 10, b"\x00\x02SLFTSRGT"));
        chunk(b"PROP", prop.len() as u64, &prop)
    }

    /// Parses `bytes` as a DFF on another thread and fails instead of hanging.
    fn read_dff_bytes(bytes: Vec<u8>) -> Result<FileMeta, String> {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.dff");
        std::fs::write(&path, bytes).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(read_file(&path).map_err(|e| e.to_string()));
        });
        rx.recv_timeout(std::time::Duration::from_secs(5))
            .expect("DFF parse did not finish (loop on a corrupt chunk size?)")
    }

    #[test]
    fn next_chunk_always_moves_forward() {
        assert_eq!(dsd::next_chunk(28, 4, 100), Some(32));
        assert_eq!(dsd::next_chunk(28, 5, 100), Some(34));
        assert_eq!(dsd::next_chunk(28, 72, 100), Some(100));
        assert_eq!(dsd::next_chunk(28, 73, 100), None);
        assert_eq!(dsd::next_chunk(28, u64::MAX - 11, u64::MAX), None);
        assert_eq!(dsd::next_chunk(28, u64::MAX, u64::MAX), None);
    }

    #[test]
    fn dff_chunk_size_that_wraps_to_itself_is_an_error_not_a_hang() {
        // A chunk whose size wraps the next offset back onto itself (28 + size = 2^64 + 16).
        let mut bytes = b"FRM8".to_vec();
        bytes.extend_from_slice(&16u64.to_be_bytes());
        bytes.extend_from_slice(b"DSD ");
        bytes.extend(chunk(b"XXXX", u64::MAX - 11, &[]));
        assert!(read_dff_bytes(bytes).is_err());
    }

    #[test]
    fn dff_corrupt_sub_chunk_sizes_are_contained() {
        // PROP whose FS sub-chunk claims nearly 2^64 bytes.
        let mut prop = b"SND ".to_vec();
        prop.extend(chunk(b"FS  ", u64::MAX - 3, &2_822_400u32.to_be_bytes()));
        let bytes = form(&[chunk(b"PROP", prop.len() as u64, &prop)]);
        assert!(read_dff_bytes(bytes).is_err());

        // DIIN whose DITI sub-chunk wraps.
        let diin = chunk(b"DITI", u64::MAX - 1, b"\0\0\0\x04Name");
        let bytes = form(&[
            snd_prop(2_822_400),
            chunk(b"DIIN", diin.len() as u64, &diin),
        ]);
        let meta = read_dff_bytes(bytes).unwrap();
        assert_eq!(meta.audio.rate, 2_822_400);
    }

    #[test]
    fn dff_sound_chunk_larger_than_the_file_is_clamped() {
        let bytes = form(&[
            snd_prop(2_822_400),
            chunk(b"DSD ", u64::MAX / 2, &[0x69; 705_600]),
        ]);
        let meta = read_dff_bytes(bytes).unwrap();
        // 705 600 bytes of stereo DSD64 is one second; the claimed size is ignored.
        assert_eq!(meta.audio.duration_ms, 1_000);
    }

    fn edit() -> TagEdit {
        TagEdit {
            title: "So What".into(),
            artists: vec!["Miles Davis".into()],
            album: "Kind of Blue".into(),
            album_artists: vec!["Miles Davis".into()],
            track_no: Some(1),
            disc_no: Some(1),
            year: Some(1959),
            genre: "Jazz".into(),
            ..Default::default()
        }
    }

    fn assert_edited(meta: &FileMeta) {
        assert_eq!(meta.tags.title.as_deref(), Some("So What"));
        assert_eq!(meta.tags.artists, ["Miles Davis"]);
        assert_eq!(meta.tags.album.as_deref(), Some("Kind of Blue"));
        assert_eq!(meta.tags.album_artists, ["Miles Davis"]);
        assert_eq!(meta.tags.track_no, Some(1));
        assert_eq!(meta.tags.disc_no, Some(1));
        assert_eq!(meta.tags.year, Some(1959));
        assert_eq!(meta.tags.genre.as_deref(), Some("Jazz"));
    }

    fn wav(path: &Path) {
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 44_100,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(path, spec).unwrap();
        for i in 0..882 {
            writer.write_sample((i % 64) as i16).unwrap();
        }
        writer.finalize().unwrap();
    }

    #[test]
    fn wav_tags_are_written_and_untouched_tags_are_kept() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("song.wav");
        wav(&path);
        write_file(&path, &edit()).unwrap();
        assert_edited(&read_file(&path).unwrap());

        // A comment is not a field of the editor; a later write must leave it alone.
        {
            let mut tagged = Probe::open(&path).unwrap().read().unwrap();
            let tag = tagged
                .primary_tag_mut()
                .expect("the first write made a tag");
            tag.insert_text(ItemKey::Comment, "keep me".into());
            tagged.save_to_path(&path, WriteOptions::new()).unwrap();
        }
        let mut second = edit();
        second.title = "Freddie Freeloader".into();
        second.artists.clear();
        second.album.clear();
        second.year = None;
        write_file(&path, &second).unwrap();

        let meta = read_file(&path).unwrap();
        assert_eq!(meta.tags.title.as_deref(), Some("Freddie Freeloader"));
        assert!(meta.tags.artists.is_empty());
        // The file name stands in for a cleared album; its own tag no longer has one.
        assert_eq!(meta.tags.year, None);
        let tagged = Probe::open(&path).unwrap().read().unwrap();
        assert_eq!(
            tagged
                .primary_tag()
                .and_then(|tag| tag.get_string(ItemKey::Comment)),
            Some("keep me")
        );
    }

    #[test]
    fn a_second_artist_spelling_does_not_survive_the_write() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("song.wav");
        wav(&path);
        write_file(&path, &edit()).unwrap();
        // Files tagged by other tools can carry the credit twice (TPE1 and TXXX:ARTISTS);
        // the reader prefers the second spelling, so clearing only TPE1 would show the old
        // name for ever.
        {
            let mut tagged = Probe::open(&path).unwrap().read().unwrap();
            let tag = tagged.primary_tag_mut().unwrap();
            tag.insert_text(ItemKey::TrackArtists, "Old Artist".into());
            tagged.save_to_path(&path, WriteOptions::new()).unwrap();
        }
        let mut second = edit();
        second.artists = vec!["New Artist".into()];
        write_file(&path, &second).unwrap();
        assert_eq!(read_file(&path).unwrap().tags.artists, ["New Artist"]);
    }

    #[test]
    fn empty_title_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("song.wav");
        wav(&path);
        let mut empty = edit();
        empty.title = "   ".into();
        assert!(write_file(&path, &empty).is_err());
    }

    /// The smallest valid-looking JPEG and PNG for the cover tests: only the signature is
    /// checked on the way in, and the bytes come back as they were stored.
    const JPEG: &[u8] = &[0xff, 0xd8, 0xff, 0xe0, 0, 16, b'J', b'F', b'I', b'F', 0, 1];
    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";

    /// Sets lyrics and a cover, checks a later edit keeps them, then replaces and removes them.
    fn lyrics_and_cover_round_trip(path: &Path) {
        let mut first = edit();
        first.lyrics = Some("[00:01.00]Line one\n[00:02.50]Line two\n".into());
        first.cover = CoverEdit::Set(JPEG.to_vec());
        write_file(path, &first).unwrap();
        let tags = read_file_tags(path).unwrap();
        assert_eq!(
            tags.lyrics.as_deref(),
            Some("[00:01.00]Line one\n[00:02.50]Line two")
        );
        assert_eq!(tags.cover.as_deref(), Some(JPEG));

        // An edit of other fields leaves both alone.
        let mut keep = edit();
        keep.title = "Blue in Green".into();
        write_file(path, &keep).unwrap();
        let tags = read_file_tags(path).unwrap();
        assert_eq!(tags.title.as_deref(), Some("Blue in Green"));
        assert!(tags.lyrics.is_some());
        assert_eq!(tags.cover.as_deref(), Some(JPEG));

        let mut replace = edit();
        replace.lyrics = Some("Plain words".into());
        replace.cover = CoverEdit::Set(PNG.to_vec());
        write_file(path, &replace).unwrap();
        let tags = read_file_tags(path).unwrap();
        assert_eq!(tags.lyrics.as_deref(), Some("Plain words"));
        assert_eq!(tags.cover.as_deref(), Some(PNG));

        let mut clear = edit();
        clear.lyrics = Some("  \n".into());
        clear.cover = CoverEdit::Remove;
        write_file(path, &clear).unwrap();
        let tags = read_file_tags(path).unwrap();
        assert_eq!(tags.lyrics, None);
        assert_eq!(tags.cover, None);
        assert_edited(&read_file(path).unwrap());
    }

    #[test]
    fn wav_lyrics_and_cover_are_written_kept_and_removed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("song.wav");
        wav(&path);
        lyrics_and_cover_round_trip(&path);
    }

    #[test]
    fn dsf_lyrics_and_cover_are_written_kept_and_removed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("song.dsf");
        std::fs::write(&path, dsf(2_822_400, &[0x69; 4096])).unwrap();
        lyrics_and_cover_round_trip(&path);
    }

    #[test]
    fn a_cover_that_is_not_jpeg_or_png_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("song.wav");
        wav(&path);
        let mut gif = edit();
        gif.cover = CoverEdit::Set(b"GIF89a....".to_vec());
        assert!(write_file(&path, &gif).is_err());
        assert_eq!(cover_mime(JPEG), Some("image/jpeg"));
        assert_eq!(cover_mime(PNG), Some("image/png"));
    }

    fn dsf(samples: u64, sound: &[u8]) -> Vec<u8> {
        let data_len = 12 + sound.len() as u64;
        let total = 28 + 52 + data_len;
        let mut out = Vec::new();
        out.extend_from_slice(b"DSD ");
        out.extend_from_slice(&28u64.to_le_bytes());
        out.extend_from_slice(&total.to_le_bytes());
        out.extend_from_slice(&0u64.to_le_bytes());
        out.extend_from_slice(b"fmt ");
        out.extend_from_slice(&52u64.to_le_bytes());
        out.extend_from_slice(&1u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&2u32.to_le_bytes());
        out.extend_from_slice(&2_822_400u32.to_le_bytes());
        out.extend_from_slice(&1u32.to_le_bytes());
        out.extend_from_slice(&samples.to_le_bytes());
        out.extend_from_slice(&4096u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_len.to_le_bytes());
        out.extend_from_slice(sound);
        out
    }

    #[test]
    fn dsf_tags_are_written_behind_the_audio_and_rewritten_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("track.dsf");
        std::fs::write(&path, dsf(2_822_400, &[0x69; 4096])).unwrap();
        let before = read_file(&path).unwrap();

        write_file(&path, &edit()).unwrap();
        let after = read_file(&path).unwrap();
        assert_edited(&after);
        assert_eq!(after.audio, before.audio);

        // The header points at the tag that follows the audio.
        let raw = std::fs::read(&path).unwrap();
        let metadata = u64::from_le_bytes(raw[20..28].try_into().unwrap());
        assert!(metadata > 28);
        assert!(raw[metadata as usize..].starts_with(b"ID3"));
        assert_eq!(
            u64::from_le_bytes(raw[12..20].try_into().unwrap()),
            raw.len() as u64
        );

        // A second write replaces the tag instead of appending another one.
        let mut second = edit();
        second.title = "Blue in Green".into();
        write_file(&path, &second).unwrap();
        let raw = std::fs::read(&path).unwrap();
        assert_eq!(raw.windows(3).filter(|w| *w == b"ID3").count(), 1);
        assert_eq!(
            read_file(&path).unwrap().tags.title.as_deref(),
            Some("Blue in Green")
        );
    }

    #[test]
    fn dff_tags_are_appended_after_the_form_and_rewritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("track.dff");
        let sound = [0x69; 705_600];
        let bytes = form(&[
            snd_prop(2_822_400),
            chunk(b"DSD ", sound.len() as u64, &sound),
        ]);
        std::fs::write(&path, &bytes).unwrap();

        write_file(&path, &edit()).unwrap();
        let meta = read_file(&path).unwrap();
        assert_edited(&meta);
        assert_eq!(meta.audio.duration_ms, 1_000);
        let raw = std::fs::read(&path).unwrap();
        assert_eq!(&raw[..func_head(&bytes)], &bytes[..func_head(&bytes)]);

        let mut second = edit();
        second.title = "All Blues".into();
        write_file(&path, &second).unwrap();
        assert_eq!(
            read_file(&path).unwrap().tags.title.as_deref(),
            Some("All Blues")
        );
    }

    /// The header bytes of `form`, whose size the append must not touch.
    fn func_head(bytes: &[u8]) -> usize {
        let size = u64::from_be_bytes(bytes[4..12].try_into().unwrap());
        12 + size as usize
    }

    #[test]
    fn dff_data_after_the_form_is_kept_and_the_tag_goes_inside() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tail.dff");
        let sound = [0x69; 705_600];
        let mut bytes = form(&[
            snd_prop(2_822_400),
            chunk(b"DSD ", sound.len() as u64, &sound),
        ]);
        bytes.extend_from_slice(b"JUNK");
        std::fs::write(&path, &bytes).unwrap();
        // No tag yet: the title comes from the file name.
        assert_eq!(
            read_file(&path).unwrap().tags.title.as_deref(),
            Some("tail")
        );

        write_file(&path, &edit()).unwrap();
        let meta = read_file(&path).unwrap();
        assert_edited(&meta);
        let raw = std::fs::read(&path).unwrap();
        assert!(raw.ends_with(b"JUNK"));
        // The form grew by the chunk, and its size still covers everything before the tail.
        assert_eq!(
            u64::from_be_bytes(raw[4..12].try_into().unwrap()),
            raw.len() as u64 - 12 - 4
        );
    }

    #[test]
    fn dff_tag_inside_the_form_is_replaced_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("inside.dff");
        let mut old = Tag::new(TagType::Id3v2);
        old.set_title("Old title".into());
        let blob = dump_id3(&Id3v2Tag::from(old)).unwrap();
        let sound = [0x69; 705_600];
        let bytes = form(&[
            snd_prop(2_822_400),
            chunk(b"DSD ", sound.len() as u64, &sound),
            chunk(b"ID3 ", blob.len() as u64, &blob),
        ]);
        std::fs::write(&path, &bytes).unwrap();
        assert_eq!(
            read_file(&path).unwrap().tags.title.as_deref(),
            Some("Old title")
        );

        write_file(&path, &edit()).unwrap();
        let meta = read_file(&path).unwrap();
        assert_edited(&meta);
        assert_eq!(meta.audio.duration_ms, 1_000);
        let raw = std::fs::read(&path).unwrap();
        // The form still holds exactly one ID3 chunk, in the place the old one had.
        assert_eq!(raw.windows(4).filter(|w| *w == b"ID3 ").count(), 1);
        assert_eq!(
            u64::from_be_bytes(raw[4..12].try_into().unwrap()),
            raw.len() as u64 - 12
        );
    }

    #[test]
    fn several_artists_are_stored_as_separate_names() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("song.wav");
        wav(&path);
        let mut two = edit();
        two.artists = vec!["Miles Davis".into(), "John Coltrane".into()];
        two.album_artists = vec!["Miles Davis".into(), "Bill Evans".into()];
        write_file(&path, &two).unwrap();
        let tags = read_file(&path).unwrap().tags;
        assert_eq!(tags.artists, ["Miles Davis", "John Coltrane"]);
        assert_eq!(tags.album_artists, ["Miles Davis", "Bill Evans"]);
    }

    #[test]
    fn several_artists_survive_a_dsd_write() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("track.dsf");
        std::fs::write(&path, dsf(2_822_400, &[0x69; 4096])).unwrap();
        let mut two = edit();
        two.artists = vec!["Miles Davis".into(), "John Coltrane".into()];
        write_file(&path, &two).unwrap();
        assert_eq!(
            read_file(&path).unwrap().tags.artists,
            ["Miles Davis", "John Coltrane"]
        );
    }

    #[test]
    fn a_cleared_field_does_not_come_back_from_a_second_tag() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("song.wav");
        wav(&path);
        // A WAV tagged by a ripper often carries RIFF INFO next to its ID3v2 tag; the reader
        // falls back to it, so leaving it alone would bring cleared values back.
        {
            let mut tagged = Probe::open(&path).unwrap().read().unwrap();
            let mut info = Tag::new(TagType::RiffInfo);
            info.insert_text(ItemKey::TrackArtist, "Old Artist".into());
            info.insert_text(ItemKey::Genre, "Old Genre".into());
            tagged.insert_tag(info);
            tagged.save_to_path(&path, WriteOptions::new()).unwrap();
        }
        let mut cleared = edit();
        cleared.artists.clear();
        cleared.genre.clear();
        write_file(&path, &cleared).unwrap();
        let tags = read_file(&path).unwrap().tags;
        assert!(tags.artists.is_empty(), "{:?}", tags.artists);
        assert_eq!(tags.genre, None);
    }

    #[test]
    fn dsd_rewrite_keeps_id3_frames_the_editor_does_not_know() {
        use lofty::id3::v2::{Frame, PrivateFrame};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("private.dff");
        let mut old = Id3v2Tag::new();
        old.set_title("Old title".into());
        old.insert(Frame::Private(PrivateFrame::new(
            "sonder-test",
            vec![1u8, 2, 3],
        )));
        let blob = dump_id3(&old).unwrap();
        let sound = [0x69; 705_600];
        let bytes = form(&[
            snd_prop(2_822_400),
            chunk(b"DSD ", sound.len() as u64, &sound),
            dsd::id3_chunk(&blob),
        ]);
        std::fs::write(&path, &bytes).unwrap();

        write_file(&path, &edit()).unwrap();
        assert_edited(&read_file(&path).unwrap());
        let raw = std::fs::read(&path).unwrap();
        assert!(
            raw.windows(11).any(|w| w == b"sonder-test"),
            "the PRIV frame was dropped"
        );
    }

    #[test]
    fn dff_unpadded_last_tag_chunk_is_replaced_not_shadowed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("unpadded.dff");
        let mut old = Tag::new(TagType::Id3v2);
        old.set_title("Old title".into());
        let mut blob = dump_id3(&Id3v2Tag::from(old)).unwrap();
        if blob.len().is_multiple_of(2) {
            blob.push(0); // ID3 padding, so the chunk is odd and needs a pad byte it lacks.
        }
        let sound = [0x69; 705_600];
        let bytes = form(&[
            snd_prop(2_822_400),
            chunk(b"DSD ", sound.len() as u64, &sound),
            chunk(b"ID3 ", blob.len() as u64, &blob),
        ]);
        std::fs::write(&path, &bytes).unwrap();
        assert_eq!(
            read_file(&path).unwrap().tags.title.as_deref(),
            Some("Old title")
        );

        write_file(&path, &edit()).unwrap();
        assert_edited(&read_file(&path).unwrap());
        let raw = std::fs::read(&path).unwrap();
        assert_eq!(raw.windows(4).filter(|w| *w == b"ID3 ").count(), 1);
    }

    #[test]
    fn a_read_only_file_is_refused_and_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("track.dsf");
        std::fs::write(&path, dsf(2_822_400, &[0x69; 4096])).unwrap();
        let before = std::fs::read(&path).unwrap();
        let mut perms = std::fs::metadata(&path).unwrap().permissions();
        perms.set_readonly(true);
        std::fs::set_permissions(&path, perms.clone()).unwrap();

        let refused = write_file(&path, &edit()).is_err();
        let after = std::fs::read(&path).unwrap();
        #[allow(clippy::permissions_set_readonly_false)]
        perms.set_readonly(false);
        std::fs::set_permissions(&path, perms).unwrap();
        assert!(refused);
        assert_eq!(after, before);
    }

    #[cfg(unix)]
    #[test]
    fn a_rewritten_file_keeps_its_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("track.dsf");
        std::fs::write(&path, dsf(2_822_400, &[0x69; 4096])).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        write_file(&path, &edit()).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o644);
    }

    #[test]
    fn a_lofty_write_leaves_no_temporary_file_behind() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("song.wav");
        wav(&path);
        write_file(&path, &edit()).unwrap();
        let names: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, [std::ffi::OsString::from("song.wav")]);
        assert_edited(&read_file(&path).unwrap());
    }

    #[test]
    fn the_editor_sees_the_file_tags_without_file_name_guesses() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("07 - Untitled.wav");
        wav(&path);
        // The library shows a track number guessed from the name ...
        assert_eq!(read_file(&path).unwrap().tags.track_no, Some(7));
        // ... but the file holds none, so the editor must not offer one to save. The title
        // stays filled in: a track needs one, and the field shows exactly what Save writes.
        let editable = read_file_tags(&path).unwrap();
        assert_eq!(editable.track_no, None);
        assert_eq!(editable.title.as_deref(), Some("Untitled"));
    }
}
