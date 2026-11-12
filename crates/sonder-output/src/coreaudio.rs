//! CoreAudio backend in hog mode with the integer physical format set to the source format.
//!
//! Hog mode gives Sonder exclusive use of the device (no system mixer, no other app). The
//! stream's *physical* format is set to the exact integer layout, so the DAC runs at the
//! source rate and depth. Since macOS 10.6 the IOProc always exchanges Float32 with the HAL;
//! a left-justified sample with at most 24 significant bits is exact in Float32 and the HAL's
//! float-to-integer conversion returns it unchanged, so 16- and 24-bit sources stay
//! bit-perfect. If a driver still exposes an integer *virtual* format matching the physical
//! one, bytes are written directly instead.
//!
//! Hog mode and the original physical format are restored when the stream ends, including
//! on panic (drop guard plus a process-wide panic hook).

use std::ffi::c_void;
use std::mem::{MaybeUninit, size_of};
use std::ptr::{NonNull, null};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{SyncSender, sync_channel};
use std::sync::{Arc, Mutex, Once};
use std::thread;
use std::time::{Duration, Instant};

use objc2_core_audio::{
    AudioDeviceCreateIOProcID, AudioDeviceDestroyIOProcID, AudioDeviceIOProcID, AudioDeviceStart,
    AudioDeviceStop, AudioObjectGetPropertyData, AudioObjectGetPropertyDataSize, AudioObjectID,
    AudioObjectPropertyAddress, AudioObjectPropertyScope, AudioObjectPropertySelector,
    AudioObjectSetPropertyData, AudioStreamRangedDescription, kAudioDevicePropertyBufferFrameSize,
    kAudioDevicePropertyDeviceIsAlive, kAudioDevicePropertyDeviceUID, kAudioDevicePropertyHogMode,
    kAudioDevicePropertyStreams, kAudioHardwarePropertyDefaultOutputDevice,
    kAudioHardwarePropertyDevices, kAudioObjectPropertyElementMain, kAudioObjectPropertyName,
    kAudioObjectPropertyScopeGlobal, kAudioObjectPropertyScopeOutput, kAudioObjectSystemObject,
    kAudioStreamPropertyAvailablePhysicalFormats, kAudioStreamPropertyPhysicalFormat,
    kAudioStreamPropertyVirtualFormat,
};
use objc2_core_audio_types::{
    AudioBufferList, AudioStreamBasicDescription, AudioTimeStamp, kAudioFormatFlagIsAlignedHigh,
    kAudioFormatFlagIsBigEndian, kAudioFormatFlagIsFloat, kAudioFormatFlagIsNonInterleaved,
    kAudioFormatFlagIsPacked, kAudioFormatFlagIsSignedInteger, kAudioFormatLinearPCM,
};
use objc2_core_foundation::{CFRetained, CFString};

use crate::format::{DeviceFormat, SampleContainer, pack, unpack};
use crate::render::Renderer;
use crate::ring::SampleSource;
use crate::stream::{Stream, StreamOptions, StreamReport, WriteTap};
use crate::{COMMON_RATES, DeviceId, DeviceInfo, OutputBackend, OutputError};

type OSStatus = i32;

/// Frames rendered per IOProc chunk; larger device buffers are filled in several chunks.
const MAX_CHUNK_FRAMES: usize = 4096;
/// Requested IO buffer size in frames.
const IO_BUFFER_FRAMES: u32 = 512;
/// Supervisor poll interval: stop requests, end of input, device liveness.
const POLL: Duration = Duration::from_millis(5);
const PREBUFFER_TIMEOUT: Duration = Duration::from_secs(2);
/// After the last audio frame was handed over, keep running this long so it is played out.
const DRAIN: Duration = Duration::from_millis(200);
/// IO cycles of DSD idle handed to the device after a stop before it is halted.
const DSD_STOP_IDLE_CYCLES: u32 = 2;
/// Upper bound on waiting for those cycles.
const DSD_STOP_IDLE_TIMEOUT: Duration = Duration::from_millis(200);
/// `kAudioDevicePropertyHogMode` value meaning nobody holds the device.
const HOG_FREE: i32 = -1;

#[derive(Debug, Default)]
pub struct CoreAudioHog;

impl CoreAudioHog {
    pub fn new() -> Self {
        Self
    }
}

// ----- property helpers -----

