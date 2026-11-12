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
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, unbounded};
use sonder_decode::{
    DSD_SOURCE_BITS, DecodeError, DsdFormat, SourceDecoder, SourceFormat, TrackFormat,
};
use sonder_output::{
    DeviceFormat, DeviceId, OutputBackend, OutputError, SampleProducer, Stream, StreamHandle,
    StreamOptions, WriteTap, negotiate, negotiate_dop, negotiate_native, sample_ring,
};

use crate::queue::Queue;
use crate::stage::{Stage, Volume};
use crate::worker::{DecodeWorker, DsdMode, OutputMode, OutputSpec};

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
    Shutdown,
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
    EndOfQueue,
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
    },
    Samples {
        data: Vec<i32>,
        taken: usize,
    },
    /// No more tracks: drain the stream and report the end of the queue.
    End,
}

/// Where a track starts in a stream's frame index space.
#[derive(Debug, Clone, Copy)]
struct Segment {
    start: u64,
    index: usize,
    offset: u64,
    source: SourceFormat,
    duration: Option<u64>,
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
#[derive(Debug, Clone, Copy)]
struct Now {
    segment: Segment,
    frame: u64,
}

struct Engine {
    backend: Box<dyn OutputBackend>,
    config: PlayerConfig,
    commands: Receiver<Command>,
    events: Sender<PlayerEvent>,
    queue: Queue,
    state: PlaybackState,
    worker: DecodeWorker,
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
    last_position: Option<Instant>,
    underruns_base: u64,
    reported_underruns: u64,
    devices: Vec<DeviceId>,
    last_poll: Instant,
    last_retry: Instant,
    /// Track whose assumed DSD mode was rejected when its stream opened and that was decoded
    /// again with the device free; a second rejection is a normal output failure.
    dsd_reprobe: Option<usize>,
}

impl Engine {
    fn new(
        backend: Box<dyn OutputBackend>,
        config: PlayerConfig,
        commands: Receiver<Command>,
        events: Sender<PlayerEvent>,
    ) -> Self {
        let devices = backend.device_ids().unwrap_or_default();
        Self {
            backend,
            config,
            commands,
            events,
            queue: Queue::default(),
            state: PlaybackState::Stopped,
            worker: DecodeWorker::new(Vec::new()),
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
            last_position: None,
            underruns_base: 0,
            reported_underruns: 0,
            devices,
            last_poll: Instant::now(),
            last_retry: Instant::now(),
            dsd_reprobe: None,
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
            Command::Next => {
                if let Some(index) = self.current_index() {
                    if index + 1 < self.queue.len() {
                        self.jump(index + 1, Pos::ZERO);
                    } else {
                        self.stop();
                    }
                }
            }
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
            Command::SetDevice(device) => self.set_device(device),
            Command::SetDsd {
                default_mode,
                modes,
                pcm_rate,
            } => self.set_dsd(default_mode, modes, pcm_rate),
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
        let stages: Vec<Box<dyn Stage>> = match db {
            Some(db) => vec![Box::new(Volume::from_db(db))],
            None => Vec::new(),
        };
        self.worker.set_stages(stages);
        // Re-decode from what is playing now so the change is heard after one ring, not
        // after everything already decoded.
        if self.state != PlaybackState::Stopped
            && self.hold.is_none()
            && let Some((index, pos)) = self.current_pos()
        {
            self.restart(index, pos);
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

    fn set_device(&mut self, device: DeviceId) {
        if device == self.config.device {
            return;
        }
        self.config.device = device;
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

    fn reset_decoding(&mut self) {
        self.feed.clear();
        self.feed_samples = 0;
        self.decoding = None;
        self.worker.reset();
        self.ending = false;
        self.end_index = None;
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
        if !self.worker.pipeline_is_empty() {
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

    /// Opens track `index` (skipping unplayable files) and queues its start marker.
    ///
    /// `continuous` is true when the track follows the previous one gaplessly, so stage
    /// tails belong in the stream; after a seek or stop they are discarded.
    fn open_track(&mut self, mut index: usize, mut pos: Pos, continuous: bool) {
        loop {
            let Some(path) = self.queue.get(index).map(Path::to_path_buf) else {
                if continuous {
                    let mut tail = Vec::new();
                    self.worker.flush_tail(&mut tail);
                    self.push_samples(tail);
                }
                self.feed.push_back(FeedItem::End);
                self.end_index = Some(index);
                return;
            };
            match open_decoder(&path, pos) {
                Ok(decoder) => {
                    let source = decoder.source_format();
                    let mut tail = Vec::new();
                    let spec = match decoder.format() {
                        TrackFormat::Pcm(_) => self.worker.prepare(source, &mut tail),
                        TrackFormat::Dsd(dsd) => {
                            let (mode, reason) = self.choose_dsd_mode(dsd);
                            self.emit(PlayerEvent::DsdMode { mode, reason });
                            self.worker.prepare_dsd(
                                dsd,
                                mode,
                                self.config.dsd_pcm_rate.rate(),
                                &mut tail,
                            )
                        }
                    };
                    if continuous {
                        self.push_samples(tail);
                    } else {
                        // Unplayable tracks may have been skipped on the way here.
                        self.target = Some((index, Pos::frames(decoder.position(), source.rate)));
                    }
                    self.feed.push_back(FeedItem::TrackStart {
                        index,
                        offset: decoder.position(),
                        source,
                        spec,
                        duration: decoder.total(),
                    });
                    self.decoding = Some(Decoding { index, decoder });
                    return;
                }
                Err(error) => {
                    self.emit(PlayerEvent::TrackFailed {
                        index,
                        path,
                        error: error.to_string(),
                    });
                    index += 1;
                    pos = Pos::ZERO;
                }
            }
        }
    }

    fn push_samples(&mut self, data: Vec<i32>) {
        if !data.is_empty() {
            self.feed_samples += data.len();
            self.feed.push_back(FeedItem::Samples { data, taken: 0 });
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
        let Some(decoding) = &mut self.decoding else {
            return false;
        };
        let index = decoding.index;
        let mut block = Vec::new();
        match self.worker.decode(&mut decoding.decoder, &mut block) {
            Ok(0) => {
                // A DSD packer may have appended one padded frame on end of stream.
                self.push_samples(block);
                self.decoding = None;
                self.open_track(index + 1, Pos::ZERO, true);
            }
            Ok(_) => self.push_samples(block),
            Err(error) => {
                // Untrusted input: a broken file ends that track, never playback.
                let path = self.queue.get(index).map(Path::to_path_buf);
                self.emit(PlayerEvent::TrackFailed {
                    index,
                    path: path.unwrap_or_default(),
                    error: error.to_string(),
                });
                self.decoding = None;
                self.open_track(index + 1, Pos::ZERO, true);
            }
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
                } => {
                    let segment = Segment {
                        start: 0,
                        index: *index,
                        offset: *offset,
                        source: *source,
                        duration: *duration,
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
                            if !self.open_output(spec, segment.index, pos) {
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

    fn open_output(&mut self, spec: OutputSpec, index: usize, pos: Pos) -> bool {
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
            Err(error) => {
                self.output_failed(error, index, pos);
                return false;
            }
        };
        self.dsd_reprobe = None;
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
            let segment = now.segment;
            if self.reported_track != Some(segment.index) {
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
    let segment = *segments.iter().rev().find(|s| s.start <= consumed)?;
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

/// Whether an open stream in `current` can carry `spec` unchanged (same rate and channels, and
/// a container wide enough for the bits). A 16-bit track after a 24-bit one stays bit-perfect
/// in the 24-bit container, so it plays gapless.
fn reusable(current: DeviceFormat, mode: OutputMode, spec: OutputSpec) -> bool {
    mode == spec.mode
        && current.rate == spec.rate
        && current.channels == spec.channels
        && current.container.holds(spec.bits)
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
