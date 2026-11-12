//! Tag and technical-format reading.
//!
//! FLAC, ALAC (MP4), WAV and AIFF go through lofty. lofty has no DSD support, so DSF and DFF
//! are parsed here: their headers give the format, and their ID3v2 blob is handed to lofty's
//! ID3v2 reader. A DFF without ID3 falls back to its native `DIIN` title/artist chunks, and any
//! file without a title falls back to its file name.

use std::fs::File;
use std::io::{self, Cursor, Read, Seek, SeekFrom};
use std::path::Path;

use lofty::config::ParseOptions;
use lofty::file::{AudioFile, FileType, TaggedFile, TaggedFileExt};
use lofty::mp4::{Mp4Codec, Mp4File};
use lofty::picture::PictureType;
use lofty::probe::Probe;
use lofty::tag::{Accessor, ItemKey, Tag};

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
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    let mut meta = match ext.as_str() {
        "dsf" => dsd::read_dsf(path)?,
        "dff" => dsd::read_dff(path)?,
        "m4a" | "mp4" => read_mp4(path)?,
        _ => read_lofty(path)?,
    };
    fill_from_file_name(&mut meta.tags, path);
    Ok(meta)
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
        lyrics: first(tags, |t| {
            t.get_string(ItemKey::Lyrics)
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

/// Parses a raw ID3v2 tag (as embedded in DSF/DFF) with lofty.
fn parse_id3v2(blob: Vec<u8>) -> Result<TrackTags, TagError> {
    // lofty's MPEG reader accepts a stream that is only an ID3v2 tag when it is not asked
    // for audio properties; that is the only public entry point to its ID3v2 parser.
    let tagged = Probe::new(Cursor::new(blob))
        .set_file_type(FileType::Mpeg)
        .options(ParseOptions::new().read_properties(false))
        .read()?;
    Ok(extract(&tagged))
}

fn read_id3_at<R: Read + Seek>(
    r: &mut R,
    offset: u64,
    len: u64,
) -> Result<Option<TrackTags>, TagError> {
    if len < 10 {
        return Ok(None);
    }
    r.seek(SeekFrom::Start(offset))?;
    let mut blob = Vec::new();
    r.take(len.min(MAX_ID3_BYTES)).read_to_end(&mut blob)?;
    if !blob.starts_with(b"ID3") {
        return Ok(None);
    }
    parse_id3v2(blob).map(Some)
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
            read_id3_at(&mut f, metadata_offset, file_len - metadata_offset)?.unwrap_or_default()
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
        let mut id3: Option<TrackTags> = None;
        let mut native = TrackTags::default();

        let mut pos = 16u64;
        while pos + 12 <= form_end {
            let (id, size) = chunk_header(&mut f, pos)?;
            let data = pos + 12;
            // A size past the end of the form is clamped: the last chunk of a truncated
            // download is still worth reading.
            let len = size.min(form_end.saturating_sub(data));
            match &id {
                b"PROP" => read_prop(&mut f, data, len, &mut rate, &mut channels)?,
                b"DSD " => sound_bytes = Some(len),
                b"DST " => dst_frames = read_dst_info(&mut f, data, len)?,
                b"DIIN" => read_diin(&mut f, data, len, &mut native)?,
                b"ID3 " => id3 = read_id3_at(&mut f, data, len)?,
                _ => {}
            }
            match next_chunk(data, size, form_end) {
                Some(next) => pos = next,
                None => break,
            }
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
        let mut tags = id3.unwrap_or_default();
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

    fn chunk_header(f: &mut File, pos: u64) -> Result<([u8; 4], u64), TagError> {
        let mut h = [0u8; 12];
        f.seek(SeekFrom::Start(pos))?;
        f.read_exact(&mut h)
            .map_err(|_| bad("DSDIFF chunk truncated"))?;
        Ok((
            h[0..4].try_into().unwrap(),
            u64::from_be_bytes(h[4..12].try_into().unwrap()),
        ))
    }

    fn read_prop(
        f: &mut File,
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
    fn read_dst_info(f: &mut File, data: u64, size: u64) -> Result<Option<(u32, u16)>, TagError> {
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
    fn read_diin(f: &mut File, data: u64, size: u64, tags: &mut TrackTags) -> Result<(), TagError> {
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
}
