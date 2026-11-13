//! PCM→DSD output settings.
//!
//! When a zone asks for DSD output, every PCM track is resampled to the modulator's input
//! rate, run through [`Sdm`](sonder_dsp::Sdm), and packed for DoP or native DSD. The device
//! must accept the carrier format; otherwise the track stays PCM and the signal path says so.

use sonder_dsp::{SdmOrder, SdmRate};

/// Output mode for PCM sources.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SdmMode {
    /// Play PCM as PCM.
    #[default]
    Off,
    /// Modulate to DSD64 and carry it as DoP or native DSD.
    Dsd64,
    Dsd128,
    Dsd256,
}

impl SdmMode {
    pub const fn rate(self) -> Option<SdmRate> {
        match self {
            Self::Off => None,
            Self::Dsd64 => Some(SdmRate::Dsd64),
            Self::Dsd128 => Some(SdmRate::Dsd128),
            Self::Dsd256 => Some(SdmRate::Dsd256),
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Off => "PCM",
            Self::Dsd64 => "DSD64",
            Self::Dsd128 => "DSD128",
            Self::Dsd256 => "DSD256",
        }
    }
}

/// PCM→DSD configuration: target rate and modulator order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SdmConfig {
    pub mode: SdmMode,
    pub order: SdmOrder,
}

impl Default for SdmConfig {
    fn default() -> Self {
        Self {
            mode: SdmMode::Off,
            order: SdmOrder::Order5,
        }
    }
}

impl SdmConfig {
    pub const fn is_on(&self) -> bool {
        !matches!(self.mode, SdmMode::Off)
    }
}
