//! Audio file decoding: integer PCM over symphonia, plus DSF and DSDIFF through [`dsd`].
//!
//! [`Decoder`] yields interleaved frames. Integer output is *left-justified* `i32`: a 16-bit
//! source sample `s` arrives as `s << 16`, a 24-bit one as `s << 8`. The valid bit count is
//! reported in [`SourceFormat::bits`], so later stages can pack samples into a device container
//! without changing their value. [`SourceDecoder`] adds DSD, whose readers emit interleaved
//! MSB-first bytes (eight 1-bit samples per channel per byte).

use std::fs::File;
use std::path::Path;

pub mod dsd;

pub use dsd::{DSD_RATES, DsdDecoder, DsdFormat};

use symphonia::core::audio::GenericAudioBufferRef;
use symphonia::core::audio::conv::ConvertibleSample;
use symphonia::core::audio::sample::SampleFormat;
use symphonia::core::codecs::audio::well_known::{
    CODEC_ID_ALAC, CODEC_ID_PCM_F32BE, CODEC_ID_PCM_F32BE_PLANAR, CODEC_ID_PCM_F32LE,
    CODEC_ID_PCM_F32LE_PLANAR, CODEC_ID_PCM_F64BE, CODEC_ID_PCM_F64BE_PLANAR, CODEC_ID_PCM_F64LE,
    CODEC_ID_PCM_F64LE_PLANAR,
};
use symphonia::core::codecs::audio::{AudioCodecParameters, AudioDecoder, AudioDecoderOptions};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::packet::Packet;
use symphonia::core::units::{Duration as TbDuration, TimeBase, Timestamp};
use symphonia_common::apple::audio::alac::MagicCookie;

/// Format of the decoded stream as stored in the source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceFormat {
    /// Sample rate in Hz.
    pub rate: u32,
    /// Valid bits per sample (16, 24, 32, ...).
    pub bits: u32,
    /// Interleaved channel count.
    pub channels: u16,
}

/// [`SourceFormat::bits`] of a DSD stream on the engine timeline (one bit per sample). No
/// PCM source reports it, so it identifies DSD tracks.
pub const DSD_SOURCE_BITS: u32 = 1;

/// What a file contains: integer PCM through symphonia, or DSD through
/// [`dsd::DsdDecoder`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackFormat {
    Pcm(SourceFormat),
    Dsd(DsdFormat),
}

/// A decoder for either signal family.
///
/// Position and seek units are the source's own: PCM frames for PCM, byte frames (eight DSD
/// bits per channel) for DSD. [`source_format`](Self::source_format) turns both into the
/// timeline the engine reports, where a DSD byte frame is one "sample" at the byte rate.
pub enum SourceDecoder {
    Pcm(Decoder),
    Dsd(DsdDecoder),
}