fn address(
    selector: AudioObjectPropertySelector,
    scope: AudioObjectPropertyScope,
) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress {
        mSelector: selector,
        mScope: scope,
        mElement: kAudioObjectPropertyElementMain,
    }
}

fn status(what: &str, code: OSStatus) -> Result<(), OutputError> {
    if code == 0 {
        Ok(())
    } else {
        Err(OutputError::Backend(format!(
            "{what} failed (OSStatus {code})"
        )))
    }
}

/// Reads a fixed-size property.
///
/// # Safety
/// `T` must be the property's C type.
unsafe fn get<T: Copy>(
    object: AudioObjectID,
    addr: AudioObjectPropertyAddress,
) -> Result<T, OutputError> {
    let mut value = MaybeUninit::<T>::zeroed();
    let mut size = size_of::<T>() as u32;
    // SAFETY: `value` has room for `size` bytes and outlives the call; the caller vouches
    // that `T` is the property's type.
    let code = unsafe {
        AudioObjectGetPropertyData(
            object,
            NonNull::from(&addr),
            0,
            null(),
            NonNull::from(&mut size),
            NonNull::new_unchecked(value.as_mut_ptr().cast()),
        )
    };
    status("AudioObjectGetPropertyData", code)?;
    // SAFETY: the HAL wrote the value (zero-initialised otherwise, valid for plain C types).
    Ok(unsafe { value.assume_init() })
}

/// Reads an array property.
///
/// # Safety
/// `T` must be the property's element C type.
unsafe fn get_vec<T: Copy>(
    object: AudioObjectID,
    addr: AudioObjectPropertyAddress,
) -> Result<Vec<T>, OutputError> {
    let mut size = 0u32;
    // SAFETY: plain out-parameter.
    let code = unsafe {
        AudioObjectGetPropertyDataSize(
            object,
            NonNull::from(&addr),
            0,
            null(),
            NonNull::from(&mut size),
        )
    };
    status("AudioObjectGetPropertyDataSize", code)?;
    let count = size as usize / size_of::<T>();
    let mut items: Vec<T> = Vec::with_capacity(count);
    if count == 0 {
        return Ok(items);
    }
    let mut size = (count * size_of::<T>()) as u32;
    // SAFETY: `items` has capacity for `size` bytes; the HAL reports how much it wrote.
    let code = unsafe {
        AudioObjectGetPropertyData(
            object,
            NonNull::from(&addr),
            0,
            null(),
            NonNull::from(&mut size),
            NonNull::new_unchecked(items.as_mut_ptr().cast()),
        )
    };
    status("AudioObjectGetPropertyData", code)?;
    // SAFETY: the HAL initialised `size` bytes of whole elements.
    unsafe { items.set_len(size as usize / size_of::<T>()) };
    Ok(items)
}

/// Writes a fixed-size property.
///
/// # Safety
/// `T` must be the property's C type.
unsafe fn set<T: Copy>(
    object: AudioObjectID,
    addr: AudioObjectPropertyAddress,
    value: &T,
) -> Result<(), OutputError> {
    // SAFETY: `value` is a valid `T` for the duration of the call.
    let code = unsafe {
        AudioObjectSetPropertyData(
            object,
            NonNull::from(&addr),
            0,
            null(),
            size_of::<T>() as u32,
            NonNull::from(value).cast(),
        )
    };
    status("AudioObjectSetPropertyData", code)
}

fn get_string(object: AudioObjectID, selector: AudioObjectPropertySelector) -> Option<String> {
    // SAFETY: string properties are a retained `CFStringRef`.
    let raw: *const CFString =
        unsafe { get(object, address(selector, kAudioObjectPropertyScopeGlobal)) }.ok()?;
    let raw = NonNull::new(raw.cast_mut())?;
    // SAFETY: the HAL returns a +1 reference that we now own.
    let string = unsafe { CFRetained::from_raw(raw) };
    Some(string.to_string())
}

// ----- device discovery -----

fn all_devices() -> Result<Vec<AudioObjectID>, OutputError> {
    // SAFETY: the device list is an array of `AudioObjectID`.
    unsafe {
        get_vec(
            kAudioObjectSystemObject as AudioObjectID,
            address(
                kAudioHardwarePropertyDevices,
                kAudioObjectPropertyScopeGlobal,
            ),
        )
    }
}

