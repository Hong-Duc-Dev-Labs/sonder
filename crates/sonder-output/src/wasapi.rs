//! WASAPI exclusive-mode backend (event driven).
//!
//! Every COM object lives on the thread that created it: device queries run on the caller's
//! thread inside a [`ComGuard`], and the stream's audio client is created on its own render
//! thread, which also registers with MMCSS as "Pro Audio".

use std::sync::mpsc::{SyncSender, sync_channel};
use std::thread;
use std::time::{Duration, Instant};

use ::wasapi::{
    AudioClient, Device, DeviceEnumerator, Direction, SampleType, StreamMode, WasapiError,
    WaveFormat, calculate_period_100ns, deinitialize, initialize_mta,
};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Media::Audio::{
    AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED, AUDCLNT_E_DEVICE_IN_USE, AUDCLNT_E_DEVICE_INVALIDATED,
    AUDCLNT_E_EXCLUSIVE_MODE_NOT_ALLOWED, AUDCLNT_E_UNSUPPORTED_FORMAT,
};
use windows::Win32::System::Threading::{
    AvRevertMmThreadCharacteristics, AvSetMmThreadCharacteristicsW,
};
use windows::core::w;

use crate::format::{DeviceFormat, SampleContainer};
use crate::render::Renderer;
use crate::ring::SampleSource;
use crate::stream::{Stream, StreamOptions, StreamReport};
use crate::{COMMON_RATES, DeviceId, DeviceInfo, OutputBackend, OutputError};

/// Byte alignment for the device period; Intel HDA devices require 128.
const PERIOD_ALIGN_BYTES: u32 = 128;
/// How long the render thread waits for the queue to fill before starting the device.
const PREBUFFER_TIMEOUT: Duration = Duration::from_secs(2);
/// Event wait slice; also bounds how quickly a stop request is noticed.
const EVENT_WAIT_MS: u32 = 100;
/// Consecutive missed device events before the stream is declared dead.
const MAX_MISSED_EVENTS: u32 = 20;
/// Silent periods written after the last audio so the device plays it out before stopping.
const DRAIN_PERIODS: u32 = 2;

#[derive(Debug, Default)]
pub struct WasapiExclusive;

impl WasapiExclusive {
    pub fn new() -> Self {
        Self
    }
}

impl OutputBackend for WasapiExclusive {
    fn devices(&self) -> Result<Vec<DeviceInfo>, OutputError> {
        let _com = ComGuard::new();
        let enumerator = DeviceEnumerator::new().map_err(backend)?;
        let default_id = enumerator
            .get_default_device(Direction::Render)
            .and_then(|d| d.get_id())
            .ok();
        let collection = enumerator
            .get_device_collection(Direction::Render)
            .map_err(backend)?;

        let mut devices = Vec::new();
        for device in &collection {
            let device = device.map_err(backend)?;
            let id = device.get_id().map_err(backend)?;
            let name = device.get_friendlyname().unwrap_or_else(|_| id.clone());
            // A device that cannot be probed (in use, disabled exclusive mode) is still listed.
            let formats = probe_formats(&device, 2).unwrap_or_default();
            devices.push(DeviceInfo {
                is_default: default_id.as_deref() == Some(id.as_str()),
                id: DeviceId(id),
                name,
                formats,
            });
        }
        Ok(devices)
    }

    fn device_ids(&self) -> Result<Vec<DeviceId>, OutputError> {
        let _com = ComGuard::new();
        let collection = DeviceEnumerator::new()
            .map_err(backend)?
            .get_device_collection(Direction::Render)
            .map_err(backend)?;
        let mut ids = Vec::new();
        for device in &collection {
            ids.push(DeviceId(device.and_then(|d| d.get_id()).map_err(backend)?));
        }
        Ok(ids)
    }

    fn is_supported(&self, device: &DeviceId, format: DeviceFormat) -> Result<bool, OutputError> {
        let _com = ComGuard::new();
        let client = find_device(device)?.get_iaudioclient().map_err(backend)?;
        Ok(accepted_format(&client, format).is_some())
    }

    fn supported_formats(
        &self,
        device: &DeviceId,
        channels: u16,
    ) -> Result<Vec<DeviceFormat>, OutputError> {
        let _com = ComGuard::new();
        probe_formats(&find_device(device)?, channels)
    }

    fn open(
        &mut self,
        device: &DeviceId,
        format: DeviceFormat,
        source: SampleSource,
        options: StreamOptions,
    ) -> Result<Stream, OutputError> {
        if source.channels != format.channels {
            return Err(OutputError::Backend(format!(
                "queue carries {} channels but the device format has {}",
                source.channels, format.channels
            )));
        }
        let handle = source.handle.clone();
        let (ready_tx, ready_rx) = sync_channel(1);
        let device = device.clone();
        let thread = thread::Builder::new()
            .name("sonder-wasapi-render".into())
            .spawn(move || render_thread(device, format, source, options, ready_tx))
            .map_err(|e| OutputError::Backend(format!("cannot spawn render thread: {e}")))?;

        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Stream::new(handle, format, thread)),
            Ok(Err(e)) => {
                let _ = thread.join();
                Err(e)
            }
            Err(_) => Err(thread.join().map_or_else(
                |_| OutputError::Backend("render thread panicked during setup".into()),
                |r| {
                    r.err()
                        .unwrap_or_else(|| OutputError::Backend("setup aborted".into()))
                },
            )),
        }
    }
}