impl SourceDecoder {
    /// Opens `path`, sniffing the container's magic before falling back to symphonia.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, DecodeError> {
        Self::open_with(path.as_ref(), false)
    }

    /// Opens `path` like [`open`](Self::open), with PCM opened through
    /// [`Decoder::open_verifying`]. DSD containers carry no checksum.
    pub fn open_verifying(path: impl AsRef<Path>) -> Result<Self, DecodeError> {
        Self::open_with(path.as_ref(), true)
    }

    fn open_with(path: &Path, verify: bool) -> Result<Self, DecodeError> {
        let mut magic = [0u8; 4];
        let head = {
            use std::io::Read;
            File::open(path).and_then(|mut file| {
                file.read_exact(&mut magic)?;
                Ok(magic)
            })
        };
        match head {
            Ok(m) if &m == b"DSD " || &m == b"FRM8" => Ok(Self::Dsd(DsdDecoder::open(path)?)),
            _ => Ok(Self::Pcm(Decoder::open_with(path, verify)?)),
        }
    }

    pub fn format(&self) -> TrackFormat {
        match self {
            Self::Pcm(d) => TrackFormat::Pcm(d.format()),
            Self::Dsd(d) => TrackFormat::Dsd(d.format()),
        }
    }

    /// The timeline view used by the engine: sample rate, valid bits and channels.
    ///
    /// A DSD stream is reported as 1-bit samples at its byte rate, so frame positions and
    /// durations line up with the byte frames the reader returns.
    pub fn source_format(&self) -> SourceFormat {
        match self {
            Self::Pcm(d) => d.format(),
            Self::Dsd(d) => {
                let dsd = d.format();
                SourceFormat {
                    rate: dsd.byte_rate(),
                    bits: DSD_SOURCE_BITS,
                    channels: dsd.channels,
                }
            }
        }
    }

    /// Source units read so far.
    pub fn position(&self) -> u64 {
        match self {
            Self::Pcm(d) => d.position(),
            Self::Dsd(d) => d.position(),
        }
    }

    /// Source units in the track, if known.
    pub fn total(&self) -> Option<u64> {
        match self {
            Self::Pcm(d) => d.total_frames(),
            Self::Dsd(d) => d.total_bytes(),
        }
    }

    /// Moves to source unit `unit`.
    pub fn seek(&mut self, unit: u64) -> Result<(), DecodeError> {
        match self {
            Self::Pcm(d) => d.seek(unit),
            Self::Dsd(d) => d.seek_bytes(unit),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    #[error("cannot open file: {0}")]
    Open(#[from] std::io::Error),
    #[error("unsupported or unrecognised file: {0}")]
    Unsupported(String),
    #[error("decode failed: {0}")]
    Decode(String),
}

impl From<SymphoniaError> for DecodeError {
    fn from(err: SymphoniaError) -> Self {
        match err {
            SymphoniaError::Unsupported(msg) => DecodeError::Unsupported(msg.to_string()),
            other => DecodeError::Decode(other.to_string()),
        }
    }
}

/// Decodes one audio track of a file into interleaved frames.
pub struct Decoder {
    reader: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track_id: u32,
    format: SourceFormat,
    is_alac: bool,
    /// Track timebase when it is not simply one tick per frame.
    time_base: Option<TimeBase>,
    /// Timestamp of the first playable frame.
    start_ts: i64,
    /// Playable frames (encoder delay and padding excluded), when the container says.
    total_frames: Option<u64>,
    /// Index of the next frame [`next_i32`](Self::next_i32) or [`next_f64`](Self::next_f64)
    /// returns, counted in playable frames from the start of the track.
    position: u64,
    /// After a seek: frames before this index are decoded and dropped.
    seek_target: Option<u64>,
    /// Packets dropped because their payload was corrupt.
    skipped_packets: u64,
}

impl Decoder {
    /// Opens `path` and selects its default audio track.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, DecodeError> {
        Self::open_with(path.as_ref(), false)
    }

    /// Opens `path` like [`open`](Self::open) and asks the codec to check the decoded audio
    /// against the checksum the file carries (FLAC's STREAMINFO MD5). Read the outcome with
    /// [`verification`](Self::verification) after the last block, without seeking.
    pub fn open_verifying(path: impl AsRef<Path>) -> Result<Self, DecodeError> {
        Self::open_with(path.as_ref(), true)
    }

    fn open_with(path: &Path, verify: bool) -> Result<Self, DecodeError> {
        let file = File::open(path)?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());

        let mut hint = Hint::new();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            hint.with_extension(ext);
        }

        let reader = symphonia::default::get_probe().probe(
            &hint,
            mss,
            FormatOptions::default(),
            MetadataOptions::default(),
        )?;

        let track = reader
            .default_track(TrackType::Audio)
            .ok_or_else(|| DecodeError::Unsupported("no audio track".into()))?;
        let track_id = track.id;
        let track_time_base = track.time_base;
        let start_ts = track.start_ts.get();
        let total_frames = track.num_frames;
        let params = track
            .codec_params
            .as_ref()
            .and_then(|p| p.audio())
            .ok_or_else(|| DecodeError::Unsupported("audio track has no codec parameters".into()))?
            .clone();

        if is_float_source(&params) {
            return Err(DecodeError::Unsupported(
                "floating-point PCM sources cannot be played bit-perfect to an integer device"
                    .into(),
            ));
        }

        let rate = params
            .sample_rate
            .ok_or_else(|| DecodeError::Unsupported("unknown sample rate".into()))?;
        let bits = params
            .bits_per_sample
            .or_else(|| alac_bit_depth(&params))
            .ok_or_else(|| DecodeError::Unsupported("unknown bit depth".into()))?;
        let channels = params
            .channels
            .as_ref()
            .map(|c| c.count())
            .ok_or_else(|| DecodeError::Unsupported("unknown channel layout".into()))?;
        let channels = u16::try_from(channels)
            .map_err(|_| DecodeError::Unsupported("too many channels".into()))?;
        if bits == 0 || bits > 32 || channels == 0 || rate == 0 {
            return Err(DecodeError::Unsupported(format!(
                "invalid stream format: {rate} Hz, {bits} bit, {channels} ch"
            )));
        }

        let decoder = symphonia::default::get_codecs()
            .make_audio_decoder(&params, &AudioDecoderOptions::default().verify(verify))?;

        // One tick per frame is the common case; anything else is converted explicitly.
        let time_base = track_time_base.filter(|tb| tb.numer.get() != 1 || tb.denom.get() != rate);

        Ok(Self {
            reader,
            decoder,
            track_id,
            format: SourceFormat {
                rate,
                bits,
                channels,
            },
            is_alac: params.codec == CODEC_ID_ALAC,
            time_base,
            start_ts,
            total_frames,
            position: 0,
            seek_target: None,
            skipped_packets: 0,
        })
    }

    pub fn format(&self) -> SourceFormat {
        self.format
    }

    /// Whether the stream is Apple Lossless (as opposed to FLAC or PCM).
    pub fn is_alac(&self) -> bool {
        self.is_alac
    }

    /// Playable frames in the track (encoder delay and padding excluded), if known.
    pub fn total_frames(&self) -> Option<u64> {
        self.total_frames
    }

    /// Index of the next frame the decoder returns, in playable frames from the track start.
    pub fn position(&self) -> u64 {
        self.position
    }

    /// Moves to exactly `frame` (playable frames from the track start).
    ///
    /// The container seek may land early (FLAC without a seek table, coarse indexes); the
    /// frames before `frame` are then decoded and dropped, so the next block starts on the
    /// requested frame.
    pub fn seek(&mut self, frame: u64) -> Result<(), DecodeError> {
        let frame = self.total_frames.map_or(frame, |total| frame.min(total));
        let ts = self.start_ts.saturating_add(self.ticks_from_frames(frame));
        self.reader.seek(
            SeekMode::Accurate,
            SeekTo::Timestamp {
                ts: Timestamp::new(ts),
                track_id: self.track_id,
            },
        )?;
        self.decoder.reset();
        self.position = frame;
        self.seek_target = Some(frame);
        Ok(())
    }

    fn frames_from_ticks(&self, ticks: u64) -> u64 {
        match self.time_base {
            None => ticks,
            Some(tb) => {
                let v =
                    u128::from(ticks) * u128::from(tb.numer.get()) * u128::from(self.format.rate)
                        / u128::from(tb.denom.get());
                u64::try_from(v).unwrap_or(u64::MAX)
            }
        }
    }

    fn ticks_from_frames(&self, frames: u64) -> i64 {
        let ticks = match self.time_base {
            None => u128::from(frames),
            Some(tb) => {
                u128::from(frames) * u128::from(tb.denom.get())
                    / (u128::from(tb.numer.get()) * u128::from(self.format.rate))
            }
        };
        i64::try_from(ticks).unwrap_or(i64::MAX)
    }

    fn frames_from_duration(&self, dur: TbDuration) -> usize {
        usize::try_from(self.frames_from_ticks(dur.get())).unwrap_or(usize::MAX)
    }

    /// Frame index (from the track start) of the first playable frame in `packet`.
    fn packet_start_frame(&self, packet: &Packet) -> i64 {
        let first_valid = packet
            .pts
            .get()
            .saturating_add_unsigned(packet.trim_start.get());
        let offset = first_valid.saturating_sub(self.start_ts);
        let frames = self.frames_from_ticks(offset.unsigned_abs());
        let frames = i64::try_from(frames).unwrap_or(i64::MAX);
        if offset < 0 { -frames } else { frames }
    }

    /// Whether the decoded audio matched the file's checksum: `None` when the decoder was not
    /// opened with [`open_verifying`](Self::open_verifying), the format has no checksum, or the
    /// file carries none. Meaningful only once the whole stream has been decoded from the start.
    pub fn verification(&mut self) -> Option<bool> {
        self.decoder.finalize().verify_ok
    }

    /// Number of corrupt packets skipped so far.
    pub fn skipped_packets(&self) -> u64 {
        self.skipped_packets
    }

    /// Replaces `out` with the next block of left-justified interleaved `i32` samples.
    ///
    /// Returns the number of frames written; `0` means end of stream.
    pub fn next_i32(&mut self, out: &mut Vec<i32>) -> Result<usize, DecodeError> {
        self.next_block(out)
    }

    /// Replaces `out` with the next block of interleaved `f64` samples in `[-1.0, 1.0)`.
    ///
    /// Returns the number of frames written; `0` means end of stream.
    pub fn next_f64(&mut self, out: &mut Vec<f64>) -> Result<usize, DecodeError> {
        self.next_block(out)
    }

    fn next_block<S: ConvertibleSample>(&mut self, out: &mut Vec<S>) -> Result<usize, DecodeError> {
        out.clear();
        loop {
            let packet = match self.reader.next_packet() {
                Ok(Some(packet)) => packet,
                Ok(None) => return Ok(0),
                Err(SymphoniaError::IoError(e))
                    if e.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    return Ok(0);
                }
                Err(e) => return Err(e.into()),
            };
            if packet.track_id != self.track_id {
                continue;
            }
            let channels = usize::from(self.format.channels);
            let trim_start = self.frames_from_duration(packet.trim_start);
            let trim_end = self.frames_from_duration(packet.trim_end);
            let start_frame = self.packet_start_frame(&packet);
            let decoded = match self.decoder.decode(&packet) {
                Ok(buf) => copy_checked(&buf, self.format.channels, out)?,
                Err(SymphoniaError::DecodeError(_)) => {
                    self.skipped_packets += 1;
                    continue;
                }
                Err(e) => return Err(e.into()),
            };

            // Encoder delay and padding are decoded but never played; this is what makes
            // gapless albums in lossy-framed containers line up.
            let keep_end = decoded.saturating_sub(trim_end);
            let mut keep_start = trim_start.min(keep_end);
            if let Some(target) = self.seek_target {
                let target = i64::try_from(target).unwrap_or(i64::MAX);
                let before = usize::try_from(target.saturating_sub(start_frame)).unwrap_or(0);
                keep_start = keep_start.saturating_add(before).min(keep_end);
                if keep_start < keep_end {
                    self.seek_target = None;
                }
            }
            out.truncate(keep_end * channels);
            out.drain(..keep_start * channels);
            let frames = keep_end - keep_start;
            if frames > 0 {
                self.position += frames as u64;
                return Ok(frames);
            }
        }
    }
}

