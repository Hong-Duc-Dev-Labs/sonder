//! Sonder's acceptance suite: the eight brainstorm criteria that can be checked without
//! hardware, each as a test that either passes with evidence or fails with numbers.
//!
//! Hardware checks (a real DAC recognising DoP, S/PDIF loopback) cannot run in CI; they are in
//! `docs/verification.md` with the commands and the recorded results.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crossbeam_channel::Receiver;
use sha2::{Digest, Sha256};
use sonder_dsp::{
    CrossfeedPreset, DsdToPcm, FilterPhase, FilterSource, FilterSpec, FilterVariant,
    ImpulseResponse, Peq, PeqBand, PeqStage, Resampler, Sdm, SdmOrder, SdmRate, SonderDesigned,
    Stage, StreamFormat,
};
use sonder_engine::{Player, PlayerConfig, PlayerEvent};
use sonder_output::format::pack;
use sonder_output::null::{NullBackend, NullControl};
use sonder_output::{DeviceFormat, SampleContainer, WriteTap};

const TIMEOUT: Duration = Duration::from_secs(30);

fn temp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sonder-verify-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A deterministic WAV whose samples exercise every bit of the requested depth.
fn wav(name: &str, rate: u32, bits: u16, frames: usize) -> PathBuf {
    let path = temp_dir().join(name);
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: rate,
        bits_per_sample: bits,
        sample_format: hound::SampleFormat::Int,
    };
    let max = (1i64 << (bits - 1)) - 1;
    let mut writer = hound::WavWriter::create(&path, spec).unwrap();
    for i in 0..frames as i64 * 2 {
        // A full-scale pattern that never repeats over the test length.
        let v = ((i * 7919 + 104_729) % (2 * max)) - max;
        writer.write_sample(v as i32).unwrap();
    }
    writer.finalize().unwrap();
    path
}

fn tone_wav(name: &str, rate: u32, bits: u16, frames: usize, freq: f64) -> PathBuf {
    let path = temp_dir().join(name);
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: rate,
        bits_per_sample: bits,
        sample_format: hound::SampleFormat::Int,
    };
    let max = f64::from((1i32 << (bits - 1)) - 1) * 0.5;
    let mut writer = hound::WavWriter::create(&path, spec).unwrap();
    for frame in 0..frames {
        let sample = (max
            * (2.0 * std::f64::consts::PI * freq * frame as f64 / f64::from(rate)).sin())
            as i32;
        writer.write_sample(sample).unwrap();
        writer.write_sample(sample).unwrap();
    }
    writer.finalize().unwrap();
    path
}

fn decoded(path: &Path) -> Vec<i32> {
    let mut decoder = sonder_decode::Decoder::open(path).unwrap();
    let mut all = Vec::new();
    let mut block = Vec::new();
    while decoder.next_i32(&mut block).unwrap() > 0 {
        all.extend_from_slice(&block);
    }
    all
}

fn packed(container: SampleContainer, samples: &[i32]) -> Vec<u8> {
    let mut out = vec![0; samples.len() * container.bytes()];
    pack(container, samples, &mut out);
    out
}

fn start(
    mut config: PlayerConfig,
    backend: NullBackend,
) -> (Player, Receiver<PlayerEvent>, NullControl) {
    let control = backend.control();
    config.device_poll = Duration::from_millis(20);
    let (player, events) = Player::spawn(Box::new(backend), config).unwrap();
    (player, events, control)
}

fn wait_end(events: &Receiver<PlayerEvent>) -> Vec<PlayerEvent> {
    let deadline = Instant::now() + TIMEOUT;
    let mut seen = Vec::new();
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match events.recv_timeout(left) {
            Ok(PlayerEvent::EndOfQueue) => return seen,
            Ok(event) => seen.push(event),
            Err(_) => panic!("timed out; events so far: {seen:#?}"),
        }
    }
}

// ----- criterion 1: bit-perfect playback with DSP off -----

