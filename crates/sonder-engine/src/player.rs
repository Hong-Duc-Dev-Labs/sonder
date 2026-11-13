//! The [`Player`] handle and the engine thread behind it.
//!
//! One engine thread owns the queue, the decoder and the output stream. It reacts to
//! [`Command`]s, decodes ahead into a small feed, pushes the feed into the lock-free ring of
//! the open stream, and reports [`PlayerEvent`]s. The device's render thread only ever sees the
//! ring, so nothing here runs on the real-time path.
//!
//! Gapless: the stream stays open across tracks while the next track's output format (after
//! DSP) fits the open device format; the next track's samples follow the previous ones in the
//! same ring. A different format drains the stream, closes it and opens a new one.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, unbounded};
use sonder_decode::{
    DSD_SOURCE_BITS, DecodeError, DsdFormat, SourceDecoder, SourceFormat, TrackFormat,
};
use sonder_output::{
    DeviceFormat, DeviceId, OutputBackend, OutputError, SampleContainer, SampleProducer, Stream,
    StreamHandle, StreamOptions, WriteTap, negotiate, negotiate_dop, negotiate_native, sample_ring,
};

use crate::crossfade::{self, CrossfadeConfig, Fade, Side, Skip, Transition};
use crate::dsp::{DspConfig, LoudnessSource, NoLoudness, TrackGain};
use crate::queue::Queue;
use crate::rate::{self, DeviceFormats, FilterResolver, SignalStage, SrcConfig, SrcPlan};
use crate::sdm::SdmConfig;
use crate::stage::{
    Convolver, Crossfeed, Dither, GainHandle, LevelingStage, Peq, PeqStage, Stage, Volume,
};
use crate::worker::{DecodeWorker, DsdMode, OutputMode, OutputSpec, SdmPacking};
use sonder_dsp::{CrossfeedPreset, Gain, ImpulseResponse, LevelingMode, SdmOrder, SdmRate};

/// How long a seek waits for the render thread to drop queued audio before reopening instead.
const FLUSH_TIMEOUT: Duration = Duration::from_millis(500);
/// Engine sleep when there is nothing to decode or push.
const IDLE_WAIT: Duration = Duration::from_millis(2);
/// "Previous" restarts the current track when it has played longer than this.
const RESTART_THRESHOLD: Duration = Duration::from_secs(3);
/// Decoded samples held outside the ring before decoding waits.
const FEED_AHEAD_SAMPLES: usize = 32_768;
/// Smallest ring, whatever the configured duration.
const MIN_RING_FRAMES: usize = 1024;
/// Highest rate convolution runs at; above it the response runs before the converter.
const MAX_CONVOLUTION_RATE: u32 = 384_000;

/// Requests to the engine thread.
#[derive(Debug)]
pub enum Command {
    Play,
    Pause,
    Stop,
    /// Moves to a position in the current track.
    Seek(Duration),
    Next,
    Previous,
    /// Starts playing the queue entry at this index from its beginning, in any state.
    PlayIndex(usize),
    Enqueue(Vec<PathBuf>),
    /// Inserts entries before this queue index (clamped to the end). Inserting right after
    /// the playing entry makes them play next, gaplessly where the formats allow.
    Insert(usize, Vec<PathBuf>),
    /// Removes the queue entry at this index. Removing the playing entry moves on to the one
    /// after it.
    Remove(usize),
    Clear,
    /// Software volume in dB; `None` removes the stage and restores bit-perfect output.
    SetVolume(Option<f64>),
    /// Replaces the PCM shaping settings (EQ, levelling, dither). A playing track restarts
    /// from its position so the change is heard after one ring, not after the whole feed.
    SetDsp(DspConfig),
    /// Replaces the rate-conversion policy, filter source and headroom. The next track (or
    /// the current one, restarted) is planned with the new settings.
    SetSrc(SrcConfig),
    /// Plays on another device from now on. Playback continues on it from the current
    /// position; a stopped player just opens it on the next play.
    SetDevice(DeviceId),
    /// Replaces the DSD handling (see [`PlayerConfig::default_dsd_mode`],
    /// [`dsd_modes`](PlayerConfig::dsd_modes), [`dsd_pcm_rate`](PlayerConfig::dsd_pcm_rate)).
    /// A playing DSD track switches from its current position; anything else picks it up on
    /// its next open.
    SetDsd {
        default_mode: DsdMode,
        modes: HashMap<DeviceId, DsdMode>,
        pcm_rate: DsdPcmRate,
    },
    /// Replaces the PCM→DSD output mode and modulator order. A playing PCM track restarts
    /// from its position so the change is heard after one ring.
    SetSdm(SdmConfig),
    /// How the queue advances when a track ends. Turning it off keeps the pre-buffered next
    /// entry; the mode applies from the next transition.
    SetRepeat(RepeatMode),
    /// Replaces the crossfade settings. A fade already in progress finishes; the next
    /// transition uses the new values.
    SetCrossfade(CrossfadeConfig),
    Shutdown,
}

/// What plays after a track ends.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RepeatMode {
    /// Stop after the last entry.
    #[default]
    Off,
    /// After the last entry, continue at the first.
    All,
    /// Play the same entry again.
    One,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackState {
    Stopped,
    Playing,
    Paused,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PlayerEvent {
    State(PlaybackState),
    /// The device started playing a different track.
    TrackChanged {
        index: usize,
        path: PathBuf,
        source: SourceFormat,
        duration: Option<Duration>,
    },
    /// A stream opened at this device format.
    OutputFormat(DeviceFormat),
    Position {
        index: usize,
        elapsed: Duration,
    },
    /// Total underruns and recovered xruns since the player started.
    Underruns(u64),
    /// The current track clipped: samples clamped at full scale in the decode step where it
    /// was first seen. Reported at most once per track, so the count is a sign, not a total.
    Clipping(u64),
    /// A track could not be opened, decoded, or played on this device; playback moves on.
    TrackFailed {
        index: usize,
        path: PathBuf,
        error: String,
    },
    /// The device disappeared (unplugged, invalidated, sleep). Playback pauses and resumes
    /// paused at the same position once the device is back.
    DeviceLost(String),
    /// Another application holds the device exclusively. Playback pauses; play retries.
    DeviceBusy,
    /// The output failed for another reason. Playback pauses; play retries.
    OutputFailed(String),
    /// The set of present devices changed.
    DevicesChanged(Vec<DeviceId>),
    /// How a DSD track is being played, with the reason when the requested mode was not
    /// possible (DoP unsupported, DSP stages active, no native DSD).
    DsdMode {
        mode: DsdMode,
        reason: Option<String>,
    },
    /// What happens to the signal from the source to the device for the announced track,
    /// including which filter source served each resampling stage.
    SignalPath(SignalPath),
    /// A recoverable configuration problem worth showing the user (for example a missing
    /// Roon filter folder, which falls back to Sonder).
    Warning(String),
    EndOfQueue,
}

/// The stages a track passes through, and whether the output is still bit-perfect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignalPath {
    pub source: SourceFormat,
    pub output_rate: u32,
    pub output_bits: u32,
    pub output_channels: u16,
    pub mode: OutputMode,
    /// In processing order: headroom, SRC, PEQ, levelling, volume, dither.
    pub stages: Vec<SignalStage>,
    /// `true` only when no stage touched the samples and the rate is unchanged. Automatic
    /// SRC fallback and any DSP setting make this `false`.
    pub bit_perfect: bool,
    /// How this track joined the previous one: the crossfade length, or why it joined
    /// gaplessly instead ("crossfade skipped: same album"). `None` for the first track of a
    /// stream and when crossfade is off.
    pub transition: Option<String>,
}

/// Builds a tap for each stream the player opens (bit-perfect verification).
pub type TapFactory = Box<dyn FnMut(DeviceFormat) -> Box<dyn WriteTap> + Send>;

/// The PCM rate DSD→PCM conversion targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DsdPcmRate {
    #[default]
    R176_4,
    R352_8,
}

impl DsdPcmRate {
    pub const fn rate(self) -> u32 {
        match self {
            Self::R176_4 => 176_400,
            Self::R352_8 => 352_800,
        }
    }
}

pub struct PlayerConfig {
    pub device: DeviceId,
    /// Audio queued between DSP and the device. Control changes (seek, volume) are heard
    /// after at most this long; shorter means more underrun risk.
    pub ring: Duration,
    /// How often the device list is checked for hot-plug changes.
    pub device_poll: Duration,
    /// How often position events are sent while playing.
    pub position_interval: Duration,
    pub tap: Option<TapFactory>,
    /// DSD handling for devices not listed in [`dsd_modes`](Self::dsd_modes).
    pub default_dsd_mode: DsdMode,
    /// Per-device DSD handling, keyed by [`DeviceId`].
    pub dsd_modes: HashMap<DeviceId, DsdMode>,
    /// Output rate for DSD→PCM conversion.
    pub dsd_pcm_rate: DsdPcmRate,
    /// Rate-conversion policy, filter source and automatic fallback. The default keeps the
    /// bit-perfect path unless the device refuses the source rate.
    pub src: SrcConfig,
    /// PCM shaping: EQ, levelling and dither.
    pub dsp: DspConfig,
    /// PCM→DSD output: rate and modulator order. `Off` keeps PCM output.
    pub sdm: SdmConfig,
    /// ReplayGain / loudness lookup for levelling; the default knows nothing.
    pub loudness: Arc<dyn LoudnessSource>,
    /// Equal-power crossfade between entries. Off keeps every transition gapless.
    pub crossfade: CrossfadeConfig,
}

impl PlayerConfig {
    pub fn new(device: DeviceId) -> Self {
        Self {
            device,
            ring: Duration::from_millis(150),
            device_poll: Duration::from_secs(2),
            position_interval: Duration::from_millis(100),
            tap: None,
            default_dsd_mode: DsdMode::Dop,
            dsd_modes: HashMap::new(),
            dsd_pcm_rate: DsdPcmRate::R176_4,
            src: SrcConfig::default(),
            dsp: DspConfig::default(),
            sdm: SdmConfig::default(),
            loudness: Arc::new(NoLoudness),
            crossfade: CrossfadeConfig::default(),
        }
    }

    /// DSD handling for `device`: its entry, else the default.
    pub fn dsd_mode_for(&self, device: &DeviceId) -> DsdMode {
        self.dsd_modes
            .get(device)
            .copied()
            .unwrap_or(self.default_dsd_mode)
    }
}

/// Handle to the engine thread. Dropping it stops playback and joins the thread.
pub struct Player {
    commands: Sender<Command>,
    thread: Option<JoinHandle<()>>,
}

impl Player {
    /// Starts the engine thread on `backend`. Events arrive on the returned receiver.
    pub fn spawn(
        backend: Box<dyn OutputBackend>,
        config: PlayerConfig,
    ) -> std::io::Result<(Self, Receiver<PlayerEvent>)> {
        let (commands, command_rx) = unbounded();
        let (event_tx, events) = unbounded();
        let thread = thread::Builder::new()
            .name("sonder-engine".into())
            .spawn(move || Engine::new(backend, config, command_rx, event_tx).run())?;
        Ok((
            Self {
                commands,
                thread: Some(thread),
            },
            events,
        ))
    }

    pub fn send(&self, command: Command) {
        // The engine only exits on shutdown, after which nothing is listening anyway.
        let _ = self.commands.send(command);
    }

    pub fn play(&self) {
        self.send(Command::Play);
    }

    pub fn pause(&self) {
        self.send(Command::Pause);
    }

    pub fn stop(&self) {
        self.send(Command::Stop);
    }

    pub fn seek(&self, position: Duration) {
        self.send(Command::Seek(position));
    }

    pub fn next(&self) {
        self.send(Command::Next);
    }

    pub fn previous(&self) {
        self.send(Command::Previous);
    }

    pub fn play_index(&self, index: usize) {
        self.send(Command::PlayIndex(index));
    }

    pub fn set_device(&self, device: DeviceId) {
        self.send(Command::SetDevice(device));
    }

    pub fn enqueue(&self, paths: Vec<PathBuf>) {
        self.send(Command::Enqueue(paths));
    }

    pub fn insert(&self, index: usize, paths: Vec<PathBuf>) {
        self.send(Command::Insert(index, paths));
    }

    pub fn remove(&self, index: usize) {
        self.send(Command::Remove(index));
    }

    pub fn clear(&self) {
        self.send(Command::Clear);
    }

    pub fn set_volume(&self, db: Option<f64>) {
        self.send(Command::SetVolume(db));
    }

    /// Replaces the PCM shaping settings: parametric EQ, levelling and dither.
    pub fn set_dsp(&self, dsp: DspConfig) {
        self.send(Command::SetDsp(dsp));
    }

    /// Replaces the rate-conversion policy, filter source and headroom.
    pub fn set_src(&self, src: SrcConfig) {
        self.send(Command::SetSrc(src));
    }

    /// Changes DSD handling: the mode for devices without an entry in `modes`, per-device
    /// modes, and the DSD→PCM output rate. Unchanged values are a no-op.
    pub fn set_dsd(
        &self,
        default_mode: DsdMode,
        modes: HashMap<DeviceId, DsdMode>,
        pcm_rate: DsdPcmRate,
    ) {
        self.send(Command::SetDsd {
            default_mode,
            modes,
            pcm_rate,
        });
    }