/// Floating-point PCM cannot reach an integer device unchanged. Readers mark it through the
/// codec id (WAV, AIFF, MP4/MOV) and rarely through `sample_format`, so check both.
fn is_float_source(params: &AudioCodecParameters) -> bool {
    matches!(
        params.codec,
        CODEC_ID_PCM_F32LE
            | CODEC_ID_PCM_F32LE_PLANAR
            | CODEC_ID_PCM_F32BE
            | CODEC_ID_PCM_F32BE_PLANAR
            | CODEC_ID_PCM_F64LE
            | CODEC_ID_PCM_F64LE_PLANAR
            | CODEC_ID_PCM_F64BE
            | CODEC_ID_PCM_F64BE_PLANAR
    ) || matches!(
        params.sample_format,
        Some(SampleFormat::F32 | SampleFormat::F64)
    )
}

/// The MP4 reader leaves `bits_per_sample` empty for ALAC; the depth lives in the codec's
/// magic cookie (the decoder left-justifies samples by the same value).
fn alac_bit_depth(params: &AudioCodecParameters) -> Option<u32> {
    if params.codec != CODEC_ID_ALAC {
        return None;
    }
    let cookie = MagicCookie::read(params.extra_data.as_deref()?).ok()?;
    Some(u32::from(cookie.bit_depth))
}