/// Initialises COM (multithreaded) for the current thread and undoes it on drop.
/// Declare it before any COM object so it is dropped last.
struct ComGuard(bool);

impl ComGuard {
    fn new() -> Self {
        Self(initialize_mta().is_ok())
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        if self.0 {
            deinitialize();
        }
    }
}

/// Registers the current thread with MMCSS "Pro Audio" and reverts on drop.
struct Mmcss(Option<HANDLE>);

impl Mmcss {
    fn enter() -> Self {
        let mut task_index = 0u32;
        // SAFETY: the task name is a static, NUL-terminated wide string and `task_index`
        // outlives the call.
        let handle = unsafe { AvSetMmThreadCharacteristicsW(w!("Pro Audio"), &mut task_index) };
        Self(handle.ok())
    }
}

impl Drop for Mmcss {
    fn drop(&mut self) {
        if let Some(handle) = self.0.take() {
            // SAFETY: `handle` came from a successful AvSetMmThreadCharacteristicsW call on this
            // thread and is reverted exactly once.
            let _ = unsafe { AvRevertMmThreadCharacteristics(handle) };
        }
    }
}

fn backend(err: impl std::fmt::Display) -> OutputError {
    OutputError::Backend(err.to_string())
}

/// Classifies a failure of a running stream: an invalidated device (unplugged, disabled, format
/// changed in the control panel, system sleep) is [`OutputError::DeviceLost`].
fn stream_error(err: WasapiError) -> OutputError {
    if let WasapiError::Windows(win) = &err
        && win.code() == AUDCLNT_E_DEVICE_INVALIDATED
    {
        return OutputError::DeviceLost(err.to_string());
    }
    backend(err)
}

fn find_device(id: &DeviceId) -> Result<Device, OutputError> {
    DeviceEnumerator::new()
        .map_err(backend)?
        .get_device(&id.0)
        .map_err(|_| OutputError::DeviceNotFound(id.0.clone()))
}

fn wave_format(format: DeviceFormat) -> WaveFormat {
    let (store_bits, valid_bits) = match format.container {
        SampleContainer::S16 => (16, 16),
        SampleContainer::S24Packed => (24, 24),
        SampleContainer::S24In32 => (32, 24),
        SampleContainer::S32 => (32, 32),
    };
    WaveFormat::new(
        store_bits,
        valid_bits,
        SampleType::Int,
        format.rate as usize,
        usize::from(format.channels),
        None,
    )
}

/// The exact `WaveFormat` the driver accepts for `format` in exclusive mode, if any.
/// Tries the channel-mask variants some drivers insist on.
fn accepted_format(client: &AudioClient, format: DeviceFormat) -> Option<WaveFormat> {
    client
        .is_supported_exclusive_with_quirks(&wave_format(format))
        .ok()
}

fn probe_formats(device: &Device, channels: u16) -> Result<Vec<DeviceFormat>, OutputError> {
    let client = device.get_iaudioclient().map_err(backend)?;
    let mut formats = Vec::new();
    for rate in COMMON_RATES {
        for container in SampleContainer::ALL {
            let format = DeviceFormat {
                rate,
                container,
                channels,
            };
            if accepted_format(&client, format).is_some() {
                formats.push(format);
            }
        }
    }
    Ok(formats)
}

fn init_error(err: WasapiError, device: &Device, format: DeviceFormat) -> OutputError {
    if let WasapiError::Windows(win) = &err {
        let code = win.code();
        if code == AUDCLNT_E_DEVICE_IN_USE {
            return OutputError::DeviceInUse;
        }
        if code == AUDCLNT_E_DEVICE_INVALIDATED {
            return OutputError::DeviceLost(err.to_string());
        }
        if code == AUDCLNT_E_EXCLUSIVE_MODE_NOT_ALLOWED {
            return OutputError::ExclusiveNotAllowed;
        }
        if code == AUDCLNT_E_UNSUPPORTED_FORMAT {
            return unsupported(device, format);
        }
    }
    OutputError::Backend(format!("IAudioClient::Initialize failed: {err}"))
}

fn unsupported(device: &Device, format: DeviceFormat) -> OutputError {
    OutputError::UnsupportedFormat {
        requested: format.to_string(),
        supported: probe_formats(device, format.channels).unwrap_or_default(),
    }
}

