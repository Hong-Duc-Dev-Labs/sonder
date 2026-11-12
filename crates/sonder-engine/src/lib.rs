//! Playback engine: queue, transport, gapless transitions and the DSP stage chain.
//!
//! [`Player`] runs an engine thread that decodes (`sonder-decode`), optionally runs the
//! [`Pipeline`], and feeds an exclusive-mode stream (`sonder-output`). With no stages the
//! samples pass through as integers, which keeps the default path bit-perfect.
//!
//! DSD takes a separate path: DoP payload or native words packed by [`dop`], or the
//! [`sonder_dsp::DsdToPcm`] decimator feeding the normal pipeline. See [`DsdMode`].

pub mod dop;
pub mod pipeline;
mod player;
pub mod queue;
pub mod stage;
mod worker;

pub use pipeline::Pipeline;
pub use player::{
    Command, DsdPcmRate, PlaybackState, Player, PlayerConfig, PlayerEvent, TapFactory,
};
pub use queue::Queue;
pub use stage::{Stage, StreamFormat, Volume};
pub use worker::{DsdMode, OutputMode, OutputSpec};
