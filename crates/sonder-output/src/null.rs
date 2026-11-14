//! A device-less backend that records every byte it would have written.
//!
//! It behaves like an exclusive-mode device (one stream at a time, fixed format per stream,
//! a render thread pulling one period at a time) so the engine can be tested without audio
//! hardware, including device failures injected through [`NullControl`].
//!
//! Recording takes a lock on the render thread; this backend is for tests and tooling only.

use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use crate::format::DeviceFormat;
use crate::render::Renderer;
use crate::ring::SampleSource;
use crate::stream::{Stream, StreamOptions, StreamReport};
use crate::{COMMON_RATES, DeviceId, DeviceInfo, OutputBackend, OutputError, SampleContainer};

/// Id of the single device the null backend exposes.
pub const NULL_DEVICE_ID: &str = "null";

/// Audio written by one stream, from open to close.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NullSession {
    pub format: DeviceFormat,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StreamFault {
    Xrun,
    Lost,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OpenFault {
    Busy,
}

#[derive(Default)]
struct NullState {
    unplugged: bool,
    open_fault: Option<OpenFault>,
    stream_fault: Option<StreamFault>,
    open_streams: usize,
    sessions: Vec<NullSession>,
}

/// Test-side view of a [`NullBackend`]: read what was played and inject device errors.
#[derive(Clone, Default)]
pub struct NullControl(Arc<Mutex<NullState>>);

impl NullControl {
    fn state(&self) -> MutexGuard<'_, NullState> {
        // A panicking test thread must not hide the recording from the assertions.
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Every stream opened so far, in order.
    pub fn sessions(&self) -> Vec<NullSession> {
        self.state().sessions.clone()
    }

    /// All audio bytes written across every session.
    pub fn played_bytes(&self) -> Vec<u8> {
        self.state()
            .sessions
            .iter()
            .flat_map(|s| s.bytes.iter().copied())
            .collect()
    }

    /// Whether a stream currently holds the device.
    pub fn is_open(&self) -> bool {
        self.state().open_streams > 0
    }

    /// The next `open` fails as if another application held the device exclusively.
    pub fn fail_next_open_busy(&self) {
        self.state().open_fault = Some(OpenFault::Busy);
    }

    /// The running stream reports one xrun and keeps playing.
    pub fn inject_xrun(&self) {
        self.state().stream_fault = Some(StreamFault::Xrun);
    }

    /// The device disappears: the running stream fails with [`OutputError::DeviceLost`] and
    /// the device leaves the device list until [`replug`](Self::replug).
    pub fn unplug(&self) {
        let mut state = self.state();
        state.unplugged = true;
        if state.open_streams > 0 {
            state.stream_fault = Some(StreamFault::Lost);
        }
    }

    /// The device comes back.
    pub fn replug(&self) {
        self.state().unplugged = false;
    }
}

/// See the module documentation.
pub struct NullBackend {
    control: NullControl,
    formats: Option<Vec<DeviceFormat>>,
    period_frames: usize,
    period: Duration,
    busy_probes: bool,
}

impl Default for NullBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl NullBackend {
    /// A device that accepts every format and pulls 512 frames per millisecond.
    pub fn new() -> Self {
        Self {
            control: NullControl::default(),
            formats: None,
            period_frames: 512,
            period: Duration::from_millis(1),
            busy_probes: false,
        }
    }

    /// Restricts the device to exactly these formats.
    pub fn with_formats(mut self, formats: Vec<DeviceFormat>) -> Self {
        self.formats = Some(formats);
        self
    }

    /// Sets how many frames the device pulls per period and how long a period lasts.
    pub fn with_period(mut self, frames: usize, period: Duration) -> Self {
        self.period_frames = frames.max(1);
        self.period = period;
        self
    }

    pub fn control(&self) -> NullControl {
        self.control.clone()
    }

    pub fn device_id() -> DeviceId {
        DeviceId(NULL_DEVICE_ID.into())
    }

    /// Format probes fail with [`OutputError::DeviceInUse`] while a stream is open, as ALSA
    /// `hw:` devices do (probing reopens the device, which the stream holds exclusively).
    pub fn with_busy_probes(mut self) -> Self {
        self.busy_probes = true;
        self
    }

    fn present(&self, device: &DeviceId) -> Result<(), OutputError> {
        if device.0 != NULL_DEVICE_ID || self.control.state().unplugged {
            return Err(OutputError::DeviceNotFound(device.0.clone()));
        }
        Ok(())
    }

    /// [`present`](Self::present), plus the busy answer of [`with_busy_probes`].
    ///
    /// [`with_busy_probes`]: Self::with_busy_probes
    fn probe_ready(&self, device: &DeviceId) -> Result<(), OutputError> {
        self.present(device)?;
        if self.busy_probes && self.control.state().open_streams > 0 {
            return Err(OutputError::DeviceInUse);
        }
        Ok(())
    }

    fn accepts(&self, format: DeviceFormat) -> bool {
        self.formats.as_ref().is_none_or(|f| f.contains(&format))
    }
}

impl OutputBackend for NullBackend {
    fn devices(&self) -> Result<Vec<DeviceInfo>, OutputError> {
        let id = Self::device_id();
        if self.present(&id).is_err() {
            return Ok(Vec::new());
        }
        let formats = self.supported_formats(&id, 2)?;
        Ok(vec![DeviceInfo {
            id,
            name: "Null output".into(),
            is_default: true,
            formats,
        }])
    }

    fn device_ids(&self) -> Result<Vec<DeviceId>, OutputError> {
        let id = Self::device_id();
        Ok(if self.present(&id).is_ok() {
            vec![id]
        } else {
            Vec::new()
        })
    }

    fn is_supported(&self, device: &DeviceId, format: DeviceFormat) -> Result<bool, OutputError> {
        self.probe_ready(device)?;
        Ok(self.accepts(format))
    }

    fn supported_formats(
        &self,
        device: &DeviceId,
        channels: u16,
    ) -> Result<Vec<DeviceFormat>, OutputError> {
        self.probe_ready(device)?;
        let mut formats = Vec::new();
        for rate in COMMON_RATES {
            for container in SampleContainer::ALL {
                let format = DeviceFormat {
                    rate,
                    container,
                    channels,
                };
                if self.accepts(format) {
                    formats.push(format);
                }
            }
        }
        Ok(formats)
    }

    fn open(
        &mut self,
        device: &DeviceId,
        format: DeviceFormat,
        source: SampleSource,
        options: StreamOptions,
    ) -> Result<Stream, OutputError> {
        self.present(device)?;
        if source.channels != format.channels {
            return Err(OutputError::Backend(format!(
                "queue carries {} channels but the device format has {}",
                source.channels, format.channels
            )));
        }
        {
            let mut state = self.control.state();
            if state.open_fault.take() == Some(OpenFault::Busy) || state.open_streams > 0 {
                return Err(OutputError::DeviceInUse);
            }
            if !self.accepts(format) {
                return Err(OutputError::UnsupportedFormat {
                    requested: format.to_string(),
                    supported: Vec::new(),
                });
            }
            state.open_streams += 1;
            state.stream_fault = None;
            state.sessions.push(NullSession {
                format,
                bytes: Vec::new(),
            });
        }

        let handle = source.handle.clone();
        let control = self.control.clone();
        let (period_frames, period) = (self.period_frames, self.period);
        let thread = thread::Builder::new()
            .name("sonder-null-render".into())
            .spawn(move || {
                let renderer =
                    Renderer::new(source, format, period_frames, options.tap, options.dop);
                let result = render(renderer, &control, period_frames, period);
                control.state().open_streams -= 1;
                result
            });
        match thread {
            Ok(thread) => Ok(Stream::new(handle, format, thread)),
            Err(e) => {
                self.control.state().open_streams -= 1;
                Err(OutputError::Backend(format!(
                    "cannot spawn render thread: {e}"
                )))
            }
        }
    }
}

/// How many periods late the render thread may fall before it gives up catching up.
const MAX_BACKLOG_PERIODS: u32 = 32;

fn render(
    mut renderer: Renderer,
    control: &NullControl,
    period_frames: usize,
    period: Duration,
) -> Result<StreamReport, OutputError> {
    let handle = renderer.handle().clone();
    let mut deadline = Instant::now();
    let result = loop {
        if handle.is_stopping() {
            break Ok(());
        }
        let fault = control.state().stream_fault.take();
        match fault {
            Some(StreamFault::Lost) => {
                break Err(OutputError::DeviceLost("null device unplugged".into()));
            }
            Some(StreamFault::Xrun) => renderer.note_xrun(),
            None => {}
        }
        let fill = renderer.fill(period_frames);
        // A DSD stream must record its idle frames too: an underrun or pause is exactly where
        // a zero frame would drop the DAC out of DSD mode.
        let recorded = if renderer.is_dsd() {
            period_frames
        } else {
            fill.payload_frames
        };
        if recorded > 0 {
            let bytes = renderer.bytes(recorded);
            if let Some(session) = control.state().sessions.last_mut() {
                session.bytes.extend_from_slice(bytes);
            }
        }
        if fill.input_exhausted {
            break Ok(());
        }
        // Pace against a clock, as hardware does: a wake-up that comes late renders the periods
        // it missed instead of slowing the device. Otherwise coarse timers on a loaded machine
        // stretch every period and the device plays several times slower than configured.
        deadline += period;
        let now = Instant::now();
        match deadline.checked_duration_since(now) {
            Some(wait) => thread::sleep(wait),
            // Like a bounded hardware buffer, drop a backlog too long to catch up on.
            None if now - deadline > period * MAX_BACKLOG_PERIODS => deadline = now,
            None => {}
        }
    };
    // Release a producer that may be blocked on a full queue.
    handle.stop();
    renderer.finish();
    result.map(|()| StreamReport {
        frames_written: handle.frames_written(),
        underruns: handle.underruns(),
    })
}