fn output_streams(device: AudioObjectID) -> Vec<AudioObjectID> {
    // SAFETY: the stream list is an array of `AudioObjectID`.
    unsafe {
        get_vec(
            device,
            address(kAudioDevicePropertyStreams, kAudioObjectPropertyScopeOutput),
        )
    }
    .unwrap_or_default()
}

/// Output devices as `(object id, UID)`. The UID is stable across reconnects, so it is the
/// [`DeviceId`]; the object id is not.
fn output_devices() -> Result<Vec<(AudioObjectID, String)>, OutputError> {
    Ok(all_devices()?
        .into_iter()
        .filter(|&d| !output_streams(d).is_empty())
        .filter_map(|d| get_string(d, kAudioDevicePropertyDeviceUID).map(|uid| (d, uid)))
        .collect())
}

fn find_device(id: &DeviceId) -> Result<AudioObjectID, OutputError> {
    output_devices()?
        .into_iter()
        .find(|(_, uid)| *uid == id.0)
        .map(|(d, _)| d)
        .ok_or_else(|| OutputError::DeviceNotFound(id.0.clone()))
}

fn first_output_stream(device: AudioObjectID) -> Result<AudioObjectID, OutputError> {
    output_streams(device)
        .first()
        .copied()
        .ok_or_else(|| OutputError::Backend("device has no output stream".into()))
}

// ----- formats -----

/// The physical stream description for `format`.
fn asbd(format: DeviceFormat) -> Option<AudioStreamBasicDescription> {
    let (bits, bytes, layout) = match format.container {
        SampleContainer::S16 => (16, 2, kAudioFormatFlagIsPacked),
        SampleContainer::S24Packed => (24, 3, kAudioFormatFlagIsPacked),
        SampleContainer::S24In32 => (24, 4, kAudioFormatFlagIsAlignedHigh),
        SampleContainer::S32 => (32, 4, kAudioFormatFlagIsPacked),
        // CoreAudio has no native-DSD sample format; DoP is the only DSD path here.
        SampleContainer::DsdU32Be => return None,
    };
    let frame = bytes * u32::from(format.channels);
    Some(AudioStreamBasicDescription {
        mSampleRate: f64::from(format.rate),
        mFormatID: kAudioFormatLinearPCM,
        mFormatFlags: kAudioFormatFlagIsSignedInteger | layout,
        mBytesPerPacket: frame,
        mFramesPerPacket: 1,
        mBytesPerFrame: frame,
        mChannelsPerFrame: u32::from(format.channels),
        mBitsPerChannel: bits,
        mReserved: 0,
    })
}

/// Whether `desc` is the same integer layout as `want`, ignoring the rate.
fn same_layout(desc: &AudioStreamBasicDescription, want: &AudioStreamBasicDescription) -> bool {
    const SIGNIFICANT: u32 = kAudioFormatFlagIsFloat
        | kAudioFormatFlagIsBigEndian
        | kAudioFormatFlagIsSignedInteger
        | kAudioFormatFlagIsNonInterleaved;
    let aligned_high = |d: &AudioStreamBasicDescription| {
        // Only meaningful when the bits do not fill the container.
        d.mBitsPerChannel * d.mChannelsPerFrame != d.mBytesPerFrame * 8
            && d.mFormatFlags & kAudioFormatFlagIsAlignedHigh != 0
    };
    desc.mFormatID == kAudioFormatLinearPCM
        && desc.mFormatFlags & SIGNIFICANT == want.mFormatFlags & SIGNIFICANT
        && desc.mBitsPerChannel == want.mBitsPerChannel
        && desc.mBytesPerFrame == want.mBytesPerFrame
        && desc.mChannelsPerFrame == want.mChannelsPerFrame
        && aligned_high(desc) == aligned_high(want)
}

fn physical_formats(stream: AudioObjectID) -> Vec<AudioStreamRangedDescription> {
    // SAFETY: the property is an array of `AudioStreamRangedDescription`.
    unsafe {
        get_vec(
            stream,
            address(
                kAudioStreamPropertyAvailablePhysicalFormats,
                kAudioObjectPropertyScopeGlobal,
            ),
        )
    }
    .unwrap_or_default()
}

fn stream_supports(stream: AudioObjectID, format: DeviceFormat) -> bool {
    let Some(want) = asbd(format) else {
        return false;
    };
    let rate = f64::from(format.rate);
    physical_formats(stream).iter().any(|r| {
        same_layout(&r.mFormat, &want)
            && r.mSampleRateRange.mMinimum <= rate
            && rate <= r.mSampleRateRange.mMaximum
    })
}