#[test]
fn dsp_off_is_byte_exact_for_every_pcm_depth_and_rate() {
    for (rate, bits) in [
        (44_100, 16),
        (44_100, 24),
        (48_000, 16),
        (48_000, 24),
        (96_000, 24),
        (192_000, 24),
    ] {
        let path = wav(&format!("perfect-{rate}-{bits}.wav"), rate, bits, 20_000);
        let config = PlayerConfig::new(NullBackend::device_id());
        let (player, events, control) = start(config, NullBackend::new());
        player.enqueue(vec![path.clone()]);
        player.play();
        let seen = wait_end(&events);

        let sessions = control.sessions();
        assert_eq!(sessions.len(), 1, "{rate}/{bits}: {seen:#?}");
        let session = &sessions[0];
        let container = session.format.container;
        assert_eq!(
            session.bytes,
            packed(container, &decoded(&path)),
            "{rate}/{bits} did not reach the device unchanged"
        );
        assert!(
            seen.iter().any(|event| matches!(
                event,
                PlayerEvent::SignalPath(path) if path.bit_perfect && path.stages.is_empty()
            )),
            "{rate}/{bits}: the signal path must say bit-perfect: {seen:#?}"
        );
    }
}

/// The `--verify-hash` hook: a tap sees exactly the bytes the device buffer receives and can
/// hash them, which is how a real exclusive-mode DAC is checked without loopback capture.
use std::sync::Arc;

/// What the tap recorded: the device format and every byte written.
type TapOutput = Arc<std::sync::Mutex<Option<(DeviceFormat, Vec<u8>)>>>;

/// A tap that hashes the bytes written into the device buffer.
struct HashTap {
    format: DeviceFormat,
    hasher: Sha256,
    out: TapOutput,
}

impl WriteTap for HashTap {
    fn on_audio(&mut self, bytes: &[u8]) {
        self.hasher.update(bytes);
        if let Ok(mut out) = self.out.lock() {
            let entry = out.get_or_insert_with(|| (self.format, Vec::new()));
            entry.1.extend_from_slice(bytes);
        }
    }

    fn finish(&mut self) {
        if let Ok(mut out) = self.out.lock() {
            out.get_or_insert_with(|| (self.format, Vec::new()));
        }
    }
}

#[test]
fn the_device_bytes_match_the_decoded_samples() {
    let path = wav("tap.wav", 44_100, 24, 10_000);
    let expected = packed(
        SampleContainer::S24Packed,
        &decoded(&path).into_iter().collect::<Vec<_>>(),
    );
    let seen = Arc::new(std::sync::Mutex::new(None));
    let seen_for_tap = seen.clone();
    let mut config = PlayerConfig::new(NullBackend::device_id());
    config.tap = Some(Box::new(move |format| {
        Box::new(HashTap {
            format,
            hasher: Sha256::new(),
            out: seen_for_tap.clone(),
        }) as Box<dyn WriteTap>
    }));
    let (player, events, control) = start(config, NullBackend::new());
    player.enqueue(vec![path]);
    player.play();
    wait_end(&events);

    let recorded = control.played_bytes();
    assert!(!recorded.is_empty());
    assert_eq!(
        recorded, expected,
        "the tap must see the device buffer bytes, and they must be the decoded samples"
    );
    let (tap_format, tap_bytes) = seen.lock().unwrap().clone().expect("tap finished");
    assert_eq!(tap_format, control.sessions()[0].format);
    assert_eq!(tap_bytes, recorded, "the tap and the device agree");
}

// ----- criterion 6 metrics: resampler images, EQ, dither, DoP, SDM -----

/// Goertzel amplitude of `freq` in channel 0 of an interleaved signal.
fn tone(signal: &[f64], channels: usize, rate: u32, freq: f64) -> f64 {
    let frames = signal.len() / channels;
    let mut re = 0.0;
    let mut im = 0.0;
    for frame in 0..frames {
        let sample = signal[frame * channels];
        let angle = 2.0 * std::f64::consts::PI * freq * frame as f64 / f64::from(rate);
        re += sample * angle.cos();
        im -= sample * angle.sin();
    }
    2.0 * (re * re + im * im).sqrt() / frames as f64
}

