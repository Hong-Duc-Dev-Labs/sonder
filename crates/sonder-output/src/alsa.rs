//! ALSA backend writing straight to `hw:CARD,DEV`: exact hardware parameters, no `plug` layer,
//! no software rate or format conversion.
//!
//! Container mapping: S16 → `S16_LE`, 24-bit packed → `S24_3LE`, 32-bit → `S32_LE`. ALSA's
//! `S24_LE` is LSB-aligned, unlike [`SampleContainer::S24In32`], so that container is not
//! offered; 24-bit sources fall back to `S32_LE`, which carries them unchanged.

use std::sync::mpsc::{SyncSender, sync_channel};
use std::thread;
use std::time::{Duration, Instant};

use ::alsa::pcm::{Access, Format, HwParams, PCM};
use ::alsa::{Ctl, Direction, ValueOr};

use crate::format::{DeviceFormat, SampleContainer};
use crate::render::Renderer;
use crate::ring::SampleSource;
use crate::stream::{Stream, StreamOptions, StreamReport};
use crate::{COMMON_RATES, DeviceId, DeviceInfo, OutputBackend, OutputError};

/// Device buffer and period targets; the driver picks the nearest it supports.
const BUFFER_US: u32 = 100_000;
const PERIOD_US: u32 = 10_000;
/// How long the render thread waits for the queue to fill before starting the device.
const PREBUFFER_TIMEOUT: Duration = Duration::from_secs(2);
/// Longest wait for buffer space before re-checking for a stop request.
const WAIT_MS: u32 = 100;
/// Upper bound on playing out the device buffer at the end of input.
const DRAIN_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Default)]
pub struct AlsaHw;

impl AlsaHw {
    pub fn new() -> Self {
        Self
    }
}

fn alsa_format(container: SampleContainer) -> Option<Format> {
    match container {
        SampleContainer::S16 => Some(Format::S16LE),
        SampleContainer::S24Packed => Some(Format::S243LE),
        SampleContainer::S32 => Some(Format::S32LE),
        SampleContainer::S24In32 => None,
    }
}

/// Maps an ALSA error to the shared error policy.
fn classify(err: &::alsa::Error, device: &DeviceId) -> OutputError {
    match err.errno() {
        libc::EBUSY => OutputError::DeviceInUse,
        libc::ENOENT | libc::ENXIO => OutputError::DeviceNotFound(device.0.clone()),
        libc::ENODEV => OutputError::DeviceLost(err.to_string()),
        _ => OutputError::Backend(err.to_string()),
    }
}

fn open_pcm(device: &DeviceId, nonblock: bool) -> Result<PCM, OutputError> {
    PCM::new(&device.0, Direction::Playback, nonblock).map_err(|e| classify(&e, device))
}

/// Applies `format` exactly to `pcm`'s hardware parameters. Nothing is committed unless
/// `commit` is set, so this doubles as a support probe.
fn set_exact(pcm: &PCM, format: DeviceFormat, commit: bool) -> Result<(), ::alsa::Error> {
    let Some(sample) = alsa_format(format.container) else {
        return Err(::alsa::Error::unsupported("S24_LE is LSB-aligned"));
    };
    let hwp = HwParams::any(pcm)?;
    hwp.set_rate_resample(false)?;
    hwp.set_access(Access::RWInterleaved)?;
    hwp.set_format(sample)?;
    hwp.set_channels(u32::from(format.channels))?;
    hwp.set_rate(format.rate, ValueOr::Nearest)?;
    if hwp.get_rate()? != format.rate {
        return Err(::alsa::Error::unsupported("rate"));
    }
    if commit {
        hwp.set_buffer_time_near(BUFFER_US, ValueOr::Nearest)?;
        hwp.set_period_time_near(PERIOD_US, ValueOr::Nearest)?;
        pcm.hw_params(&hwp)?;
    }
    Ok(())
}

fn probe(device: &DeviceId, channels: u16) -> Result<Vec<DeviceFormat>, OutputError> {
    let pcm = open_pcm(device, true)?;
    let mut formats = Vec::new();
    for rate in COMMON_RATES {
        for container in SampleContainer::ALL {
            let format = DeviceFormat {
                rate,
                container,
                channels,
            };
            if set_exact(&pcm, format, false).is_ok() {
                formats.push(format);
            }
        }
    }
    Ok(formats)
}

/// Every playback PCM device as `(hw:CARD,DEV, display name)`.
fn hw_devices() -> Result<Vec<(DeviceId, String)>, OutputError> {
    let mut out = Vec::new();
    for card in ::alsa::card::Iter::new() {
        let card = card.map_err(|e| OutputError::Backend(e.to_string()))?;
        let Ok(ctl) = Ctl::from_card(&card, false) else {
            continue;
        };
        let card_name = card.get_name().unwrap_or_default();
        for dev in ::alsa::ctl::DeviceIter::new(&ctl) {
            let Ok(dev) = u32::try_from(dev) else {
                continue;
            };
            let Ok(info) = ctl.pcm_info(dev, 0, Direction::Playback) else {
                continue; // capture-only device
            };
            let pcm_name = info.get_name().unwrap_or_default().to_string();
            out.push((
                DeviceId(format!("hw:{},{dev}", card.get_index())),
                format!("{card_name}: {pcm_name}"),
            ));
        }
    }
    Ok(out)
}

