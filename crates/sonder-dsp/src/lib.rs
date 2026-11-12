//! Sonder's DSP building blocks.
//!
//! Phase 5 owns the DSD→PCM decimator; later phases add the shared FIR/resampler/PEQ
//! stages. The coefficient tables are designed at build time by `build.rs`.

mod dsd_tables {
    include!(concat!(env!("OUT_DIR"), "/dsd_tables.rs"));
}

pub mod dsd2pcm;

pub use dsd2pcm::{DSD_PCM_RATES, DsdToPcm};
