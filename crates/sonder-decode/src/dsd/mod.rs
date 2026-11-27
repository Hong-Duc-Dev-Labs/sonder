//! DSF and DSDIFF (DFF) readers.
//!
//! Both containers are normalised to one representation: interleaved, **MSB-first** DSD
//! bytes, one byte (eight 1-bit samples) per channel per byte frame. DSF stores
//! channel-major blocks and may store bits LSB-first; the readers undo both, so everything
//! downstream (DoP packing, DSD→PCM) only sees the DFF layout.
//!
//! DST-compressed DSDIFF is rejected with [`DecodeError::Unsupported`]; only uncompressed
//! `DSD ` audio is in scope.
//!
//! Chunk sizes from the file are validated against the file length before anything is
//! allocated, so a corrupt header cannot make the reader reserve unbounded memory.

mod dff;
mod dsf;

use std::fs::File;
use std::io::{Seek, SeekFrom};
use std::path::Path;
use std::time::Duration;

use crate::{DecodeError, ReadSeek};

/// DSD sample rates the player supports, in bits per second per channel.
pub const DSD_RATES: [u32; 3] = [2_822_400, 5_644_800, 11_289_600];

/// Format of a DSD stream as stored in the source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DsdFormat {
    /// DSD bit rate per channel in bits per second (2822400, 5644800, 11289600).
    pub rate: u32,
    /// Interleaved channel count.
    pub channels: u16,
    /// Playable byte frames per channel, when the container states it.
    pub bytes: Option<u64>,
}

impl DsdFormat {
    /// Byte frames (eight DSD bits per channel) per second.
    pub const fn byte_rate(&self) -> u32 {
        self.rate / 8
    }

    /// DSD64/128/256 designation for this rate, for logs and the UI.
    pub const fn name(&self) -> &'static str {
        match self.rate {
            2_822_400 => "DSD64",
            5_644_800 => "DSD128",
            11_289_600 => "DSD256",
            _ => "DSD",
        }
    }

    pub fn duration(&self) -> Option<Duration> {
        let bytes = self.bytes?;
        Some(Duration::from_micros(
            u64::try_from(u128::from(bytes) * 1_000_000 / u128::from(self.byte_rate()))
                .unwrap_or(u64::MAX),
        ))
    }
}

/// Reads one DSF or DSDIFF track, emitting interleaved MSB-first DSD bytes.
pub struct DsdDecoder {
    inner: Inner,
}

enum Inner {
    Dsf(dsf::Dsf),
    Dff(dff::Dff),
}

impl DsdDecoder {
    /// Opens `path`, choosing the container its header declares.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, DecodeError> {
        let file = File::open(path.as_ref())?;
        let file_len = file.metadata()?.len();
        Self::open_source(Box::new(file), Some(file_len))
    }

    /// Opens any seekable byte stream, choosing DSF or DSDIFF from its 4-byte magic header.
    pub fn open_source(
        mut source: Box<dyn ReadSeek>,
        byte_len: Option<u64>,
    ) -> Result<Self, DecodeError> {
        let file_len = match byte_len {
            Some(len) => len,
            None => source.seek(SeekFrom::End(0))?,
        };
        source.seek(SeekFrom::Start(0))?;
        let mut head = [0u8; 4];
        source
            .read_exact(&mut head)
            .map_err(|e| DecodeError::Decode(format!("truncated DSD header: {e}")))?;
        source.seek(SeekFrom::Start(0))?;
        Ok(Self {
            inner: match &head {
                b"DSD " => Inner::Dsf(dsf::Dsf::open_source(source, file_len)?),
                b"FRM8" => Inner::Dff(dff::Dff::open_source(source, file_len)?),
                other => {
                    return Err(DecodeError::Unsupported(format!(
                        "not a DSF or DSDIFF file (magic {:?})",
                        String::from_utf8_lossy(other)
                    )));
                }
            },
        })
    }

    pub fn format(&self) -> DsdFormat {
        match &self.inner {
            Inner::Dsf(r) => r.format,
            Inner::Dff(r) => r.format,
        }
    }

    /// Playable byte frames per channel read so far.
    pub fn position(&self) -> u64 {
        match &self.inner {
            Inner::Dsf(r) => r.position,
            Inner::Dff(r) => r.position,
        }
    }

    /// Playable byte frames per channel in the stream, if known.
    pub fn total_bytes(&self) -> Option<u64> {
        self.format().bytes
    }

    /// Moves to byte frame `frame` (eight DSD bits per channel).
    pub fn seek_bytes(&mut self, frame: u64) -> Result<(), DecodeError> {
        let frame = self.total_bytes().map_or(frame, |total| frame.min(total));
        match &mut self.inner {
            Inner::Dsf(r) => r.seek_bytes(frame),
            Inner::Dff(r) => r.seek_bytes(frame),
        }
    }

    /// Replaces `out` with interleaved MSB-first DSD bytes and returns the byte frames read.
    ///
    /// `0` means end of stream.
    pub fn next_bytes(&mut self, out: &mut Vec<u8>) -> Result<usize, DecodeError> {
        out.clear();
        match &mut self.inner {
            Inner::Dsf(r) => r.read_bytes(out),
            Inner::Dff(r) => r.read_bytes(out),
        }
    }
}

/// Rejects rates outside the DSD family the player supports.
fn valid_rate(rate: u32) -> Result<u32, DecodeError> {
    if DSD_RATES.contains(&rate) {
        Ok(rate)
    } else {
        Err(DecodeError::Unsupported(format!(
            "unsupported DSD rate {rate} Hz (supported: DSD64, DSD128, DSD256)"
        )))
    }
}

fn valid_channels(channels: u32) -> Result<u16, DecodeError> {
    match channels {
        1..=32 => Ok(channels as u16),
        _ => Err(DecodeError::Unsupported(format!(
            "invalid DSD channel count {channels}"
        ))),
    }
}

/// Largest block/chunk the readers allocate from an untrusted header.
const MAX_ALLOC: u64 = 1 << 24;

fn read_exact(file: &mut dyn ReadSeek, buf: &mut [u8]) -> Result<(), DecodeError> {
    file.read_exact(buf)
        .map_err(|e| DecodeError::Decode(format!("truncated DSD file: {e}")))
}

fn seek_to(file: &mut dyn ReadSeek, at: u64) -> Result<(), DecodeError> {
    file.seek(SeekFrom::Start(at))
        .map(|_| ())
        .map_err(|e| DecodeError::Decode(format!("DSD seek failed: {e}")))
}

fn skip(file: &mut dyn ReadSeek, bytes: u64) -> Result<(), DecodeError> {
    if bytes > 0 {
        file.seek(SeekFrom::Current(i64::try_from(bytes).unwrap_or(i64::MAX)))
            .map_err(|e| DecodeError::Decode(format!("DSD seek failed: {e}")))?;
    }
    Ok(())
}

fn be_u32(src: &[u8]) -> u32 {
    u32::from_be_bytes([src[0], src[1], src[2], src[3]])
}

fn le_u32(src: &[u8]) -> u32 {
    u32::from_le_bytes([src[0], src[1], src[2], src[3]])
}

fn be_u64(src: &[u8]) -> u64 {
    u64::from_be_bytes([
        src[0], src[1], src[2], src[3], src[4], src[5], src[6], src[7],
    ])
}

fn le_u64(src: &[u8]) -> u64 {
    u64::from_le_bytes([
        src[0], src[1], src[2], src[3], src[4], src[5], src[6], src[7],
    ])
}

/// Largest number of bytes read from the file at once; also bounds per-channel blocks.
const READ_BLOCK: usize = 64 * 1024;
