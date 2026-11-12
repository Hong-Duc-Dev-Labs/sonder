//! Device sample containers and lossless packing of left-justified `i32` samples.
//!
//! The pipeline carries every sample as a *left-justified* `i32` (see `sonder-decode`): a 16-bit
//! sample `s` is `s << 16`, a 24-bit one is `s << 8`. Packing into a device container only
//! drops low-order bits that are guaranteed zero when the container is at least as wide as the
//! source, which is what [`SampleContainer::holds`] checks.

use std::fmt;

/// How one sample is laid out in the device buffer (little-endian, signed integer).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SampleContainer {
    /// 16 bits in 2 bytes.
    S16,
    /// 24 bits in 3 bytes.
    S24Packed,
    /// 24 valid bits, most-significant aligned in a 4-byte container.
    S24In32,
    /// 32 bits in 4 bytes.
    S32,
    /// Native DSD on ALSA: four DSD bytes per channel in a big-endian word.
    ///
    /// Each frame carries 32 consecutive DSD bits, already MSB-first; the container is not a
    /// PCM integer and is never produced by [`SampleContainer::candidates`]. The word's
    /// 32-bit value written by [`pack`] carries byte 0 in bits 31..24.
    DsdU32Be,
}

impl SampleContainer {
    pub const ALL: [SampleContainer; 4] = [
        SampleContainer::S16,
        SampleContainer::S24Packed,
        SampleContainer::S24In32,
        SampleContainer::S32,
    ];

    /// Containers that carry PCM integers, for negotiation and probing.
    pub const PCM: [SampleContainer; 4] = Self::ALL;

    /// Bytes one sample occupies in the device buffer.
    pub const fn bytes(self) -> usize {
        match self {
            SampleContainer::S16 => 2,
            SampleContainer::S24Packed => 3,
            SampleContainer::S24In32 | SampleContainer::S32 | SampleContainer::DsdU32Be => 4,
        }
    }

    /// Bits of the container that carry signal.
    pub const fn valid_bits(self) -> u32 {
        match self {
            SampleContainer::S16 => 16,
            SampleContainer::S24Packed | SampleContainer::S24In32 => 24,
            SampleContainer::S32 | SampleContainer::DsdU32Be => 32,
        }
    }

    /// Whether a source with `source_bits` valid bits survives this container unchanged.
    ///
    /// Always false for [`DsdU32Be`](Self::DsdU32Be): native DSD is a different signal path,
    /// never a lossless PCM container.
    pub const fn holds(self, source_bits: u32) -> bool {
        !matches!(self, SampleContainer::DsdU32Be) && source_bits <= self.valid_bits()
    }

    /// Containers able to carry `source_bits` losslessly, in order of preference.
    ///
    /// Tightest fit first; wider containers follow as fallbacks for devices that reject the
    /// native one. A 16-bit source in a 24-bit container is still bit-perfect (the extra bits
    /// are zero).
    pub fn candidates(source_bits: u32) -> Vec<SampleContainer> {
        SampleContainer::ALL
            .into_iter()
            .filter(|c| c.holds(source_bits))
            .collect()
    }
}

impl fmt::Display for SampleContainer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            SampleContainer::S16 => "16-bit",
            SampleContainer::S24Packed => "24-bit packed",
            SampleContainer::S24In32 => "24-bit in 32",
            SampleContainer::S32 => "32-bit",
            SampleContainer::DsdU32Be => "DSD 32-bit BE",
        })
    }
}

/// A concrete device stream format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DeviceFormat {
    pub rate: u32,
    pub container: SampleContainer,
    pub channels: u16,
}

impl DeviceFormat {
    /// Bytes per interleaved frame.
    pub fn frame_bytes(&self) -> usize {
        self.container.bytes() * usize::from(self.channels)
    }
}

impl fmt::Display for DeviceFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} Hz, {}, {} ch",
            self.rate, self.container, self.channels
        )
    }
}

/// Packs left-justified samples into `dst` using `container` layout.
///
/// `dst` must be exactly `src.len() * container.bytes()` long. Allocation-free.
///
/// # Panics
/// Panics if `dst` has the wrong length; callers size it from the same frame count.
pub fn pack(container: SampleContainer, src: &[i32], dst: &mut [u8]) {
    assert_eq!(
        dst.len(),
        src.len() * container.bytes(),
        "destination size mismatch"
    );
    match container {
        SampleContainer::S16 => {
            for (s, out) in src.iter().zip(dst.as_chunks_mut::<2>().0) {
                *out = ((*s >> 16) as i16).to_le_bytes();
            }
        }
        SampleContainer::S24Packed => {
            for (s, out) in src.iter().zip(dst.as_chunks_mut::<3>().0) {
                let [_, b0, b1, b2] = s.to_le_bytes();
                *out = [b0, b1, b2];
            }
        }
        SampleContainer::S24In32 => {
            for (s, out) in src.iter().zip(dst.as_chunks_mut::<4>().0) {
                *out = (s & !0xFF).to_le_bytes();
            }
        }
        SampleContainer::S32 => {
            for (s, out) in src.iter().zip(dst.as_chunks_mut::<4>().0) {
                *out = s.to_le_bytes();
            }
        }
        SampleContainer::DsdU32Be => {
            for (s, out) in src.iter().zip(dst.as_chunks_mut::<4>().0) {
                *out = s.to_be_bytes();
            }
        }
    }
}

