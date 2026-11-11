//! Command-line harness: list exclusive-mode devices and play one file bit-perfect.

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use clap::{Parser, Subcommand};
use sha2::{Digest, Sha256};
use sonder_decode::Decoder;
use sonder_output::format::pack;
use sonder_output::{
    DeviceInfo, OutputBackend, SampleContainer, StreamOptions, WriteTap, negotiate, sample_ring,
};

/// Seconds of decoded audio buffered between the decoder and the render thread.
const QUEUE_SECONDS: usize = 1;

#[derive(Parser)]
#[command(
    name = "sonder-cli",
    version,
    about = "Sonder bit-perfect playback harness"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List output devices and the stereo formats they accept in exclusive mode.
    Devices,
    /// Play one file in exclusive mode at its native rate and bit depth.
    Play {
        file: PathBuf,
        /// Device index from `sonder-cli devices` (default: the system default device).
        #[arg(long)]
        device: Option<usize>,
        /// Hash the bytes written into the device buffer and compare them with the decoded
        /// samples packed into the same container.
        #[arg(long)]
        verify_hash: bool,
    },
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Devices => list_devices(),
        Command::Play {
            file,
            device,
            verify_hash,
        } => play(file, device, verify_hash),
    }
}

#[cfg(windows)]
fn backend() -> Result<Box<dyn OutputBackend>> {
    Ok(Box::new(sonder_output::wasapi::WasapiExclusive::new()))
}

#[cfg(not(windows))]
fn backend() -> Result<Box<dyn OutputBackend>> {
    bail!("no exclusive-mode output backend exists for this platform yet")
}

fn list_devices() -> Result<()> {
    let devices = backend()?.devices()?;
    if devices.is_empty() {
        println!("No active output devices.");
    }
    for (index, device) in devices.iter().enumerate() {
        let marker = if device.is_default { " (default)" } else { "" };
        println!("[{index}] {}{marker}", device.name);
        if device.formats.is_empty() {
            println!("      exclusive mode unavailable (device busy or exclusive access disabled)");
            continue;
        }
        let mut by_rate: BTreeMap<u32, Vec<SampleContainer>> = BTreeMap::new();
        for format in &device.formats {
            by_rate
                .entry(format.rate)
                .or_default()
                .push(format.container);
        }
        for (rate, containers) in by_rate {
            let names: Vec<String> = containers.iter().map(ToString::to_string).collect();
            println!("      {rate:>6} Hz: {}", names.join(", "));
        }
    }
    Ok(())
}

fn pick_device(devices: &[DeviceInfo], index: Option<usize>) -> Result<&DeviceInfo> {
    match index {
        Some(i) => devices
            .get(i)
            .ok_or_else(|| anyhow!("no device with index {i}; run `sonder-cli devices`")),
        None => devices
            .iter()
            .find(|d| d.is_default)
            .or_else(|| devices.first())
            .ok_or_else(|| anyhow!("no active output device")),
    }
}

/// Records the SHA-256 of every audio byte the render thread writes into the device buffer.
struct HashTap {
    hasher: Sha256,
    result: Arc<Mutex<Option<String>>>,
}

impl WriteTap for HashTap {
    fn on_audio(&mut self, bytes: &[u8]) {
        self.hasher.update(bytes);
    }