#[test]
fn resampler_rejects_images_by_more_than_100_db() {
    let source_rate = 44_100u32;
    let target_rate = 96_000u32;
    let designer = SonderDesigned::new(None);
    let spec = FilterSpec::new(
        target_rate,
        source_rate,
        FilterPhase::Linear,
        FilterVariant::Precise,
    );
    let table = designer.get(spec).expect("160/147 table");
    let mut resampler = Resampler::new(table);
    let channels = 1usize;
    let format = resampler.configure(StreamFormat {
        rate: source_rate,
        channels: 1,
    });
    assert_eq!(format.rate, target_rate);

    let frames = 44_100;
    let input: Vec<f64> = (0..frames)
        .map(|frame| {
            0.5 * (2.0 * std::f64::consts::PI * 1000.0 * frame as f64 / f64::from(source_rate))
                .sin()
        })
        .collect();
    let mut out = vec![0.0; resampler.max_out(frames)];
    let produced = resampler.process(&input, &mut out);
    assert!(produced > 90_000, "resampler produced {produced} frames");
    let body = &out[1000..produced - 1000];

    let signal = tone(body, channels, target_rate, 1000.0);
    // Upsampling puts the first image at the source rate: 44.1k ± 1k.
    let image = tone(body, channels, target_rate, 43_100.0).max(tone(
        body,
        channels,
        target_rate,
        45_100.0,
    ));
    let rejection = 20.0 * (signal / image.max(f64::MIN_POSITIVE)).log10();
    assert!(
        rejection > 100.0,
        "image rejection {rejection:.1} dB is below the 100 dB target"
    );
}

/// A peaking biquad's magnitude at its centre frequency is the requested gain, in dB: that is
/// the defining property of the RBJ cookbook peak filter.
fn expected_peak_gain(band: &PeqBand) -> f64 {
    10f64.powf(band.gain_db / 20.0)
}

#[test]
fn peq_matches_the_rbj_response() {
    let rate = 48_000u32;
    let band = PeqBand {
        freq_hz: 1000.0,
        gain_db: 6.0,
        q: 1.0,
        ..PeqBand::default()
    };
    let peq = Peq {
        preamp_db: 0.0,
        bands: vec![band],
    };
    let mut stage = PeqStage::new(peq);
    stage.configure(StreamFormat { rate, channels: 1 });

    let frames = 48_000;
    let input: Vec<f64> = (0..frames)
        .map(|frame| {
            0.25 * (2.0 * std::f64::consts::PI * 1000.0 * frame as f64 / f64::from(rate)).sin()
        })
        .collect();
    let mut out = vec![0.0; stage.max_out(frames)];
    let produced = stage.process(&input, &mut out);
    let body = &out[5000..produced - 1000];
    let gain = tone(body, 1, rate, 1000.0) / 0.25;
    let expected = expected_peak_gain(&band);
    assert!(
        (gain / expected - 1.0).abs() < 0.02,
        "PEQ gain {gain:.4} does not match the RBJ response {expected:.4}"
    );
}

/// A -90 dBFS tone: without dither, 16-bit truncation turns it into a square wave with strong
/// harmonics; with TPDF dither it becomes a clean noise floor.
fn low_level_harmonics(dither: bool) -> f64 {
    let rate = 44_100u32;
    let bit = 1.0 / 32_768.0;
    let amplitude = 1.5 * bit;
    let mut stage = if dither {
        Some(sonder_dsp::Dither::new(16))
    } else {
        None
    };
    if let Some(stage) = &mut stage {
        stage.configure(StreamFormat { rate, channels: 1 });
    }
    let frames = 44_100;
    let input: Vec<f64> = (0..frames)
        .map(|frame| {
            amplitude * (2.0 * std::f64::consts::PI * 1000.0 * frame as f64 / f64::from(rate)).sin()
        })
        .collect();
    let quantised: Vec<f64> = if let Some(stage) = &mut stage {
        let mut out = vec![0.0; stage.max_out(frames)];
        let produced = stage.process(&input, &mut out);
        out[..produced].to_vec()
    } else {
        input.iter().map(|s| (s / bit).round() * bit).collect()
    };
    // 1 kHz at 44.1 kHz has a 44.1-sample cycle: 10 cycles are 441 samples. Whole cycles keep
    // the fundamental's leakage out of the harmonic bin.
    let start = 4410;
    let cycles = (quantised.len() - start) / 441;
    let body = &quantised[start..start + cycles * 441];
    let fundamental = tone(body, 1, rate, 1000.0);
    let third = tone(body, 1, rate, 3000.0);
    20.0 * (third / fundamental.max(f64::MIN_POSITIVE)).log10()
}

