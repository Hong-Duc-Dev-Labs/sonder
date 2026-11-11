//! Decodes generated WAV and FLAC files and checks format detection and exact sample values.

use std::path::{Path, PathBuf};

use sonder_decode::{Decoder, SourceFormat};

/// Deterministic test signal covering full-scale extremes, zero, -1 and a pseudo-random spread.
fn signal(bits: u32, channels: u16, frames: usize) -> Vec<i32> {
    let max = (1i64 << (bits - 1)) - 1;
    let min = -(1i64 << (bits - 1));
    (0..frames * usize::from(channels))
        .map(|i| match i % 7 {
            0 => max as i32,
            1 => min as i32,
            2 => 0,
            3 => -1,
            _ => {
                let v = (i as i64 * 7919) % (max + 1);
                (if i % 2 == 0 { v } else { -v }) as i32
            }
        })
        .collect()
}

fn temp_path(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sonder-decode-tests-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

fn write_wav(path: &Path, rate: u32, bits: u16, channels: u16, samples: &[i32]) {
    let spec = hound::WavSpec {
        channels,
        sample_rate: rate,
        bits_per_sample: bits,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    for &s in samples {
        w.write_sample(s).unwrap();
    }
    w.finalize().unwrap();
}

fn decode_all(path: &Path) -> (SourceFormat, Vec<i32>) {
    let mut dec = Decoder::open(path).unwrap();
    let fmt = dec.format();
    let mut all = Vec::new();
    let mut block = Vec::new();
    while dec.next_i32(&mut block).unwrap() > 0 {
        all.extend_from_slice(&block);
    }
    assert_eq!(dec.skipped_packets(), 0);
    (fmt, all)
}

fn assert_left_justified(decoded: &[i32], source: &[i32], bits: u32) {
    assert_eq!(decoded.len(), source.len());
    for (i, (&d, &s)) in decoded.iter().zip(source).enumerate() {
        let expected = if bits == 32 { s } else { s << (32 - bits) };
        assert_eq!(d, expected, "sample {i}");
    }
}

#[test]
fn wav_16_bit_stereo() {
    let src = signal(16, 2, 4096);
    let path = temp_path("s16.wav");
    write_wav(&path, 44_100, 16, 2, &src);
    let (fmt, out) = decode_all(&path);
    assert_eq!(
        fmt,
        SourceFormat {
            rate: 44_100,
            bits: 16,
            channels: 2
        }
    );
    assert_left_justified(&out, &src, 16);
}

#[test]
fn wav_24_bit_stereo() {
    let src = signal(24, 2, 4096);
    let path = temp_path("s24.wav");
    write_wav(&path, 96_000, 24, 2, &src);
    let (fmt, out) = decode_all(&path);
    assert_eq!(
        fmt,
        SourceFormat {
            rate: 96_000,
            bits: 24,
            channels: 2
        }
    );
    assert_left_justified(&out, &src, 24);
}

#[test]
fn flac_16_bit_44k() {
    let src = signal(16, 2, 10_000);
    let path = temp_path("s16.flac");
    flac::write(&path, 44_100, 16, 2, &src);
    let (fmt, out) = decode_all(&path);
    assert_eq!(
        fmt,
        SourceFormat {
            rate: 44_100,
            bits: 16,
            channels: 2
        }
    );
    assert_left_justified(&out, &src, 16);
}

#[test]
fn flac_24_bit_192k() {
    let src = signal(24, 2, 10_000);
    let path = temp_path("s24.flac");
    flac::write(&path, 192_000, 24, 2, &src);
    let (fmt, out) = decode_all(&path);
    assert_eq!(
        fmt,
        SourceFormat {
            rate: 192_000,
            bits: 24,
            channels: 2
        }
    );
    assert_left_justified(&out, &src, 24);
}

#[test]
fn f64_output_is_normalised() {
    let src = vec![i16::MAX as i32, i16::MIN as i32, 0, 16_384];
    let path = temp_path("f64.wav");
    write_wav(&path, 48_000, 16, 2, &src);
    let mut dec = Decoder::open(&path).unwrap();
    let mut out = Vec::new();
    assert_eq!(dec.next_f64(&mut out).unwrap(), 2);
    assert_eq!(out, vec![32_767.0 / 32_768.0, -1.0, 0.0, 0.5]);
}

#[test]
fn float_wav_is_refused_as_floating_point() {
    let path = temp_path("f32.wav");
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: 48_000,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut w = hound::WavWriter::create(&path, spec).unwrap();
    for s in [0.25f32, -0.25, 0.5, -0.5] {
        w.write_sample(s).unwrap();
    }
    w.finalize().unwrap();

    let err = Decoder::open(&path)
        .err()
        .expect("float WAV must be refused");
    assert!(err.to_string().contains("floating-point"), "{err}");
}

fn decode_from(path: &Path, frame: u64) -> Vec<i32> {
    let mut dec = Decoder::open(path).unwrap();
    dec.seek(frame).unwrap();
    assert_eq!(dec.position(), frame);
    let mut all = Vec::new();
    let mut block = Vec::new();
    while dec.next_i32(&mut block).unwrap() > 0 {
        all.extend_from_slice(&block);
    }
    all
}

#[test]
fn seek_lands_on_the_exact_frame() {
    let src = signal(16, 2, 20_000);
    let wav = temp_path("seek.wav");
    write_wav(&wav, 44_100, 16, 2, &src);
    let flac_path = temp_path("seek.flac");
    flac::write(&flac_path, 44_100, 16, 2, &src);
    let (_, full) = decode_all(&flac_path);

    for path in [&wav, &flac_path] {
        // Inside a block, on a block boundary, the first and the last frame.
        for frame in [0u64, 1, 4095, 4096, 12_345, 19_999] {
            let start = frame as usize * 2;
            assert_eq!(
                decode_from(path, frame),
                full[start..],
                "{} at {frame}",
                path.display()
            );
        }
    }
}

#[test]
fn seek_past_the_end_yields_nothing() {
    let src = signal(16, 2, 1_000);
    let path = temp_path("seek-end.flac");
    flac::write(&path, 44_100, 16, 2, &src);
    let mut dec = Decoder::open(&path).unwrap();
    assert_eq!(dec.total_frames(), Some(1_000));
    if dec.seek(5_000).is_ok() {
        let mut block = Vec::new();
        assert_eq!(dec.next_i32(&mut block).unwrap(), 0);
    }
}

#[test]
fn garbage_file_is_an_error_not_a_panic() {
    let path = temp_path("garbage.flac");
    std::fs::write(&path, b"definitely not audio").unwrap();
    assert!(Decoder::open(&path).is_err());
}

#[test]
fn missing_file_is_an_error() {
    assert!(Decoder::open(temp_path("does-not-exist.flac")).is_err());
}

/// Minimal FLAC writer using fixed-blocksize frames with VERBATIM subframes. It exists only to
/// produce valid FLAC test input without an external encoder or committed binaries.
mod flac {
    use std::path::Path;

    const BLOCK: usize = 4096;

    struct Bits {
        out: Vec<u8>,
        acc: u8,
        n: u32,
    }

    impl Bits {
        fn new() -> Self {
            Self {
                out: Vec::new(),
                acc: 0,
                n: 0,
            }
        }

        fn put(&mut self, value: u64, bits: u32) {
            for i in (0..bits).rev() {
                self.acc = (self.acc << 1) | ((value >> i) & 1) as u8;
                self.n += 1;
                if self.n == 8 {
                    self.out.push(self.acc);
                    self.acc = 0;
                    self.n = 0;
                }
            }
        }

        fn finish(mut self) -> Vec<u8> {
            if self.n > 0 {
                let pad = 8 - self.n;
                self.put(0, pad);
            }
            self.out
        }
    }

    fn crc8(data: &[u8]) -> u8 {
        let mut crc = 0u8;
        for &b in data {
            crc ^= b;
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

    fn crc16(data: &[u8]) -> u16 {
        let mut crc = 0u16;
        for &b in data {
            crc ^= u16::from(b) << 8;
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

    /// UTF-8-style coded frame number.
    fn utf8_number(n: u32) -> Vec<u8> {
        if n < 0x80 {
            return vec![n as u8];
        }
        let mut cont = Vec::new();
        let mut v = n;
        let mut lead_bits = 5u32;
        while v >= (1 << lead_bits) {
            cont.push(0x80 | (v & 0x3F) as u8);
            v >>= 6;
            lead_bits -= 1;
        }
        let len = cont.len() as u32 + 1;
        let prefix = (0xFF00u32 >> len) as u8;
        let mut out = vec![prefix | v as u8];
        out.extend(cont.into_iter().rev());
        out
    }

    pub fn write(path: &Path, rate: u32, bits: u32, channels: u16, samples: &[i32]) {
        let ch = usize::from(channels);
        let total_frames = samples.len() / ch;

        let mut file = b"fLaC".to_vec();
        let mut si = Bits::new();
        si.put(BLOCK as u64, 16); // min block size
        si.put(BLOCK as u64, 16); // max block size
        si.put(0, 24); // min frame size unknown
        si.put(0, 24); // max frame size unknown
        si.put(u64::from(rate), 20);
        si.put(u64::from(channels) - 1, 3);
        si.put(u64::from(bits) - 1, 5);
        si.put(total_frames as u64, 36);
        si.put(0, 64); // MD5 unknown
        si.put(0, 64);
        let si = si.finish();
        file.push(0x80); // last metadata block, type STREAMINFO
        file.extend_from_slice(&(si.len() as u32).to_be_bytes()[1..]);
        file.extend_from_slice(&si);

        let size_code: u8 = match bits {
            16 => 0b100,
            24 => 0b110,
            _ => panic!("test writer supports 16 and 24 bit"),
        };

        for (frame_no, chunk) in samples.chunks(BLOCK * ch).enumerate() {
            let frames = chunk.len() / ch;
            // Sync + fixed blocksize; block size as 16-bit value after the header; rate from
            // STREAMINFO; independent channels.
            let mut hdr = vec![0xFF, 0xF8, 0x70];
            hdr.push(((channels as u8 - 1) << 4) | (size_code << 1));
            hdr.extend(utf8_number(frame_no as u32));
            hdr.extend_from_slice(&((frames - 1) as u16).to_be_bytes());
            hdr.push(crc8(&hdr));

            let mut body = Bits::new();
            for c in 0..ch {
                body.put(0b0000_0010, 8); // VERBATIM subframe, no wasted bits
                for f in 0..frames {
                    let s = (chunk[f * ch + c] as i64 as u64) & ((1u64 << bits) - 1);
                    body.put(s, bits);
                }
            }
            let mut frame = hdr;
            frame.extend(body.finish());
            let crc = crc16(&frame);
            frame.extend_from_slice(&crc.to_be_bytes());
            file.extend(frame);
        }
        std::fs::write(path, file).unwrap();
    }
}
