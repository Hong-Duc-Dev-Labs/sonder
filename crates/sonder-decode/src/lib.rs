//! Audio file decoding over symphonia.
//!
//! [`Decoder`] yields interleaved frames. Integer output is *left-justified* `i32`: a 16-bit
//! source sample `s` arrives as `s << 16`, a 24-bit one as `s << 8`. The valid bit count is
//! reported in [`SourceFormat::bits`], so later stages can pack samples into a device container
//! without changing their value.

use std::fs::File;
use std::path::Path;

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
use symphonia::core::formats::{FormatOptions, FormatReader, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
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
    /// Packets dropped because their payload was corrupt.
    skipped_packets: u64,
}

impl Decoder {
    /// Opens `path` and selects its default audio track.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, DecodeError> {
        let path = path.as_ref();
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
            .make_audio_decoder(&params, &AudioDecoderOptions::default())?;

        Ok(Self {
            reader,
            decoder,
            track_id,
            format: SourceFormat {
                rate,
                bits,
                channels,
            },
            skipped_packets: 0,
        })
    }

    pub fn format(&self) -> SourceFormat {
        self.format
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
            match self.decoder.decode(&packet) {
                Ok(buf) => {
                    let frames = copy_checked(&buf, self.format.channels, out)?;
                    if frames > 0 {
                        return Ok(frames);
                    }
                }
                Err(SymphoniaError::DecodeError(_)) => self.skipped_packets += 1,
                Err(e) => return Err(e.into()),
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