/// Opens `device` in event-driven exclusive mode at its default period, re-initialising once
/// with the driver's aligned buffer size if the first period is rejected as unaligned.
fn init_exclusive(
    device: &Device,
    format: DeviceFormat,
) -> Result<(AudioClient, WaveFormat), OutputError> {
    let mut client = device.get_iaudioclient().map_err(backend)?;
    let wave = accepted_format(&client, format).ok_or_else(|| unsupported(device, format))?;

    let (default_period, _min_period) = client.get_device_period().map_err(backend)?;
    let period = client
        .calculate_aligned_period_near(default_period, Some(PERIOD_ALIGN_BYTES), &wave)
        .map_err(backend)?;

    match client.initialize_client(
        &wave,
        Direction::Render,
        StreamMode::EventsExclusive { period_hns: period },
        None,
    ) {
        Ok(()) => Ok((client, wave)),
        Err(WasapiError::Windows(win)) if win.code() == AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED => {
            // Documented recovery: take the aligned size the failed client reports, then
            // initialise a fresh client with the matching period.
            let frames = client.get_buffer_size().map_err(backend)?;
            let aligned = calculate_period_100ns(i64::from(frames), i64::from(format.rate));
            let mut client = device.get_iaudioclient().map_err(backend)?;
            client
                .initialize_client(
                    &wave,
                    Direction::Render,
                    StreamMode::EventsExclusive {
                        period_hns: aligned,
                    },
                    None,
                )
                .map_err(|e| init_error(e, device, format))?;
            Ok((client, wave))
        }
        Err(e) => Err(init_error(e, device, format)),
    }
}

fn render_thread(
    device_id: DeviceId,
    format: DeviceFormat,
    source: SampleSource,
    options: StreamOptions,
    ready: SyncSender<Result<(), OutputError>>,
) -> Result<StreamReport, OutputError> {
    let handle = source.handle.clone();
    let result = run_stream(device_id, format, source, options, ready);
    // Whatever happened, release a producer that may be blocked on a full queue.
    handle.stop();
    result.map(|()| StreamReport {
        frames_written: handle.frames_written(),
        underruns: handle.underruns(),
    })
}

fn run_stream(
    device_id: DeviceId,
    format: DeviceFormat,
    source: SampleSource,
    options: StreamOptions,
    ready: SyncSender<Result<(), OutputError>>,
) -> Result<(), OutputError> {
    let _com = ComGuard::new();

    let setup = (|| {
        let device = find_device(&device_id)?;
        let (client, _wave) = init_exclusive(&device, format)?;
        let event = client.set_get_eventhandle().map_err(backend)?;
        let render_client = client.get_audiorenderclient().map_err(backend)?;
        let buffer_frames = client.get_buffer_size().map_err(backend)? as usize;
        Ok((client, event, render_client, buffer_frames))
    })();
    let (client, event, render_client, buffer_frames) = match setup {
        Ok(parts) => {
            let _ = ready.send(Ok(()));
            parts
        }
        Err(e) => {
            // `open` receives the error and discards this thread's own result.
            let _ = ready.send(Err(e));
            return Ok(());
        }
    };

    let mut renderer = Renderer::new(source, format, buffer_frames, options.tap);
    let _mmcss = Mmcss::enter();

    // Let the decoder get ahead before the device clock starts.
    let target = renderer
        .queue_capacity()
        .min(buffer_frames * usize::from(format.channels) * 4);
    let started = Instant::now();
    while renderer.queued_samples() < target
        && !renderer.input_finished()
        && !renderer.handle().is_stopping()
        && started.elapsed() < PREBUFFER_TIMEOUT
    {
        // A seek during prebuffering must not wait for the first device period.
        renderer.service_flush();
        thread::sleep(Duration::from_millis(1));
    }

    let result = (|| {
        // Fill the whole buffer before starting so playback opens with real audio.
        let frames = client
            .get_available_space_in_frames()
            .map_err(stream_error)? as usize;
        let mut exhausted = renderer.fill(frames).input_exhausted;
        render_client
            .write_to_device(renderer.bytes(frames), None)
            .map_err(stream_error)?;
        client.start_stream().map_err(stream_error)?;

        let mut drain_left = DRAIN_PERIODS;
        let mut missed = 0;
        while !renderer.handle().is_stopping() {
            if event.wait_for_event(EVENT_WAIT_MS).is_err() {
                missed += 1;
                if missed > MAX_MISSED_EVENTS {
                    // Seen after system sleep and on some driver resets.
                    return Err(OutputError::DeviceLost(
                        "the device stopped requesting audio".into(),
                    ));
                }
                continue;
            }
            missed = 0;

            let frames = client
                .get_available_space_in_frames()
                .map_err(stream_error)? as usize;
            let frames = frames.min(buffer_frames);
            if frames == 0 {
                continue;
            }
            if exhausted {
                if drain_left == 0 {
                    break;
                }
                drain_left -= 1;
                renderer.silence(frames);
            } else {
                exhausted = renderer.fill(frames).input_exhausted;
            }
            render_client
                .write_to_device(renderer.bytes(frames), None)
                .map_err(stream_error)?;
        }
        Ok(())
    })();

    let _ = client.stop_stream();
    renderer.finish();
    // A mid-stream failure surfaces through `Stream::wait`.
    result
}