fn probe(device: AudioObjectID, channels: u16) -> Result<Vec<DeviceFormat>, OutputError> {
    let stream = first_output_stream(device)?;
    let mut formats = Vec::new();
    for rate in COMMON_RATES {
        for container in SampleContainer::ALL {
            let format = DeviceFormat {
                rate,
                container,
                channels,
            };
            if stream_supports(stream, format) {
                formats.push(format);
            }
        }
    }
    Ok(formats)
}

// ----- hog mode -----

/// Devices currently hogged by this process, for the panic hook.
static HOGGED: Mutex<Vec<HogRecord>> = Mutex::new(Vec::new());
static PANIC_HOOK: Once = Once::new();

#[derive(Clone, Copy)]
struct HogRecord {
    device: AudioObjectID,
    stream: AudioObjectID,
    original: AudioStreamBasicDescription,
}

impl HogRecord {
    fn release(&self) {
        // SAFETY: physical format is an ASBD and hog mode a `pid_t`.
        unsafe {
            let _ = set(
                self.stream,
                address(
                    kAudioStreamPropertyPhysicalFormat,
                    kAudioObjectPropertyScopeGlobal,
                ),
                &self.original,
            );
            let _ = set(
                self.device,
                address(kAudioDevicePropertyHogMode, kAudioObjectPropertyScopeGlobal),
                &HOG_FREE,
            );
        }
    }
}

/// Releases every hogged device if the process panics, then runs the previous hook.
fn install_panic_hook() {
    PANIC_HOOK.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            // `try_lock`: the panic may have happened while the list was locked.
            if let Ok(records) = HOGGED.try_lock() {
                for record in records.iter() {
                    record.release();
                }
            }
            previous(info);
        }));
    });
}

/// Exclusive access plus the physical format change; undone on drop.
struct HogGuard(HogRecord);

impl HogGuard {
    fn acquire(device: AudioObjectID, format: DeviceFormat) -> Result<Self, OutputError> {
        install_panic_hook();
        let hog_addr = address(kAudioDevicePropertyHogMode, kAudioObjectPropertyScopeGlobal);
        let pid = std::process::id() as i32;
        // SAFETY: hog mode is a `pid_t`.
        let owner: i32 = unsafe { get(device, hog_addr) }?;
        if owner != HOG_FREE && owner != pid {
            return Err(OutputError::DeviceInUse);
        }
        let stream = first_output_stream(device)?;
        let phys_addr = address(
            kAudioStreamPropertyPhysicalFormat,
            kAudioObjectPropertyScopeGlobal,
        );
        // SAFETY: the physical format is an ASBD.
        let original: AudioStreamBasicDescription = unsafe { get(stream, phys_addr) }?;
        // SAFETY: as above.
        unsafe { set(device, hog_addr, &pid) }.map_err(|_| OutputError::DeviceInUse)?;
        let guard = Self(HogRecord {
            device,
            stream,
            original,
        });
        if let Ok(mut records) = HOGGED.lock() {
            records.push(guard.0);
        }

        if !stream_supports(stream, format) {
            return Err(unsupported(device, format));
        }
        let want = asbd(format).expect("stream_supports checked the layout");
        // SAFETY: the physical format is an ASBD.
        unsafe { set(stream, phys_addr, &want) }.map_err(|_| unsupported(device, format))?;
        // The change is asynchronous; wait until the device reports it.
        let started = Instant::now();
        loop {
            // SAFETY: as above.
            let now: AudioStreamBasicDescription = unsafe { get(stream, phys_addr) }?;
            if same_layout(&now, &want) && now.mSampleRate == want.mSampleRate {
                break;
            }
            if started.elapsed() > Duration::from_secs(2) {
                return Err(unsupported(device, format));
            }
            thread::sleep(POLL);
        }
        Ok(guard)
    }
}

impl Drop for HogGuard {
    fn drop(&mut self) {
        self.0.release();
        if let Ok(mut records) = HOGGED.lock() {
            records.retain(|r| r.device != self.0.device);
        }
    }
}

fn unsupported(device: AudioObjectID, format: DeviceFormat) -> OutputError {
    OutputError::UnsupportedFormat {
        requested: format.to_string(),
        supported: probe(device, format.channels).unwrap_or_default(),
    }
}

