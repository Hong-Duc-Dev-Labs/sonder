//! DSDIFF (DFF) reader.
//!
//! DSDIFF is an IFF-style, big-endian container: an `FRM8` form whose `DSD ` form type holds
//! `FVER`, `PROP` (`FS `, `CHNL`, `CMPR`), the interleaved `DSD ` audio chunk and optional
//! metadata. Audio bytes are already MSB-first and interleaved, which is the internal layout,
//! so they are passed through unchanged. `DST `-compressed audio is rejected.

use super::{DsdFormat, READ_BLOCK, be_u32, be_u64, read_exact, seek_to};
use crate::{DecodeError, ReadSeek};

pub(super) struct Dff {
    file: Box<dyn ReadSeek>,
    pub(super) format: DsdFormat,
    data_start: u64,
    /// Total bytes in the `DSD ` chunk (all channels).
    data_len: u64,
    pub(super) position: u64,
    buf: Vec<u8>,
}

impl Dff {
    #[cfg(test)]
    pub(super) fn open(path: &std::path::Path) -> Result<Self, DecodeError> {
        let file = std::fs::File::open(path).map_err(DecodeError::Open)?;
        let file_len = file.metadata().map_err(DecodeError::Open)?.len();
        Self::open_source(Box::new(file), file_len)
    }

    pub(super) fn open_source(
        mut file: Box<dyn ReadSeek>,
        file_len: u64,
    ) -> Result<Self, DecodeError> {
        let mut head = [0u8; 16];
        read_exact(&mut *file, &mut head)?;
        if &head[..4] != b"FRM8" || &head[12..16] != b"DSD " {
            return Err(DecodeError::Unsupported(
                "not a DSDIFF (FRM8/DSD) file".into(),
            ));
        }
        let form_size = be_u64(&head[4..]);
        if form_size < 4 || form_size > file_len.saturating_sub(12) {
            return Err(DecodeError::Decode(format!(
                "invalid DSDIFF form size {form_size}"
            )));
        }

        let mut rate = None;
        let mut channels = None;
        let mut compressed = false;
        let mut audio: Option<(u64, u64)> = None;
        let form_end = 12u64.saturating_add(form_size);
        let mut at = 16u64;
        while at + 12 <= form_end {
            seek_to(&mut *file, at)?;
            let mut chunk = [0u8; 12];
            read_exact(&mut *file, &mut chunk)?;
            let id = &chunk[..4];
            let size = be_u64(&chunk[4..]);
            let data_start = at + 12;
            if size > form_end.saturating_sub(data_start) || data_start + size > file_len {
                return Err(DecodeError::Decode(format!(
                    "DSDIFF chunk {:?} runs past the end of the form",
                    String::from_utf8_lossy(id)
                )));
            }
            match id {
                b"PROP" => {
                    seek_to(&mut *file, data_start)?;
                    let mut kind = [0u8; 4];
                    read_exact(&mut *file, &mut kind)?;
                    if &kind == b"SND " {
                        parse_prop(
                            &mut *file,
                            data_start + 4,
                            data_start + size,
                            &mut rate,
                            &mut channels,
                            &mut compressed,
                        )?;
                    }
                }
                b"DSD " => {
                    if audio.is_some() {
                        return Err(DecodeError::Decode(
                            "DSDIFF with more than one DSD chunk".into(),
                        ));
                    }
                    audio = Some((data_start, size));
                }
                b"DST " => {
                    return Err(DecodeError::Unsupported(
                        "DST-compressed DSDIFF is not supported".into(),
                    ));
                }
                _ => {}
            }
            at = data_start + size + (size & 1);
        }

        let rate =
            rate.ok_or_else(|| DecodeError::Unsupported("DSDIFF without FS chunk".into()))?;
        let rate = super::valid_rate(rate)?;
        let channels = super::valid_channels(
            channels.ok_or_else(|| DecodeError::Unsupported("DSDIFF without CHNL chunk".into()))?,
        )?;
        if compressed {
            return Err(DecodeError::Unsupported(
                "DST-compressed DSDIFF is not supported".into(),
            ));
        }
        let (data_start, data_len) =
            audio.ok_or_else(|| DecodeError::Unsupported("DSDIFF without a DSD chunk".into()))?;
        let total = data_len / u64::from(channels);
        if total == 0 {
            return Err(DecodeError::Decode("DSDIFF stream is empty".into()));
        }

        let mut reader = Self {
            file,
            format: DsdFormat {
                rate,
                channels,
                bytes: Some(total),
            },
            data_start,
            data_len,
            position: 0,
            buf: vec![0; READ_BLOCK],
        };
        reader.seek_bytes(0)?;
        Ok(reader)
    }