#[test]
fn dither_replaces_truncation_distortion_with_noise() {
    let truncated = low_level_harmonics(false);
    let dithered = low_level_harmonics(true);
    assert!(
        truncated > -40.0,
        "undithered truncation should show strong harmonics, got {truncated:.1} dB"
    );
    // What the bin holds with dither is the (intended) flat noise floor, not distortion: with
    // ~90k samples one bin of TPDF noise sits near -55 dB. The test is that it is far below
    // the truncation harmonic and below the -50 dB mark.
    assert!(
        dithered < -50.0,
        "dithered bin {dithered:.1} dB is above the -50 dB target"
    );
    assert!(
        truncated - dithered > 10.0,
        "dither only improved the harmonic by {:.1} dB",
        truncated - dithered
    );
}

#[test]
fn dop_round_trip_preserves_every_byte() {
    let bytes: Vec<u8> = (0..40_000u32)
        .map(|i| ((i * 37 + 11) % 251) as u8)
        .collect();
    let mut packer = sonder_engine::dop::Packer::dop(2);
    let mut words = Vec::new();
    packer.push(&bytes, &mut words);
    packer.finish(&mut words);
    // The renderer adds markers; strip them back to the marker-free payload layout. Every
    // carrier frame is one 24-bit word per channel: byte 0 of each channel, then byte 1.
    let mut recovered = Vec::new();
    for frame in words.as_chunks::<2>().0 {
        for word in frame {
            recovered.push(((word >> 16) & 0xFF) as u8);
        }
        for word in frame {
            recovered.push(((word >> 8) & 0xFF) as u8);
        }
    }
    assert_eq!(&recovered[..bytes.len()], &bytes[..]);
}

#[test]
fn sdm_output_decodes_to_the_input_tone() {
    for (rate, order) in [
        (SdmRate::Dsd64, SdmOrder::Order5),
        (SdmRate::Dsd64, SdmOrder::Order7),
        (SdmRate::Dsd128, SdmOrder::Order5),
        (SdmRate::Dsd256, SdmOrder::Order7),
    ] {
        let channels = 2u16;
        let mut sdm = Sdm::new(rate, order, channels);
        let frames = rate.input_rate() as usize / 5; // 200 ms
        let input: Vec<f64> = (0..frames)
            .flat_map(|frame| {
                let sample = 0.7
                    * (2.0 * std::f64::consts::PI * 1000.0 * frame as f64
                        / f64::from(rate.input_rate()))
                    .sin();
                [sample; 2]
            })
            .collect();
        let mut bytes = Vec::new();
        sdm.process(&input, &mut bytes);
        sdm.flush(&mut bytes);

        let mut converter = DsdToPcm::new(rate.bit_rate(), 176_400, channels).unwrap();
        let mut pcm = Vec::new();
        converter.process(&bytes, &mut pcm);
        // 1 kHz at 176.4 kHz repeats every 882 samples (five cycles); measure whole cycles so
        // the tone's leakage cannot masquerade as a harmonic.
        let start = 882 * 10;
        let cycles = (pcm.len() - start) / 882;
        let body = &pcm[start..start + cycles * 882];
        let amplitude = tone(body, 2, 176_400, 1000.0);
        // 0 dBFS PCM = 50 % modulation, so 0.7 input becomes 0.35.
        assert!(
            (amplitude - 0.35).abs() < 0.04,
            "{rate:?}/{order:?}: decoded tone {amplitude} should be about 0.35"
        );
        let harmonic = tone(body, 2, 176_400, 2000.0);
        let thd = 20.0 * (harmonic / amplitude.max(f64::MIN_POSITIVE)).log10();
        assert!(
            thd < -60.0,
            "{rate:?}/{order:?}: second harmonic {thd:.1} dB is above -60 dB"
        );
    }
}