/// Inverse of [`pack`]: reads device-container bytes back into left-justified samples.
///
/// Used by tests and verification tooling; not on the real-time path.
pub fn unpack(container: SampleContainer, src: &[u8], dst: &mut [i32]) {
    assert_eq!(
        src.len(),
        dst.len() * container.bytes(),
        "source size mismatch"
    );
    match container {
        SampleContainer::S16 => {
            for (b, out) in src.as_chunks::<2>().0.iter().zip(dst.iter_mut()) {
                *out = i32::from(i16::from_le_bytes(*b)) << 16;
            }
        }
        SampleContainer::S24Packed => {
            for (&[b0, b1, b2], out) in src.as_chunks::<3>().0.iter().zip(dst.iter_mut()) {
                *out = i32::from_le_bytes([0, b0, b1, b2]);
            }
        }
        SampleContainer::S24In32 | SampleContainer::S32 => {
            for (b, out) in src.as_chunks::<4>().0.iter().zip(dst.iter_mut()) {
                *out = i32::from_le_bytes(*b);
            }
        }
        SampleContainer::DsdU32Be => {
            for (b, out) in src.as_chunks::<4>().0.iter().zip(dst.iter_mut()) {
                *out = i32::from_be_bytes(*b);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(container: SampleContainer, samples: &[i32]) -> Vec<i32> {
        let mut bytes = vec![0u8; samples.len() * container.bytes()];
        pack(container, samples, &mut bytes);
        let mut out = vec![0i32; samples.len()];
        unpack(container, &bytes, &mut out);
        out
    }

    #[test]
    fn s16_roundtrips_every_value() {
        let src: Vec<i32> = (i16::MIN..=i16::MAX).map(|v| i32::from(v) << 16).collect();
        assert_eq!(roundtrip(SampleContainer::S16, &src), src);
    }

    #[test]
    fn s16_bytes_are_little_endian_twos_complement() {
        let mut dst = [0u8; 8];
        pack(
            SampleContainer::S16,
            &[0x1234 << 16, -2 << 16, i32::MIN, 0x7FFF << 16],
            &mut dst,
        );
        assert_eq!(dst, [0x34, 0x12, 0xFE, 0xFF, 0x00, 0x80, 0xFF, 0x7F]);
    }

    #[test]
    fn s24_packed_roundtrips_every_value() {
        let src: Vec<i32> = (-(1i32 << 23)..(1i32 << 23)).map(|v| v << 8).collect();
        assert_eq!(roundtrip(SampleContainer::S24Packed, &src), src);
    }

    #[test]
    fn s24_packed_sign_extension_and_layout() {
        let mut dst = [0u8; 9];
        pack(
            SampleContainer::S24Packed,
            &[-1 << 8, 0x123456 << 8, i32::MIN],
            &mut dst,
        );
        assert_eq!(dst, [0xFF, 0xFF, 0xFF, 0x56, 0x34, 0x12, 0x00, 0x00, 0x80]);
        let mut back = [0i32; 3];
        unpack(SampleContainer::S24Packed, &dst, &mut back);
        assert_eq!(back, [-1 << 8, 0x123456 << 8, i32::MIN]);
    }

    #[test]
    fn s24_in_32_is_msb_aligned() {
        let mut dst = [0u8; 8];
        pack(
            SampleContainer::S24In32,
            &[0x123456 << 8, -1 << 8],
            &mut dst,
        );
        assert_eq!(dst, [0x00, 0x56, 0x34, 0x12, 0x00, 0xFF, 0xFF, 0xFF]);
    }

    #[test]
    fn s24_in_32_roundtrips_boundaries() {
        let src = [
            i32::MIN,
            i32::MAX & !0xFF,
            0,
            -1 << 8,
            1 << 8,
            0x7F_FFFF << 8,
        ];
        assert_eq!(roundtrip(SampleContainer::S24In32, &src), src);
    }

    #[test]
    fn s16_source_in_wider_containers_keeps_value() {
        let src: Vec<i32> = (i16::MIN..=i16::MAX)
            .step_by(7)
            .map(|v| i32::from(v) << 16)
            .collect();
        for container in [
            SampleContainer::S24Packed,
            SampleContainer::S24In32,
            SampleContainer::S32,
        ] {
            assert_eq!(roundtrip(container, &src), src, "{container}");
        }
    }

    #[test]
    fn s32_roundtrips_boundaries() {
        let src = [i32::MIN, i32::MAX, 0, -1, 1, 0x1234_5678, -0x1234_5678];
        assert_eq!(roundtrip(SampleContainer::S32, &src), src);
    }

    #[test]
    fn candidate_order_prefers_tightest_container() {
        use SampleContainer::*;
        assert_eq!(
            SampleContainer::candidates(16),
            vec![S16, S24Packed, S24In32, S32]
        );
        assert_eq!(
            SampleContainer::candidates(24),
            vec![S24Packed, S24In32, S32]
        );
        assert_eq!(SampleContainer::candidates(32), vec![S32]);
    }

    #[test]
    fn holds_rejects_lossy_containers() {
        assert!(!SampleContainer::S16.holds(24));
        assert!(!SampleContainer::S24In32.holds(32));
        assert!(SampleContainer::S32.holds(24));
    }

    #[test]
    fn dsd_word_is_big_endian_and_never_a_pcm_candidate() {
        let src = [0x0102_0304, 0x0506_0708];
        let mut bytes = [0u8; 8];
        pack(SampleContainer::DsdU32Be, &src, &mut bytes);
        assert_eq!(bytes, [1, 2, 3, 4, 5, 6, 7, 8]);
        let mut back = [0i32; 2];
        unpack(SampleContainer::DsdU32Be, &bytes, &mut back);
        assert_eq!(back, src);
        assert!(!SampleContainer::candidates(32).contains(&SampleContainer::DsdU32Be));
        assert!(!SampleContainer::DsdU32Be.holds(1));
    }

    #[test]
    #[should_panic(expected = "destination size mismatch")]
    fn pack_rejects_wrong_destination_length() {
        pack(SampleContainer::S16, &[0, 0], &mut [0u8; 3]);
    }
}
