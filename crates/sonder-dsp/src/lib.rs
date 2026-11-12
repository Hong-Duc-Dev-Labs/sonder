//! Sonder's DSP building blocks: the shared polyphase FIR and the stage contract, plus the
//! DSD→PCM decimator from phase 5.
//!
//! A rate conversion is a chain of [`Resampler`] stages fed by tables from a
//! [`FilterSourceChain`]: Roon tables when the user supplied a folder and the table exists,
//! otherwise Sonder's runtime design. See [`multistage`] for how large ratios are split.

mod dsd_tables {
    include!(concat!(env!("OUT_DIR"), "/dsd_tables.rs"));
}

pub mod dsd2pcm;
pub mod filter_source;
pub mod fir;
pub mod headroom;
pub mod multistage;
pub mod resampler;
pub mod roon_tables;
pub mod sonder_design;
pub mod stage;

pub use dsd2pcm::{DSD_PCM_RATES, DsdToPcm};
pub use filter_source::{
    FilterPhase, FilterSource, FilterSourceChain, FilterSourceKind, FilterSpec, FilterTable,
    FilterVariant,
};
pub use fir::FirCore;
pub use headroom::Headroom;
pub use resampler::{Ratio, Resampler};
pub use roon_tables::RoonTables;
pub use sonder_design::SonderDesigned;
pub use stage::{Stage, StreamFormat};