    /// Replaces the PCM→DSD output mode and order. The current track restarts from its
    /// position when the change reaches it.
    pub fn set_sdm(&self, sdm: SdmConfig) {
        self.send(Command::SetSdm(sdm));
    }

    /// Sets what plays after the current track ends.
    pub fn set_repeat(&self, repeat: RepeatMode) {
        self.send(Command::SetRepeat(repeat));
    }

    /// Replaces the crossfade settings.
    pub fn set_crossfade(&self, crossfade: CrossfadeConfig) {
        self.send(Command::SetCrossfade(crossfade));
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.send(Command::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// A position as `ticks / rate` seconds, so frame positions survive without rounding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Pos {
    ticks: u64,
    rate: u32,
}

impl Pos {
    const ZERO: Pos = Pos { ticks: 0, rate: 1 };

    fn frames(frames: u64, rate: u32) -> Self {
        Self {
            ticks: frames,
            rate: rate.max(1),
        }
    }

    fn from_duration(d: Duration) -> Self {
        Self {
            ticks: u64::try_from(d.as_nanos()).unwrap_or(u64::MAX),
            rate: 1_000_000_000,
        }
    }

    fn frames_at(self, rate: u32) -> u64 {
        let v = u128::from(self.ticks) * u128::from(rate) / u128::from(self.rate);
        u64::try_from(v).unwrap_or(u64::MAX)
    }

    fn duration(self) -> Duration {
        let nanos = u128::from(self.ticks) * 1_000_000_000 / u128::from(self.rate);
        Duration::from_nanos(u64::try_from(nanos).unwrap_or(u64::MAX))
    }
}

enum FeedItem {
    /// The following samples belong to this track, starting at `offset` source frames.
    TrackStart {
        index: usize,
        offset: u64,
        source: SourceFormat,
        spec: OutputSpec,
        duration: Option<u64>,
        path: Arc<SignalPath>,
    },
    Samples {
        data: Vec<i32>,
        taken: usize,
    },
    /// No more tracks: drain the stream and report the end of the queue.
    End,
}

/// Where a track starts in a stream's frame index space.
#[derive(Debug, Clone)]
struct Segment {
    start: u64,
    index: usize,
    offset: u64,
    source: SourceFormat,
    duration: Option<u64>,
    path: Arc<SignalPath>,
}

struct Output {
    stream: Stream,
    /// `None` once the end of input was signalled and the stream is draining.
    producer: Option<SampleProducer>,
    handle: StreamHandle,
    format: DeviceFormat,
    /// How the ring's samples are laid out (PCM, DoP payload or native DSD words).
    mode: OutputMode,
    /// Frames pushed into the ring so far.
    pushed: u64,
    segments: Vec<Segment>,
}

struct Decoding {
    index: usize,
    decoder: SourceDecoder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HoldReason {
    Lost,
    Busy,
    Failed,
}

/// Playback is paused on a device problem; this is where it resumes.
#[derive(Debug, Clone, Copy)]
struct Hold {
    index: usize,
    pos: Pos,
    reason: HoldReason,
}

/// What the device is playing right now.
#[derive(Debug, Clone)]
struct Now {
    segment: Segment,
    frame: u64,
}

/// What the worker's chain is built from, compared to decide whether it needs rebuilding.
#[derive(Debug, Clone, PartialEq)]
struct StageKey {
    /// Source, target and channels of the rate-conversion plan.
    plan: Option<(u32, u32, u16)>,
    /// The response's identity and whether it runs before the converter.
    convolution: Option<(usize, bool)>,
    peq: Option<Peq>,
    crossfeed: Option<CrossfeedPreset>,
    leveling: Option<LevelingMode>,
    volume: Option<u64>,
    dither: Option<u32>,
    /// DoP or native DSD: every shaping stage is left out.
    bitstream: bool,
}

/// A track opened and its chain prepared, before it is announced.
struct Opened {
    index: usize,
    decoder: SourceDecoder,
    source: SourceFormat,
    spec: OutputSpec,
    duration: Option<u64>,
}

/// One track's decode side: its stage chain, the rate-conversion plan and the fields the
/// chain was built from. The engine owns the current one; a crossfade builds the incoming
/// track's beside it (see [`Crossfade`]) and installs it when the fade ends.
struct ChainState {
    worker: DecodeWorker,
    /// The rate-conversion plan applied to the worker, and the source/target key it was
    /// built for. `None` key means the bit-perfect path.
    plan: Option<SrcPlan>,
    plan_key: Option<(u32, u16, Option<u32>)>,
    /// Channel count of the planned track, so stage changes can re-decide dither.
    channels: u16,
    /// Rate the planned track enters the chain at (the DSD→PCM rate for a converted DSD
    /// track), so the signal path can say where convolution runs.
    chain_rate: u32,
    /// Target word length for dither when the device container is narrower than the signal.
    dither_bits: Option<u32>,
    /// The open track goes out as DoP or native DSD, so the chain must stay empty.
    bitstream: bool,
    /// The PCM→DSD rate applied to the current plan, if any.
    sdm_rate: Option<SdmRate>,
    /// What the worker's chain currently contains, so it is rebuilt only when it changes.
    stages_key: Option<StageKey>,
    /// Levelling gain shared with the stage in the chain; updated per track.
    leveling: GainHandle,
    /// What the chain produces, for the crossfade's format rule.
    spec: Option<OutputSpec>,
}

impl Default for ChainState {
    fn default() -> Self {
        Self {
            worker: DecodeWorker::new(Vec::new()),
            plan: None,
            plan_key: None,
            channels: 2,
            chain_rate: 0,
            dither_bits: None,
            bitstream: false,
            sdm_rate: None,
            stages_key: None,
            leveling: GainHandle::default(),
            spec: None,
        }
    }
}

/// Consecutive decode failures of one queue entry. A whole queue's worth ends the run, so a
/// repeat mode cannot retry a file that fails mid-stream forever.
#[derive(Default)]
struct FailureRun {
    index: Option<usize>,
    count: usize,
}

impl FailureRun {
    fn clear(&mut self) {
        self.index = None;
        self.count = 0;
    }

    /// Notes a failure of `index`; `true` when the run should end instead of retrying.
    fn note(&mut self, index: usize, queue_len: usize) -> bool {
        if self.index != Some(index) {
            self.index = Some(index);
            self.count = 0;
        }
        self.count += 1;
        self.count >= queue_len.max(1)
    }
}

/// How the open track joined the previous one, for its signal path.
enum TransitionNote {
    /// The track faded in over this many seconds.
    Faded(f64),
    /// The transition stayed gapless instead; this is why.
    Skipped(&'static str),
}

/// A crossfade in progress: the incoming track's chain and decoder, and the mixer that joins
/// it to the outgoing track's last frames.
struct Crossfade {
    index: usize,
    source: SourceFormat,
    spec: OutputSpec,
    decoder: SourceDecoder,
    chain: ChainState,
    fade: Fade,
    /// Decoded but unmixed chain output of each side, interleaved at [`Self::spec`]'s rate.
    pending_out: Vec<f64>,
    pending_in: Vec<f64>,
    /// Mixer scratch: mixed floats, then the samples that reach the feed.
    mixed: Vec<f64>,
    samples: Vec<i32>,
    out_ended: bool,
    in_ended: bool,
    /// The incoming decoder failed; it must not become the current one.
    in_failed: bool,
}

struct Engine {
    backend: Box<dyn OutputBackend>,
    config: PlayerConfig,
    commands: Receiver<Command>,
    events: Sender<PlayerEvent>,
    queue: Queue,
    state: PlaybackState,
    /// How the queue advances after a track ends.
    repeat: RepeatMode,
    /// The current track's decode side.
    chain: ChainState,
    /// Source format of the open track.
    source: Option<SourceFormat>,
    decoding: Option<Decoding>,
    feed: VecDeque<FeedItem>,
    feed_samples: usize,
    out: Option<Output>,
    /// The end of the queue was signalled to the open stream; it is draining.
    ending: bool,
    /// Track index decoding would continue at if tracks are appended after the end.
    end_index: Option<usize>,
    hold: Option<Hold>,
    /// Where play starts from a stop.
    stopped_at: Pos,
    /// Where the last restart is heading, until its track start reaches the ring. Commands
    /// issued in that window (a second Next, a seek right after Next) act on this track.
    target: Option<(usize, Pos)>,
    /// Set when the queue played to the end: play then starts at this index (the first track
    /// added afterwards) instead of replaying the queue.
    after_end: Option<usize>,
    reported_track: Option<usize>,
    /// Last signal path sent, so a mid-track change (volume, SRC) is reported again.
    reported_path: Option<Arc<SignalPath>>,
    last_position: Option<Instant>,
    underruns_base: u64,
    reported_underruns: u64,
    devices: Vec<DeviceId>,
    last_poll: Instant,
    last_retry: Instant,
    /// Track whose assumed DSD mode was rejected when its stream opened and that was decoded
    /// again with the device free; a second rejection is a normal output failure.
    dsd_reprobe: Option<usize>,
    resolver: FilterResolver,
    /// Source rates the device refused, mapped to the rate it accepted. Cleared on a device
    /// change, so the next track re-probes the new device.
    fallback: HashMap<(u32, u16), u32>,
    volume_db: Option<f64>,
    /// Response and channel count the "does not fit" warning was last given for.
    convolution_warned: Option<(usize, u16)>,
    /// Channel count the "crossfeed needs stereo" warning was last given for.
    crossfeed_warned: Option<u16>,
    /// Whether clipping was already reported for the current track.
    clipping_reported: bool,
    /// Samples the dither stage clamped; it quantises before the worker's conversion, so the
    /// worker's own clip count never sees them.
    dither_clipped: Arc<AtomicU64>,
    /// Track whose fallback SRC was already tried after a refusal; a second refusal is a
    /// normal output failure.
    src_retry: Option<usize>,
    /// The modulator-limit warning was announced for the current track.
    sdm_warned: bool,
    /// Crossfade in preparation or progress, if any.
    crossfade: Option<Crossfade>,
    /// The transition into the open track was already decided, so it is not re-planned.
    fade_checked: bool,
    /// The incoming entry and fade length this track will fade into, read once near the end.
    fade_plan: Option<(usize, f64)>,
    /// Note the transition into the next track adds to its signal path (a crossfade length
    /// or the reason it was skipped).
    pending_transition: Option<TransitionNote>,
    /// Decode failures of the current entry; repeat modes stop after a whole queue's worth
    /// rather than looping over one broken file.
    failures: FailureRun,
    /// Device and rate the "cannot carry PCM→DSD" warning was last shown for, so it is said
    /// once, not on every track.
    sdm_carrier_warned: Option<(DeviceId, SdmRate)>,
    /// Device formats per (device, channels), queried once while the device is free.
    device_formats: DeviceFormats,
}

impl Engine {
    fn new(
        backend: Box<dyn OutputBackend>,
        mut config: PlayerConfig,
        commands: Receiver<Command>,
        events: Sender<PlayerEvent>,
    ) -> Self {
        // The settings contract is enforced here too, not only in `SetCrossfade`.
        config.crossfade = config.crossfade.sanitized();
        let devices = backend.device_ids().unwrap_or_default();
        let (resolver, warnings) =
            FilterResolver::new(&config.src, sonder_dsp::sonder_design::default_cache_dir());
        let engine = Self {
            backend,
            config,
            commands,
            events,
            queue: Queue::default(),
            state: PlaybackState::Stopped,
            repeat: RepeatMode::default(),
            chain: ChainState::default(),
            source: None,
            decoding: None,
            feed: VecDeque::new(),
            feed_samples: 0,
            out: None,
            ending: false,
            end_index: None,
            hold: None,
            stopped_at: Pos::ZERO,
            target: None,
            after_end: None,
            reported_track: None,
            reported_path: None,
            last_position: None,
            underruns_base: 0,
            reported_underruns: 0,
            devices,
            last_poll: Instant::now(),
            last_retry: Instant::now(),
            dsd_reprobe: None,
            resolver,
            fallback: HashMap::new(),
            volume_db: None,
            convolution_warned: None,
            crossfeed_warned: None,
            clipping_reported: false,
            dither_clipped: Arc::new(AtomicU64::new(0)),
            src_retry: None,
            sdm_warned: false,
            crossfade: None,
            fade_checked: false,
            fade_plan: None,
            pending_transition: None,
            failures: FailureRun::default(),
            sdm_carrier_warned: None,
            device_formats: HashMap::new(),
        };
        for warning in warnings {
            engine.emit(PlayerEvent::Warning(warning));
        }
        engine.report_convolution_error();
        engine
    }

    /// Says why a configured impulse response cannot be used; playback goes on without it.
    fn report_convolution_error(&self) {
        if let Some(Err(error)) = &self.config.dsp.convolution {
            self.emit(PlayerEvent::Warning(format!("convolution is off: {error}")));
        }
    }

    /// The response the convolution stage runs, when one is loaded and fits the track.
    fn convolution(&self) -> Option<&Arc<ImpulseResponse>> {
        match &self.config.dsp.convolution {
            Some(Ok(ir)) if !self.chain.bitstream && ir.fits(self.chain.channels) => Some(ir),
            _ => None,
        }
    }

    /// Whether convolution runs before the converter: when the converter feeds the DSD
    /// modulator (or any target above [`MAX_CONVOLUTION_RATE`]), the response runs at the
    /// source rate instead of being stretched to millions of taps.
    fn convolution_before_src(&self) -> bool {
        self.chain
            .plan
            .as_ref()
            .is_some_and(|plan| plan.target_rate > MAX_CONVOLUTION_RATE)
    }

    /// The crossfeed preset, when crossfeed is on and the track is stereo.
    fn crossfeed(&self) -> Option<CrossfeedPreset> {
        self.config
            .dsp
            .crossfeed
            .filter(|_| !self.chain.bitstream && self.chain.channels == 2)
    }

    /// Warns once per response and channel count when a configured stage cannot run on the
    /// planned track.
    fn report_unfit_stages(&mut self) {
        if self.chain.bitstream {
            return;
        }
        if let Some(Ok(ir)) = &self.config.dsp.convolution
            && !ir.fits(self.chain.channels)
        {
            let key = (Arc::as_ptr(ir) as usize, self.chain.channels);
            if self.convolution_warned != Some(key) {
                self.convolution_warned = Some(key);
                self.emit(PlayerEvent::Warning(format!(
                    "convolution skipped: {} has {} channels, the track has {}",
                    ir.name(),
                    ir.channel_count(),
                    self.chain.channels
                )));
            }
        }
        if self.config.dsp.crossfeed.is_some()
            && self.chain.channels != 2
            && self.crossfeed_warned != Some(self.chain.channels)
        {
            self.crossfeed_warned = Some(self.chain.channels);
            self.emit(PlayerEvent::Warning(format!(
                "crossfeed skipped: it needs a stereo track, this one has {} channels",
                self.chain.channels
            )));
        }
    }

    fn run(mut self) {
        let mut busy = false;
        loop {
            let first = if busy {
                self.commands.try_recv().map_err(|e| match e {
                    crossbeam_channel::TryRecvError::Empty => RecvTimeoutError::Timeout,
                    crossbeam_channel::TryRecvError::Disconnected => RecvTimeoutError::Disconnected,
                })
            } else {
                self.commands.recv_timeout(IDLE_WAIT)
            };
            match first {
                Ok(command) => {
                    if !self.handle(command) {
                        break;
                    }
                    // Apply everything already queued before doing audio work.
                    while let Ok(command) = self.commands.try_recv() {
                        if !self.handle(command) {
                            return self.shutdown();
                        }
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }

            self.check_output();
            busy = self.pump();
            busy |= self.produce();
            self.report();
            self.poll_devices();
        }
        self.shutdown();
    }

    fn shutdown(&mut self) {
        self.close_output();
    }

    fn emit(&self, event: PlayerEvent) {
        let _ = self.events.send(event);
    }

    // ----- commands -----

    /// Returns `false` on shutdown.
    fn handle(&mut self, command: Command) -> bool {
        match command {
            Command::Play => self.play(),
            Command::Pause => {
                if self.state == PlaybackState::Playing {
                    self.set_state(PlaybackState::Paused);
                }
            }
            Command::Stop => self.stop(),
            Command::Seek(to) => self.seek(Pos::from_duration(to)),
            Command::Next => self.next(),
            Command::Previous => {
                if let Some((index, pos)) = self.current_pos() {
                    if pos.duration() > RESTART_THRESHOLD || index == 0 {
                        self.jump(index, Pos::ZERO);
                    } else {
                        self.jump(index - 1, Pos::ZERO);
                    }
                }
            }
            Command::PlayIndex(index) => {
                if index < self.queue.len() {
                    self.jump(index, Pos::ZERO);
                    self.play();
                }
            }
            Command::Enqueue(paths) => self.enqueue(paths),
            Command::Insert(index, paths) => self.insert(index, paths),
            Command::Remove(index) => self.remove(index),
            Command::Clear => {
                self.stop();
                self.queue.clear();
                self.reported_track = None;
                self.after_end = None;
            }
            Command::SetVolume(db) => self.set_volume(db),
            Command::SetDsp(dsp) => self.set_dsp(dsp),
            Command::SetSrc(src) => self.set_src(src),
            Command::SetDevice(device) => self.set_device(device),
            Command::SetDsd {
                default_mode,
                modes,
                pcm_rate,
            } => self.set_dsd(default_mode, modes, pcm_rate),
            Command::SetSdm(sdm) => self.set_sdm(sdm),
            Command::SetRepeat(mode) => self.repeat = mode,
            Command::SetCrossfade(crossfade) => {
                self.config.crossfade = crossfade.sanitized();
            }
            Command::Shutdown => return false,
        }
        true
    }

    fn set_state(&mut self, state: PlaybackState) {
        self.state = state;
        if let Some(out) = &self.out {
            out.handle.set_paused(state != PlaybackState::Playing);
        }
        if state == PlaybackState::Playing {
            self.last_position = None;
        }
        self.emit(PlayerEvent::State(state));
    }

    fn play(&mut self) {
        match self.state {
            PlaybackState::Playing => {}
            PlaybackState::Paused => {
                self.set_state(PlaybackState::Playing);
                if let Some(hold) = self.hold.take() {
                    self.restart(hold.index, hold.pos);
                }
            }
            PlaybackState::Stopped => {
                let Some(index) = self.start_index() else {
                    return;
                };
                let pos = std::mem::replace(&mut self.stopped_at, Pos::ZERO);
                self.after_end = None;
                self.set_state(PlaybackState::Playing);
                self.restart(index, pos);
            }
        }
    }

    fn stop(&mut self) {
        let index = self.current_index();
        self.close_output();
        self.reset_decoding();
        self.hold = None;
        self.target = None;
        // Playing again from a stop announces the track afresh.
        self.reported_track = None;
        self.queue.set_current(index);
        self.stopped_at = Pos::ZERO;
        if self.state != PlaybackState::Stopped {
            self.set_state(PlaybackState::Stopped);
        }
        if let Some(index) = index {
            self.emit(PlayerEvent::Position {
                index,
                elapsed: Duration::ZERO,
            });
        }
    }

    fn seek(&mut self, pos: Pos) {
        if let Some(index) = self.current_index() {
            self.jump(index, pos);
            self.emit(PlayerEvent::Position {
                index,
                elapsed: pos.duration(),
            });
        }
    }

    /// Moves to `pos` in track `index` in whatever state the player is in.
    fn jump(&mut self, index: usize, pos: Pos) {
        if self.state == PlaybackState::Stopped {
            self.queue.set_current(Some(index));
            self.after_end = None;
            self.stopped_at = pos;
        } else if let Some(hold) = &mut self.hold {
            hold.index = index;
            hold.pos = pos;
        } else {
            self.restart(index, pos);
        }
    }

    fn enqueue(&mut self, paths: Vec<PathBuf>) {
        self.queue.extend(paths);
        self.continue_after_end();
    }

    /// When the end of the queue was already decoded and entries now follow it, continues
    /// decoding into the same stream (gapless) if it has not been told to drain yet.
    fn continue_after_end(&mut self) {
        if let Some(next) = self.end_index.filter(|&i| i < self.queue.len()) {
            self.end_index = None;
            if matches!(self.feed.back(), Some(FeedItem::End)) {
                self.feed.pop_back();
            }
            self.ending = false;
            self.open_track(next, Pos::ZERO, true);
        }
    }

    fn insert(&mut self, index: usize, paths: Vec<PathBuf>) {
        let n = paths.len();
        if n == 0 {
            return;
        }
        self.rebuild_after_edit();
        // A planned (not yet started) fade belongs to the old order; plan again.
        self.fade_plan = None;
        self.fade_checked = false;
        let index = index.min(self.queue.len());
        let at = self.current_pos();
        let reach = self.pipeline_reach();
        self.queue.insert(index, paths);
        // `end_index` and `after_end` name the slot after the old end: entries inserted
        // exactly there are the ones that come next, so they do not move.
        self.remap(
            |i| Some(if i >= index { i + n } else { i }),
            |i| if i > index { i + n } else { i },
        );
        if let Some((cur, pos)) = at
            && self.state != PlaybackState::Stopped
            && self.hold.is_none()
            && index > cur
            && reach.is_some_and(|r| index <= r)
        {
            // Audio after the playing entry is already decoded from the old order.
            self.restart(cur, pos);
        } else {
            self.continue_after_end();
        }
        self.report_current();
    }

    /// A crossfade already decoded audio of the incoming entry, which a queue edit may move
    /// or remove. Rebuild the current track from where it is heard instead of remapping it.
    fn rebuild_after_edit(&mut self) {
        if self.crossfade.is_none() {
            return;
        }
        match self.current_pos() {
            Some((index, pos)) if self.state != PlaybackState::Stopped && self.hold.is_none() => {
                self.restart(index, pos);
            }
            _ => self.crossfade = None,
        }
    }

    /// Re-announces the current entry after the queue changed under it: its index may have
    /// moved, and while paused no position report would follow on its own.
    fn report_current(&self) {
        if self.queue.current().is_none() {
            return;
        }
        if let Some((index, pos)) = self.current_pos() {
            self.emit(PlayerEvent::Position {
                index,
                elapsed: pos.duration(),
            });
        }
    }

    fn remove(&mut self, index: usize) {
        if index >= self.queue.len() {
            return;
        }
        self.rebuild_after_edit();
        self.fade_plan = None;
        self.fade_checked = false;
        let at = self.current_pos();
        let reach = self.pipeline_reach();
        self.queue.remove(index);
        self.remap(
            |i| match i.cmp(&index) {
                std::cmp::Ordering::Less => Some(i),
                std::cmp::Ordering::Equal => None,
                std::cmp::Ordering::Greater => Some(i - 1),
            },
            |i| if i > index { i - 1 } else { i },
        );
        let Some((cur, pos)) = at else {
            return;
        };
        if cur == index {
            // The entry the player is on is gone: carry on with the one that took its place.
            if index >= self.queue.len() {
                if self.state == PlaybackState::Stopped {
                    self.queue.set_current(None);
                } else {
                    self.close_output();
                    self.reset_decoding();
                    self.hold = None;
                    self.finish_queue();
                }
            } else if self.state == PlaybackState::Stopped {
                self.queue.set_current(Some(index));
                self.stopped_at = Pos::ZERO;
            } else if let Some(hold) = &mut self.hold {
                hold.index = index;
                hold.pos = Pos::ZERO;
            } else {
                self.restart(index, Pos::ZERO);
            }
        } else if self.state != PlaybackState::Stopped
            && self.hold.is_none()
            && index > cur
            && reach.is_some_and(|r| index <= r)
        {
            // The removed entry is already decoded ahead: rebuild from where we are.
            self.restart(cur, pos);
        }
        self.report_current();
    }

    /// The highest queue index whose audio is decoding, waiting in the feed, or in the ring.
    fn pipeline_reach(&self) -> Option<usize> {
        let feed = self.feed.iter().filter_map(|item| match item {
            FeedItem::TrackStart { index, .. } => Some(*index),
            _ => None,
        });
        let segments = self
            .out
            .iter()
            .flat_map(|o| o.segments.iter().map(|s| s.index));
        self.decoding
            .as_ref()
            .map(|d| d.index)
            .into_iter()
            .chain(feed)
            .chain(segments)
            .chain(self.crossfade.as_ref().map(|fade| fade.index))
            .max()
    }

    /// Rewrites every stored queue index after an insert or a removal. `map` gives an entry's
    /// new index, `None` when it was removed; audio of a removed entry that is decoded ahead
    /// or queued in the ring loses its bookkeeping (the caller restarts in that case), and a
    /// hold or restart target on it is left for the caller to move. `slot` remaps the indices
    /// that name a position after the last entry (`end_index`, `after_end`).
    fn remap(&mut self, map: impl Fn(usize) -> Option<usize>, slot: impl Fn(usize) -> usize) {
        let current = self.queue.current().and_then(&map);
        self.queue.set_current(current);
        self.reported_track = self.reported_track.and_then(&map);
        if let Some(hold) = &mut self.hold
            && let Some(i) = map(hold.index)
        {
            hold.index = i;
        }
        if let Some((index, _)) = &mut self.target
            && let Some(i) = map(*index)
        {
            *index = i;
        }
        self.end_index = self.end_index.map(&slot);
        self.after_end = self.after_end.map(&slot);
        if let Some(decoding) = &mut self.decoding {
            match map(decoding.index) {
                Some(i) => decoding.index = i,
                None => self.decoding = None,
            }
        }
        for item in &mut self.feed {
            if let FeedItem::TrackStart { index, .. } = item
                && let Some(i) = map(*index)
            {
                *index = i;
            }
        }
        if let Some(out) = &mut self.out {
            out.segments.retain_mut(|s| match map(s.index) {
                Some(i) => {
                    s.index = i;
                    true
                }
                None => false,
            });
        }
    }

    fn set_volume(&mut self, db: Option<f64>) {
        if self.volume_db == db {
            return;
        }
        self.volume_db = db;
        self.refresh_dither(self.chain.channels, false);
        self.sync_stages();
        self.restart_if_playing();
    }

    fn set_dsp(&mut self, dsp: DspConfig) {
        let dsp = dsp.sanitized();
        if self.config.dsp == dsp {
            return;
        }
        let new_error = matches!(&dsp.convolution, Some(Err(_)))
            && dsp.convolution != self.config.dsp.convolution;
        self.config.dsp = dsp;
        if new_error {
            self.report_convolution_error();
        }
        self.refresh_dither(self.chain.channels, false);
        self.sync_stages();
        self.restart_if_playing();
    }

    /// Replaces the rate-conversion policy and rebuilds the resolver. The next plan is built
    /// from scratch: a different filter source, phase or target can change every stage.
    fn set_src(&mut self, src: SrcConfig) {
        if self.config.src == src {
            return;
        }
        self.config.src = src;
        let (resolver, warnings) = FilterResolver::new(
            &self.config.src,
            sonder_dsp::sonder_design::default_cache_dir(),
        );
        self.resolver = resolver;
        for warning in warnings {
            self.emit(PlayerEvent::Warning(warning));
        }
        self.chain.plan = None;
        self.chain.plan_key = None;
        self.fallback.clear();
        self.src_retry = None;
        self.refresh_dither(self.chain.channels, false);
        self.sync_stages();
        self.restart_if_playing();
    }

    /// Restarts the current track from its position so a stage change is heard after one
    /// ring, not after everything already decoded.
    fn restart_if_playing(&mut self) {
        if self.state != PlaybackState::Stopped
            && self.hold.is_none()
            && let Some((index, pos)) = self.current_pos()
        {
            self.restart(index, pos);
        }
    }

    /// The full stage list in Roon's order: headroom, SRC, convolution, PEQ, crossfeed,
    /// levelling, volume, dither. Convolution moves ahead of SRC when SRC feeds the DSD
    /// modulator (see [`Self::convolution_before_src`]). Empty while DoP or native DSD plays:
    /// those send the source bits untouched.
    fn compose_stages(&self) -> Vec<Box<dyn Stage>> {
        let mut stages: Vec<Box<dyn Stage>> = Vec::new();
        if self.chain.bitstream {
            return stages;
        }
        let mut convolver = self
            .convolution()
            .map(|ir| Box::new(Convolver::new(ir.clone())) as Box<dyn Stage>);
        if self.convolution_before_src() {
            stages.extend(convolver.take());
        }
        if let Some(plan) = &self.chain.plan {
            stages.extend(plan.stages());
        }
        stages.extend(convolver);
        if let Some(peq) = self.config.dsp.peq.clone().filter(Peq::active) {
            stages.push(Box::new(PeqStage::new(peq)));
        }
        if let Some(preset) = self.crossfeed() {
            stages.push(Box::new(Crossfeed::new(preset)));
        }
        if self.config.dsp.leveling.is_on() {
            stages.push(Box::new(LevelingStage::new(self.chain.leveling.clone())));
        }
        if let Some(db) = self.volume_db {
            stages.push(Box::new(Volume::from_db(db)));
        }
        if let Some(bits) = self.chain.dither_bits {
            stages.push(Box::new(
                Dither::new(bits).with_clip_counter(self.dither_clipped.clone()),
            ));
        }
        stages
    }

    /// What the worker's chain is built from; a change means it has to be rebuilt.
    fn stage_key(&self) -> StageKey {
        StageKey {
            plan: self
                .chain
                .plan
                .as_ref()
                .map(|p| (p.source_rate, p.target_rate, p.channels)),
            convolution: self
                .convolution()
                .map(|ir| (Arc::as_ptr(ir) as usize, self.convolution_before_src())),
            peq: self.config.dsp.peq.clone().filter(Peq::active),
            crossfeed: self.crossfeed(),
            leveling: self
                .config
                .dsp
                .leveling
                .is_on()
                .then_some(self.config.dsp.leveling),
            volume: self.volume_db.map(f64::to_bits),
            dither: self.chain.dither_bits,
            bitstream: self.chain.bitstream,
        }
    }

    /// Rebuilds the worker's chain when its contents changed. The old chain's history is
    /// dropped; callers that need continuity flush its tail first.
    fn sync_stages(&mut self) {
        self.report_unfit_stages();
        let key = self.stage_key();
        if self.chain.stages_key.as_ref() == Some(&key) {
            return;
        }
        self.chain.worker.set_stages(self.compose_stages());
        self.chain.stages_key = Some(key);
    }

    /// Rebuilds the stage list when the rate-conversion plan changes. `continuous` keeps the
    /// old chain's tail in the stream before the switch.
    fn ensure_plan(
        &mut self,
        rate: u32,
        bits: u32,
        channels: u16,
        continuous: bool,
        tail: &mut Vec<i32>,
        converted: bool,
    ) {
        // A PCM→DSD track always goes to the modulator's input rate; the explicit SRC
        // policy and the fallback table only choose rates for PCM output.
        let target = match self.chain.sdm_rate {
            Some(rate_sdm) => Some(rate_sdm.input_rate()).filter(|&target| target != rate),
            None => self.target_for(rate, bits, channels),
        };
        let key = (rate, channels, target);
        let plan_changed = self.chain.plan_key != Some(key);
        self.chain.channels = channels;
        self.chain.chain_rate = rate;
        if plan_changed {
            let plan = target.and_then(|target| {
                let plan = self.resolver.plan(rate, target, channels, &self.config.src);
                if plan.is_none() {
                    self.emit(PlayerEvent::Warning(format!(
                        "no filter can convert {rate} Hz to {target} Hz; keeping the source rate"
                    )));
                }
                plan
            });
            if continuous {
                self.chain.worker.flush_tail(tail);
            }
            self.chain.plan = plan;
            self.chain.plan_key = Some(key);
        }
        let dither_changed = self.refresh_dither(channels, converted);
        if !plan_changed && dither_changed && continuous {
            self.chain.worker.flush_tail(tail);
        }
        self.sync_stages();
    }

    /// Removes the rate-conversion plan from the worker before a DoP or native DSD track,
    /// whose chain is empty. `continuous` first drains what the old chain holds: the
    /// converter's history, and convolution's held frames and tail even without a plan.
    fn drop_plan(&mut self, continuous: bool, tail: &mut Vec<i32>) {
        self.chain.plan_key = None;
        if continuous {
            self.chain.worker.flush_tail(tail);
        }
        self.chain.plan = None;
        self.sync_stages();
    }

    /// Recomputes the dither target for this channel count; `true` when it changed.
    ///
    /// Dither only applies when the device's widest container is narrower than the processed
    /// signal (24 bits) and the chain actually shapes the signal: a bit-perfect path must not
    /// gain a dither stage just because the device is 16-bit. `converted` marks a signal the
    /// worker derives (DSD→PCM), which counts as shaped. A device that cannot be probed keeps
    /// the signal undithered because its width is unknown.
    fn refresh_dither(&mut self, channels: u16, converted: bool) -> bool {
        if self.chain.sdm_rate.is_some() {
            // The modulator's own noise shaping replaces dither; quantising to a PCM word
            // first would only add noise.
            let changed = self.chain.dither_bits.is_some();
            self.chain.dither_bits = None;
            return changed;
        }
        let shaping = converted
            || self.chain.plan.is_some()
            || self.convolution().is_some()
            || self.crossfeed().is_some()
            || self.config.dsp.peq_active()
            || self.config.dsp.leveling.is_on()
            || self.volume_db.is_some();
        if !self.config.dsp.dither || !shaping {
            let changed = self.chain.dither_bits.is_some();
            self.chain.dither_bits = None;
            return changed;
        }
        // A probe that fails while an exclusive stream is open leaves the previous decision
        // (and the signal) alone; rewriting it would drop dither mid-stream.
        let Some(width) = self.max_container_bits(channels) else {
            return false;
        };
        let bits = (width < crate::worker::DSP_OUTPUT_BITS).then_some(width);
        let changed = self.chain.dither_bits != bits;
        self.chain.dither_bits = bits;
        changed
    }

    /// Valid bits of the widest container the device accepts for `channels`.
    fn max_container_bits(&mut self, channels: u16) -> Option<u32> {
        self.formats_for(channels)?
            .iter()
            .map(|format| format.container.valid_bits())
            .max()
    }

    /// A rate the device accepts for this source, explicitly chosen or learned from a
    /// refusal. `None` keeps the bit-perfect path.
    fn target_for(&mut self, rate: u32, bits: u32, channels: u16) -> Option<u32> {
        if let Some(&target) = self.fallback.get(&(rate, channels)) {
            return (target != rate).then_some(target);
        }
        let supported = self.supported_rates(bits, channels);
        rate::choose_target(&self.config.src, rate, &supported).filter(|&target| target != rate)
    }

    /// Rates the device accepts with a container that holds `bits`, probed once while the
    /// device is free and then cached per channel count. Empty when the probe fails.
    fn supported_rates(&mut self, bits: u32, channels: u16) -> Vec<u32> {
        self.probed_rates(bits, channels).unwrap_or_default()
    }

    /// Like [`supported_rates`](Self::supported_rates), but `None` when the device could not
    /// be asked (an exclusive stream holds it), so "unknown" is not mistaken for "none".
    fn probed_rates(&mut self, bits: u32, channels: u16) -> Option<Vec<u32>> {
        let mut rates: Vec<u32> = self
            .formats_for(channels)?
            .iter()
            .filter(|format| format.container.holds(bits))
            .map(|format| format.rate)
            .collect();
        rates.sort_unstable();
        rates.dedup();
        Some(rates)
    }

    /// Device formats for `channels`, queried once while the device is free. `None` when the
    /// device could not be asked (an exclusive stream holds it).
    fn formats_for(&mut self, channels: u16) -> Option<Vec<DeviceFormat>> {
        let key = (self.config.device.0.clone(), channels);
        if let Some(formats) = self.device_formats.get(&key) {
            return Some(formats.clone());
        }
        let formats = self
            .backend
            .supported_formats(&self.config.device, channels)
            .ok()?;
        self.device_formats.insert(key, formats.clone());
        Some(formats)
    }

    /// Publishes the levelling gain for `index` before its first block is decoded.
    ///
    /// `Auto` uses album values while the queue stays inside one album, so an album played
    /// in order keeps its relative track loudness.
    fn update_leveling(&self, index: usize, path: &Path) {
        if !self.config.dsp.leveling.is_on() {
            self.chain.leveling.set_db(0.0);
            return;
        }
        let Some(values) = self.loudness_gain(path) else {
            self.chain.leveling.set_db(0.0);
            return;
        };
        let gain = match self.config.dsp.leveling {
            LevelingMode::Off => Gain::default(),
            LevelingMode::Track => values.track,
            LevelingMode::Album => values.album,
            LevelingMode::Auto => {
                if values
                    .album_id
                    .is_some_and(|album| self.in_album_run(index, album))
                {
                    values.album
                } else {
                    values.track
                }
            }
        };
        self.chain.leveling.set_gain(gain);
    }

    fn loudness_gain(&self, path: &Path) -> Option<TrackGain> {
        self.config
            .loudness
            .gain(path, self.config.dsp.leveling_target_lufs)
    }

    /// Whether a neighbour of queue entry `index` belongs to `album`, i.e. this entry is part
    /// of a contiguous run of the album's tracks.
    fn in_album_run(&self, index: usize, album: i64) -> bool {
        let same_album = |i: usize| {
            self.queue
                .get(i)
                .and_then(|path| self.loudness_gain(path))
                .and_then(|gain| gain.album_id)
                == Some(album)
        };
        index.checked_sub(1).is_some_and(same_album) || same_album(index + 1)
    }

    /// Whether two queue entries are consecutive tracks of one album. Entries without
    /// library data have no album identity, so they count as different albums.
    fn same_album(&self, left: usize, right: usize) -> bool {
        let album = |i: usize| {
            self.queue
                .get(i)
                .and_then(|path| self.loudness_gain(path))
                .and_then(|gain| gain.album_id)
        };
        match (album(left), album(right)) {
            (Some(a), Some(b)) => a == b,
            _ => false,
        }
    }

    /// What happens to the signal for the announced track, including how it joined the
    /// previous one (`transition`: the crossfade length, or why it stayed gapless).
    fn signal_path(
        &self,
        source: SourceFormat,
        spec: OutputSpec,
        transition: Option<TransitionNote>,
    ) -> SignalPath {
        let mut stages = Vec::new();
        // A plan only exists for a PCM source, and every PCM stage runs before either a PCM
        // container or the modulator. DoP and native DSD from a DSD source run no stages
        // (see `compose_stages`), so only stages that actually shape the signal are listed.
        let shaped = !self.chain.bitstream;
        let before_src = self.convolution_before_src();
        let mut convolution = self.convolution().map(|ir| {
            let rate = match &self.chain.plan {
                Some(plan) if !before_src => plan.target_rate,
                _ => self.chain.chain_rate,
            };
            let taps = (ir.frames() as u64 * u64::from(rate)).div_ceil(u64::from(ir.rate()));
            let resampled = if rate == ir.rate() {
                String::new()
            } else {
                format!(", resampled from {} Hz", ir.rate())
            };
            SignalStage {
                name: "Convolution".into(),
                detail: format!("{}: {taps} taps at {rate} Hz{resampled}", ir.name()),
            }
        });
        if before_src {
            stages.extend(convolution.take());
        }
        if let Some(plan) = &self.chain.plan {
            stages.extend(plan.describe());
        }
        stages.extend(convolution);
        if let Some(peq) = self
            .config
            .dsp
            .peq
            .as_ref()
            .filter(|p| shaped && p.active())
        {
            let bands = peq.bands.iter().filter(|b| b.enabled).count();
            stages.push(SignalStage {
                name: "PEQ".into(),
                detail: format!("{bands} bands, preamp {:+.1} dB", peq.preamp_db),
            });
        }
        if let Some(preset) = self.crossfeed() {
            stages.push(SignalStage {
                name: "Crossfeed".into(),
                detail: format!(
                    "{} ({:.0} Hz, {:.1} dB)",
                    preset.label(),
                    preset.cutoff_hz(),
                    preset.feed_db()
                ),
            });
        }
        if shaped && self.config.dsp.leveling.is_on() {
            stages.push(SignalStage {
                name: "Levelling".into(),
                detail: format!(
                    "{} {:+.1} dB to {:.0} LUFS",
                    match self.config.dsp.leveling {
                        LevelingMode::Track => "track",
                        LevelingMode::Album => "album",
                        LevelingMode::Auto => "album/track",
                        LevelingMode::Off => "off",
                    },
                    self.chain.leveling.db(),
                    self.config.dsp.leveling_target_lufs
                ),
            });
        }
        if let Some(db) = self.volume_db.filter(|_| shaped) {
            stages.push(SignalStage {
                name: "Volume".into(),
                detail: format!("{db:+.1} dB"),
            });
        }
        if let Some(bits) = self
            .chain
            .dither_bits
            .filter(|_| spec.mode == OutputMode::Pcm)
        {
            stages.push(SignalStage {
                name: "Dither".into(),
                detail: format!("TPDF to {bits}-bit"),
            });
        }
        if let Some(rate) = self.chain.sdm_rate.filter(|_| spec.mode != OutputMode::Pcm) {
            stages.push(SignalStage {
                name: "DSD modulator".into(),
                detail: format!(
                    "{}: {} order, {} → {} bit samples",
                    rate.as_str(),
                    match self.config.sdm.order {
                        SdmOrder::Order5 => "5th",
                        SdmOrder::Order7 => "7th",
                    },
                    rate.input_rate(),
                    rate.bit_rate()
                ),
            });
            stages.push(SignalStage {
                name: if spec.mode == OutputMode::Dop {
                    "DoP"
                } else {
                    "Native DSD"
                }
                .into(),
                detail: format!("carrier {} Hz", spec.rate),
            });
        }
        // A transition that faded this track's head touched the samples: the path names the
        // mixer, so it stops claiming bit-perfection. A skipped crossfade is only a note.
        if let Some(TransitionNote::Faded(seconds)) = transition {
            stages.push(SignalStage {
                name: "Crossfade".into(),
                detail: format!("{seconds:.1} s, equal power"),
            });
        }
        let transition = transition.map(|note| match note {
            TransitionNote::Faded(seconds) => format!("crossfade {seconds:.1} s, equal power"),
            TransitionNote::Skipped(reason) => format!("crossfade skipped: {reason}"),
        });
        let bit_perfect = stages.is_empty()
            && (spec.mode != OutputMode::Pcm
                || (source.bits != DSD_SOURCE_BITS && spec.rate == source.rate));
        SignalPath {
            source,
            output_rate: spec.rate,
            output_bits: spec.bits,
            output_channels: spec.channels,
            mode: spec.mode,
            stages,
            bit_perfect,
            transition,
        }
    }

    fn set_dsd(
        &mut self,
        default_mode: DsdMode,
        modes: HashMap<DeviceId, DsdMode>,
        pcm_rate: DsdPcmRate,
    ) {
        let config = &mut self.config;
        if config.default_dsd_mode == default_mode
            && config.dsd_modes == modes
            && config.dsd_pcm_rate == pcm_rate
        {
            return;
        }
        let mode_before = config.dsd_mode_for(&config.device);
        let rate_changed = config.dsd_pcm_rate != pcm_rate;
        config.default_dsd_mode = default_mode;
        config.dsd_modes = modes;
        config.dsd_pcm_rate = pcm_rate;
        // Re-decode a playing DSD track from its position, like a volume change, but only
        // when the change reaches it: its device's mode, or the rate while it is converted.
        let mode_changed = config.dsd_mode_for(&config.device) != mode_before;
        let converting = self.out.as_ref().is_some_and(|o| o.mode == OutputMode::Pcm);
        if (mode_changed || (rate_changed && converting))
            && self.state != PlaybackState::Stopped
            && self.hold.is_none()
            && let Some(now) = self.now_playing()
            && now.segment.source.bits == DSD_SOURCE_BITS
        {
            let at = (
                now.segment.index,
                Pos::frames(now.frame, now.segment.source.rate),
            );
            self.restart(at.0, at.1);
        }
    }

    fn set_sdm(&mut self, sdm: SdmConfig) {
        if self.config.sdm == sdm {
            return;
        }
        let was_on = self.config.sdm.is_on();
        self.config.sdm = sdm;
        // A stopped player picks the mode up with the next track. A playing PCM track has to
        // re-decode so the new output mode is reached after one ring, not the whole feed.
        if sdm.is_on() != was_on && !sdm.is_on() {
            self.chain.sdm_rate = None;
            self.sync_stages();
        }
        self.restart_if_playing();
    }

    fn set_device(&mut self, device: DeviceId) {
        if device == self.config.device {
            return;
        }
        self.config.device = device;
        self.fallback.clear();
        self.src_retry = None;
        if self.state == PlaybackState::Stopped {
            return;
        }
        // Release the old device before the new one opens, and read the position only after
        // its render thread has exited so no frame is lost or played twice.
        let at = match self.out.take() {
            Some(out) => {
                let Output {
                    stream,
                    handle,
                    pushed,
                    segments,
                    format,
                    ..
                } = out;
                let _ = stream.stop();
                self.underruns_base += handle.underruns();
                now_in(&handle, pushed, &segments, format)
                    .map(|now| {
                        (
                            now.segment.index,
                            Pos::frames(now.frame, now.segment.source.rate),
                        )
                    })
                    .or_else(|| self.current_pos())
            }
            None => self.current_pos(),
        };
        self.reset_decoding();
        // A hold (device lost or busy) keeps its position and retries on the new device the
        // usual way.
        if self.hold.is_none()
            && let Some((index, pos)) = at
        {
            self.restart(index, pos);
        }
    }

    // ----- position bookkeeping -----

    fn now_playing(&self) -> Option<Now> {
        let out = self.out.as_ref()?;
        now_in(&out.handle, out.pushed, &out.segments, out.format)
    }

    fn current_pos(&self) -> Option<(usize, Pos)> {
        if let Some(now) = self.now_playing() {
            return Some((
                now.segment.index,
                Pos::frames(now.frame, now.segment.source.rate),
            ));
        }
        if let Some(hold) = self.hold {
            return Some((hold.index, hold.pos));
        }
        if let Some(target) = self.target {
            return Some(target);
        }
        if let Some(index) = self.reported_track.or(self.queue.current()) {
            let pos = if self.state == PlaybackState::Stopped {
                self.stopped_at
            } else {
                Pos::ZERO
            };
            return Some((index, pos));
        }
        self.start_index().map(|i| (i, self.stopped_at))
    }

    /// Where play starts from a stop: the first track added after the queue ended, else the
    /// current track, else the first.
    fn start_index(&self) -> Option<usize> {
        self.after_end
            .filter(|&i| i < self.queue.len())
            .or_else(|| self.queue.start_index())
    }

    fn current_index(&self) -> Option<usize> {
        self.current_pos().map(|(index, _)| index)
    }

    // ----- decoding -----

    fn take_chain(&mut self) -> ChainState {
        std::mem::take(&mut self.chain)
    }

    fn install_chain(&mut self, chain: ChainState) {
        self.chain = chain;
    }

    fn reset_decoding(&mut self) {
        self.crossfade = None;
        self.fade_checked = false;
        self.fade_plan = None;
        // A skip note names a transition; after a seek or stop there is none pending.
        self.pending_transition = None;
        self.feed.clear();
        self.feed_samples = 0;
        self.decoding = None;
        self.chain.worker.reset();
        self.ending = false;
        self.end_index = None;
        self.failures.clear();
    }

    /// Drops everything decoded ahead, empties the ring (or reopens), and starts decoding
    /// `index` at `pos`.
    fn restart(&mut self, index: usize, pos: Pos) {
        self.reset_decoding();
        self.hold = None;
        if let Some(out) = &mut self.out {
            let flushed = out
                .producer
                .as_mut()
                .is_some_and(|p| p.flush(FLUSH_TIMEOUT));
            if flushed {
                out.segments.clear();
            } else {
                self.close_output();
            }
        }
        // `reported_track` stays: a seek within the playing track is not a track change.
        self.queue.set_current(Some(index));
        self.target = Some((index, pos));
        self.open_track(index, pos, false);
    }

    /// Picks the carrier for a PCM track that should be modulated to DSD.
    ///
    /// `None` means "stay PCM": the mode is off, or the device takes neither DoP nor native
    /// DSD at that rate. An open stream that already carries this rate is reused without asking
    /// the device again (gapless tracks). A probe that fails because our own stream holds the
    /// device assumes the requested carrier; [`open_output`](Self::open_output) re-decides once
    /// the device is free, exactly like [`choose_dsd_mode`](Self::choose_dsd_mode).
    fn choose_sdm(&self, source: SourceFormat) -> Option<(SdmRate, SdmPacking)> {
        let rate = self.config.sdm.mode.rate()?;
        let requested = self.config.dsd_mode_for(&self.config.device);
        let backend = self.backend.as_ref();
        let device = &self.config.device;
        let channels = source.channels;
        if let Some(out) = &self.out
            && out.format.channels == channels
        {
            match out.mode {
                OutputMode::NativeDsd
                    if requested == DsdMode::Native && out.format.rate == rate.native_rate() =>
                {
                    return Some((rate, SdmPacking::Native));
                }
                OutputMode::Dop if out.format.rate == rate.dop_rate() => {
                    return Some((rate, SdmPacking::Dop));
                }
                _ => {}
            }
        }
        let native = || {
            probe(negotiate_native(
                backend,
                device,
                rate.native_rate(),
                channels,
            ))
        };
        let dop = || probe(negotiate_dop(backend, device, rate.dop_rate(), channels));
        let packing = if requested == DsdMode::Native {
            match native() {
                Probe::Yes | Probe::Unknown => SdmPacking::Native,
                Probe::No => match dop() {
                    Probe::Yes | Probe::Unknown => SdmPacking::Dop,
                    Probe::No => None?,
                },
            }
        } else {
            match dop() {
                Probe::Yes | Probe::Unknown => SdmPacking::Dop,
                Probe::No => None?,
            }
        };
        Some((rate, packing))
    }

    /// Picks how a DSD track reaches the current device, with a reason when the configured
    /// mode cannot be used.
    ///
    /// DoP and native DSD are bit-perfect: any active PCM stage would corrupt them, so the
    /// engine refuses DoP there and converts to PCM instead (the same fallback used when the
    /// device does not accept DoP).
    ///
    /// The track is chosen while the current stream may still hold the device (gapless
    /// prefetch, seek, volume change). An open DSD stream that already carries this rate is
    /// reused without asking the device again. A probe that fails for any reason other than
    /// an unsupported format (an exclusive device answers "busy") leaves the answer unknown:
    /// the requested mode is assumed and [`open_output`](Self::open_output) re-decides once
    /// the device is free.
    fn choose_dsd_mode(&self, dsd: DsdFormat) -> (DsdMode, Option<String>) {
        let requested = self.config.dsd_mode_for(&self.config.device);
        if requested == DsdMode::Pcm {
            return (DsdMode::Pcm, None);
        }
        // Only stages that apply to every track count here. A rate-conversion plan belongs to
        // the PCM source it was built for and is dropped for DoP and native DSD; EQ and volume
        // shape every signal, so DSD has to become PCM first. Levelling only does so when this
        // track has a gain (`update_leveling` published it before the decoder opened): DSD is
        // never measured and rarely tagged, and a 0 dB stage is no reason to lose native DSD.
        if self.volume_db.is_some()
            || self.config.dsp.peq_active()
            || (self.config.dsp.leveling.is_on() && self.chain.leveling.db() != 0.0)
        {
            return (
                DsdMode::Pcm,
                Some("PCM DSP stages are active; DSD is converted to PCM".into()),
            );
        }
        let dop_reason = || {
            (requested == DsdMode::Native)
                .then(|| "native DSD is not available; using DoP".to_string())
        };
        if let Some(out) = &self.out
            && out.format.channels == dsd.channels
        {
            match out.mode {
                OutputMode::NativeDsd
                    if requested == DsdMode::Native && out.format.rate == dsd.rate / 32 =>
                {
                    return (DsdMode::Native, None);
                }
                OutputMode::Dop if out.format.rate == dsd.rate / 16 => {
                    return (DsdMode::Dop, dop_reason());
                }
                _ => {}
            }
        }

        let backend = self.backend.as_ref();
        let device = &self.config.device;
        let dop = || probe(negotiate_dop(backend, device, dsd.rate / 16, dsd.channels));
        let to_pcm = |what: &str| {
            (
                DsdMode::Pcm,
                Some(format!(
                    "the device accepts {what} at {} Hz; converting DSD to PCM",
                    dsd.rate / 16
                )),
            )
        };
        let native = || {
            probe(negotiate_native(
                backend,
                device,
                dsd.rate / 32,
                dsd.channels,
            ))
        };
        if requested == DsdMode::Native {
            match native() {
                Probe::Yes | Probe::Unknown => (DsdMode::Native, None),
                Probe::No => match dop() {
                    Probe::Yes | Probe::Unknown => (DsdMode::Dop, dop_reason()),
                    Probe::No => to_pcm("neither native DSD nor DoP"),
                },
            }
        } else {
            match dop() {
                Probe::Yes | Probe::Unknown => (DsdMode::Dop, None),
                Probe::No => to_pcm("no DoP"),
            }
        }
    }

    /// Opens `index` (skipping unplayable entries), prepares its chain in the engine's
    /// chain fields and returns the decoder, without announcing the track. `continuous`
    /// keeps the previous chain's tail in `tail` (the caller pushes it into the stream).
    ///
    /// `Err(index)` means the queue ran out at `index`.
    fn begin_chain(
        &mut self,
        mut index: usize,
        mut pos: Pos,
        continuous: bool,
        tail: &mut Vec<i32>,
        report: bool,
    ) -> Result<Opened, usize> {
        loop {
            let Some(path) = self.queue.get(index).map(Path::to_path_buf) else {
                return Err(index);
            };
            self.update_leveling(index, &path);
            match open_decoder(&path, pos) {
                Ok(decoder) => {
                    let source = decoder.source_format();
                    let mut spec = match decoder.format() {
                        TrackFormat::Pcm(_) => {
                            self.chain.bitstream = false;
                            let modulation = self.choose_sdm(source);
                            let carrier_key = self
                                .config
                                .sdm
                                .mode
                                .rate()
                                .map(|rate| (self.config.device.clone(), rate));
                            if modulation.is_none()
                                && carrier_key.is_some()
                                && self.sdm_carrier_warned != carrier_key
                            {
                                self.sdm_carrier_warned = carrier_key;
                                self.emit(PlayerEvent::Warning(format!(
                                    "the device does not carry {} as DoP or native DSD; playing PCM",
                                    self.config.sdm.mode.as_str()
                                )));
                            }
                            self.chain.sdm_rate = modulation.map(|(rate, _)| rate);
                            self.ensure_plan(
                                source.rate,
                                source.bits,
                                source.channels,
                                continuous,
                                tail,
                                false,
                            );
                            if let Some((rate, packing)) = modulation {
                                self.chain.worker.prepare_sdm(
                                    source,
                                    rate,
                                    self.config.sdm.order,
                                    packing,
                                    tail,
                                )
                            } else {
                                let mut spec = self.chain.worker.prepare(source, tail);
                                if !self.chain.worker.pipeline_is_empty()
                                    && spec.bits > 16
                                    && self
                                        .probed_rates(24, source.channels)
                                        .is_some_and(|rates| !rates.contains(&spec.rate))
                                {
                                    // A 16-bit-only device still plays the resampled signal.
                                    // A device that could not be asked keeps the 24-bit request.
                                    spec.bits = source.bits.max(16);
                                }
                                spec
                            }
                        }
                        TrackFormat::Dsd(dsd) => {
                            self.chain.sdm_rate = None;
                            let (mode, reason) = self.choose_dsd_mode(dsd);
                            self.emit(PlayerEvent::DsdMode { mode, reason });
                            self.chain.bitstream = mode != DsdMode::Pcm;
                            if mode == DsdMode::Pcm {
                                let rate = self.config.dsd_pcm_rate.rate();
                                self.ensure_plan(rate, 24, source.channels, continuous, tail, true);
                            } else {
                                // DoP and native DSD never resample: drop the plan a previous
                                // PCM track left, so the pipeline is empty as they require.
                                self.chain.dither_bits = None;
                                self.drop_plan(continuous, tail);
                            }
                            self.chain.worker.prepare_dsd(
                                dsd,
                                mode,
                                self.config.dsd_pcm_rate.rate(),
                                tail,
                            )
                        }
                    };
                    // Dither already quantised the signal to the device's word length, so the
                    // container must be exactly that wide (and is lossless for the result).
                    if let Some(bits) = self
                        .chain
                        .dither_bits
                        .filter(|_| spec.mode == OutputMode::Pcm)
                    {
                        spec.bits = bits;
                    }
                    self.chain.spec = Some(spec);
                    self.source = Some(source);
                    let duration = decoder.total();
                    return Ok(Opened {
                        index,
                        decoder,
                        source,
                        spec,
                        duration,
                    });
                }
                Err(error) => {
                    if report {
                        self.emit(PlayerEvent::TrackFailed {
                            index,
                            path,
                            error: error.to_string(),
                        });
                    }
                    index += 1;
                    pos = Pos::ZERO;
                }
            }
        }
    }

    /// Opens track `index` (skipping unplayable files) and queues its start marker.
    ///
    /// `continuous` is true when the track follows the previous one gaplessly, so stage
    /// tails belong in the stream; after a seek or stop they are discarded.
    fn open_track(&mut self, index: usize, pos: Pos, continuous: bool) {
        self.clipping_reported = false;
        self.sdm_warned = false;
        self.fade_checked = false;
        self.fade_plan = None;
        let mut tail = Vec::new();
        match self.begin_chain(index, pos, continuous, &mut tail, true) {
            Ok(opened) => {
                if continuous {
                    self.push_samples(tail);
                } else {
                    // Unplayable tracks may have been skipped on the way here.
                    self.target = Some((
                        opened.index,
                        Pos::frames(opened.decoder.position(), opened.source.rate),
                    ));
                }
                let note = self.pending_transition.take();
                let path = Arc::new(self.signal_path(opened.source, opened.spec, note));
                self.feed.push_back(FeedItem::TrackStart {
                    index: opened.index,
                    offset: opened.decoder.position(),
                    source: opened.source,
                    spec: opened.spec,
                    duration: opened.duration,
                    path,
                });
                self.decoding = Some(Decoding {
                    index: opened.index,
                    decoder: opened.decoder,
                });
            }
            Err(index) => {
                if continuous {
                    let mut tail = Vec::new();
                    self.chain.worker.flush_tail(&mut tail);
                    self.push_samples(tail);
                }
                self.pending_transition = None;
                self.feed.push_back(FeedItem::End);
                self.end_index = Some(index);
            }
        }
    }

    fn push_samples(&mut self, data: Vec<i32>) {
        if !data.is_empty() {
            self.feed_samples += data.len();
            self.feed.push_back(FeedItem::Samples { data, taken: 0 });
        }
    }

    /// The entry after `index` when a track ends on its own.
    fn auto_next(&self, index: usize) -> Option<usize> {
        match self.repeat {
            RepeatMode::One => Some(index),
            RepeatMode::All if !self.queue.is_empty() => Some((index + 1) % self.queue.len()),
            RepeatMode::All => None,
            RepeatMode::Off => (index + 1 < self.queue.len()).then_some(index + 1),
        }
    }

    /// Manual Next: the entry after `index`, wrapping under repeat-all.
    fn manual_next(&self, index: usize) -> Option<usize> {
        if index + 1 < self.queue.len() {
            Some(index + 1)
        } else if self.repeat == RepeatMode::All && !self.queue.is_empty() {
            Some(0)
        } else {
            None
        }
    }

    /// Next pressed during playback: moves to the next entry, fading into it when crossfade
    /// is on and the transition allows it, else jumping as before.
    fn next(&mut self) {
        let Some(index) = self.current_index() else {
            return;
        };
        let Some(next) = self.manual_next(index) else {
            self.stop();
            return;
        };
        if self.state == PlaybackState::Playing
            && self.hold.is_none()
            && self.config.crossfade.is_on()
            && self.start_fade(index, next, self.current_pos().map(|(_, pos)| pos))
        {
            return;
        }
        self.jump(next, Pos::ZERO);
    }

    /// Records why the transition into the next entry stays gapless, for its signal path.
    fn skip_transition(&mut self, skip: Skip) {
        self.fade_checked = true;
        self.pending_transition = Some(TransitionNote::Skipped(skip.reason()));
    }

    /// Starts the fade for the open track once it is close enough to its end. Returns
    /// whether a fade now runs.
    fn maybe_start_fade(&mut self, index: usize) -> bool {
        if !self.config.crossfade.is_on() || self.fade_checked {
            return false;
        }
        let Some(next) = self.auto_next(index) else {
            self.fade_checked = true;
            return false;
        };
        let (Some(source), Some(out_spec), Some(decoding)) =
            (self.source, self.chain.spec, self.decoding.as_ref())
        else {
            return false;
        };
        let Some(total) = decoding.decoder.total() else {
            self.skip_transition(Skip::UnknownLength);
            return false;
        };
        let remaining = total.saturating_sub(decoding.decoder.position());
        let remaining_secs = remaining as f64 / f64::from(source.rate);
        // The settings allow at most this much; the track halves clamp it further below.
        if remaining_secs > self.config.crossfade.seconds + 0.1 {
            return false;
        }
        let planned = match self.fade_plan {
            Some((planned_for, seconds)) if planned_for == next => seconds,
            _ => {
                let Some(path) = self.queue.get(next).map(Path::to_path_buf) else {
                    return false;
                };
                let Ok(decoder) = open_decoder(&path, Pos::ZERO) else {
                    // The normal advance reports the failure and moves on.
                    self.fade_checked = true;
                    return false;
                };
                let incoming = Side {
                    spec: out_spec,
                    pcm: matches!(decoder.format(), TrackFormat::Pcm(_)),
                    seconds: decoder
                        .total()
                        .map(|total| total as f64 / f64::from(decoder.source_format().rate)),
                };
                let outgoing = Side {
                    spec: out_spec,
                    pcm: source.bits != DSD_SOURCE_BITS,
                    seconds: Some(total as f64 / f64::from(source.rate)),
                };
                let transition = Transition {
                    outgoing,
                    incoming,
                    repeat_one: self.repeat == RepeatMode::One,
                    same_entry: index == next,
                    same_album: self.same_album(index, next),
                };
                match crossfade::decide(self.config.crossfade, transition) {
                    Ok(seconds) => seconds,
                    Err(skip) => {
                        self.skip_transition(skip);
                        return false;
                    }
                }
            }
        };
        self.fade_plan = Some((next, planned));
        if remaining_secs > planned + 0.1 {
            return false;
        }
        self.start_fade(index, next, None)
    }

    /// Builds the incoming track's chain and starts mixing it into the outgoing one.
    ///
    /// Automatic: the outgoing track's remaining frames are the fade, ending exactly at its
    /// end. Manual (`manual` holds the heard position): the stream is rebuilt there and the
    /// fade runs for the planned length, so Next is heard at once.
    fn start_fade(&mut self, from: usize, next: usize, manual: Option<Pos>) -> bool {
        let Some(source) = self.source else {
            return false;
        };
        let Some(out_spec) = self.chain.spec else {
            return false;
        };
        let Some(decoding) = self.decoding.as_ref() else {
            return false;
        };
        let Some(total) = decoding.decoder.total() else {
            self.skip_transition(Skip::UnknownLength);
            return false;
        };
        let position = match manual {
            Some(pos) => pos.frames_at(source.rate).min(total),
            None => decoding.decoder.position(),
        };
        let remaining = total.saturating_sub(position);
        let Some(path) = self.queue.get(next).map(Path::to_path_buf) else {
            return false;
        };
        let Ok(decoder) = open_decoder(&path, Pos::ZERO) else {
            self.fade_checked = true;
            return false;
        };
        let incoming_seconds = decoder
            .total()
            .map(|total| total as f64 / f64::from(decoder.source_format().rate));
        let outgoing = Side {
            spec: out_spec,
            pcm: source.bits != DSD_SOURCE_BITS,
            seconds: Some(total as f64 / f64::from(source.rate)),
        };
        let incoming = Side {
            spec: out_spec,
            pcm: matches!(decoder.format(), TrackFormat::Pcm(_)),
            seconds: incoming_seconds,
        };
        let transition = Transition {
            outgoing,
            incoming,
            repeat_one: self.repeat == RepeatMode::One,
            same_entry: from == next,
            same_album: self.same_album(from, next),
        };
        let clamped = match crossfade::decide(self.config.crossfade, transition) {
            Ok(seconds) => seconds,
            Err(skip) => {
                self.skip_transition(skip);
                return false;
            }
        };
        drop(decoder);

        // Prepare the incoming chain: it says what format that track reaches and what its
        // signal path is, and it decodes the fade's incoming side.
        let mut saved = self.take_chain();
        let saved_source = self.source;
        let mut tail = Vec::new();
        // Planning must not report failures: the normal advance will, once.
        let opened = match self.begin_chain(next, Pos::ZERO, false, &mut tail, false) {
            Ok(opened) => opened,
            Err(_) => {
                let _ = self.take_chain();
                self.install_chain(saved);
                self.source = saved_source;
                self.fade_checked = true;
                return false;
            }
        };
        let in_spec = self.chain.spec;
        if in_spec != Some(out_spec) {
            let _ = self.take_chain();
            self.install_chain(saved);
            self.source = saved_source;
            self.skip_transition(Skip::DifferentFormat);
            return false;
        }
        let rate = fade_rate(out_spec);
        let seconds = match manual {
            // Never more than the decision allows: the setting capped by half of the
            // shorter track, and never past what is left of the outgoing track.
            Some(_) => clamped.min(remaining as f64 / f64::from(source.rate)),
            // Automatic: the fade ends exactly at the outgoing track's end (the trigger
            // already waited for the clamped length).
            None => remaining as f64 / f64::from(source.rate),
        };
        let frames = (seconds * f64::from(rate)).round() as u64;
        if frames == 0 || rate == 0 {
            let _ = self.take_chain();
            self.install_chain(saved);
            self.source = saved_source;
            self.fade_checked = true;
            return false;
        }
        let path = self.signal_path(
            opened.source,
            out_spec,
            Some(TransitionNote::Faded(seconds)),
        );
        let path = Arc::new(path);
        let mut incoming_chain = self.take_chain();
        if manual.is_none() {
            // The outgoing carrier packer may hold a partial DoP/native frame; the incoming
            // packer continues it, so the fade does not drop one to three DSD bytes.
            let pending = saved.worker.take_packer_pending();
            incoming_chain.worker.adopt_packer_pending(pending);
        }
        // The outgoing chain goes back, so it can keep decoding (automatic) or be rebuilt at
        // the position the listener is at (manual).
        self.install_chain(saved);
        self.source = saved_source;
        if let Some(pos) = manual {
            self.reset_decoding();
            self.hold = None;
            if let Some(out) = &mut self.out {
                let flushed = out
                    .producer
                    .as_mut()
                    .is_some_and(|p| p.flush(FLUSH_TIMEOUT));
                if flushed {
                    out.segments.clear();
                } else {
                    self.close_output();
                }
            }
            let mut tail = Vec::new();
            match self.begin_chain(from, pos, false, &mut tail, true) {
                Ok(outgoing) => {
                    self.target = Some((
                        outgoing.index,
                        Pos::frames(outgoing.decoder.position(), outgoing.source.rate),
                    ));
                    self.decoding = Some(Decoding {
                        index: outgoing.index,
                        decoder: outgoing.decoder,
                    });
                }
                Err(_) => {
                    self.decoding = None;
                    self.target = None;
                    return false;
                }
            }
        }
        self.clipping_reported = false;
        self.sdm_warned = false;
        self.fade_checked = true;
        self.fade_plan = None;
        self.crossfade = Some(Crossfade {
            index: opened.index,
            source: opened.source,
            spec: out_spec,
            decoder: opened.decoder,
            chain: incoming_chain,
            fade: Fade::new(frames),
            pending_out: Vec::new(),
            pending_in: Vec::new(),
            mixed: Vec::new(),
            samples: Vec::new(),
            out_ended: false,
            in_ended: false,
            in_failed: false,
        });
        // The incoming entry starts in the stream here: every mixed sample belongs to it.
        self.feed.push_back(FeedItem::TrackStart {
            index: opened.index,
            offset: 0,
            source: opened.source,
            spec: out_spec,
            duration: opened.duration,
            path,
        });
        true
    }

    /// Decodes both sides of the running crossfade and pushes the mixed samples, one block at
    /// a time, until the fade is done or neither side has anything more to give.
    fn produce_fade(&mut self) -> bool {
        let mut progress = false;
        let mut finished = false;
        loop {
            let Some(fade) = self.crossfade.as_mut() else {
                return progress;
            };
            if fade.fade.is_done() {
                finished = true;
                break;
            }
            let channels = usize::from(fade.spec.channels).max(1);
            if fade.in_ended && fade.pending_in.len() < channels {
                // The incoming file ended inside the fade (only possible when its stated
                // length was wrong): play out what is left of the outgoing side.
                finished = true;
                break;
            }
            let mut decoded = false;
            if !fade.out_ended && fade.pending_out.len() < channels {
                let mut block = Vec::new();
                let result = match &mut self.decoding {
                    Some(decoding) => self
                        .chain
                        .worker
                        .decode_float(&mut decoding.decoder, &mut block),
                    None => Ok(0),
                };
                match result {
                    Ok(0) => {
                        fade.out_ended = true;
                        self.chain.worker.flush_float(&mut fade.pending_out);
                    }
                    Ok(_) => {
                        fade.pending_out.extend_from_slice(&block);
                        decoded = true;
                    }
                    Err(error) => {
                        // The event names the track that was decoding, not the incoming one.
                        let index = self
                            .decoding
                            .as_ref()
                            .map_or(fade.index, |decoding| decoding.index);
                        let path = self.queue.get(index).map(Path::to_path_buf);
                        let _ = self.events.send(PlayerEvent::TrackFailed {
                            index,
                            path: path.unwrap_or_default(),
                            error: error.to_string(),
                        });
                        fade.out_ended = true;
                        self.chain.worker.flush_float(&mut fade.pending_out);
                    }
                }
            }
            if !fade.in_ended && fade.pending_in.len() < channels {
                let mut block = Vec::new();
                match fade
                    .chain
                    .worker
                    .decode_float(&mut fade.decoder, &mut block)
                {
                    Ok(0) => {
                        fade.in_ended = true;
                        fade.chain.worker.flush_float(&mut fade.pending_in);
                    }
                    Ok(_) => {
                        fade.pending_in.extend_from_slice(&block);
                        decoded = true;
                    }
                    Err(error) => {
                        let path = self.queue.get(fade.index).map(Path::to_path_buf);
                        let _ = self.events.send(PlayerEvent::TrackFailed {
                            index: fade.index,
                            path: path.unwrap_or_default(),
                            error: error.to_string(),
                        });
                        fade.in_ended = true;
                        fade.in_failed = true;
                        fade.chain.worker.flush_float(&mut fade.pending_in);
                    }
                }
            }
            let out_frames = fade.pending_out.len() / channels;
            let in_frames = fade.pending_in.len() / channels;
            let outgoing = (out_frames > 0).then_some(fade.pending_out.as_slice());
            let incoming = (in_frames > 0).then_some(fade.pending_in.as_slice());
            let mixed = fade.fade.mix(outgoing, incoming, channels, &mut fade.mixed);
            if mixed > 0 {
                fade.samples.clear();
                fade.chain.worker.emit_float(&fade.mixed, &mut fade.samples);
                let data = std::mem::take(&mut fade.samples);
                self.feed_samples += data.len();
                self.feed.push_back(FeedItem::Samples { data, taken: 0 });
                let out_take = mixed.min(out_frames) * channels;
                let in_take = mixed.min(in_frames) * channels;
                fade.pending_out.drain(..out_take);
                fade.pending_in.drain(..in_take);
                progress = true;
                if fade.fade.is_done() {
                    finished = true;
                    break;
                }
                if self.feed_samples >= FEED_AHEAD_SAMPLES {
                    // The rest of the fade waits for pump to drain the feed, so commands and
                    // the device keep their turns and memory stays at the usual bound.
                    break;
                }
            } else if !decoded {
                break;
            }
        }
        if finished {
            self.finish_fade();
            return true;
        }
        progress
    }

    /// Makes the incoming track the current one once the fade's samples are queued.
    fn finish_fade(&mut self) {
        let Some(fade) = self.crossfade.take() else {
            return;
        };
        self.chain = fade.chain;
        self.source = Some(fade.source);
        self.fade_checked = false;
        self.fade_plan = None;
        if fade.in_failed {
            // The incoming file broke inside the fade: move past it like a decode failure
            // instead of installing a decoder that already failed.
            self.decoding = None;
            let stop = self.failures.note(fade.index, self.queue.len());
            let next = if stop {
                self.queue.len()
            } else {
                self.auto_next(fade.index).unwrap_or(self.queue.len())
            };
            self.open_track(next, Pos::ZERO, true);
            return;
        }
        self.decoding = Some(Decoding {
            index: fade.index,
            decoder: fade.decoder,
        });
        self.failures.clear();
        // Samples the incoming side decoded past the end of the fade continue the stream.
        if !fade.pending_in.is_empty() {
            let mut samples = Vec::new();
            self.chain.worker.emit_float(&fade.pending_in, &mut samples);
            self.push_samples(samples);
        }
    }

    /// Decodes one block ahead if there is room. Returns whether anything happened.
    fn produce(&mut self) -> bool {
        if self.state == PlaybackState::Stopped
            || self.hold.is_some()
            || self.feed_samples >= FEED_AHEAD_SAMPLES
        {
            return false;
        }
        if self.crossfade.is_some() {
            return self.produce_fade();
        }
        if let Some(index) = self.decoding.as_ref().map(|decoding| decoding.index)
            && self.maybe_start_fade(index)
        {
            return true;
        }
        if self.crossfade.is_some() {
            return self.produce_fade();
        }
        let Some(decoding) = &mut self.decoding else {
            return false;
        };
        let index = decoding.index;
        let mut block = Vec::new();
        match self.chain.worker.decode(&mut decoding.decoder, &mut block) {
            Ok(0) => {
                // A DSD packer may have appended one padded frame on end of stream.
                self.push_samples(block);
                self.decoding = None;
                let next = self.auto_next(index).unwrap_or(self.queue.len());
                self.open_track(next, Pos::ZERO, true);
            }
            Ok(_) => {
                self.push_samples(block);
                self.failures.clear();
            }
            Err(error) => {
                // Untrusted input: a broken file ends that track, never playback. A repeat
                // mode must not loop over it either: a whole queue of failures ends the run.
                let path = self.queue.get(index).map(Path::to_path_buf);
                self.emit(PlayerEvent::TrackFailed {
                    index,
                    path: path.unwrap_or_default(),
                    error: error.to_string(),
                });
                self.decoding = None;
                let stop = self.failures.note(index, self.queue.len());
                let next = if stop {
                    self.queue.len()
                } else {
                    self.auto_next(index).unwrap_or(self.queue.len())
                };
                self.open_track(next, Pos::ZERO, true);
            }
        }
        // Surface a boost that ran out of headroom; once per track is enough to act on it.
        let clipped =
            self.chain.worker.take_clipped() + self.dither_clipped.swap(0, Ordering::Relaxed);
        if clipped > 0 && !self.clipping_reported {
            self.clipping_reported = true;
            self.emit(PlayerEvent::Clipping(clipped));
        }
        if self.chain.worker.take_modulator_limited() && !self.sdm_warned {
            self.sdm_warned = true;
            self.emit(PlayerEvent::Warning(
                "the DSD modulator left its stable range and reset; lower the volume or EQ boost"
                    .into(),
            ));
        }
        true
    }

    // ----- output -----

    /// Moves feed items into the output ring. Returns whether anything moved.
    fn pump(&mut self) -> bool {
        let mut progress = false;
        while let Some(item) = self.feed.front_mut() {
            match item {
                FeedItem::TrackStart {
                    index,
                    offset,
                    source,
                    spec,
                    duration,
                    path,
                } => {
                    let segment = Segment {
                        start: 0,
                        index: *index,
                        offset: *offset,
                        source: *source,
                        duration: *duration,
                        path: path.clone(),
                    };
                    let spec = *spec;
                    match &mut self.out {
                        // Draining towards a format change: wait for the stream to finish.
                        Some(out) if out.producer.is_none() => return progress,
                        Some(out) if !reusable(out.format, out.mode, spec) => {
                            out.producer = None;
                            return true;
                        }
                        Some(_) => {}
                        None => {
                            let pos = Pos::frames(segment.offset, segment.source.rate);
                            if !self.open_output(spec, segment.index, pos, segment.source) {
                                return true;
                            }
                        }
                    }
                    let out = self.out.as_mut().expect("output open");
                    out.segments.push(Segment {
                        start: out.pushed,
                        ..segment
                    });
                    self.target = None;
                    self.feed.pop_front();
                    progress = true;
                }
                FeedItem::Samples { data, taken } => {
                    let Some(out) = &mut self.out else {
                        // Samples never outlive their stream; drop strays defensively.
                        self.feed_samples -= data.len() - *taken;
                        self.feed.pop_front();
                        continue;
                    };
                    let Some(producer) = &mut out.producer else {
                        return progress;
                    };
                    let n = producer.push_some(&data[*taken..]);
                    if n == 0 {
                        return progress;
                    }
                    *taken += n;
                    out.pushed += (n / usize::from(out.format.channels)) as u64;
                    self.feed_samples -= n;
                    progress = true;
                    if *taken == data.len() {
                        self.feed.pop_front();
                    }
                }
                FeedItem::End => {
                    self.feed.pop_front();
                    match &mut self.out {
                        Some(out) => {
                            out.producer = None;
                            self.ending = true;
                        }
                        None => self.finish_queue(),
                    }
                    progress = true;
                }
            }
        }
        progress
    }

    fn open_output(
        &mut self,
        spec: OutputSpec,
        index: usize,
        pos: Pos,
        track_source: SourceFormat,
    ) -> bool {
        let backend = self.backend.as_ref();
        let device = &self.config.device;
        let format = match spec.mode {
            OutputMode::Pcm => negotiate(backend, device, spec.rate, spec.bits, spec.channels),
            OutputMode::Dop => negotiate_dop(backend, device, spec.rate, spec.channels),
            OutputMode::NativeDsd => negotiate_native(backend, device, spec.rate, spec.channels),
        };
        let format = match format {
            Ok(format) => format,
            Err(OutputError::UnsupportedFormat { .. })
                if spec.mode != OutputMode::Pcm && self.dsd_reprobe != Some(index) =>
            {
                // The DSD mode was assumed while the device was busy; now that it is free,
                // decode the track again so `choose_dsd_mode` gets a real answer. Once only,
                // so a device that contradicts its own probe cannot loop.
                self.dsd_reprobe = Some(index);
                self.restart(index, pos);
                return false;
            }
            Err(error)
                if spec.mode == OutputMode::Pcm
                    && self.config.src.auto_fallback
                    && self.src_retry != Some(index)
                    && matches!(&error, OutputError::UnsupportedFormat { .. }) =>
            {
                // The device refuses this rate even with DSP off: pick one it accepts,
                // resample, and decode the track again from the same position. The signal
                // path marks the result "not bit-perfect".
                // DSD→PCM already converted to the configured PCM rate; fallback starts
                // from that rate, not from the DSD bit rate.
                let dsd_pcm = track_source.bits == DSD_SOURCE_BITS;
                let pre_rate = if dsd_pcm {
                    self.config.dsd_pcm_rate.rate()
                } else {
                    track_source.rate
                };
                let pre_bits = if dsd_pcm { 24 } else { track_source.bits };
                let supported = match &error {
                    OutputError::UnsupportedFormat { supported, .. } => supported.clone(),
                    _ => Vec::new(),
                };
                let mut rates: Vec<u32> = supported.iter().map(|format| format.rate).collect();
                rates.sort_unstable();
                rates.dedup();
                if rates.is_empty() {
                    rates = self.supported_rates(pre_bits, track_source.channels);
                }
                if spec.rate != pre_rate && rates.contains(&pre_rate) {
                    // An explicit target was refused but the source rate is fine: drop the
                    // resampler and play the track as it is.
                    self.fallback
                        .insert((pre_rate, track_source.channels), pre_rate);
                    self.src_retry = Some(index);
                    self.emit(PlayerEvent::Warning(format!(
                        "the device cannot play {} Hz; keeping the source rate {pre_rate} Hz",
                        spec.rate
                    )));
                    self.restart(index, pos);
                    return false;
                }
                if let Some(target) = rate::fallback_target(pre_rate, &rates) {
                    self.fallback
                        .insert((pre_rate, track_source.channels), target);
                    self.src_retry = Some(index);
                    self.emit(PlayerEvent::Warning(format!(
                        "the device cannot play {pre_rate} Hz; resampling to {target} Hz \
                         (not bit-perfect)"
                    )));
                    self.restart(index, pos);
                    return false;
                }
                self.output_failed(error, index, pos);
                return false;
            }
            Err(error) => {
                self.output_failed(error, index, pos);
                return false;
            }
        };
        self.dsd_reprobe = None;
        self.src_retry = None;
        let frames = (u128::from(spec.rate) * self.config.ring.as_millis() / 1000) as usize;
        let (producer, source) = sample_ring(frames.max(MIN_RING_FRAMES), spec.channels);
        source
            .handle()
            .set_paused(self.state != PlaybackState::Playing);
        let options = StreamOptions {
            tap: self.config.tap.as_mut().map(|make| make(format)),
            dop: spec.mode == OutputMode::Dop,
        };
        match self
            .backend
            .open(&self.config.device, format, source, options)
        {
            Ok(stream) => {
                self.out = Some(Output {
                    handle: stream.handle(),
                    stream,
                    producer: Some(producer),
                    format,
                    mode: spec.mode,
                    pushed: 0,
                    segments: Vec::new(),
                });
                self.emit(PlayerEvent::OutputFormat(format));
                true
            }
            Err(error) => {
                self.output_failed(error, index, pos);
                false
            }
        }
    }

    fn close_output(&mut self) {
        if let Some(out) = self.out.take() {
            self.underruns_base += out.handle.underruns();
            // Dropping the stream stops it and releases the device.
            drop(out);
        }
    }

    /// Applies the device-error policy for a failure while opening or playing `index`.
    fn output_failed(&mut self, error: OutputError, index: usize, pos: Pos) {
        let (reason, event) = match error {
            OutputError::UnsupportedFormat { .. } => {
                // Refuse rather than convert: this track is skipped with the reason.
                let path = self.queue.get(index).map(Path::to_path_buf);
                self.emit(PlayerEvent::TrackFailed {
                    index,
                    path: path.unwrap_or_default(),
                    error: error.to_string(),
                });
                self.close_output();
                self.reset_decoding();
                self.open_track(index + 1, Pos::ZERO, false);
                return;
            }
            OutputError::DeviceInUse => (HoldReason::Busy, PlayerEvent::DeviceBusy),
            OutputError::DeviceLost(_) | OutputError::DeviceNotFound(_) => {
                (HoldReason::Lost, PlayerEvent::DeviceLost(error.to_string()))
            }
            other => (
                HoldReason::Failed,
                PlayerEvent::OutputFailed(other.to_string()),
            ),
        };
        self.close_output();
        self.reset_decoding();
        self.hold = Some(Hold { index, pos, reason });
        self.target = None;
        self.last_retry = Instant::now();
        self.emit(event);
        if self.state == PlaybackState::Playing {
            self.set_state(PlaybackState::Paused);
        }
    }

    /// Reaps a stream whose render thread exited.
    fn check_output(&mut self) {
        if !self.out.as_ref().is_some_and(|o| o.stream.is_finished()) {
            return;
        }
        // The counters are final now: announce a track the device reached since the last
        // report (a short last track can start and finish in between).
        self.report();
        let now = self.now_playing();
        let Some(out) = self.out.take() else {
            return;
        };
        self.underruns_base += out.handle.underruns();
        let segments = out.segments.clone();
        match out.stream.wait() {
            Ok(_) if self.ending => self.finish_queue(),
            // Drained for a format change: the next track start opens a new stream.
            Ok(_) => {}
            Err(error) => {
                let (index, pos) = match now {
                    Some(now) => (
                        now.segment.index,
                        Pos::frames(now.frame, now.segment.source.rate),
                    ),
                    None => segments
                        .first()
                        .map(|s| (s.index, Pos::frames(s.offset, s.source.rate)))
                        .unwrap_or((0, Pos::ZERO)),
                };
                self.output_failed(error, index, pos);
            }
        }
    }

    fn finish_queue(&mut self) {
        self.crossfade = None;
        self.fade_checked = false;
        self.fade_plan = None;
        self.pending_transition = None;
        self.ending = false;
        self.end_index = None;
        self.reported_track = None;
        self.queue.set_current(None);
        self.target = None;
        self.after_end = Some(self.queue.len());
        self.stopped_at = Pos::ZERO;
        self.emit(PlayerEvent::EndOfQueue);
        self.set_state(PlaybackState::Stopped);
    }

    // ----- reporting -----

    fn report(&mut self) {
        if let Some(now) = self.now_playing() {
            let segment = now.segment.clone();
            let track_changed = self.reported_track != Some(segment.index);
            if track_changed {
                self.reported_track = Some(segment.index);
                self.queue.set_current(Some(segment.index));
                self.last_position = None;
                self.emit(PlayerEvent::TrackChanged {
                    index: segment.index,
                    path: self
                        .queue
                        .get(segment.index)
                        .map(Path::to_path_buf)
                        .unwrap_or_default(),
                    source: segment.source,
                    duration: segment
                        .duration
                        .map(|d| Pos::frames(d, segment.source.rate).duration()),
                });
            }
            if track_changed || self.reported_path.as_deref() != Some(segment.path.as_ref()) {
                self.reported_path = Some(segment.path.clone());
                self.emit(PlayerEvent::SignalPath((*segment.path).clone()));
            }
            let due = self
                .last_position
                .is_none_or(|t| t.elapsed() >= self.config.position_interval);
            if self.state == PlaybackState::Playing && due {
                self.last_position = Some(Instant::now());
                self.emit(PlayerEvent::Position {
                    index: segment.index,
                    elapsed: Pos::frames(now.frame, segment.source.rate).duration(),
                });
            }
        }
        let underruns = self.underruns_base + self.out.as_ref().map_or(0, |o| o.handle.underruns());
        if underruns != self.reported_underruns {
            self.reported_underruns = underruns;
            self.emit(PlayerEvent::Underruns(underruns));
        }
    }

    fn poll_devices(&mut self) {
        if self.last_poll.elapsed() < self.config.device_poll {
            return;
        }
        self.last_poll = Instant::now();
        let Ok(ids) = self.backend.device_ids() else {
            return;
        };
        if ids != self.devices {
            self.devices = ids.clone();
            self.emit(PlayerEvent::DevicesChanged(ids));
        }
        // Retry a lost device once it is listed again. Sleep/resume often keeps it listed,
        // so presence (not only a change) triggers the retry, rate-limited.
        if let Some(hold) = self.hold
            && hold.reason == HoldReason::Lost
            && self.devices.contains(&self.config.device)
            && self.last_retry.elapsed() >= self.config.device_poll
        {
            self.last_retry = Instant::now();
            self.restart(hold.index, hold.pos);
        }
    }
}

/// What a stream with these counters is playing.
fn now_in(
    handle: &StreamHandle,
    pushed: u64,
    segments: &[Segment],
    format: DeviceFormat,
) -> Option<Now> {
    let consumed = handle.frames_consumed().min(pushed);
    let segment = segments.iter().rev().find(|s| s.start <= consumed)?.clone();
    let played = consumed - segment.start;
    let played = Pos::frames(played, format.rate).frames_at(segment.source.rate);
    let mut frame = segment.offset + played;
    if let Some(total) = segment.duration {
        frame = frame.min(total);
    }
    Some(Now { segment, frame })
}

/// What a format probe says about the device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Probe {
    Yes,
    No,
    /// The device could not be asked, typically because our own stream holds it.
    Unknown,
}

fn probe(result: Result<DeviceFormat, OutputError>) -> Probe {
    match result {
        Ok(_) => Probe::Yes,
        Err(OutputError::UnsupportedFormat { .. }) => Probe::No,
        Err(_) => Probe::Unknown,
    }
}

/// Rate of the samples the crossfade mixes: the chain's output rate. For a modulated stream
/// that is the modulator's input rate, not the carrier rate on the wire (a DoP frame carries
/// two input-rate ticks per sample, a native word four).
fn fade_rate(spec: OutputSpec) -> u32 {
    match spec.mode {
        OutputMode::Pcm => spec.rate,
        OutputMode::Dop => spec.rate.saturating_mul(2),
        OutputMode::NativeDsd => spec.rate.saturating_mul(4),
    }
}

/// Whether an open stream in `current` can carry `spec` unchanged (same rate and channels, and
/// a container wide enough for the bits). A 16-bit track after a 24-bit one stays bit-perfect
/// in the 24-bit container, so it plays gapless. Native DSD is not a PCM container, so it is
/// reused when the word container matches instead of asking `holds`.
fn reusable(current: DeviceFormat, mode: OutputMode, spec: OutputSpec) -> bool {
    let container = match mode {
        OutputMode::NativeDsd => current.container == SampleContainer::DsdU32Be,
        _ => current.container.holds(spec.bits),
    };
    mode == spec.mode && current.rate == spec.rate && current.channels == spec.channels && container
}

fn open_decoder(path: &Path, pos: Pos) -> Result<SourceDecoder, DecodeError> {
    let mut decoder = SourceDecoder::open(path)?;
    let frame = pos.frames_at(decoder.source_format().rate);
    if frame > 0 {
        decoder.seek(frame)?;
    }
    Ok(decoder)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sonder_output::SampleContainer;

    #[test]
    fn positions_convert_without_rounding_at_the_same_rate() {
        let p = Pos::frames(12_345, 44_100);
        assert_eq!(p.frames_at(44_100), 12_345);
        assert_eq!(p.frames_at(88_200), 24_690);
        assert_eq!(
            Pos::from_duration(Duration::from_secs(2)).frames_at(96_000),
            192_000
        );
        assert_eq!(
            Pos::frames(44_100, 44_100).duration(),
            Duration::from_secs(1)
        );
    }

    #[test]
    fn a_whole_queue_of_failures_ends_the_run_but_a_new_track_resets_it() {
        let mut run = FailureRun::default();
        assert!(
            !run.note(1, 3),
            "one failure of a three-track queue retries"
        );
        assert!(
            !run.note(1, 3),
            "the same broken file retries under repeat-one"
        );
        assert!(run.note(1, 3), "a whole queue's worth ends the run");
        run.clear();
        assert!(!run.note(2, 3), "a new track gets its own budget");
        assert!(!run.note(0, 3), "another index starts over");
        assert!(!run.note(0, 3));
        assert!(run.note(0, 3));
    }

    #[test]
    fn wider_container_keeps_the_stream_open() {
        let s24 = DeviceFormat {
            rate: 44_100,
            container: SampleContainer::S24Packed,
            channels: 2,
        };
        let spec = |rate, bits| OutputSpec {
            rate,
            bits,
            channels: 2,
            mode: OutputMode::Pcm,
        };
        assert!(reusable(s24, OutputMode::Pcm, spec(44_100, 16)));
        assert!(reusable(s24, OutputMode::Pcm, spec(44_100, 24)));
        assert!(!reusable(s24, OutputMode::Pcm, spec(44_100, 32)));
        assert!(!reusable(s24, OutputMode::Pcm, spec(48_000, 16)));
        // A DoP stream can never carry PCM samples, even at the same rate and container.
        assert!(!reusable(s24, OutputMode::Dop, spec(44_100, 24)));
    }
}