impl OutputBackend for AlsaHw {
    fn devices(&self) -> Result<Vec<DeviceInfo>, OutputError> {
        let devices = hw_devices()?;
        Ok(devices
            .into_iter()
            .enumerate()
            .map(|(i, (id, name))| DeviceInfo {
                // A device in use is still listed, just without formats.
                formats: probe(&id, 2).unwrap_or_default(),
                is_default: i == 0,
                id,
                name,
            })
            .collect())
    }

    fn device_ids(&self) -> Result<Vec<DeviceId>, OutputError> {
        Ok(hw_devices()?.into_iter().map(|(id, _)| id).collect())
    }

    fn is_supported(&self, device: &DeviceId, format: DeviceFormat) -> Result<bool, OutputError> {
        let pcm = open_pcm(device, true)?;
        Ok(set_exact(&pcm, format, false).is_ok())
    }

    fn supported_formats(
        &self,
        device: &DeviceId,
        channels: u16,
    ) -> Result<Vec<DeviceFormat>, OutputError> {
        probe(device, channels)
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
            .name("sonder-alsa-render".into())
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

fn render_thread(
    device: DeviceId,
    format: DeviceFormat,
    source: SampleSource,
    options: StreamOptions,
    ready: SyncSender<Result<(), OutputError>>,
) -> Result<StreamReport, OutputError> {
    let handle = source.handle.clone();
    let result = run_stream(&device, format, source, options, ready);
    handle.stop();
    result.map(|()| StreamReport {
        frames_written: handle.frames_written(),
        underruns: handle.underruns(),
    })
}

fn run_stream(
    device: &DeviceId,
    format: DeviceFormat,
    source: SampleSource,
    options: StreamOptions,
    ready: SyncSender<Result<(), OutputError>>,
) -> Result<(), OutputError> {
    let setup = (|| {
        // Non-blocking: a blocking open of a busy `hw:` device waits in the kernel until the
        // other application lets go, instead of failing with EBUSY. Writes below wait on
        // EAGAIN themselves, which also keeps stop requests responsive.
        let pcm = open_pcm(device, true)?;
        set_exact(&pcm, format, true).map_err(|e| match classify(&e, device) {
            OutputError::Backend(_) => OutputError::UnsupportedFormat {
                requested: format.to_string(),
                supported: Vec::new(),
            },
            other => other,
        })?;
        let period = pcm
            .hw_params_current()
            .and_then(|h| h.get_period_size())
            .map_err(|e| classify(&e, device))?;
        let period = usize::try_from(period).unwrap_or(0).max(64);
        Ok((pcm, period))
    })();
    let (pcm, period) = match setup {
        Ok(parts) => {
            let _ = ready.send(Ok(()));
            parts
        }
        Err(e) => {
            let _ = ready.send(Err(e));
            return Ok(());
        }
    };

    let mut renderer = Renderer::new(source, format, period, options.tap);
    let target = renderer
        .queue_capacity()
        .min(period * usize::from(format.channels) * 4);
    let started = Instant::now();
    while renderer.queued_samples() < target
        && !renderer.input_finished()
        && !renderer.handle().is_stopping()
        && started.elapsed() < PREBUFFER_TIMEOUT
    {
        renderer.service_flush();
        thread::sleep(Duration::from_millis(1));
    }

    let io = pcm.io_bytes();
    let result = (|| {
        while !renderer.handle().is_stopping() {
            let fill = renderer.fill(period);
            // Write the whole period, silence included, so pause keeps the clock running.
            let mut bytes = renderer.bytes(period);
            while !bytes.is_empty() {
                if renderer.handle().is_stopping() {
                    return Ok(());
                }
                match io.writei(bytes) {
                    Ok(frames) => bytes = &bytes[frames * format.frame_bytes()..],
                    Err(e) if e.errno() == libc::EAGAIN => {
                        // Device buffer full: sleep until a period frees up.
                        // An xrun or unplug surfaces here too; the next `writei` reports and
                        // classifies it, so the wait result itself is not needed.
                        let _ = pcm.wait(Some(WAIT_MS));
                    }
                    Err(e) if e.errno() == libc::EPIPE => {
                        // Underrun: recover in place and keep the data we were writing.
                        renderer.note_xrun();
                        pcm.prepare().map_err(|e| classify(&e, device))?;
                    }
                    Err(e) if e.errno() == libc::ESTRPIPE => {
                        // System suspend: resume or re-prepare.
                        renderer.note_xrun();
                        pcm.try_recover(e, true)
                            .map_err(|e| OutputError::DeviceLost(e.to_string()))?;
                    }
                    Err(e) if e.errno() == libc::ENODEV => {
                        return Err(OutputError::DeviceLost(e.to_string()));
                    }
                    Err(e) => return Err(classify(&e, device)),
                }
            }
            if fill.input_exhausted {
                // Let the device play everything out before closing. `drain` would return
                // EAGAIN in non-blocking mode, so wait for the queued delay to reach zero.
                let started = Instant::now();
                while !renderer.handle().is_stopping() && started.elapsed() < DRAIN_TIMEOUT {
                    match pcm.avail_delay() {
                        Ok((_, delay)) if delay > 0 => thread::sleep(Duration::from_millis(5)),
                        _ => break,
                    }
                }
                break;
            }
        }
        Ok(())
    })();
    if renderer.handle().is_stopping() {
        let _ = pcm.drop();
    }
    renderer.finish();
    result
}