fn is_alive(device: AudioObjectID) -> bool {
    // SAFETY: the property is a `UInt32` flag.
    unsafe {
        get::<u32>(
            device,
            address(
                kAudioDevicePropertyDeviceIsAlive,
                kAudioObjectPropertyScopeGlobal,
            ),
        )
    }
    .is_ok_and(|alive| alive != 0)
}

// ----- backend -----

impl OutputBackend for CoreAudioHog {
    fn devices(&self) -> Result<Vec<DeviceInfo>, OutputError> {
        // SAFETY: the default device is an `AudioObjectID`.
        let default: Option<AudioObjectID> = unsafe {
            get(
                kAudioObjectSystemObject as AudioObjectID,
                address(
                    kAudioHardwarePropertyDefaultOutputDevice,
                    kAudioObjectPropertyScopeGlobal,
                ),
            )
        }
        .ok();
        Ok(output_devices()?
            .into_iter()
            .map(|(device, uid)| DeviceInfo {
                name: get_string(device, kAudioObjectPropertyName).unwrap_or_else(|| uid.clone()),
                is_default: default == Some(device),
                formats: probe(device, 2).unwrap_or_default(),
                id: DeviceId(uid),
            })
            .collect())
    }

    fn device_ids(&self) -> Result<Vec<DeviceId>, OutputError> {
        Ok(output_devices()?
            .into_iter()
            .map(|(_, uid)| DeviceId(uid))
            .collect())
    }

    fn is_supported(&self, device: &DeviceId, format: DeviceFormat) -> Result<bool, OutputError> {
        let device = find_device(device)?;
        Ok(stream_supports(first_output_stream(device)?, format))
    }

    fn supported_formats(
        &self,
        device: &DeviceId,
        channels: u16,
    ) -> Result<Vec<DeviceFormat>, OutputError> {
        probe(find_device(device)?, channels)
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
            .name("sonder-coreaudio".into())
            .spawn(move || supervise(device, format, source, options, ready_tx))
            .map_err(|e| OutputError::Backend(format!("cannot spawn stream thread: {e}")))?;
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Stream::new(handle, format, thread)),
            Ok(Err(e)) => {
                let _ = thread.join();
                Err(e)
            }
            Err(_) => Err(thread.join().map_or_else(
                |_| OutputError::Backend("stream thread panicked during setup".into()),
                |r| {
                    r.err()
                        .unwrap_or_else(|| OutputError::Backend("setup aborted".into()))
                },
            )),
        }
    }
}

/// State shared with the IOProc, which CoreAudio calls on its own real-time thread.
struct IoState {
    renderer: Renderer,
    /// The HAL wants Float32; the renderer then packs into S32 and the IOProc converts.
    float_out: bool,
    channels: usize,
    device_frame_bytes: usize,
    exhausted: Arc<AtomicBool>,
    /// Set by the supervisor when a DSD stream stops: the IOProc then writes only DSD idle
    /// (continuing the DoP marker phase) and counts the cycles in `idle_cycles`.
    idle_only: Arc<AtomicBool>,
    idle_cycles: Arc<AtomicU32>,
}

/// Converts left-justified `i32` LE words in `buf` to native-endian Float32 in place.
fn i32_to_f32_in_place(buf: &mut [u8]) {
    for s in buf.as_chunks_mut::<4>().0 {
        let v = i32::from_le_bytes(*s);
        *s = (v as f32 / 2_147_483_648.0).to_ne_bytes();
    }
}

