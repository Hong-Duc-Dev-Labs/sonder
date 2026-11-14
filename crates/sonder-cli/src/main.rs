//! Command-line harness: list exclusive-mode devices, play files or folders bit-perfect, and
//! scan folders into a library database.

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, anyhow, bail};
use clap::{Parser, Subcommand, ValueEnum};
use sha2::{Digest, Sha256};
use sonder_decode::Decoder;
use sonder_engine::{DsdMode, DsdPcmRate, Player, PlayerConfig, PlayerEvent, SdmMode, SdmOrder};
use sonder_output::format::pack;
use sonder_output::{DeviceFormat, DeviceInfo, OutputBackend, SampleContainer, WriteTap};

/// File extensions queued when a folder is given.
const AUDIO_EXTENSIONS: [&str; 7] = ["flac", "wav", "aif", "aiff", "m4a", "mp4", "alac"];
/// Extensions `check` also picks up from folders: the PCM ones plus DSD.
const CHECK_EXTENSIONS: [&str; 9] = [
    "flac", "wav", "aif", "aiff", "m4a", "mp4", "alac", "dsf", "dff",
];

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
    /// Play files and folders gapless in exclusive mode at their native rate and bit depth.
    Play {
        /// Audio files, or folders whose audio files are queued in name order.
        #[arg(required = true)]
        paths: Vec<PathBuf>,
        /// Device index from `sonder-cli devices` (default: the system default device).
        #[arg(long)]
        device: Option<usize>,
        /// Software volume in dB (disables bit-perfect output).
        #[arg(long, allow_hyphen_values = true)]
        volume: Option<f64>,
        /// Hash the bytes written into the device buffer and compare them with the decoded
        /// samples packed into the same container. Needs every track to share one stream.
        /// With `--volume` the check must fail: that run proves the harness sees changed bytes.
        #[arg(long, conflicts_with = "pcm_to_dsd")]
        verify_hash: bool,
        /// How DSD files (DSF, DFF) reach the device. `dop` and `native` fall back to the
        /// next option the device accepts, ending at PCM conversion.
        #[arg(long, value_enum, default_value_t = DsdArg::Dop)]
        dsd_mode: DsdArg,
        /// Output rate when DSD is converted to PCM.
        #[arg(long, value_enum, default_value_t = DsdRateArg::R176400)]
        dsd_pcm_rate: DsdRateArg,
        /// Modulate PCM tracks to DSD (DoP or native, as `--dsd-mode` allows).
        #[arg(long, value_enum, default_value_t = DsdOutArg::Off)]
        pcm_to_dsd: DsdOutArg,
        /// Sigma-delta modulator order for `--pcm-to-dsd`.
        #[arg(long, value_enum, default_value_t = SdmOrderArg::Fifth)]
        sdm_order: SdmOrderArg,
    },
    /// Add folders to a library database and scan every library folder, printing timings.
    Scan {
        /// Folders to add as library roots (none: rescan the existing roots).
        paths: Vec<PathBuf>,
        /// Library database file.
        #[arg(long, default_value = "sonder-library.db")]
        db: PathBuf,
        /// Cover thumbnail cache folder (default: `art` next to the database).
        #[arg(long)]
        art: Option<PathBuf>,
        /// Follow symbolic links inside the added folders, even when they lead outside.
        #[arg(long)]
        follow_symlinks: bool,
    },
    /// Check whether lossless files are genuine: spectral cutoff, lossy fingerprints, real
    /// bit depth, hi-res content and the FLAC MD5. A spectral estimate, not proof.
    Check {
        /// Audio files, or folders whose audio files (DSD included) are checked in name order.
        #[arg(required = true)]
        paths: Vec<PathBuf>,
        /// Print each report as one line of JSON instead of text.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum DsdArg {
    /// DSD over PCM (DoP), else PCM conversion.
    Dop,
    /// Native DSD (ALSA DSD_U32_BE), else DoP, else PCM conversion.
    Native,
    /// Always convert to PCM.
    Pcm,
}

