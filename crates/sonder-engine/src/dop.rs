//! DoP and native-DSD packing: interleaved MSB-first DSD bytes → one `i32` per carrier frame.
//!
//! # DoP
//!
//! Each carrier frame carries 16 DSD bits per channel in the 24-bit word
//! `marker<<24 | dsd_byte0<<16 | dsd_byte1<<8` (byte 0 is the earlier DSD byte, MSB first).
//! The marker (0x05/0xFA) is written by the output renderer, not here: it is the single
//! writer of the device buffer and continues the same marker phase through payload, idle,
//! pause and underrun frames. This module supplies the marker-free payload
//! (`dsd_byte0<<16 | dsd_byte1<<8`, low byte zero).
//!
//! # Native DSD
//!
//! `DsdU32Be` frames carry four DSD bytes per channel in one big-endian 32-bit word:
//! `b0<<24 | b1<<16 | b2<<8 | b3`. The renderer packs them with `pack`, so this packer only
//! assembles the word.
//!
//! Both packers carry a partial frame across calls, and [`Packer::finish`] pads the last
//! frame with 0x69 (the DSD silence pattern) so a track never ends mid-frame.

/// DSD idle/silence byte: alternating bits with zero mean.
pub const DSD_SILENCE: u8 = 0x69;

/// DoP marker bytes, alternating every carrier frame.
pub const DOP_MARKER_A: u8 = 0x05;
pub const DOP_MARKER_B: u8 = 0xFA;

/// Kind of carrier frame the packer produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Packing {
    /// 2 DSD bytes per channel, marker-free 24-bit payload.
    Dop,
    /// 4 DSD bytes per channel in one big-endian word.
    Native,
}

/// Groups interleaved DSD bytes into carrier payload samples.
pub struct Packer {
    packing: Packing,
    channels: usize,
    /// Bytes of a partial carrier frame carried to the next call.
    pending: Vec<u8>,
}

impl Packer {
    pub fn new(packing: Packing, channels: u16) -> Self {
        let channels = usize::from(channels.max(1));
        Self {
            packing,
            channels,
            pending: Vec::with_capacity(channels * 4),
        }
    }

    pub fn dop(channels: u16) -> Self {
        Self::new(Packing::Dop, channels)
    }

    pub fn native(channels: u16) -> Self {
        Self::new(Packing::Native, channels)
    }

    pub fn packing(&self) -> Packing {
        self.packing
    }

    /// Bytes in one carrier frame for all channels.
    fn frame_bytes(&self) -> usize {
        self.channels * self.bytes_per_channel()
    }

    fn bytes_per_channel(&self) -> usize {
        match self.packing {
            Packing::Dop => 2,
            Packing::Native => 4,
        }
    }

    /// Appends one `i32` per carrier frame that `input` completes.
    pub fn push(&mut self, input: &[u8], out: &mut Vec<i32>) {
        let frame_bytes = self.frame_bytes();
        let mut at = 0;
        if !self.pending.is_empty() {
            let need = frame_bytes - self.pending.len();
            if input.len() < need {
                self.pending.extend_from_slice(input);
                return;
            }
            self.pending.extend_from_slice(&input[..need]);
            at = need;
            let pending = std::mem::take(&mut self.pending);
            self.pack_frame(&pending, out);
        }
        let usable = (input.len() - at) / frame_bytes * frame_bytes;
        for frame in input[at..at + usable].chunks_exact(frame_bytes) {
            self.pack_frame(frame, out);
        }
        self.pending.extend_from_slice(&input[at + usable..]);
    }

    /// Pads and packs the carried bytes, if any, so the stream ends on a whole frame.
    pub fn finish(&mut self, out: &mut Vec<i32>) {
        if self.pending.is_empty() {
            return;
        }
        let frame_bytes = self.frame_bytes();
        self.pending.resize(frame_bytes, DSD_SILENCE);
        debug_assert_eq!(self.pending.len(), frame_bytes);
        let pending = std::mem::take(&mut self.pending);
        self.pack_frame(&pending, out);
    }

