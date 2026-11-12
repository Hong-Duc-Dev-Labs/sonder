//! Sony DSF reader.
//!
//! DSF layout: a 28-byte `DSD ` header, a 52-byte `fmt ` chunk, then the `data` chunk.
//! Audio is stored as *channel-major blocks* of `block_size` bytes; within a byte the bits
//! are LSB-first when `bits_per_sample` is 1 and MSB-first when it is 8. Both quirks are
//! undone here, so callers always get interleaved MSB-first bytes.

use std::fs::File;
use std::path::Path;

use super::{DsdFormat, MAX_ALLOC, le_u32, le_u64, read_exact, seek_to, skip};
use crate::DecodeError;

pub(super) struct Dsf {
    file: File,
    pub(super) format: DsdFormat,
    /// File offset of the first audio byte.
    data_start: u64,
    /// Playable byte frames per channel (from the fmt sample count, bounded by the chunk).
    audio_bytes: u64,
    /// Bytes per channel in one block group.
    block_size: usize,
    /// `true` when bytes are already MSB-first.
    msb_first: bool,
    pub(super) position: u64,
    /// Bytes to drop from the current block after a seek.
    skip: usize,
    /// One block per channel, interleaved into the output on each read.
    blocks: Vec<Vec<u8>>,
}

impl Dsf {
    pub(super) fn open(path: &Path) -> Result<Self, DecodeError> {
        let mut file = File::open(path).map_err(DecodeError::Open)?;
        let file_len = file.metadata().map_err(DecodeError::Open)?.len();

        // "DSD " chunk: id, chunk size (28), total file size, pointer to ID3 metadata.
        let mut head = [0u8; 28];
        read_exact(&mut file, &mut head)?;
        if &head[..4] != b"DSD " || le_u64(&head[4..]) != 28 {
            return Err(DecodeError::Unsupported("not a DSF file".into()));
        }
        let declared = le_u64(&head[12..]);
        if declared > file_len {
            return Err(DecodeError::Decode(format!(
                "DSF header claims {declared} bytes but the file has {file_len}"
            )));
        }

        // "fmt " chunk (52 bytes including the chunk header).
        seek_to(&mut file, 28)?;
        let mut fmt = [0u8; 52];
        read_exact(&mut file, &mut fmt)?;
        if &fmt[..4] != b"fmt " || le_u64(&fmt[4..]) != 52 {
            return Err(DecodeError::Unsupported(
                "DSF without a standard fmt chunk".into(),
            ));
        }
        let version = le_u32(&fmt[12..]);
        let format_id = le_u32(&fmt[16..]);
        let channels = le_u32(&fmt[24..]);
        let rate = le_u32(&fmt[28..]);
        let bits_per_sample = le_u32(&fmt[32..]);
        let sample_count = le_u64(&fmt[36..]);
        let block_size = le_u32(&fmt[44..]);
        if version != 1 {
            return Err(DecodeError::Unsupported(format!(
                "DSF format version {version}"
            )));
        }
        if format_id != 0 {
            return Err(DecodeError::Unsupported(format!(
                "DSF format id {format_id} (only DSD raw is supported)"
            )));
        }
        let channels = super::valid_channels(channels)?;
        let rate = super::valid_rate(rate)?;
        let msb_first = match bits_per_sample {
            1 => false,
            8 => true,
            other => {
                return Err(DecodeError::Unsupported(format!(
                    "DSF bit order {other} (expected 1 or 8)"
                )));
            }
        };
        if block_size == 0 || u64::from(block_size) > MAX_ALLOC {
            return Err(DecodeError::Decode(format!(
                "invalid DSF block size {block_size}"
            )));
        }
        if u64::from(block_size) * u64::from(channels) > MAX_ALLOC {
            return Err(DecodeError::Decode(
                "DSF block size × channels is too large".into(),
            ));
        }
        let block_size = block_size as usize;

        // "data" chunk header; its size counts the 12-byte header itself.
        let mut chunk = [0u8; 12];
        read_exact(&mut file, &mut chunk)?;
        if &chunk[..4] != b"data" {
            return Err(DecodeError::Unsupported("DSF without a data chunk".into()));
        }
        let chunk_size = le_u64(&chunk[4..]);
        if chunk_size < 12 {
            return Err(DecodeError::Decode(format!(
                "invalid DSF data chunk size {chunk_size}"
            )));
        }
        let data_start = 28u64 + 52 + 12;
        let data_len = chunk_size - 12;
        let end = data_start
            .checked_add(data_len)
            .ok_or_else(|| DecodeError::Decode("DSF data chunk size overflow".into()))?;
        if end > file_len {
            return Err(DecodeError::Decode(format!(
                "DSF data chunk runs past the end of the file \
                 ({data_start} + {data_len} > {file_len})"
            )));
        }

        // The sample count is in bits per channel; the data chunk may be padded past it.
        // Writers that leave it zero still get their audio from the chunk length.
        let declared_bytes = if sample_count == 0 {
            data_len / u64::from(channels)
        } else {
            sample_count / 8
        };
        let mut audio_bytes = declared_bytes.min(data_len / u64::from(channels));
        // Every block group, the last included, must be padded to `block_size` per channel
        // (the reader finds channel N's block at N × block_size). A writer that left the last
        // group short would make later channels read past the data chunk, so keep only the
        // whole groups the chunk really holds.
        let block = block_size as u64;
        let group_bytes = block * u64::from(channels);
        if audio_bytes.div_ceil(block) * group_bytes > data_len {
            audio_bytes = data_len / group_bytes * block;
        }
        if audio_bytes == 0 {
            return Err(DecodeError::Decode("DSF stream is empty".into()));
        }
        let blocks = vec![vec![0; block_size]; usize::from(channels)];
        let mut reader = Self {
            file,
            format: DsdFormat {
                rate,
                channels,
                bytes: Some(audio_bytes),
            },
            data_start,
            audio_bytes,
            block_size,
            msb_first,
            position: 0,
            skip: 0,
            blocks,
        };
        reader.seek_bytes(0)?;
        Ok(reader)
    }