// ----- DSP off vs on for the same file, and the signal path model -----

#[test]
fn the_signal_path_lists_every_active_stage() {
    let path = tone_wav("path.wav", 44_100, 16, 20_000, 1000.0);
    let mut config = PlayerConfig::new(NullBackend::device_id());
    config.src.target = sonder_engine::SrcTarget::Fixed(88_200);
    config.dsp.peq = Some(Peq {
        preamp_db: -3.0,
        bands: vec![PeqBand {
            freq_hz: 1_000.0,
            gain_db: 3.0,
            q: 1.0,
            ..PeqBand::default()
        }],
    });
    let (player, events, _control) = start(config, NullBackend::new());
    player.enqueue(vec![path]);
    player.play();
    let seen = wait_end(&events);

    let path = seen
        .iter()
        .find_map(|event| match event {
            PlayerEvent::SignalPath(path) => Some(path),
            _ => None,
        })
        .expect("signal path");
    assert!(!path.bit_perfect);
    assert_eq!(path.output_rate, 88_200);
    assert!(
        path.stages.iter().any(|s| s.name == "Resampler"),
        "{path:#?}"
    );
    assert!(path.stages.iter().any(|s| s.name == "PEQ"), "{path:#?}");
}

fn dirac(rate: u32, frames: usize) -> std::sync::Arc<ImpulseResponse> {
    let mut h = vec![0.0; frames];
    h[0] = 1.0;
    std::sync::Arc::new(ImpulseResponse::new("dirac.wav".into(), rate, vec![h]).unwrap())
}

#[test]
fn a_dirac_convolution_reaches_the_device_unchanged_across_a_gapless_join() {
    // Two tracks that do not fill a whole number of partitions: frames the convolver holds
    // must carry into the next track, and the response's tail must only come at the end.
    let first = wav("conv-a.wav", 44_100, 24, 10_001);
    let second = wav("conv-b.wav", 44_100, 24, 7_777);
    let mut config = PlayerConfig::new(NullBackend::device_id());
    config.dsp.convolution = Some(Ok(dirac(44_100, 3_000)));
    let (player, events, control) = start(config, NullBackend::new());
    player.enqueue(vec![first.clone(), second.clone()]);
    player.play();
    let seen = wait_end(&events);

    let sessions = control.sessions();
    assert_eq!(sessions.len(), 1, "{seen:#?}");
    let session = &sessions[0];
    let mut expected = decoded(&first);
    expected.extend(decoded(&second));
    let mut device = session.bytes.clone();
    let wanted = packed(session.format.container, &expected);
    // The response's own tail (zeros for a Dirac) follows the music at the end of the queue.
    let tail = device.split_off(wanted.len().min(device.len()));
    assert_eq!(device.len(), wanted.len(), "{seen:#?}");
    assert!(
        device == wanted,
        "the convolved samples differ from the source"
    );
    assert!(
        tail.iter().all(|&b| b == 0),
        "the tail after the music is not silent"
    );
}

