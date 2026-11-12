//! Audio output: device formats, sample packing and exclusive-mode backends.
//!
//! Data path: a producer thread pushes left-justified `i32` samples into a lock-free queue
//! ([`sample_ring`]); the backend's render thread packs them into the negotiated device
//! container without changing their value.

#[cfg(target_os = "linux")]
pub mod alsa;
#[cfg(target_os = "macos")]
pub mod coreaudio;
mod error;
pub mod format;
pub mod null;
mod render;
mod ring;
mod stream;
#[cfg(windows)]
pub mod wasapi;

pub use error::OutputError;
pub use format::{DeviceFormat, SampleContainer};
pub use ring::{SampleProducer, SampleSource, sample_ring};
pub use stream::{Stream, StreamHandle, StreamOptions, StreamReport, WriteTap};

/// Sample rates probed when listing what a device supports.
pub const COMMON_RATES: [u32; 10] = [
    44_100, 48_000, 88_200, 96_000, 176_400, 192_000, 352_800, 384_000, 705_600, 768_000,
];

/// Containers DoP may use: a 24-bit word in a 32-bit slot, or a plain 32-bit slot.
pub const DOP_CONTAINERS: [SampleContainer; 2] = [SampleContainer::S24In32, SampleContainer::S32];

/// Opaque, backend-specific device identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DeviceId(pub String);

#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub id: DeviceId,
    pub name: String,
    pub is_default: bool,
    /// Stereo formats the device accepts in exclusive mode: [`COMMON_RATES`] × the PCM
    /// containers, plus native-DSD formats on ALSA devices that expose them.
    pub formats: Vec<DeviceFormat>,
}

/// An exclusive-mode output API.
///
/// Errors from [`open`](Self::open) and from a running [`Stream`] use these variants so the
/// engine can apply one recovery policy everywhere:
/// - [`OutputError::DeviceInUse`]: another application holds the device exclusively.
/// - [`OutputError::DeviceLost`] / [`OutputError::DeviceNotFound`]: the device went away
///   (unplugged, invalidated, system sleep).
/// - xruns are recovered inside the stream and only counted in
///   [`StreamHandle::underruns`].
pub trait OutputBackend: Send {
    fn devices(&self) -> Result<Vec<DeviceInfo>, OutputError>;

    /// Ids of the devices currently present. Cheap enough to poll every few seconds: it does
    /// not probe formats or open the device.
    fn device_ids(&self) -> Result<Vec<DeviceId>, OutputError> {
        Ok(self.devices()?.into_iter().map(|d| d.id).collect())
    }

    /// Whether `device` accepts `format` exactly, without any conversion.
    fn is_supported(&self, device: &DeviceId, format: DeviceFormat) -> Result<bool, OutputError>;

    /// Every format from [`COMMON_RATES`] × [`SampleContainer::ALL`] the device accepts.
    fn supported_formats(
        &self,
        device: &DeviceId,
        channels: u16,
    ) -> Result<Vec<DeviceFormat>, OutputError>;

    /// Starts a stream that plays everything pushed into the producer paired with `source`.
    fn open(
        &mut self,
        device: &DeviceId,
        format: DeviceFormat,
        source: SampleSource,
        options: StreamOptions,
    ) -> Result<Stream, OutputError>;
}

/// Picks the first container that carries `bits` losslessly at `rate` and that the device
/// accepts. Never resamples or truncates: if nothing fits, the error lists what the device
/// does support.
pub fn negotiate(
    backend: &dyn OutputBackend,
    device: &DeviceId,
    rate: u32,
    bits: u32,
    channels: u16,
) -> Result<DeviceFormat, OutputError> {
    for container in SampleContainer::candidates(bits) {
        let format = DeviceFormat {
            rate,
            container,
            channels,
        };
        if backend.is_supported(device, format)? {
            return Ok(format);
        }
    }
    Err(OutputError::UnsupportedFormat {
        requested: format!("{rate} Hz, {bits}-bit, {channels} ch"),
        supported: backend
            .supported_formats(device, channels)
            .unwrap_or_default(),
    })
}

/// Picks a DoP carrier format for the carrier `rate` (DSD rate / 16).
///
/// DoP needs a 24-in-32 or 32-bit container. Returns the exact format to open; callers fall
/// back to DSD→PCM when this fails.
pub fn negotiate_dop(
    backend: &dyn OutputBackend,
    device: &DeviceId,
    rate: u32,
    channels: u16,
) -> Result<DeviceFormat, OutputError> {
    for container in DOP_CONTAINERS {
        let format = DeviceFormat {
            rate,
            container,
            channels,
        };
        if backend.is_supported(device, format)? {
            return Ok(format);
        }
    }
    Err(OutputError::UnsupportedFormat {
        requested: format!("DoP {rate} Hz, {channels} ch"),
        supported: backend
            .supported_formats(device, channels)
            .unwrap_or_default(),
    })
}

