//! Test signals for hardware checks, written without an external encoder.
//!
//! FLAC files use VERBATIM subframes only: valid FLAC that any decoder reads back to the exact
//! samples written, with no compression.

use std::io::{self, Write};
use std::path::Path;

/// Samples per FLAC frame (the last frame may be shorter).
const FLAC_BLOCK: usize = 4096;

/// Interleaved stereo samples of a sine at `freq` Hz and `dbfs` (relative to full scale at
/// `bits`), lasting `seconds`.
pub fn sine(rate: u32, bits: u32, freq: f64, dbfs: f64, seconds: f64) -> Vec<i32> {
    let amplitude = f64::from((1i32 << (bits - 1)) - 1) * 10f64.powf(dbfs / 20.0);
    let frames = (f64::from(rate) * seconds).round() as usize;
    let mut out = Vec::with_capacity(frames * 2);
    for n in 0..frames {
        let v = (amplitude * (std::f64::consts::TAU * freq * n as f64 / f64::from(rate)).sin())
            .round() as i32;
        out.extend([v, v]);
    }
    out
}

/// Interleaved stereo unit impulse (full scale at `bits`) followed by silence.
pub fn dirac(bits: u32, frames: usize) -> Vec<i32> {
    let mut out = vec![0; frames.max(1) * 2];
    let one = (1i32 << (bits - 1)) - 1;
    out[0] = one;
    out[1] = one;
    out
}

/// Writes interleaved stereo integer samples as a WAV file.
pub fn write_wav(path: &Path, rate: u32, bits: u16, samples: &[i32]) -> io::Result<()> {
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: rate,
        bits_per_sample: bits,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec).map_err(io::Error::other)?;
    for &s in samples {
        writer.write_sample(s).map_err(io::Error::other)?;
    }
    writer.finalize().map_err(io::Error::other)
}

/// Writes interleaved stereo integer samples (16 or 24 bits) as an uncompressed FLAC file.
pub fn write_flac(path: &Path, rate: u32, bits: u32, samples: &[i32]) -> io::Result<()> {
    write_flac_with_md5(path, rate, bits, samples, [0; 16])
}

/// [`write_flac`] with `md5` stored in STREAMINFO (all zeros means "not computed").
pub fn write_flac_with_md5(
    path: &Path,
    rate: u32,
    bits: u32,
    samples: &[i32],
    md5: [u8; 16],
) -> io::Result<()> {
    assert!(matches!(bits, 16 | 24), "only 16- and 24-bit FLAC");
    assert!(
        samples.len().is_multiple_of(2),
        "stereo samples come in pairs"
    );
    let frames = samples.len() / 2;
    // `utf8_number` codes frame numbers in at most four bytes (below 2^21).
    assert!(
        frames.div_ceil(FLAC_BLOCK) <= 1 << 21,
        "too long for this writer's frame numbering"
    );
    let mut out = Vec::new();
    out.extend_from_slice(b"fLaC");

    // STREAMINFO, the only metadata block. Frame sizes are left as "unknown".
    out.push(0x80);
    out.extend_from_slice(&34u32.to_be_bytes()[1..]);
    out.extend_from_slice(&(FLAC_BLOCK as u16).to_be_bytes());
    out.extend_from_slice(&(FLAC_BLOCK as u16).to_be_bytes());
    out.extend_from_slice(&[0; 6]);
    let packed: u64 = (u64::from(rate) << 44)
        | (1u64 << 41) // two channels
        | (u64::from(bits - 1) << 36)
        | frames as u64;
    out.extend_from_slice(&packed.to_be_bytes());
    out.extend_from_slice(&md5);

    let bytes = (bits / 8) as usize;
    for (index, block) in samples.chunks(FLAC_BLOCK * 2).enumerate() {
        let start = out.len();
        // Sync, fixed block size; block size from the 16 bits after the frame number; rate
        // and depth from STREAMINFO; left/right channels.
        out.extend_from_slice(&[0xFF, 0xF8, 0x70, 0x10]);
        utf8_number(&mut out, index as u32);
        out.extend_from_slice(&((block.len() / 2 - 1) as u16).to_be_bytes());
        out.push(crc8(&out[start..]));
        for channel in 0..2 {
            out.push(0x02); // VERBATIM subframe, no wasted bits
            for frame in block.chunks(2) {
                out.extend_from_slice(&frame[channel].to_be_bytes()[4 - bytes..]);
            }
        }
        let crc = crc16(&out[start..]);
        out.extend_from_slice(&crc.to_be_bytes());
    }
    std::fs::File::create(path)?.write_all(&out)
}

/// FLAC's UTF-8-style coding of the frame number.
fn utf8_number(out: &mut Vec<u8>, n: u32) {
    match n {
        0..0x80 => out.push(n as u8),
        0x80..0x800 => out.extend([0xC0 | (n >> 6) as u8, 0x80 | (n & 0x3F) as u8]),
        0x800..0x10000 => out.extend([
            0xE0 | (n >> 12) as u8,
            0x80 | ((n >> 6) & 0x3F) as u8,
            0x80 | (n & 0x3F) as u8,
        ]),
        _ => out.extend([
            0xF0 | (n >> 18) as u8,
            0x80 | ((n >> 12) & 0x3F) as u8,
            0x80 | ((n >> 6) & 0x3F) as u8,
            0x80 | (n & 0x3F) as u8,
        ]),
    }
}

/// CRC-8, polynomial x^8 + x^2 + x + 1, as the frame header uses.
fn crc8(data: &[u8]) -> u8 {
    let mut crc = 0u8;
    for &byte in data {
        crc ^= byte;
        for _ in 0..8 {
            crc = if crc & 0x80 != 0 {
                (crc << 1) ^ 0x07
            } else {
                crc << 1
            };
        }
    }
    crc
}

/// CRC-16, polynomial x^16 + x^15 + x^2 + 1, over the whole frame.
fn crc16(data: &[u8]) -> u16 {
    let mut crc = 0u16;
    for &byte in data {
        crc ^= u16::from(byte) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x8005
            } else {
                crc << 1
            };
        }
    }
    crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crcs_match_the_flac_reference_values() {
        // The CRC-8/SMBUS and CRC-16/UMTS check values for "123456789".
        assert_eq!(crc8(b"123456789"), 0xF4);
        assert_eq!(crc16(b"123456789"), 0xFEE8);
    }

    #[test]
    fn a_minus_20_dbfs_sine_peaks_at_a_tenth_of_full_scale() {
        let s = sine(44_100, 16, 1000.0, -20.0, 0.1);
        let peak = s.iter().map(|v| v.abs()).max().unwrap();
        assert!((3270..=3277).contains(&peak), "{peak}");
    }
}