#[test]
fn convolution_and_crossfeed_take_their_places_in_the_signal_path() {
    let path = tone_wav("path-conv.wav", 44_100, 16, 20_000, 1000.0);
    let mut config = PlayerConfig::new(NullBackend::device_id());
    config.src.target = sonder_engine::SrcTarget::Fixed(88_200);
    config.dsp.convolution = Some(Ok(dirac(48_000, 4_800)));
    config.dsp.crossfeed = Some(CrossfeedPreset::ChuMoy);
    config.dsp.peq = Some(Peq {
        preamp_db: -3.0,
        bands: vec![PeqBand {
            freq_hz: 1_000.0,
            gain_db: 3.0,
            q: 1.0,
            ..PeqBand::default()
        }],
    });
    let (player, events, _control) = start(config, NullBackend::new());
    player.enqueue(vec![path]);
    player.play();
    let seen = wait_end(&events);

    let path = seen
        .iter()
        .find_map(|event| match event {
            PlayerEvent::SignalPath(path) => Some(path),
            _ => None,
        })
        .expect("signal path");
    assert!(!path.bit_perfect);
    let names: Vec<&str> = path.stages.iter().map(|s| s.name.as_str()).collect();
    let at = |name: &str| names.iter().position(|n| *n == name);
    let (src, conv, peq, cross) = (
        at("Resampler").expect("resampler"),
        at("Convolution").expect("convolution"),
        at("PEQ").expect("peq"),
        at("Crossfeed").expect("crossfeed"),
    );
    assert!(src < conv && conv < peq && peq < cross, "{names:?}");
    let detail = &path.stages[conv].detail;
    assert!(
        detail.contains("8820 taps at 88200 Hz") && detail.contains("resampled from 48000 Hz"),
        "{detail}"
    );
    assert!(path.stages[cross].detail.starts_with("Chu Moy"));
}

#[test]
fn a_response_that_cannot_be_read_turns_convolution_off_with_a_warning() {
    let path = tone_wav("conv-missing.wav", 44_100, 16, 5_000, 1000.0);
    let mut config = PlayerConfig::new(NullBackend::device_id());
    config.dsp.convolution = Some(ImpulseResponse::load_cached(Path::new("no/such/ir.wav")));
    let (player, events, control) = start(config, NullBackend::new());
    player.enqueue(vec![path.clone()]);
    player.play();
    let seen = wait_end(&events);

    assert!(
        seen.iter().any(|event| matches!(
            event,
            PlayerEvent::Warning(text) if text.starts_with("convolution is off")
        )),
        "{seen:#?}"
    );
    // Nothing else is on, so the track still plays bit-perfect.
    let session = &control.sessions()[0];
    assert_eq!(
        session.bytes,
        packed(session.format.container, &decoded(&path))
    );
}

// ----- criterion: the bundle never contains Roon data -----

#[test]
fn the_repository_has_no_roon_filter_files() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    // Paths relative to the repository, so the folder the clone lives in never matches.
    let forbidden = |name: &str, path: &Path| {
        name.starts_with("polyphase_")
            || name == "filters_manifest.json"
            || name.ends_with(".decrypted")
            || (name.ends_with(".csv")
                && path.components().any(|c| {
                    c.as_os_str()
                        .to_string_lossy()
                        .to_lowercase()
                        .contains("filter")
                }))
    };
    let mut found = Vec::new();
    for entry in repository_files(root) {
        let name = entry
            .file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if forbidden(&name, &entry) {
            found.push(entry);
        }
    }
    assert!(
        found.is_empty(),
        "forbidden Roon data in the tree: {found:#?}"
    );
}

/// The files git tracks, relative to `root`, so a local, ignored filter folder (where a user
/// may keep their own Roon tables) does not fail the check. Without git, every file under
/// `root` except build output and dependencies.
fn repository_files(root: &Path) -> Vec<PathBuf> {
    let tracked = std::process::Command::new("git")
        .args(["ls-files", "-z"])
        .current_dir(root)
        .output()
        .ok()
        .filter(|output| output.status.success());
    match tracked {
        Some(output) => output
            .stdout
            .split(|&byte| byte == 0)
            .filter(|path| !path.is_empty())
            .map(|path| PathBuf::from(String::from_utf8_lossy(path).into_owned()))
            .collect(),
        None => walk_files(root)
            .into_iter()
            .filter_map(|path| path.strip_prefix(root).ok().map(Path::to_path_buf))
            .collect(),
    }
}

fn walk_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = entry.file_name();
            if name == "target" || name == "node_modules" || name == ".git" {
                continue;
            }
            out.extend(walk_files(&path));
        } else {
            out.push(path);
        }
    }
    out
}
