//! Sonder's DSP building blocks: the shared polyphase FIR, the EQ and levelling stages, the
//! dither and volume stages, the stage contract, and the DSD→PCM decimator from phase 5.
//!
//! A rate conversion is a chain of [`Resampler`] stages fed by tables from a
//! [`FilterSourceChain`]: Roon tables when the user supplied a folder and the table exists,
//! otherwise Sonder's runtime design. See [`multistage`] for how large ratios are split.
//! [`PeqStage`], [`LevelingStage`], [`Volume`] and [`Dither`] are the remaining PCM stages;
//! the engine orders them headroom → SRC → convolution → PEQ → crossfeed → levelling → volume →
//! dither ([`Convolver`] and [`Crossfeed`] are the two optional shaping stages).

mod dsd_tables {
    include!(concat!(env!("OUT_DIR"), "/dsd_tables.rs"));
}

pub mod biquad;
pub mod convolver;
pub mod crossfeed;
pub mod dither;
pub mod dsd2pcm;
pub mod filter_source;
pub mod fir;
pub mod headroom;
pub mod leveling;
pub mod multistage;
pub mod peq;
pub mod resampler;
pub mod roon_tables;
pub mod sdm;
pub mod sonder_design;
pub mod stage;
pub mod volume;

pub use biquad::{Biquad, BiquadCoeffs, BiquadKind};
pub use convolver::{Convolver, ImpulseResponse, IrError, MAX_IR_SECONDS};
pub use crossfeed::{Crossfeed, CrossfeedPreset};
pub use dither::Dither;
pub use dsd2pcm::{DSD_PCM_RATES, DsdToPcm};
pub use filter_source::{
    FilterPhase, FilterSource, FilterSourceChain, FilterSourceKind, FilterSpec, FilterTable,
    FilterVariant,
};
pub use fir::FirCore;
pub use headroom::Headroom;
pub use leveling::{Gain, GainHandle, LevelingMode, LevelingStage};
pub use peq::{Peq, PeqBand, PeqStage};
pub use resampler::{Ratio, Resampler};
pub use roon_tables::RoonTables;
pub use sdm::{OVERSAMPLE, Sdm, SdmOrder, SdmRate};
pub use sonder_design::SonderDesigned;
pub use stage::{Stage, StreamFormat};
pub use volume::Volume;