/// The IOProc: fills the first output buffer from the ring. Real-time safe.
unsafe extern "C-unwind" fn io_proc(
    _device: AudioObjectID,
    _now: NonNull<AudioTimeStamp>,
    _input: NonNull<AudioBufferList>,
    _input_time: NonNull<AudioTimeStamp>,
    output: NonNull<AudioBufferList>,
    _output_time: NonNull<AudioTimeStamp>,
    client: *mut c_void,
) -> OSStatus {
    // SAFETY: `client` is the `IoState` registered with this IOProc; the HAL calls the IOProc
    // serially and never after `AudioDeviceStop` returned, so this is the only reference.
    let state = unsafe { &mut *client.cast::<IoState>() };
    // SAFETY: the HAL passes a valid buffer list for this cycle.
    let list = unsafe { &mut *output.as_ptr() };
    if list.mNumberBuffers == 0 || list.mBuffers[0].mData.is_null() {
        return 0;
    }
    let buffer = list.mBuffers[0];
    // SAFETY: `mData` points to `mDataByteSize` writable bytes for this cycle.
    let dst = unsafe {
        std::slice::from_raw_parts_mut(buffer.mData.cast::<u8>(), buffer.mDataByteSize as usize)
    };
    if buffer.mNumberChannels as usize != state.channels {
        // Not our layout; play nothing this cycle. A DSD stream gets DSD idle (DoP samples
        // are 4 bytes in both the integer and the Float32 layout), never zeros, which would
        // drop the DAC out of DSD mode.
        if state.renderer.is_dsd() {
            state
                .renderer
                .idle_into(buffer.mNumberChannels as usize, dst);
            if state.float_out {
                i32_to_f32_in_place(dst);
            }
        } else {
            dst.fill(0);
        }
        return 0;
    }
    let idle = state.idle_only.load(Ordering::Acquire);
    let total = dst.len() / state.device_frame_bytes;
    let mut done = 0;
    while done < total {
        let frames = (total - done).min(MAX_CHUNK_FRAMES);
        let exhausted = if idle {
            state.renderer.silence(frames);
            false
        } else {
            state.renderer.fill(frames).input_exhausted
        };
        let src = state.renderer.bytes(frames);
        let out =
            &mut dst[done * state.device_frame_bytes..(done + frames) * state.device_frame_bytes];
        out.copy_from_slice(src);
        if state.float_out {
            i32_to_f32_in_place(out);
        }
        if exhausted {
            state.exhausted.store(true, Ordering::Release);
        }
        done += frames;
    }
    if idle {
        state.idle_cycles.fetch_add(1, Ordering::Release);
    }
    0
}

fn supervise(
    device_id: DeviceId,
    format: DeviceFormat,
    source: SampleSource,
    options: StreamOptions,
    ready: SyncSender<Result<(), OutputError>>,
) -> Result<StreamReport, OutputError> {
    let handle = source.handle.clone();
    let result = run_stream(&device_id, format, source, options, ready);
    handle.stop();
    result.map(|()| StreamReport {
        frames_written: handle.frames_written(),
        underruns: handle.underruns(),
    })
}