    fn finish(&mut self) {
        let digest = std::mem::take(&mut self.hasher).finalize();
        if let Ok(mut slot) = self.result.lock() {
            *slot = Some(hex(&digest));
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

struct DecodeSummary {
    /// SHA-256 of the decoded samples packed into the device container (verify mode only).
    expected_hash: Option<String>,
    skipped_packets: u64,
    /// Samples with non-zero bits below the source bit depth (verify mode only).
    misaligned: u64,
    error: Option<String>,
}

fn play(file: PathBuf, device: Option<usize>, verify_hash: bool) -> Result<()> {
    let mut decoder =
        Decoder::open(&file).with_context(|| format!("cannot open {}", file.display()))?;
    let source = decoder.format();

    let mut backend = backend()?;
    let devices = backend.devices()?;
    let device = pick_device(&devices, device)?;
    let format = negotiate(
        backend.as_ref(),
        &device.id,
        source.rate,
        source.bits,
        source.channels,
    )
    .with_context(|| format!("cannot play on {}", device.name))?;

    eprintln!(
        "Source: {} Hz, {}-bit, {} ch",
        source.rate, source.bits, source.channels
    );
    eprintln!("Device: {} (exclusive) at {format}", device.name);

    let (mut producer, ring_source) =
        sample_ring(source.rate as usize * QUEUE_SECONDS, source.channels);
    let device_hash = Arc::new(Mutex::new(None));
    let options = StreamOptions {
        tap: verify_hash.then(|| {
            Box::new(HashTap {
                hasher: Sha256::new(),
                result: device_hash.clone(),
            }) as Box<dyn WriteTap>
        }),
    };
    let stream = backend.open(&device.id, format, ring_source, options)?;
    let handle = stream.handle();

    let container = format.container;
    let decode_thread = thread::Builder::new()
        .name("sonder-decode".into())
        .spawn(move || {
            let mut hasher = verify_hash.then(Sha256::new);
            let low_mask = if source.bits >= 32 {
                0
            } else {
                (1i32 << (32 - source.bits)) - 1
            };
            let mut misaligned = 0u64;
            let mut block = Vec::new();
            let mut packed = Vec::new();
            let mut error = None;
            loop {
                match decoder.next_i32(&mut block) {
                    Ok(0) => break,
                    Ok(_) => {
                        if let Some(hasher) = hasher.as_mut() {
                            // `pack` drops low bits, so a decoder that failed to left-justify
                            // would hash identically on both sides. Catch that here.
                            misaligned +=
                                block.iter().filter(|&&s| s & low_mask != 0).count() as u64;
                            packed.resize(block.len() * container.bytes(), 0);
                            pack(container, &block, &mut packed);
                            hasher.update(&packed);
                        }
                        if !producer.push(&block) {
                            break;
                        }
                    }
                    Err(e) => {
                        // Untrusted input: a broken file ends the track, never the process.
                        error = Some(e.to_string());
                        break;
                    }
                }
            }
            DecodeSummary {
                expected_hash: hasher.map(|h| hex(&h.finalize())),
                skipped_packets: decoder.skipped_packets(),
                misaligned,
                error,
            }
            // Dropping `producer` here marks the end of input.
        })
        .context("cannot spawn decoder thread")?;

    while !stream.is_finished() {
        thread::sleep(Duration::from_millis(250));
        let secs = handle.frames_written() / u64::from(source.rate);
        eprint!(
            "\r{:02}:{:02}  underruns: {}",
            secs / 60,
            secs % 60,
            handle.underruns()
        );
        let _ = std::io::stderr().flush();
    }
    eprintln!();

    let report = stream.wait()?;
    let decoded = decode_thread
        .join()
        .map_err(|_| anyhow!("decoder thread panicked"))?;

    eprintln!(
        "Frames played: {}, underruns: {}",
        report.frames_written, report.underruns
    );
    if decoded.skipped_packets > 0 {
        eprintln!("Skipped {} corrupt packets", decoded.skipped_packets);
    }
    if let Some(e) = &decoded.error {
        eprintln!("Decoding stopped early: {e}");
    }

    if verify_hash {
        let expected = decoded.expected_hash.unwrap_or_default();
        let written = device_hash
            .lock()
            .map_err(|_| anyhow!("hash lock poisoned"))?
            .clone();
        let written = written.unwrap_or_default();
        eprintln!("Decoded  sha256: {expected}");
        eprintln!("Device   sha256: {written}");
        if expected.is_empty() || expected != written {
            bail!("bit-perfect check FAILED: device bytes differ from decoded samples");
        }
        if decoded.misaligned > 0 {
            bail!(
                "bit-perfect check FAILED: {} decoded samples carry bits below {}-bit",
                decoded.misaligned,
                source.bits
            );
        }
        if report.underruns > 0 {
            bail!(
                "bit-perfect check FAILED: {} underruns inserted silence",
                report.underruns
            );
        }
        eprintln!("Bit-perfect check PASSED ({container} container)");
    }
    Ok(())
}