    pub(super) fn seek_bytes(&mut self, frame: u64) -> Result<(), DecodeError> {
        let at = self
            .data_start
            .checked_add(frame.saturating_mul(u64::from(self.format.channels)))
            .ok_or_else(|| DecodeError::Decode("DSDIFF seek overflow".into()))?;
        seek_to(&mut *self.file, at)?;
        self.position = frame;
        Ok(())
    }

    pub(super) fn read_bytes(&mut self, out: &mut Vec<u8>) -> Result<usize, DecodeError> {
        out.clear();
        let channels = usize::from(self.format.channels);
        let total = self.data_len / u64::from(self.format.channels);
        if self.position >= total {
            return Ok(0);
        }
        let frames = usize::try_from(total - self.position)
            .unwrap_or(usize::MAX)
            .min(self.buf.len() / channels);
        let bytes = frames * channels;
        read_exact(&mut *self.file, &mut self.buf[..bytes])?;
        out.extend_from_slice(&self.buf[..bytes]);
        self.position += frames as u64;
        Ok(frames)
    }
}

/// Parses the subchunks of a `SND ` property block.
fn parse_prop(
    file: &mut dyn ReadSeek,
    mut at: u64,
    end: u64,
    rate: &mut Option<u32>,
    channels: &mut Option<u32>,
    compressed: &mut bool,
) -> Result<(), DecodeError> {
    while at + 12 <= end {
        seek_to(file, at)?;
        let mut chunk = [0u8; 12];
        read_exact(file, &mut chunk)?;
        let id = &chunk[..4];
        let size = be_u64(&chunk[4..]);
        let data_start = at + 12;
        if size > end.saturating_sub(data_start) {
            return Err(DecodeError::Decode(format!(
                "DSDIFF property {:?} runs past its PROP chunk",
                String::from_utf8_lossy(id)
            )));
        }
        match id {
            b"FS  " => {
                if size < 4 {
                    return Err(DecodeError::Decode("DSDIFF FS chunk is too small".into()));
                }
                let mut bytes = [0u8; 4];
                read_exact(file, &mut bytes)?;
                *rate = Some(be_u32(&bytes));
            }
            b"CHNL" => {
                let mut count = [0u8; 2];
                read_exact(file, &mut count)?;
                let count = u16::from_be_bytes(count);
                if size < 2 + u64::from(count) * 4 {
                    return Err(DecodeError::Decode("DSDIFF CHNL chunk is too small".into()));
                }
                *channels = Some(u32::from(count));
            }
            b"CMPR" => {
                let mut kind = [0u8; 4];
                read_exact(file, &mut kind)?;
                match &kind {
                    b"DSD " => {}
                    b"DST " => *compressed = true,
                    other => {
                        return Err(DecodeError::Unsupported(format!(
                            "DSDIFF compression {:?}",
                            String::from_utf8_lossy(other)
                        )));
                    }
                }
            }
            _ => {}
        }
        at = data_start + size + (size & 1);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use std::path::Path;

    const RATE: u32 = 2_822_400;

    fn chunk(out: &mut Vec<u8>, id: &[u8; 4], data: &[u8]) {
        out.extend_from_slice(id);
        out.extend_from_slice(&(data.len() as u64).to_be_bytes());
        out.extend_from_slice(data);
        if data.len() % 2 == 1 {
            out.push(0);
        }
    }

    fn write_dff(path: &Path, rate: u32, channels: u16, data: &[u8], cmpr: &[u8; 4]) {
        let mut prop = Vec::new();
        prop.extend_from_slice(b"SND ");
        chunk(&mut prop, b"FS  ", &rate.to_be_bytes());
        let mut chnl = Vec::new();
        chnl.extend_from_slice(&channels.to_be_bytes());
        for id in [b"SLFT", b"SRGT"] {
            chnl.extend_from_slice(id);
        }
        chunk(&mut prop, b"CHNL", &chnl);
        chunk(&mut prop, b"CMPR", cmpr);

        let mut body = Vec::new();
        body.extend_from_slice(b"DSD ");
        chunk(&mut body, b"FVER", &0x01050000u32.to_be_bytes());
        chunk(&mut body, b"PROP", &prop);
        chunk(&mut body, b"JUNK", &[1, 2, 3]);
        chunk(&mut body, b"DSD ", data);

        let mut file = File::create(path).unwrap();
        file.write_all(b"FRM8").unwrap();
        file.write_all(&(body.len() as u64).to_be_bytes()).unwrap();
        file.write_all(&body).unwrap();
    }

    fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("sonder-dff-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    #[test]
    fn reads_interleaved_msb_first_bytes() {
        let path = temp("basic.dff");
        let data: Vec<u8> = vec![1, 101, 2, 102, 3, 103, 4, 104];
        write_dff(&path, RATE, 2, &data, b"DSD ");
        let mut reader = Dff::open(&path).unwrap();
        assert_eq!(reader.format.rate, RATE);
        assert_eq!(reader.format.channels, 2);
        assert_eq!(reader.format.bytes, Some(4));

        let mut out = Vec::new();
        assert_eq!(reader.read_bytes(&mut out).unwrap(), 4);
        assert_eq!(out, data);
        assert_eq!(reader.read_bytes(&mut out).unwrap(), 0);
    }

    #[test]
    fn seek_reads_from_an_exact_byte_frame() {
        let path = temp("seek.dff");
        let data: Vec<u8> = vec![1, 101, 2, 102, 3, 103, 4, 104];
        write_dff(&path, RATE, 2, &data, b"DSD ");
        let mut reader = Dff::open(&path).unwrap();
        reader.seek_bytes(2).unwrap();
        let mut out = Vec::new();
        assert_eq!(reader.read_bytes(&mut out).unwrap(), 2);
        assert_eq!(out, vec![3, 103, 4, 104]);
    }

    #[test]
    fn dst_compression_is_rejected() {
        let path = temp("dst.dff");
        write_dff(&path, RATE, 2, &[0; 8], b"DST ");
        assert!(matches!(Dff::open(&path), Err(DecodeError::Unsupported(_))));
    }

    #[test]
    fn unknown_chunks_are_skipped_and_following_data_still_reads() {
        let path = temp("unknown.dff");
        // write_dff inserts an odd-sized JUNK chunk before the audio.
        write_dff(&path, RATE, 2, &[9, 8, 7, 6], b"DSD ");
        let mut reader = Dff::open(&path).unwrap();
        let mut out = Vec::new();
        reader.read_bytes(&mut out).unwrap();
        assert_eq!(out, vec![9, 8, 7, 6]);
    }

    #[test]
    fn chunk_past_eof_is_rejected() {
        let path = temp("past-eof.dff");
        write_dff(&path, RATE, 2, &[0; 8], b"DSD ");
        use std::io::{Seek, SeekFrom};
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        // First chunk after FRM8 is FVER at 16: rewrite its size past the end.
        file.seek(SeekFrom::Start(16 + 4)).unwrap();
        file.write_all(&u64::MAX.to_be_bytes()).unwrap();
        assert!(Dff::open(&path).is_err());
    }
}
