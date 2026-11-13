//! DSP stage contract re-exported for the engine, and the stages that ship with it.
//!
//! The contract and the PCM stages live in `sonder-dsp` so filters, resamplers, EQ, levelling,
//! volume and dither share one interface; the engine re-exports what its own modules use.

pub use sonder_dsp::stage::{Stage, StreamFormat};
pub use sonder_dsp::{
    Convolver, Crossfeed, Dither, GainHandle, LevelingStage, Peq, PeqStage, Volume,
};