impl From<DsdArg> for DsdMode {
    fn from(arg: DsdArg) -> Self {
        match arg {
            DsdArg::Dop => DsdMode::Dop,
            DsdArg::Native => DsdMode::Native,
            DsdArg::Pcm => DsdMode::Pcm,
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum DsdRateArg {
    #[value(name = "176400")]
    R176400,
    #[value(name = "352800")]
    R352800,
}

impl From<DsdRateArg> for DsdPcmRate {
    fn from(arg: DsdRateArg) -> Self {
        match arg {
            DsdRateArg::R176400 => DsdPcmRate::R176_4,
            DsdRateArg::R352800 => DsdPcmRate::R352_8,
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum DsdOutArg {
    /// Leave PCM tracks as PCM.
    Off,
    #[value(name = "dsd64")]
    Dsd64,
    #[value(name = "dsd128")]
    Dsd128,
    #[value(name = "dsd256")]
    Dsd256,
}

impl From<DsdOutArg> for SdmMode {
    fn from(arg: DsdOutArg) -> Self {
        match arg {
            DsdOutArg::Off => SdmMode::Off,
            DsdOutArg::Dsd64 => SdmMode::Dsd64,
            DsdOutArg::Dsd128 => SdmMode::Dsd128,
            DsdOutArg::Dsd256 => SdmMode::Dsd256,
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum SdmOrderArg {
    #[value(name = "5")]
    Fifth,
    #[value(name = "7")]
    Seventh,
}

impl From<SdmOrderArg> for SdmOrder {
    fn from(arg: SdmOrderArg) -> Self {
        match arg {
            SdmOrderArg::Fifth => SdmOrder::Order5,
            SdmOrderArg::Seventh => SdmOrder::Order7,
        }
    }
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Devices => list_devices(),
        Command::Play {
            paths,
            device,
            volume,
            verify_hash,
            dsd_mode,
            dsd_pcm_rate,
            pcm_to_dsd,
            sdm_order,
        } => play(
            paths,
            device,
            volume,
            verify_hash,
            dsd_mode.into(),
            dsd_pcm_rate.into(),
            pcm_to_dsd.into(),
            sdm_order.into(),
        ),
        Command::Scan {
            paths,
            db,
            art,
            follow_symlinks,
        } => scan(paths, db, art, follow_symlinks),
        Command::Check { paths, json } => check(&paths, json),
    }
}

fn check(paths: &[PathBuf], json: bool) -> Result<()> {
    use sonder_quality::{CriterionId, EvidenceStatus};

    let cancel = std::sync::atomic::AtomicBool::new(false);
    let mut failed = 0;
    for path in collect_tracks(paths, &CHECK_EXTENSIONS)? {
        let started = std::time::Instant::now();
        let report = match sonder_quality::analyze(&path, &Default::default(), &cancel, |_| {}) {
            Ok(report) => report,
            Err(e) => {
                eprintln!("{}: {e}", path.display());
                failed += 1;
                continue;
            }
        };
        if json {
            println!("{}", serde_json::to_string(&report)?);
            continue;
        }
        let source = report.source;
        let format = if source.dsd {
            format!("DSD {:.1} MHz", f64::from(source.rate) / 1e6)
        } else {
            format!(
                "{}-bit / {:.1} kHz",
                source.bits,
                f64::from(source.rate) / 1e3
            )
        };
        println!("{}", path.display());
        println!(
            "  {} ({} confidence) — {format}, {:.0} s, checked in {:.2?}",
            report.verdict.as_str(),
            report.confidence.as_str(),
            report.duration_secs,
            started.elapsed()
        );
        for e in &report.evidence {
            let status = match e.status {
                EvidenceStatus::Pass => "pass",
                EvidenceStatus::Warn => "warn",
                EvidenceStatus::Fail => "FAIL",
                EvidenceStatus::Info => "info",
                EvidenceStatus::NotMeasured => "not measured",
            };
            let value = e.value.map_or(String::new(), |v| match e.id {
                CriterionId::Cutoff | CriterionId::ChannelSymmetry | CriterionId::Stability => {
                    format!("{:.1} kHz", v / 1e3)
                }
                CriterionId::Holes => format!("{:.0} % of frames", v * 100.0),
                CriterionId::BitDepth => format!("{v:.0} bits"),
                CriterionId::QuietLevel => format!("{v:.1} dBFS"),
                _ => format!("{v:.1} dB"),
            });
            println!("  {:<18} {status:<13} {value}", format!("{:?}", e.id));
        }
    }
    if failed > 0 {
        bail!("{failed} file(s) could not be checked");
    }
    Ok(())
}

fn scan(
    paths: Vec<PathBuf>,
    db: PathBuf,
    art: Option<PathBuf>,
    follow_symlinks: bool,
) -> Result<()> {
    let art = art.unwrap_or_else(|| db.with_file_name("art"));
    let mut library = sonder_library::Library::open(&db, &art)
        .with_context(|| format!("opening {}", db.display()))?;
    for path in &paths {
        library
            .add_root(path, follow_symlinks)
            .with_context(|| format!("adding {}", path.display()))?;
    }
    let report = library.scan()?;
    for failure in &report.failed {
        eprintln!("skipped {}: {}", failure.path.display(), failure.reason);
    }
    let albums = library.albums(sonder_library::AlbumSort::Title)?;
    let tracks: u32 = albums.iter().map(|a| a.track_count).sum();
    println!(
        "{} added, {} updated, {} removed, {} unchanged, {} failed in {:.2?}",
        report.added,
        report.updated,
        report.removed,
        report.unchanged,
        report.failed.len(),
        report.elapsed
    );
    println!("library: {} albums, {tracks} tracks", albums.len());
    Ok(())
}

#[cfg(windows)]
fn backend() -> Result<Box<dyn OutputBackend>> {
    Ok(Box::new(sonder_output::wasapi::WasapiExclusive::new()))
}

#[cfg(target_os = "macos")]
fn backend() -> Result<Box<dyn OutputBackend>> {
    Ok(Box::new(sonder_output::coreaudio::CoreAudioHog::new()))
}

#[cfg(target_os = "linux")]
fn backend() -> Result<Box<dyn OutputBackend>> {
    Ok(Box::new(sonder_output::alsa::AlsaHw::new()))
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
fn backend() -> Result<Box<dyn OutputBackend>> {
    bail!("no exclusive-mode output backend exists for this platform")
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

/// Expands folders into their audio files, sorted by name; files are kept in the given order.
fn collect_tracks(paths: &[PathBuf], extensions: &[&str]) -> Result<Vec<PathBuf>> {
    let mut tracks = Vec::new();
    for path in paths {
        if path.is_dir() {
            let mut files: Vec<PathBuf> = std::fs::read_dir(path)
                .with_context(|| format!("cannot read {}", path.display()))?
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.is_file() && has_extension(p, extensions))
                .collect();
            files.sort();
            tracks.extend(files);
        } else {
            tracks.push(path.clone());
        }
    }
    if tracks.is_empty() {
        bail!("no audio files found");
    }
    Ok(tracks)
}

fn has_extension(path: &Path, extensions: &[&str]) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| extensions.contains(&e.to_ascii_lowercase().as_str()))
}

/// SHA-256 of every audio byte written into the device buffer, one entry per stream.
type Digests = Arc<Mutex<Vec<(DeviceFormat, String)>>>;

struct HashTap {
    format: DeviceFormat,
    hasher: Sha256,
    result: Digests,
}

impl WriteTap for HashTap {
    fn on_audio(&mut self, bytes: &[u8]) {
        self.hasher.update(bytes);
    }

    fn finish(&mut self) {
        let digest = std::mem::take(&mut self.hasher).finalize();
        if let Ok(mut all) = self.result.lock() {
            all.push((self.format, hex(&digest)));
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[allow(clippy::too_many_arguments)]
fn play(
    paths: Vec<PathBuf>,
    device: Option<usize>,
    volume: Option<f64>,
    verify_hash: bool,
    dsd_mode: DsdMode,
    dsd_pcm_rate: DsdPcmRate,
    sdm_mode: SdmMode,
    sdm_order: SdmOrder,
) -> Result<()> {
    let tracks = collect_tracks(&paths, &AUDIO_EXTENSIONS)?;
    let backend = backend()?;
    let devices = backend.devices()?;
    let device = pick_device(&devices, device)?;
    eprintln!("Device: {} (exclusive)", device.name);

    let mut config = PlayerConfig::new(device.id.clone());
    config.default_dsd_mode = dsd_mode;
    config.dsd_pcm_rate = dsd_pcm_rate;
    config.sdm.mode = sdm_mode;
    config.sdm.order = sdm_order;
    let digests: Digests = Arc::default();
    if verify_hash {
        let digests = digests.clone();
        config.tap = Some(Box::new(move |format| {
            Box::new(HashTap {
                format,
                hasher: Sha256::new(),
                result: digests.clone(),
            }) as Box<dyn WriteTap>
        }));
    }

    let (player, events) = Player::spawn(backend, config).context("cannot start the player")?;
    if volume.is_some() {
        player.set_volume(volume);
    }
    player.enqueue(tracks.clone());
    player.play();

    let mut failures = 0;
    let mut underruns = 0;
    let mut lost = false;
    for event in events.iter() {
        match event {
            PlayerEvent::TrackChanged {
                index,
                path,
                source,
                ..
            } => {
                eprintln!(
                    "\n[{}/{}] {} ({} Hz, {}-bit, {} ch)",
                    index + 1,
                    tracks.len(),
                    path.file_name().unwrap_or_default().to_string_lossy(),
                    source.rate,
                    source.bits,
                    source.channels
                );
            }
            PlayerEvent::OutputFormat(format) => {
                eprintln!("Output: {format}");
                // After a device loss the player reopens paused; a CLI run just carries on.
                if std::mem::take(&mut lost) {
                    player.play();
                }
            }
            PlayerEvent::Position { elapsed, .. } => {
                let secs = elapsed.as_secs();
                eprint!(
                    "\r{:02}:{:02}  underruns: {underruns}",
                    secs / 60,
                    secs % 60
                );
                let _ = std::io::stderr().flush();
            }
            PlayerEvent::Underruns(total) => underruns = total,
            PlayerEvent::DsdMode { mode, reason } => {
                eprintln!(
                    "\nDSD: {mode:?}{}",
                    reason.map(|r| format!(" ({r})")).unwrap_or_default()
                );
            }
            PlayerEvent::TrackFailed { path, error, .. } => {
                failures += 1;
                eprintln!("\nSkipped {}: {error}", path.display());
            }
            PlayerEvent::DeviceLost(reason) => {
                lost = true;
                eprintln!("\nDevice lost ({reason}); waiting for it to come back...");
            }
            PlayerEvent::DeviceBusy => {
                bail!("the device is in use by another application");
            }
            PlayerEvent::OutputFailed(reason) => bail!("output failed: {reason}"),
            PlayerEvent::SignalPath(path) => {
                let stages = path
                    .stages
                    .iter()
                    .map(|stage| format!("{} ({})", stage.name, stage.detail))
                    .collect::<Vec<_>>()
                    .join(" -> ");
                eprintln!(
                    "Signal path: {} Hz, {}-bit, {} ch, {}{}",
                    path.output_rate,
                    path.output_bits,
                    path.output_channels,
                    if path.bit_perfect {
                        "bit-perfect"
                    } else {
                        "not bit-perfect"
                    },
                    if stages.is_empty() {
                        String::new()
                    } else {
                        format!("; {stages}")
                    }
                );
            }
            PlayerEvent::Warning(message) => eprintln!("\nWarning: {message}"),
            PlayerEvent::Clipping(count) => {
                eprintln!("\nClipping: {count} samples hit full scale; lower the EQ boost")
            }
            PlayerEvent::EndOfQueue => break,
            _ => {}
        }
    }
    eprintln!();
    drop(player);

    eprintln!("Underruns: {underruns}");
    if verify_hash {
        verify(&tracks, &digests, underruns)?;
    }
    if failures > 0 {
        bail!("{failures} track(s) could not be played");
    }
    Ok(())
}

/// Compares the device-side digest with the decoded tracks packed into the same container.
fn verify(tracks: &[PathBuf], digests: &Digests, underruns: u64) -> Result<()> {
    let sessions = digests
        .lock()
        .map_err(|_| anyhow!("hash lock poisoned"))?
        .clone();
    let [(format, written)] = sessions.as_slice() else {
        bail!(
            "verification needs one continuous stream, but {} streams were opened \
             (tracks differ in rate or channels)",
            sessions.len()
        );
    };
    let container = format.container;
    let mut hasher = Sha256::new();
    let mut block = Vec::new();
    let mut packed = Vec::new();
    for track in tracks {
        let mut decoder = Decoder::open(track)?;
        let bits = decoder.format().bits;
        let low_mask = if bits >= 32 {
            0
        } else {
            (1i32 << (32 - bits)) - 1
        };
        while decoder.next_i32(&mut block)? > 0 {
            // `pack` drops low bits, so a decoder that failed to left-justify would hash
            // identically on both sides. Catch that here.
            if block.iter().any(|&s| s & low_mask != 0) {
                bail!(
                    "bit-perfect check FAILED: {} carries bits below {bits}-bit",
                    track.display()
                );
            }
            packed.resize(block.len() * container.bytes(), 0);
            pack(container, &block, &mut packed);
            hasher.update(&packed);
        }
    }
    let expected = hex(&hasher.finalize());
    eprintln!("Decoded  sha256: {expected}");
    eprintln!("Device   sha256: {written}");
    if expected != *written {
        bail!("bit-perfect check FAILED: device bytes differ from decoded samples");
    }
    if underruns > 0 {
        bail!("bit-perfect check FAILED: {underruns} underruns inserted silence");
    }
    eprintln!("Bit-perfect check PASSED ({container} container)");
    Ok(())
}