fn run_stream(
    device_id: &DeviceId,
    format: DeviceFormat,
    source: SampleSource,
    options: StreamOptions,
    ready: SyncSender<Result<(), OutputError>>,
) -> Result<(), OutputError> {
    let setup = (|| {
        let device = find_device(device_id)?;
        let guard = HogGuard::acquire(device, format)?;
        // SAFETY: the virtual format is an ASBD.
        let virtual_format: AudioStreamBasicDescription = unsafe {
            get(
                guard.0.stream,
                address(
                    kAudioStreamPropertyVirtualFormat,
                    kAudioObjectPropertyScopeGlobal,
                ),
            )
        }?;
        let integer = asbd(format).is_some_and(|want| same_layout(&virtual_format, &want));
        let float32 = virtual_format.mFormatFlags & kAudioFormatFlagIsFloat != 0
            && virtual_format.mBitsPerChannel == 32
            && virtual_format.mFormatFlags & kAudioFormatFlagIsNonInterleaved == 0;
        if !integer && !float32 {
            return Err(OutputError::Backend(
                "unexpected CoreAudio virtual format".into(),
            ));
        }
        // SAFETY: the buffer size is a `UInt32`; failure leaves the driver default.
        let _ = unsafe {
            set(
                device,
                address(
                    kAudioDevicePropertyBufferFrameSize,
                    kAudioObjectPropertyScopeGlobal,
                ),
                &IO_BUFFER_FRAMES,
            )
        };
        Ok((
            device,
            guard,
            !integer,
            virtual_format.mBytesPerFrame as usize,
        ))
    })();
    let (device, guard, float_out, device_frame_bytes) = match setup {
        Ok(parts) => {
            let _ = ready.send(Ok(()));
            parts
        }
        Err(e) => {
            let _ = ready.send(Err(e));
            return Ok(());
        }
    };

    // In float mode the renderer packs S32 so the IOProc can convert from plain `i32`.
    let render_format = if float_out {
        DeviceFormat {
            container: SampleContainer::S32,
            ..format
        }
    } else {
        format
    };
    // A tap must see the bytes the DAC receives, not the S32 staging layout.
    let tap = match options.tap {
        Some(tap) if float_out => Some(Box::new(RepackTap::new(tap, format)) as Box<dyn WriteTap>),
        other => other,
    };
    let exhausted = Arc::new(AtomicBool::new(false));
    let idle_only = Arc::new(AtomicBool::new(false));
    let idle_cycles = Arc::new(AtomicU32::new(0));
    let mut state = Box::new(IoState {
        renderer: Renderer::new(source, render_format, MAX_CHUNK_FRAMES, tap, options.dop),
        float_out,
        channels: usize::from(format.channels),
        device_frame_bytes,
        exhausted: exhausted.clone(),
        idle_only: idle_only.clone(),
        idle_cycles: idle_cycles.clone(),
    });
    let handle = state.renderer.handle().clone();
    let dsd = state.renderer.is_dsd();

    let target = state
        .renderer
        .queue_capacity()
        .min(MAX_CHUNK_FRAMES * usize::from(format.channels));
    let started = Instant::now();
    while state.renderer.queued_samples() < target
        && !state.renderer.input_finished()
        && !handle.is_stopping()
        && started.elapsed() < PREBUFFER_TIMEOUT
    {
        state.renderer.service_flush();
        thread::sleep(Duration::from_millis(1));
    }

    let client: *mut IoState = &mut *state;
    let mut proc_id: AudioDeviceIOProcID = None;
    // SAFETY: `io_proc` matches the IOProc signature; `client` stays valid until the proc is
    // destroyed below, before `state` is dropped.
    let code = unsafe {
        AudioDeviceCreateIOProcID(
            device,
            Some(io_proc),
            client.cast(),
            NonNull::from(&mut proc_id),
        )
    };
    status("AudioDeviceCreateIOProcID", code)?;
    // SAFETY: `proc_id` was just created for `device`.
    let code = unsafe { AudioDeviceStart(device, proc_id) };
    let result = status("AudioDeviceStart", code).and_then(|()| {
        let mut drain_from = None;
        loop {
            if handle.is_stopping() {
                if dsd {
                    // End a DSD stream on idle frames: the IOProc stops reading the queue
                    // and hands the device DSD idle until it has taken a couple of cycles.
                    idle_only.store(true, Ordering::Release);
                    let since = Instant::now();
                    while idle_cycles.load(Ordering::Acquire) < DSD_STOP_IDLE_CYCLES
                        && since.elapsed() < DSD_STOP_IDLE_TIMEOUT
                    {
                        thread::sleep(Duration::from_millis(1));
                    }
                }
                return Ok(());
            }
            if !is_alive(device) {
                return Err(OutputError::DeviceLost(
                    "the device is no longer alive".into(),
                ));
            }
            if exhausted.load(Ordering::Acquire) {
                let since = *drain_from.get_or_insert_with(Instant::now);
                if since.elapsed() >= DRAIN {
                    return Ok(());
                }
            }
            thread::sleep(POLL);
        }
    });
    // SAFETY: stopping from a non-IO thread is synchronous: the IOProc is not running and
    // will not run again once these return, so `state` can be dropped afterwards.
    unsafe {
        AudioDeviceStop(device, proc_id);
        AudioDeviceDestroyIOProcID(device, proc_id);
    }
    drop(guard);
    let IoState { renderer, .. } = *state;
    renderer.finish();
    result
}

/// Re-packs the S32 staging bytes of float mode into the physical container before handing
/// them to the user's tap. Buffers are sized up front; `on_audio` does not allocate.
struct RepackTap {
    inner: Box<dyn WriteTap>,
    container: SampleContainer,
    samples: Vec<i32>,
    bytes: Vec<u8>,
}

impl RepackTap {
    fn new(inner: Box<dyn WriteTap>, format: DeviceFormat) -> Self {
        let max = MAX_CHUNK_FRAMES * usize::from(format.channels);
        Self {
            inner,
            container: format.container,
            samples: vec![0; max],
            bytes: vec![0; max * format.container.bytes()],
        }
    }
}

impl WriteTap for RepackTap {
    fn on_audio(&mut self, bytes: &[u8]) {
        let n = (bytes.len() / 4).min(self.samples.len());
        unpack(
            SampleContainer::S32,
            &bytes[..n * 4],
            &mut self.samples[..n],
        );
        let out = &mut self.bytes[..n * self.container.bytes()];
        pack(self.container, &self.samples[..n], out);
        self.inner.on_audio(out);
    }

    fn finish(&mut self) {
        self.inner.finish();
    }
}