/// Picks the native-DSD format for the word `rate` (DSD rate / 32).
///
/// Native DSD needs a backend that advertises [`SampleContainer::DsdU32Be`]; ALSA does on
/// Linux when the DAC exposes it. Callers fall back to DoP or PCM when this fails.
pub fn negotiate_native(
    backend: &dyn OutputBackend,
    device: &DeviceId,
    rate: u32,
    channels: u16,
) -> Result<DeviceFormat, OutputError> {
    let format = DeviceFormat {
        rate,
        container: SampleContainer::DsdU32Be,
        channels,
    };
    if backend.is_supported(device, format)? {
        Ok(format)
    } else {
        Err(OutputError::UnsupportedFormat {
            requested: format!("native DSD {rate} Hz, {channels} ch"),
            supported: backend
                .supported_formats(device, channels)
                .unwrap_or_default(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Backend that accepts a fixed format list, for negotiation tests.
    struct FakeBackend(Vec<DeviceFormat>);

    impl OutputBackend for FakeBackend {
        fn devices(&self) -> Result<Vec<DeviceInfo>, OutputError> {
            Ok(Vec::new())
        }
        fn is_supported(&self, _: &DeviceId, f: DeviceFormat) -> Result<bool, OutputError> {
            Ok(self.0.contains(&f))
        }
        fn supported_formats(
            &self,
            _: &DeviceId,
            _: u16,
        ) -> Result<Vec<DeviceFormat>, OutputError> {
            Ok(self.0.clone())
        }
        fn open(
            &mut self,
            _: &DeviceId,
            _: DeviceFormat,
            _: SampleSource,
            _: StreamOptions,
        ) -> Result<Stream, OutputError> {
            unimplemented!()
        }
    }

    fn fmt(rate: u32, container: SampleContainer) -> DeviceFormat {
        DeviceFormat {
            rate,
            container,
            channels: 2,
        }
    }

    #[test]
    fn falls_back_from_packed_24_to_24_in_32_to_32() {
        let dev = DeviceId("d".into());
        let only32 = FakeBackend(vec![fmt(96_000, SampleContainer::S32)]);
        assert_eq!(
            negotiate(&only32, &dev, 96_000, 24, 2).unwrap().container,
            SampleContainer::S32
        );

        let both = FakeBackend(vec![
            fmt(96_000, SampleContainer::S24In32),
            fmt(96_000, SampleContainer::S32),
        ]);
        assert_eq!(
            negotiate(&both, &dev, 96_000, 24, 2).unwrap().container,
            SampleContainer::S24In32
        );
    }

    #[test]
    fn refuses_rate_mismatch_and_lists_supported_formats() {
        let dev = DeviceId("d".into());
        let backend = FakeBackend(vec![fmt(48_000, SampleContainer::S24In32)]);
        let err = negotiate(&backend, &dev, 44_100, 16, 2).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("44100 Hz, 16-bit, 2 ch"), "{msg}");
        assert!(msg.contains("48000 Hz, 24-bit in 32, 2 ch"), "{msg}");
    }

    #[test]
    fn never_truncates_a_24_bit_source_to_16() {
        let dev = DeviceId("d".into());
        let backend = FakeBackend(vec![fmt(44_100, SampleContainer::S16)]);
        assert!(negotiate(&backend, &dev, 44_100, 24, 2).is_err());
    }

    #[test]
    fn dop_prefers_24_in_32_and_falls_back_to_32() {
        let dev = DeviceId("d".into());
        let both = FakeBackend(vec![
            fmt(352_800, SampleContainer::S32),
            fmt(352_800, SampleContainer::S24In32),
        ]);
        assert_eq!(
            negotiate_dop(&both, &dev, 352_800, 2).unwrap(),
            fmt(352_800, SampleContainer::S24In32)
        );
        let only32 = FakeBackend(vec![fmt(352_800, SampleContainer::S32)]);
        assert_eq!(
            negotiate_dop(&only32, &dev, 352_800, 2).unwrap().container,
            SampleContainer::S32
        );
        let none = FakeBackend(vec![fmt(352_800, SampleContainer::S16)]);
        assert!(negotiate_dop(&none, &dev, 352_800, 2).is_err());
    }

    #[test]
    fn native_dsd_needs_the_alsa_word_format() {
        let dev = DeviceId("d".into());
        let native = FakeBackend(vec![DeviceFormat {
            rate: 88_200,
            container: SampleContainer::DsdU32Be,
            channels: 2,
        }]);
        assert_eq!(
            negotiate_native(&native, &dev, 88_200, 2).unwrap(),
            DeviceFormat {
                rate: 88_200,
                container: SampleContainer::DsdU32Be,
                channels: 2
            }
        );
        let pcm_only = FakeBackend(vec![fmt(88_200, SampleContainer::S32)]);
        assert!(negotiate_native(&pcm_only, &dev, 88_200, 2).is_err());
    }
}