    /// Forgets a partial frame (seek, stop). The caller re-decodes from a byte boundary.
    pub fn reset(&mut self) {
        self.pending.clear();
    }

    fn pack_frame(&self, frame: &[u8], out: &mut Vec<i32>) {
        // Interleaved byte frames: byte k of channel c is at k * channels + c.
        match self.packing {
            Packing::Dop => {
                for channel in 0..self.channels {
                    let b0 = frame[channel];
                    let b1 = frame[self.channels + channel];
                    out.push((i32::from(b0) << 16) | (i32::from(b1) << 8));
                }
            }
            Packing::Native => {
                for channel in 0..self.channels {
                    let b0 = frame[channel];
                    let b1 = frame[self.channels + channel];
                    let b2 = frame[2 * self.channels + channel];
                    let b3 = frame[3 * self.channels + channel];
                    out.push(
                        (i32::from(b0) << 24)
                            | (i32::from(b1) << 16)
                            | (i32::from(b2) << 8)
                            | i32::from(b3),
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Inverse of the DoP payload packing for tests: 2 bytes per channel per frame.
    fn unpack_dop(words: &[i32], channels: usize) -> Vec<u8> {
        let mut out = Vec::new();
        for frame in words.chunks_exact(channels) {
            for shift in [16u32, 8] {
                for &sample in frame {
                    let value = (sample as u32 >> shift) as u8;
                    out.push(value);
                }
            }
        }
        out
    }

    fn source(frames: usize, channels: usize) -> Vec<u8> {
        (0..frames * channels)
            .map(|i| (i.wrapping_mul(37) + 11) as u8)
            .collect()
    }

    #[test]
    fn dop_payload_roundtrips_across_every_split() {
        let channels = 2;
        let bytes = source(9, channels);
        for split in 0..bytes.len() {
            let mut packer = Packer::dop(channels as u16);
            let mut words = Vec::new();
            packer.push(&bytes[..split], &mut words);
            packer.push(&bytes[split..], &mut words);
            packer.finish(&mut words);
            // Nine byte frames = four DoP frames plus one byte frame of padding.
            assert_eq!(words.len(), 5 * channels, "split {split}");
            let back = unpack_dop(&words, channels);
            assert_eq!(&back[..bytes.len()], &bytes[..], "split {split}");
            assert!(back[bytes.len()..].iter().all(|&b| b == DSD_SILENCE));
        }
    }

    #[test]
    fn dop_words_place_bytes_in_the_24_bit_word() {
        let mut packer = Packer::dop(1);
        let mut out = Vec::new();
        packer.push(&[0x12, 0x34, 0x56, 0x78], &mut out);
        packer.finish(&mut out);
        assert_eq!(out, vec![0x0012_3400, 0x0056_7800]);
    }

    #[test]
    fn native_words_are_big_endian_groups_of_four() {
        let mut packer = Packer::native(2);
        let mut out = Vec::new();
        // Two channels, one carrier frame: L = 01 02 03 04, R = 05 06 07 08.
        packer.push(&[1, 5, 2, 6, 3, 7, 4, 8], &mut out);
        assert_eq!(out, vec![0x0102_0304, 0x0506_0708]);

        packer.push(&[9, 13], &mut out);
        packer.finish(&mut out);
        assert_eq!(&out[out.len() - 2..], &[0x0969_6969, 0x0D69_6969]);
    }

    #[test]
    fn finish_on_a_whole_frame_is_a_noop() {
        let mut packer = Packer::dop(2);
        let mut out = Vec::new();
        packer.push(&[1, 2, 3, 4], &mut out);
        let before = out.len();
        packer.finish(&mut out);
        assert_eq!(out.len(), before);
    }

    #[test]
    fn reset_drops_a_partial_frame() {
        let mut packer = Packer::dop(2);
        let mut out = Vec::new();
        packer.push(&[1], &mut out);
        assert!(out.is_empty());
        packer.reset();
        packer.finish(&mut out);
        assert!(out.is_empty());
    }
}