    pub(super) fn seek_bytes(&mut self, frame: u64) -> Result<(), DecodeError> {
        let group = frame / self.block_size as u64;
        let offset = (frame % self.block_size as u64) as usize;
        let group_bytes = self.block_size as u64 * u64::from(self.format.channels);
        let at = self
            .data_start
            .checked_add(group.saturating_mul(group_bytes))
            .ok_or_else(|| DecodeError::Decode("DSF seek overflow".into()))?;
        seek_to(&mut self.file, at)?;
        self.position = frame;
        self.skip = offset;
        Ok(())
    }

    pub(super) fn read_bytes(&mut self, out: &mut Vec<u8>) -> Result<usize, DecodeError> {
        out.clear();
        let drop_frames = self.skip.min(self.block_size);
        self.skip = 0;
        // After a seek `position` is the requested frame; the block holding it starts earlier.
        let block_start = self.position.saturating_sub(drop_frames as u64);
        if block_start >= self.audio_bytes {
            return Ok(0);
        }
        let frames = usize::try_from((self.audio_bytes - block_start).min(self.block_size as u64))
            .unwrap_or(self.block_size);
        let channels = usize::from(self.format.channels);
        let drop_frames = drop_frames.min(frames);

        for block in &mut self.blocks[..channels] {
            read_exact(&mut self.file, &mut block[..frames])?;
            // Blocks are fixed size even at the end of the stream; the padding is not audio.
            skip(&mut self.file, (self.block_size - frames) as u64)?;
        }

        let emitted = frames - drop_frames;
        out.reserve(emitted * channels);
        for i in drop_frames..frames {
            for block in &self.blocks[..channels] {
                let byte = block[i];
                out.push(if self.msb_first {
                    byte
                } else {
                    byte.reverse_bits()
                });
            }
        }
        self.position = block_start + frames as u64;
        Ok(emitted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const RATE: u32 = 2_822_400;

    /// Builds a minimal DSF file: `data` is channel-major blocks of `block_size` bytes.
    fn write_dsf(path: &Path, channels: u32, block_size: u32, bits: u32, data: &[Vec<u8>]) {
        let per_channel = data.first().map_or(0, Vec::len) as u64;
        let block_size_usize = block_size as usize;
        let groups = per_channel.div_ceil(u64::from(block_size));
        let data_len = groups * u64::from(block_size) * u64::from(channels);
        let mut file = File::create(path).unwrap();
        file.write_all(b"DSD ").unwrap();
        file.write_all(&28u64.to_le_bytes()).unwrap();
        file.write_all(&(28 + 52 + 12 + data_len).to_le_bytes())
            .unwrap();
        file.write_all(&0u64.to_le_bytes()).unwrap(); // no ID3

        file.write_all(b"fmt ").unwrap();
        file.write_all(&52u64.to_le_bytes()).unwrap();
        file.write_all(&1u32.to_le_bytes()).unwrap(); // version
        file.write_all(&0u32.to_le_bytes()).unwrap(); // raw DSD
        file.write_all(&2u32.to_le_bytes()).unwrap(); // channel type: stereo
        file.write_all(&channels.to_le_bytes()).unwrap();
        file.write_all(&RATE.to_le_bytes()).unwrap();
        file.write_all(&bits.to_le_bytes()).unwrap();
        file.write_all(&(per_channel * 8).to_le_bytes()).unwrap(); // sample count (bits)
        file.write_all(&block_size.to_le_bytes()).unwrap();
        file.write_all(&0u32.to_le_bytes()).unwrap(); // reserved

        file.write_all(b"data").unwrap();
        file.write_all(&(12 + data_len).to_le_bytes()).unwrap();
        for g in 0..groups as usize {
            let start = g * block_size_usize;
            for block in data {
                let end = (start + block_size_usize).min(block.len());
                let chunk = if start >= block.len() {
                    &[][..]
                } else {
                    &block[start..end]
                };
                file.write_all(chunk).unwrap();
                file.write_all(&vec![0; block_size_usize - chunk.len()])
                    .unwrap();
            }
        }
    }

    fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("sonder-dsf-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    #[test]
    fn lsb_first_blocks_are_interleaved_and_bit_reversed() {
        let path = temp("lsb.dsf");
        // Two 4-byte blocks per channel.
        let left: Vec<u8> = vec![0x01, 0x02, 0x80, 0x00, 0x0F, 0xF0, 0xAA, 0x55];
        let right: Vec<u8> = vec![0xFF, 0x00, 0x0F, 0xF0, 0xAA, 0x55, 0x01, 0x02];
        write_dsf(&path, 2, 4, 1, &[left.clone(), right.clone()]);

        let mut reader = Dsf::open(&path).unwrap();
        assert_eq!(reader.format.rate, RATE);
        assert_eq!(reader.format.channels, 2);
        assert_eq!(reader.format.bytes, Some(8));

        let mut out = Vec::new();
        assert_eq!(reader.read_bytes(&mut out).unwrap(), 4);
        let mut expected = Vec::new();
        for i in 0..4 {
            expected.push(left[i].reverse_bits());
            expected.push(right[i].reverse_bits());
        }
        assert_eq!(out, expected);

        assert_eq!(reader.read_bytes(&mut out).unwrap(), 4);
        let mut second = Vec::new();
        for i in 4..8 {
            second.push(left[i].reverse_bits());
            second.push(right[i].reverse_bits());
        }
        assert_eq!(out, second);
        assert_eq!(reader.read_bytes(&mut out).unwrap(), 0);
    }

    #[test]
    fn msb_first_blocks_are_passed_through() {
        let path = temp("msb.dsf");
        let left = vec![0x12, 0x34, 0x56, 0x78];
        let right = vec![0x9A, 0xBC, 0xDE, 0xF0];
        write_dsf(&path, 2, 4, 8, &[left.clone(), right.clone()]);

        let mut reader = Dsf::open(&path).unwrap();
        let mut out = Vec::new();
        reader.read_bytes(&mut out).unwrap();
        assert_eq!(out, vec![0x12, 0x9A, 0x34, 0xBC, 0x56, 0xDE, 0x78, 0xF0]);
    }

    #[test]
    fn seek_lands_on_exact_byte_frame() {
        let path = temp("seek.dsf");
        let left: Vec<u8> = (0..8).collect();
        let right: Vec<u8> = (100..108).collect();
        write_dsf(&path, 2, 4, 8, &[left.clone(), right.clone()]);

        let mut reader = Dsf::open(&path).unwrap();
        reader.seek_bytes(5).unwrap();
        let mut out = Vec::new();
        assert_eq!(reader.read_bytes(&mut out).unwrap(), 3);
        assert_eq!(
            out,
            vec![left[5], right[5], left[6], right[6], left[7], right[7]]
        );
        assert_eq!(reader.position, 8);
    }

    /// A last block group written without padding: channel 1's short block follows channel
    /// 0's directly. Only the whole groups are audio; nothing past the chunk is ever read.
    #[test]
    fn unpadded_last_group_is_not_read_past_the_data_chunk() {
        let path = temp("unpadded.dsf");
        let left: Vec<u8> = (0..6).collect();
        let right: Vec<u8> = (100..106).collect();
        write_dsf(&path, 2, 4, 8, &[left.clone(), right.clone()]);
        // Rewrite the file with the last group unpadded and trailing metadata after it.
        let bytes = std::fs::read(&path).unwrap();
        let header = 28 + 52;
        let mut file = bytes[..header].to_vec();
        let audio: Vec<u8> = [&left[..4], &right[..4], &left[4..], &right[4..]].concat();
        file.extend_from_slice(b"data");
        file.extend_from_slice(&(12 + audio.len() as u64).to_le_bytes());
        file.extend_from_slice(&audio);
        file.extend_from_slice(b"ID3\x04trailing metadata");
        std::fs::write(&path, &file).unwrap();

        let mut reader = Dsf::open(&path).unwrap();
        assert_eq!(reader.format.bytes, Some(4));
        let mut out = Vec::new();
        assert_eq!(reader.read_bytes(&mut out).unwrap(), 4);
        assert_eq!(out, vec![0, 100, 1, 101, 2, 102, 3, 103]);
        assert_eq!(reader.read_bytes(&mut out).unwrap(), 0);
    }

    #[test]
    fn oversized_block_is_rejected_before_allocation() {
        let path = temp("bad-block.dsf");
        write_dsf(&path, 2, 4, 1, &[vec![0; 4], vec![0; 4]]);
        // Rewrite the block size field with an absurd value.
        use std::io::{Seek, SeekFrom};
        let mut file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        file.seek(SeekFrom::Start(28 + 44)).unwrap();
        file.write_all(&u32::MAX.to_le_bytes()).unwrap();
        assert!(matches!(Dsf::open(&path), Err(DecodeError::Decode(_))));
    }

    #[test]
    fn data_chunk_past_eof_is_rejected() {
        let path = temp("past-eof.dsf");
        write_dsf(&path, 2, 4, 1, &[vec![0; 4], vec![0; 4]]);
        use std::io::{Seek, SeekFrom};
        let mut file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        file.seek(SeekFrom::Start(28 + 52 + 4)).unwrap();
        file.write_all(&u64::MAX.to_le_bytes()).unwrap();
        assert!(Dsf::open(&path).is_err());
    }
}