fn copy_checked<S: ConvertibleSample>(
    buf: &GenericAudioBufferRef<'_>,
    channels: u16,
    out: &mut Vec<S>,
) -> Result<usize, DecodeError> {
    let got = buf.spec().channels().count();
    if got != usize::from(channels) {
        return Err(DecodeError::Decode(format!(
            "channel count changed mid-stream ({channels} -> {got})"
        )));
    }
    buf.copy_to_vec_interleaved(out);
    Ok(buf.frames())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(codec: symphonia::core::codecs::audio::AudioCodecId) -> AudioCodecParameters {
        let mut p = AudioCodecParameters::new();
        p.for_codec(codec)
            .with_sample_rate(48_000)
            .with_bits_per_sample(32);
        p
    }

    /// The MP4/MOV reader reports float LPCM only through the codec id, with a 32-bit depth
    /// and no sample format, which looks exactly like 32-bit integer PCM otherwise.
    #[test]
    fn float_codec_ids_are_detected_without_sample_format() {
        for codec in [
            CODEC_ID_PCM_F32LE,
            CODEC_ID_PCM_F32BE,
            CODEC_ID_PCM_F64LE,
            CODEC_ID_PCM_F64BE,
        ] {
            assert!(is_float_source(&params(codec)), "{codec:?}");
        }
    }

    #[test]
    fn integer_pcm_is_not_float() {
        use symphonia::core::codecs::audio::well_known::CODEC_ID_PCM_S32LE;
        assert!(!is_float_source(&params(CODEC_ID_PCM_S32LE)));
        assert!(!is_float_source(&params(CODEC_ID_ALAC)));
    }

    #[test]
    fn float_sample_format_is_detected() {
        let mut p = params(CODEC_ID_ALAC);
        p.with_sample_format(SampleFormat::F32);
        assert!(is_float_source(&p));
    }
}
