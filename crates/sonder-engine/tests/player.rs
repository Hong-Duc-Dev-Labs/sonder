//! Drives the player against the null backend and checks the exact bytes it would have
//! written to the device.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crossbeam_channel::Receiver;
use sonder_decode::Decoder;
use sonder_engine::{
    DsdMode, DsdPcmRate, DspConfig, PlaybackState, Player, PlayerConfig, PlayerEvent,
};
use sonder_output::format::{pack, unpack};
use sonder_output::null::{NullBackend, NullControl};
use sonder_output::{DeviceFormat, SampleContainer};

const TIMEOUT: Duration = Duration::from_secs(20);

fn temp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sonder-engine-tests-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Writes a WAV of a deterministic, track-specific signal and returns its path.
fn wav(name: &str, rate: u32, bits: u16, frames: usize, seed: i64) -> PathBuf {
    let path = temp_dir().join(name);
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: rate,
        bits_per_sample: bits,
        sample_format: hound::SampleFormat::Int,
    };
    let max = (1i64 << (bits - 1)) - 1;
    let mut w = hound::WavWriter::create(&path, spec).unwrap();
    for i in 0..frames as i64 * 2 {
        let v = ((i + seed) * 7919 + seed * 104_729) % (2 * max) - max;
        w.write_sample(v as i32).unwrap();
    }
    w.finalize().unwrap();
    path
}

fn decoded(path: &Path) -> Vec<i32> {
    let mut dec = Decoder::open(path).unwrap();
    let mut all = Vec::new();
    let mut block = Vec::new();
    while dec.next_i32(&mut block).unwrap() > 0 {
        all.extend_from_slice(&block);
    }
    all
}

fn packed(container: SampleContainer, samples: &[i32]) -> Vec<u8> {
    let mut out = vec![0; samples.len() * container.bytes()];
    pack(container, samples, &mut out);
    out
}

fn start(backend: NullBackend) -> (Player, Receiver<PlayerEvent>, NullControl) {
    start_with(PlayerConfig::new(NullBackend::device_id()), backend)
}

fn start_with(
    mut config: PlayerConfig,
    backend: NullBackend,
) -> (Player, Receiver<PlayerEvent>, NullControl) {
    let control = backend.control();
    config.device_poll = Duration::from_millis(20);
    let (player, events) = Player::spawn(Box::new(backend), config).unwrap();
    (player, events, control)
}

/// A device slow enough (64k frames/s) that commands land mid-track.
fn slow() -> NullBackend {
    NullBackend::new().with_period(64, Duration::from_millis(1))
}

/// Waits for an event matching `pred`, returning it and everything seen before it.
fn wait_for(
    events: &Receiver<PlayerEvent>,
    pred: impl Fn(&PlayerEvent) -> bool,
) -> (PlayerEvent, Vec<PlayerEvent>) {
    let deadline = Instant::now() + TIMEOUT;
    let mut seen = Vec::new();
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match events.recv_timeout(left) {
            Ok(event) if pred(&event) => return (event, seen),
            Ok(event) => seen.push(event),
            Err(_) => panic!("timed out; events so far: {seen:#?}"),
        }
    }
}

fn wait_end(events: &Receiver<PlayerEvent>) -> Vec<PlayerEvent> {
    wait_for(events, |e| *e == PlayerEvent::EndOfQueue).1
}

#[test]
fn same_format_tracks_play_gapless_and_byte_exact() {
    let a = wav("gapless-a.wav", 44_100, 16, 30_001, 1);
    let b = wav("gapless-b.wav", 44_100, 16, 20_000, 2);
    let (player, events, control) = start(NullBackend::new());
    player.enqueue(vec![a.clone(), b.clone()]);
    player.play();
    let seen = wait_end(&events);

    let sessions = control.sessions();
    assert_eq!(sessions.len(), 1, "one stream for the whole album");
    assert_eq!(
        sessions[0].format,
        DeviceFormat {
            rate: 44_100,
            container: SampleContainer::S16,
            channels: 2
        }
    );
    let mut expected = decoded(&a);
    expected.extend(decoded(&b));
    assert_eq!(sessions[0].bytes, packed(SampleContainer::S16, &expected));

    let tracks: Vec<usize> = seen
        .iter()
        .filter_map(|e| match e {
            PlayerEvent::TrackChanged { index, .. } => Some(*index),
            _ => None,
        })
        .collect();
    assert_eq!(tracks, vec![0, 1]);
}

#[test]
fn narrower_track_stays_in_the_open_wider_container() {
    let a = wav("narrow-a.wav", 48_000, 24, 10_000, 3);
    let b = wav("narrow-b.wav", 48_000, 16, 10_000, 4);
    let (player, events, control) = start(NullBackend::new());
    player.enqueue(vec![a.clone(), b.clone()]);
    player.play();
    wait_end(&events);

    let sessions = control.sessions();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].format.container, SampleContainer::S24Packed);
    let mut expected = decoded(&a);
    expected.extend(decoded(&b));
    assert_eq!(
        sessions[0].bytes,
        packed(SampleContainer::S24Packed, &expected)
    );
}

#[test]
fn rate_change_drains_and_reopens_the_device() {
    let a = wav("switch-a.wav", 44_100, 16, 15_000, 5);
    let b = wav("switch-b.wav", 96_000, 24, 15_000, 6);
    let (player, events, control) = start(NullBackend::new());
    player.enqueue(vec![a.clone(), b.clone()]);
    player.play();
    let seen = wait_end(&events);

    let sessions = control.sessions();
    assert_eq!(sessions.len(), 2);
    assert_eq!(sessions[0].format.rate, 44_100);
    assert_eq!(sessions[1].format.rate, 96_000);
    assert_eq!(sessions[1].format.container, SampleContainer::S24Packed);
    assert_eq!(
        sessions[0].bytes,
        packed(SampleContainer::S16, &decoded(&a))
    );
    assert_eq!(
        sessions[1].bytes,
        packed(SampleContainer::S24Packed, &decoded(&b))
    );
    let formats = seen
        .iter()
        .filter(|e| matches!(e, PlayerEvent::OutputFormat(_)))
        .count();
    assert_eq!(formats, 2);
}

#[test]
fn seek_before_play_starts_on_the_exact_frame() {
    let a = wav("seek-stopped.wav", 44_100, 16, 50_000, 7);
    let (player, events, control) = start(NullBackend::new());
    player.enqueue(vec![a.clone()]);
    // 0.5 s = frame 22 050.
    player.seek(Duration::from_millis(500));
    player.play();
    wait_end(&events);

    let all = decoded(&a);
    assert_eq!(
        control.played_bytes(),
        packed(SampleContainer::S16, &all[22_050 * 2..])
    );
}

#[test]
fn seek_while_playing_continues_from_the_target() {
    let a = wav("seek-playing.wav", 44_100, 16, 132_300, 8);
    let (player, events, control) = start(slow());
    player.enqueue(vec![a.clone()]);
    player.play();
    wait_for(
        &events,
        |e| matches!(e, PlayerEvent::Position { elapsed, .. } if !elapsed.is_zero()),
    );
    player.seek(Duration::from_secs(2));
    let seen = wait_end(&events);
    assert!(
        !seen
            .iter()
            .any(|e| matches!(e, PlayerEvent::TrackChanged { .. })),
        "a seek within the track is not a track change: {seen:#?}"
    );

    let all = packed(SampleContainer::S16, &decoded(&a));
    let tail = &all[2 * 44_100 * 4..];
    let played = control.played_bytes();
    assert!(
        played.ends_with(tail),
        "output must end with the track from 2 s"
    );
    let head = &played[..played.len() - tail.len()];
    assert!(
        !head.is_empty() && all.starts_with(head),
        "before the seek: the track start"
    );
}

#[test]
fn pause_holds_the_device_and_resume_finishes_exactly() {
    let a = wav("pause.wav", 44_100, 16, 88_200, 9);
    let (player, events, control) = start(slow());
    player.enqueue(vec![a.clone()]);
    player.play();
    wait_for(
        &events,
        |e| matches!(e, PlayerEvent::Position { elapsed, .. } if !elapsed.is_zero()),
    );
    player.pause();
    wait_for(&events, |e| *e == PlayerEvent::State(PlaybackState::Paused));
    std::thread::sleep(Duration::from_millis(50));
    let frozen = control.played_bytes().len();
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(
        control.played_bytes().len(),
        frozen,
        "no audio while paused"
    );
    assert!(control.is_open(), "exclusive access is kept while paused");

    player.play();
    wait_end(&events);
    assert_eq!(
        control.played_bytes(),
        packed(SampleContainer::S16, &decoded(&a))
    );
}

#[test]
fn next_jumps_and_keeps_the_rest_gapless() {
    let a = wav("next-a.wav", 44_100, 16, 132_300, 10);
    let b = wav("next-b.wav", 44_100, 16, 10_000, 11);
    let c = wav("next-c.wav", 44_100, 16, 10_000, 12);
    let (player, events, control) = start(slow());
    player.enqueue(vec![a.clone(), b.clone(), c.clone()]);
    player.play();
    wait_for(
        &events,
        |e| matches!(e, PlayerEvent::Position { index: 0, elapsed } if !elapsed.is_zero()),
    );
    player.next();
    wait_end(&events);

    let mut rest = decoded(&b);
    rest.extend(decoded(&c));
    let rest = packed(SampleContainer::S16, &rest);
    let played = control.played_bytes();
    assert_eq!(control.sessions().len(), 1);
    assert!(played.ends_with(&rest));
    let head = &played[..played.len() - rest.len()];
    assert!(packed(SampleContainer::S16, &decoded(&a)).starts_with(head));
}

#[test]
fn device_loss_pauses_and_replug_resumes_where_it_stopped() {
    let a = wav("lost.wav", 44_100, 16, 88_200, 13);
    let (player, events, control) = start(slow());
    player.enqueue(vec![a.clone()]);
    player.play();
    wait_for(
        &events,
        |e| matches!(e, PlayerEvent::Position { elapsed, .. } if !elapsed.is_zero()),
    );
    control.unplug();
    // The stream error and the device-list poll race; both must arrive, in either order.
    let (_, mut seen) = wait_for(&events, |e| *e == PlayerEvent::State(PlaybackState::Paused));
    if !seen.contains(&PlayerEvent::DevicesChanged(Vec::new())) {
        seen.push(wait_for(&events, |e| *e == PlayerEvent::DevicesChanged(Vec::new())).0);
    }
    assert!(
        seen.iter().any(|e| matches!(e, PlayerEvent::DeviceLost(_))),
        "{seen:#?}"
    );

    control.replug();
    wait_for(
        &events,
        |e| matches!(e, PlayerEvent::DevicesChanged(ids) if !ids.is_empty()),
    );
    // The player reopens on its own, paused, at the saved position.
    wait_for(&events, |e| matches!(e, PlayerEvent::OutputFormat(_)));
    player.play();
    wait_end(&events);

    assert_eq!(control.sessions().len(), 2);
    assert_eq!(
        control.played_bytes(),
        packed(SampleContainer::S16, &decoded(&a)),
        "nothing lost or repeated across the reconnect"
    );
}

#[test]
fn busy_device_pauses_and_play_retries() {
    let a = wav("busy.wav", 44_100, 16, 20_000, 14);
    let (player, events, control) = start(NullBackend::new());
    control.fail_next_open_busy();
    player.enqueue(vec![a.clone()]);
    player.play();
    wait_for(&events, |e| *e == PlayerEvent::DeviceBusy);
    wait_for(&events, |e| *e == PlayerEvent::State(PlaybackState::Paused));
    assert!(control.sessions().is_empty());

    player.play();
    wait_end(&events);
    assert_eq!(
        control.played_bytes(),
        packed(SampleContainer::S16, &decoded(&a))
    );
}

#[test]
fn xrun_is_recovered_in_place_and_reported() {
    let a = wav("xrun.wav", 44_100, 16, 44_100, 15);
    let (player, events, control) = start(slow());
    player.enqueue(vec![a.clone()]);
    player.play();
    wait_for(&events, |e| matches!(e, PlayerEvent::OutputFormat(_)));
    control.inject_xrun();
    wait_for(
        &events,
        |e| matches!(e, PlayerEvent::Underruns(n) if *n >= 1),
    );
    wait_end(&events);
    assert_eq!(control.sessions().len(), 1);
    assert_eq!(
        control.played_bytes(),
        packed(SampleContainer::S16, &decoded(&a))
    );
}

#[test]
fn unsupported_format_is_refused_and_skipped() {
    let a = wav("unsupported-a.wav", 96_000, 16, 5_000, 16);
    let b = wav("unsupported-b.wav", 44_100, 16, 5_000, 17);
    let only = DeviceFormat {
        rate: 44_100,
        container: SampleContainer::S16,
        channels: 2,
    };
    let mut config = PlayerConfig::new(NullBackend::device_id());
    config.src.auto_fallback = false;
    let (player, events, control) = start_with(config, NullBackend::new().with_formats(vec![only]));
    player.enqueue(vec![a, b.clone()]);
    player.play();
    let (failed, _) = wait_for(&events, |e| matches!(e, PlayerEvent::TrackFailed { .. }));
    let PlayerEvent::TrackFailed { index, error, .. } = failed else {
        unreachable!()
    };
    assert_eq!(index, 0);
    assert!(error.contains("96000 Hz"), "{error}");
    wait_end(&events);
    assert_eq!(
        control.played_bytes(),
        packed(SampleContainer::S16, &decoded(&b))
    );
}

#[test]
fn unsupported_rate_falls_back_to_src_and_says_not_bit_perfect() {
    let a = wav("fallback-src.wav", 96_000, 16, 5_000, 117);
    let only = DeviceFormat {
        rate: 44_100,
        container: SampleContainer::S16,
        channels: 2,
    };
    let (player, events, control) = start(NullBackend::new().with_formats(vec![only]));
    player.enqueue(vec![a]);
    player.play();
    let seen = wait_end(&events);

    assert!(
        seen.iter().any(|event| matches!(
            event,
            PlayerEvent::Warning(message) if message.contains("resampling to 44100 Hz")
        )),
        "no fallback warning: {seen:#?}"
    );
    let path = seen
        .iter()
        .find_map(|event| match event {
            PlayerEvent::SignalPath(path) => Some(path),
            _ => None,
        })
        .expect("signal path for the resampled track");
    assert_eq!(path.output_rate, 44_100);
    assert!(!path.bit_perfect, "SRC fallback must not claim bit-perfect");
    assert!(
        path.stages.iter().any(|stage| stage.name == "Resampler"),
        "signal path lacks a resampler: {path:#?}"
    );
    assert_eq!(control.sessions().len(), 1);
    assert_eq!(control.sessions()[0].format.rate, 44_100);
}

#[test]
fn a_192k_file_on_a_96k_device_plays_via_src_fallback() {
    let a = wav("fallback-192k.wav", 192_000, 24, 5_000, 118);
    let only = DeviceFormat {
        rate: 96_000,
        container: SampleContainer::S24Packed,
        channels: 2,
    };
    let (player, events, control) = start(NullBackend::new().with_formats(vec![only]));
    player.enqueue(vec![a]);
    player.play();
    let seen = wait_end(&events);

    let path = seen
        .iter()
        .find_map(|event| match event {
            PlayerEvent::SignalPath(path) => Some(path),
            _ => None,
        })
        .expect("signal path for the resampled track");
    assert_eq!(path.output_rate, 96_000);
    assert!(!path.bit_perfect);
    assert!(seen.iter().any(|event| matches!(
        event,
        PlayerEvent::Warning(message) if message.contains("resampling to 96000 Hz")
    )));
    assert_eq!(control.sessions().len(), 1);
    assert_eq!(control.sessions()[0].format.rate, 96_000);
}

#[test]
fn roon_enabled_without_a_48_to_44_table_reports_sonder() {
    let a = wav("roon-48k.wav", 48_000, 16, 5_000, 119);
    let only = DeviceFormat {
        rate: 44_100,
        container: SampleContainer::S16,
        channels: 2,
    };
    let roon = temp_dir().join("roon-empty");
    std::fs::create_dir_all(&roon).unwrap();
    let mut config = PlayerConfig::new(NullBackend::device_id());
    config.src.filter_source = sonder_engine::FilterSourceConfig::Roon { dir: roon };
    let (player, events, _) = start_with(config, NullBackend::new().with_formats(vec![only]));
    player.enqueue(vec![a]);
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
    assert!(
        path.stages.iter().any(|stage| {
            stage.detail.contains("48000 → 44100")
                && stage.detail.contains("Sonder")
                && !stage.detail.contains("Roon")
        }),
        "signal path must name Sonder for the missing Roon ratio: {path:#?}"
    );
}

#[test]
fn refused_fixed_target_falls_back_to_the_bit_perfect_source_rate() {
    let a = wav("fixed-target.wav", 44_100, 16, 5_000, 120);
    let only = DeviceFormat {
        rate: 44_100,
        container: SampleContainer::S16,
        channels: 2,
    };
    let mut config = PlayerConfig::new(NullBackend::device_id());
    config.src.target = sonder_engine::SrcTarget::Fixed(96_000);
    let (player, events, control) = start_with(config, NullBackend::new().with_formats(vec![only]));
    player.enqueue(vec![a]);
    player.play();
    let seen = wait_end(&events);

    assert!(seen.iter().any(|event| matches!(
        event,
        PlayerEvent::Warning(message) if message.contains("keeping the source rate 44100 Hz")
    )));
    let path = seen
        .iter()
        .find_map(|event| match event {
            PlayerEvent::SignalPath(path) => Some(path),
            _ => None,
        })
        .expect("signal path");
    assert_eq!(path.output_rate, 44_100);
    assert!(path.bit_perfect, "{path:#?}");
    assert_eq!(control.sessions()[0].format.rate, 44_100);
}

#[test]
fn volume_stage_scales_and_removing_it_restores_bit_perfect() {
    let a = wav("volume.wav", 44_100, 16, 8_000, 18);
    let (player, events, control) = start(NullBackend::new());
    player.set_volume(Some(-20.0));
    player.enqueue(vec![a.clone()]);
    player.play();
    wait_end(&events);

    let session = &control.sessions()[0];
    assert_eq!(session.format.container, SampleContainer::S24Packed);
    let mut out = vec![0i32; session.bytes.len() / 3];
    unpack(SampleContainer::S24Packed, &session.bytes, &mut out);
    let src = decoded(&a);
    assert_eq!(out.len(), src.len());
    for (o, s) in out.iter().zip(&src) {
        let expected = f64::from(*s) * 0.1;
        // Packing to 24 bits truncates the low byte.
        assert!(
            (f64::from(*o) - expected).abs() <= 257.0,
            "{o} vs {expected}"
        );
    }

    player.set_volume(None);
    player.play();
    wait_end(&events);
    assert_eq!(
        control.sessions()[1].bytes,
        packed(SampleContainer::S16, &src)
    );
}

#[test]
fn broken_file_is_skipped_with_a_reason() {
    let bad = temp_dir().join("broken.flac");
    std::fs::write(&bad, b"not audio").unwrap();
    let b = wav("after-broken.wav", 44_100, 16, 5_000, 19);
    let (player, events, control) = start(NullBackend::new());
    player.enqueue(vec![bad, b.clone()]);
    player.play();
    wait_for(&events, |e| {
        matches!(e, PlayerEvent::TrackFailed { index: 0, .. })
    });
    wait_end(&events);
    assert_eq!(
        control.played_bytes(),
        packed(SampleContainer::S16, &decoded(&b))
    );
}

fn track_changes(events: &[PlayerEvent]) -> Vec<usize> {
    events
        .iter()
        .filter_map(|e| match e {
            PlayerEvent::TrackChanged { index, .. } => Some(*index),
            _ => None,
        })
        .collect()
}

#[test]
fn next_pressed_twice_skips_two_tracks() {
    let a = wav("next2-a.wav", 44_100, 16, 132_300, 20);
    let b = wav("next2-b.wav", 44_100, 16, 10_000, 21);
    let c = wav("next2-c.wav", 44_100, 16, 10_000, 22);
    let (player, events, control) = start(slow());
    player.enqueue(vec![a.clone(), b, c.clone()]);
    player.play();
    wait_for(
        &events,
        |e| matches!(e, PlayerEvent::Position { index: 0, elapsed } if !elapsed.is_zero()),
    );
    player.next();
    player.next();
    let seen = wait_end(&events);

    assert_eq!(track_changes(&seen), vec![2]);
    let tail = packed(SampleContainer::S16, &decoded(&c));
    let played = control.played_bytes();
    assert!(played.ends_with(&tail));
    let head = &played[..played.len() - tail.len()];
    assert!(packed(SampleContainer::S16, &decoded(&a)).starts_with(head));
}

#[test]
fn seek_right_after_next_applies_to_the_new_track() {
    let a = wav("next-seek-a.wav", 44_100, 16, 132_300, 23);
    let b = wav("next-seek-b.wav", 44_100, 16, 88_200, 24);
    let (player, events, control) = start(slow());
    player.enqueue(vec![a.clone(), b.clone()]);
    player.play();
    wait_for(
        &events,
        |e| matches!(e, PlayerEvent::Position { index: 0, elapsed } if !elapsed.is_zero()),
    );
    player.next();
    player.seek(Duration::from_secs(1));
    wait_end(&events);

    let all_b = decoded(&b);
    let tail = packed(SampleContainer::S16, &all_b[44_100 * 2..]);
    let played = control.played_bytes();
    assert!(played.ends_with(&tail), "track 2 from 1 s");
    let head = &played[..played.len() - tail.len()];
    assert!(packed(SampleContainer::S16, &decoded(&a)).starts_with(head));
}

#[test]
fn play_after_the_end_starts_at_the_first_added_track() {
    let a = wav("after-end-a.wav", 44_100, 16, 5_000, 25);
    let b = wav("after-end-b.wav", 44_100, 16, 5_000, 26);
    let (player, events, control) = start(NullBackend::new());
    player.enqueue(vec![a.clone()]);
    player.play();
    wait_end(&events);

    player.enqueue(vec![b.clone()]);
    player.play();
    let seen = wait_end(&events);
    assert_eq!(track_changes(&seen), vec![1]);
    let sessions = control.sessions();
    assert_eq!(sessions.len(), 2);
    assert_eq!(
        sessions[1].bytes,
        packed(SampleContainer::S16, &decoded(&b))
    );

    // With nothing new added, play after the end starts the queue over.
    player.play();
    let seen = wait_end(&events);
    assert_eq!(track_changes(&seen), vec![0, 1]);
}

#[test]
fn play_index_starts_that_entry_from_stop_and_while_playing() {
    let a = wav("index-a.wav", 44_100, 16, 132_300, 30);
    let b = wav("index-b.wav", 44_100, 16, 10_000, 31);
    let c = wav("index-c.wav", 44_100, 16, 10_000, 32);
    let (player, events, control) = start(slow());
    player.enqueue(vec![a.clone(), b.clone(), c.clone()]);

    player.play_index(2);
    let (event, _) = wait_for(&events, |e| matches!(e, PlayerEvent::TrackChanged { .. }));
    assert!(matches!(event, PlayerEvent::TrackChanged { index: 2, .. }));
    wait_end(&events);
    assert_eq!(
        control.played_bytes(),
        packed(SampleContainer::S16, &decoded(&c))
    );

    player.play_index(0);
    wait_for(
        &events,
        |e| matches!(e, PlayerEvent::Position { index: 0, elapsed } if !elapsed.is_zero()),
    );
    player.play_index(1);
    let (event, _) = wait_for(
        &events,
        |e| matches!(e, PlayerEvent::TrackChanged { index, .. } if *index != 0),
    );
    assert!(matches!(event, PlayerEvent::TrackChanged { index: 1, .. }));
    wait_end(&events);
    let session = control.sessions().pop().unwrap();
    let mut rest = decoded(&b);
    rest.extend(decoded(&c));
    assert!(
        session
            .bytes
            .ends_with(&packed(SampleContainer::S16, &rest))
    );
}

#[test]
fn switching_device_moves_playback_without_losing_or_repeating_frames() {
    let a = wav("switch.wav", 44_100, 16, 88_200, 33);
    let (player, events, control) = start(slow());
    player.enqueue(vec![a.clone()]);
    player.play();
    wait_for(
        &events,
        |e| matches!(e, PlayerEvent::Position { elapsed, .. } if !elapsed.is_zero()),
    );

    // A device that is not present: playback holds and pauses.
    player.set_device(sonder_output::DeviceId("missing".into()));
    let (_, seen) = wait_for(&events, |e| *e == PlayerEvent::State(PlaybackState::Paused));
    assert!(
        seen.iter().any(|e| matches!(e, PlayerEvent::DeviceLost(_))),
        "{seen:#?}"
    );
    assert!(!control.is_open(), "the old device is released");

    // Back to a present device: the held position reopens there.
    player.set_device(NullBackend::device_id());
    wait_for(&events, |e| matches!(e, PlayerEvent::OutputFormat(_)));
    player.play();
    wait_end(&events);

    assert_eq!(control.sessions().len(), 2);
    assert_eq!(
        control.played_bytes(),
        packed(SampleContainer::S16, &decoded(&a))
    );
}

/// Track indices of the `TrackChanged` events in `events`, with their file names.
fn changes(events: &[PlayerEvent]) -> Vec<(usize, String)> {
    events
        .iter()
        .filter_map(|e| match e {
            PlayerEvent::TrackChanged { index, path, .. } => Some((
                *index,
                path.file_name().unwrap().to_string_lossy().into_owned(),
            )),
            _ => None,
        })
        .collect()
}

fn concat(paths: &[&PathBuf]) -> Vec<u8> {
    let mut all = Vec::new();
    for p in paths {
        all.extend(decoded(p));
    }
    packed(SampleContainer::S16, &all)
}

fn wait_playing(events: &Receiver<PlayerEvent>, index: usize) -> Vec<PlayerEvent> {
    let (event, mut seen) = wait_for(
        events,
        |e| matches!(e, PlayerEvent::Position { index: i, elapsed } if *i == index && !elapsed.is_zero()),
    );
    seen.push(event);
    seen
}

#[test]
fn removing_an_entry_decoded_ahead_skips_it_gaplessly() {
    // `a` is short enough that `b` is already decoded when the removal lands.
    let a = wav("rm-ahead-a.wav", 44_100, 16, 30_000, 40);
    let b = wav("rm-ahead-b.wav", 44_100, 16, 20_000, 41);
    let c = wav("rm-ahead-c.wav", 44_100, 16, 20_000, 42);
    let (player, events, control) = start(slow());
    player.enqueue(vec![a.clone(), b.clone(), c.clone()]);
    player.play();
    let mut seen = wait_playing(&events, 0);
    player.remove(1);
    seen.extend(wait_end(&events));

    assert_eq!(
        control.sessions().len(),
        1,
        "one stream: the skip stays gapless"
    );
    assert_eq!(control.played_bytes(), concat(&[&a, &c]));
    assert_eq!(
        changes(&seen),
        [(0, "rm-ahead-a.wav".into()), (1, "rm-ahead-c.wav".into())]
    );
}

#[test]
fn removing_an_entry_before_the_playing_one_shifts_indices() {
    let a = wav("rm-before-a.wav", 44_100, 16, 10_000, 43);
    let b = wav("rm-before-b.wav", 44_100, 16, 60_000, 44);
    let c = wav("rm-before-c.wav", 44_100, 16, 10_000, 45);
    let (player, events, control) = start(slow());
    player.enqueue(vec![a.clone(), b.clone(), c.clone()]);
    player.play_index(1);
    let mut seen = wait_playing(&events, 1);
    player.remove(0);
    // `b` is now entry 0 and keeps playing.
    seen.extend(wait_playing(&events, 0));
    seen.extend(wait_end(&events));

    assert_eq!(control.played_bytes(), concat(&[&b, &c]));
    assert_eq!(
        changes(&seen),
        [(1, "rm-before-b.wav".into()), (1, "rm-before-c.wav".into())]
    );
}

#[test]
fn removing_the_playing_entry_moves_on_to_the_next() {
    let a = wav("rm-current-a.wav", 44_100, 16, 132_300, 46);
    let b = wav("rm-current-b.wav", 44_100, 16, 10_000, 47);
    let (player, events, control) = start(slow());
    player.enqueue(vec![a.clone(), b.clone()]);
    player.play();
    wait_playing(&events, 0);
    player.remove(0);
    let seen = wait_end(&events);

    assert_eq!(changes(&seen), [(0, "rm-current-b.wav".into())]);
    let played = control.played_bytes();
    let rest = concat(&[&b]);
    assert!(played.ends_with(&rest));
    assert!(concat(&[&a]).starts_with(&played[..played.len() - rest.len()]));
}

#[test]
fn removing_the_last_playing_entry_ends_the_queue() {
    let a = wav("rm-last-a.wav", 44_100, 16, 132_300, 48);
    let (player, events, control) = start(slow());
    player.enqueue(vec![a.clone()]);
    player.play();
    wait_playing(&events, 0);
    player.remove(0);
    wait_end(&events);
    wait_for(&events, |e| {
        *e == PlayerEvent::State(PlaybackState::Stopped)
    });
    assert!(!control.is_open());
}

#[test]
fn inserting_after_the_playing_entry_plays_it_next_gaplessly() {
    let a = wav("ins-next-a.wav", 44_100, 16, 30_000, 49);
    let b = wav("ins-next-b.wav", 44_100, 16, 20_000, 50);
    let c = wav("ins-next-c.wav", 44_100, 16, 20_000, 51);
    let (player, events, control) = start(slow());
    player.enqueue(vec![a.clone(), c.clone()]);
    player.play();
    let mut seen = wait_playing(&events, 0);
    player.insert(1, vec![b.clone()]);
    seen.extend(wait_end(&events));

    assert_eq!(control.sessions().len(), 1);
    assert_eq!(control.played_bytes(), concat(&[&a, &b, &c]));
    assert_eq!(
        changes(&seen),
        [
            (0, "ins-next-a.wav".into()),
            (1, "ins-next-b.wav".into()),
            (2, "ins-next-c.wav".into())
        ]
    );
}

#[test]
fn inserting_after_a_fully_decoded_queue_continues_the_stream() {
    let a = wav("ins-end-a.wav", 44_100, 16, 30_000, 52);
    let b = wav("ins-end-b.wav", 44_100, 16, 10_000, 53);
    let (player, events, control) = start(slow());
    player.enqueue(vec![a.clone()]);
    player.play();
    wait_playing(&events, 0);
    player.insert(1, vec![b.clone()]);
    let seen = wait_end(&events);

    assert_eq!(control.sessions().len(), 1);
    assert_eq!(control.played_bytes(), concat(&[&a, &b]));
    assert_eq!(changes(&seen), [(1, "ins-end-b.wav".into())]);
}

#[test]
fn inserting_before_the_playing_entry_shifts_it() {
    let a = wav("ins-before-a.wav", 44_100, 16, 60_000, 54);
    let z = wav("ins-before-z.wav", 44_100, 16, 10_000, 55);
    let (player, events, control) = start(slow());
    player.enqueue(vec![a.clone()]);
    player.play();
    wait_playing(&events, 0);
    player.insert(0, vec![z]);
    wait_playing(&events, 1);
    wait_end(&events);
    assert_eq!(
        control.played_bytes(),
        concat(&[&a]),
        "the inserted entry is not played"
    );
}

#[test]
fn queue_edits_while_paused_report_the_moved_current_entry() {
    let a = wav("paused-edit-a.wav", 44_100, 16, 132_300, 56);
    let z = wav("paused-edit-z.wav", 44_100, 16, 10_000, 57);
    let (player, events, _control) = start(slow());
    player.enqueue(vec![a]);
    player.play();
    wait_playing(&events, 0);
    player.pause();
    wait_for(&events, |e| *e == PlayerEvent::State(PlaybackState::Paused));
    player.insert(0, vec![z]);
    wait_for(&events, |e| {
        matches!(e, PlayerEvent::Position { index: 1, .. })
    });
    player.remove(0);
    wait_for(&events, |e| {
        matches!(e, PlayerEvent::Position { index: 0, .. })
    });
}

// ----- DSD fixtures and DoP verification -----

const DSD64: u32 = 2_822_400;

/// Deterministic interleaved DSD bytes with no 0x69 byte, so 0x6969 can only be DoP idle.
fn dsd_bytes(frames: usize, seed: u32) -> Vec<u8> {
    (0..frames * 2)
        .map(|i| {
            let byte = ((i as u32 * 37 + seed * 101 + 11) % 251) as u8;
            if byte == 0x69 { 0x6A } else { byte }
        })
        .collect()
}

/// Writes a DSF whose audio is `interleaved` (channel-interleaved MSB-first bytes),
/// stored as channel-major blocks as the format requires.
fn dsf(name: &str, interleaved: &[u8]) -> PathBuf {
    let path = temp_dir().join(name);
    let channels = 2usize;
    let block = 512usize;
    let frames = interleaved.len() / channels;
    let groups = frames.div_ceil(block);
    let data_len = (groups * block * channels) as u64;

    let mut file = std::fs::File::create(&path).unwrap();
    file.write_all(b"DSD ").unwrap();
    file.write_all(&28u64.to_le_bytes()).unwrap();
    file.write_all(&(28 + 52 + 12 + data_len).to_le_bytes())
        .unwrap();
    file.write_all(&0u64.to_le_bytes()).unwrap();
    file.write_all(b"fmt ").unwrap();
    file.write_all(&52u64.to_le_bytes()).unwrap();
    file.write_all(&1u32.to_le_bytes()).unwrap(); // version
    file.write_all(&0u32.to_le_bytes()).unwrap(); // raw DSD
    file.write_all(&2u32.to_le_bytes()).unwrap(); // stereo
    file.write_all(&2u32.to_le_bytes()).unwrap();
    file.write_all(&DSD64.to_le_bytes()).unwrap();
    file.write_all(&8u32.to_le_bytes()).unwrap(); // MSB first
    file.write_all(&((frames as u64) * 8).to_le_bytes())
        .unwrap();
    file.write_all(&(block as u32).to_le_bytes()).unwrap();
    file.write_all(&0u32.to_le_bytes()).unwrap();
    file.write_all(b"data").unwrap();
    file.write_all(&(12 + data_len).to_le_bytes()).unwrap();
    for group in 0..groups {
        let start = group * block;
        for channel in 0..channels {
            for frame in start..start + block {
                let at = frame * channels + channel;
                file.write_all(&[interleaved.get(at).copied().unwrap_or(0)])
                    .unwrap();
            }
        }
    }
    path
}

fn chunk(out: &mut Vec<u8>, id: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(id);
    out.extend_from_slice(&(data.len() as u64).to_be_bytes());
    out.extend_from_slice(data);
    if data.len() % 2 == 1 {
        out.push(0);
    }
}

/// Writes an uncompressed DSDIFF file holding `interleaved` bytes.
fn dff(name: &str, interleaved: &[u8]) -> PathBuf {
    let path = temp_dir().join(name);
    let mut prop = Vec::new();
    prop.extend_from_slice(b"SND ");
    chunk(&mut prop, b"FS  ", &DSD64.to_be_bytes());
    let mut chnl = Vec::new();
    chnl.extend_from_slice(&2u16.to_be_bytes());
    chnl.extend_from_slice(b"SLFTSRGT");
    chunk(&mut prop, b"CHNL", &chnl);
    chunk(&mut prop, b"CMPR", b"DSD ");

    let mut body = Vec::new();
    body.extend_from_slice(b"DSD ");
    chunk(&mut body, b"FVER", &0x0105_0000u32.to_be_bytes());
    chunk(&mut body, b"PROP", &prop);
    chunk(&mut body, b"DSD ", interleaved);

    let mut file = std::fs::File::create(&path).unwrap();
    file.write_all(b"FRM8").unwrap();
    file.write_all(&(body.len() as u64).to_be_bytes()).unwrap();
    file.write_all(&body).unwrap();
    path
}

/// Decodes a recorded DoP session, asserting marker alternation and no zero words, and
/// returning the source DSD bytes (idle frames dropped) plus the idle-frame count.
fn decode_dop(bytes: &[u8], channels: usize) -> (Vec<u8>, usize) {
    let words: Vec<u32> = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|w| u32::from_le_bytes(*w))
        .collect();
    assert_eq!(words.len() % channels, 0, "partial carrier frame");
    let frames = words.len() / channels;
    let mut out = Vec::new();
    let mut idle = 0;
    for frame in 0..frames {
        let expected = if frame % 2 == 0 { 0x05 } else { 0xFA };
        let mut payloads = Vec::with_capacity(channels);
        for channel in 0..channels {
            let word = words[frame * channels + channel];
            assert_ne!(word, 0, "DoP stream contains a zero word");
            assert_eq!(word >> 24, expected, "marker at frame {frame}");
            payloads.push(((word >> 8) & 0xFFFF) as u16);
        }
        if payloads.iter().all(|&p| p == 0x6969) {
            idle += 1;
            continue;
        }
        for byte in [0u16, 1] {
            for &payload in &payloads {
                out.push((payload >> (8 * (1 - byte as u32))) as u8);
            }
        }
    }
    (out, idle)
}

/// The null device paced at 64k frames/s (128k frames/s for stereo), slower than any DSD
/// carrier so decoding keeps up instead of injecting underruns.
fn dsd_device() -> NullBackend {
    NullBackend::new().with_period(512, Duration::from_millis(8))
}

#[test]
fn dop_playback_roundtrips_the_source_dsd_bytes() {
    let expected = dsd_bytes(20_000, 1);
    let track = dsf("dop.dsf", &expected);
    let (player, events, control) = start(dsd_device());
    player.enqueue(vec![track]);
    player.play();
    let seen = wait_end(&events);

    assert!(
        seen.iter().any(|e| matches!(
            e,
            PlayerEvent::DsdMode {
                mode: DsdMode::Dop,
                reason: None
            }
        )),
        "events: {seen:#?}"
    );
    let sessions = control.sessions();
    assert_eq!(sessions.len(), 1, "one DoP stream");
    assert_eq!(
        sessions[0].format,
        DeviceFormat {
            rate: 176_400,
            container: SampleContainer::S24In32,
            channels: 2
        }
    );
    let (payload, idle) = decode_dop(&sessions[0].bytes, 2);
    assert_eq!(payload, expected, "idle frames: {idle}");
}

#[test]
fn dop_marker_phase_continues_across_gapless_tracks() {
    let first = dsd_bytes(12_000, 2);
    let second = dsd_bytes(10_000, 3);
    let a = dsf("dop-gapless-a.dsf", &first);
    let b = dsf("dop-gapless-b.dsf", &second);
    let (player, events, control) = start(dsd_device());
    player.enqueue(vec![a, b]);
    player.play();
    wait_end(&events);

    let sessions = control.sessions();
    assert_eq!(sessions.len(), 1, "same-rate DSD stays on one stream");
    let (payload, idle) = decode_dop(&sessions[0].bytes, 2);
    let mut expected = first;
    expected.extend_from_slice(&second);
    assert_eq!(payload, expected, "idle frames: {idle}");
}

#[test]
fn dff_plays_through_dop_with_the_same_bytes() {
    let expected = dsd_bytes(8_000, 4);
    let track = dff("dop.dff", &expected);
    let (player, events, control) = start(dsd_device());
    player.enqueue(vec![track]);
    player.play();
    wait_end(&events);

    let sessions = control.sessions();
    assert_eq!(sessions.len(), 1);
    let (payload, idle) = decode_dop(&sessions[0].bytes, 2);
    assert_eq!(payload, expected, "idle frames: {idle}");
}

#[test]
fn native_dsd_sends_four_source_bytes_per_word() {
    let expected = dsd_bytes(8_000, 5);
    let track = dsf("native.dsf", &expected);
    let mut config = PlayerConfig::new(NullBackend::device_id());
    config.default_dsd_mode = DsdMode::Native;
    let (player, events, control) = start_with(config, dsd_device());
    player.enqueue(vec![track]);
    player.play();
    let seen = wait_end(&events);

    assert!(seen.iter().any(|e| matches!(
        e,
        PlayerEvent::DsdMode {
            mode: DsdMode::Native,
            ..
        }
    )));
    let sessions = control.sessions();
    assert_eq!(
        sessions[0].format,
        DeviceFormat {
            rate: 88_200,
            container: SampleContainer::DsdU32Be,
            channels: 2
        }
    );
    let mut audio = Vec::new();
    for group in sessions[0].bytes.as_chunks::<8>().0 {
        if group.iter().all(|&b| b == 0x69) {
            continue;
        }
        for byte in 0..4 {
            audio.push(group[byte]);
            audio.push(group[4 + byte]);
        }
    }
    assert_eq!(audio, expected);
}

#[test]
fn dsd_mode_pcm_converts_instead_of_packing() {
    let expected = dsd_bytes(8_000, 6);
    let track = dsf("pcm.dsf", &expected);
    let mut config = PlayerConfig::new(NullBackend::device_id());
    config.default_dsd_mode = DsdMode::Pcm;
    config.dsd_pcm_rate = DsdPcmRate::R352_8;
    let (player, events, control) = start_with(config, dsd_device());
    player.enqueue(vec![track]);
    player.play();
    let seen = wait_end(&events);

    assert!(seen.iter().any(|e| matches!(
        e,
        PlayerEvent::DsdMode {
            mode: DsdMode::Pcm,
            reason: None
        }
    )));
    let sessions = control.sessions();
    assert_eq!(sessions[0].format.rate, 352_800);
    assert_eq!(sessions[0].format.container, SampleContainer::S24Packed);

    // The device bytes are exactly the converter's output, down to the frame.
    let mut converter = sonder_dsp::DsdToPcm::new(DSD64, 352_800, 2).unwrap();
    let mut pcm = Vec::new();
    converter.process(&expected, &mut pcm);
    let samples: Vec<i32> = pcm
        .iter()
        .map(|&s| {
            (s * 2_147_483_648.0)
                .round()
                .clamp(-2_147_483_648.0, 2_147_483_647.0) as i32
        })
        .collect();
    assert_eq!(
        sessions[0].bytes,
        packed(SampleContainer::S24Packed, &samples)
    );
}

#[test]
fn a_pcm_stage_forces_dsd_to_pcm_with_a_reason() {
    let expected = dsd_bytes(8_000, 7);
    let track = dsf("fallback.dsf", &expected);
    let (player, events, control) = start(dsd_device());
    player.set_volume(Some(-6.0));
    player.enqueue(vec![track]);
    player.play();
    let seen = wait_end(&events);

    let reason = seen.iter().find_map(|e| match e {
        PlayerEvent::DsdMode {
            mode: DsdMode::Pcm,
            reason,
        } => reason.clone(),
        _ => None,
    });
    let reason = reason.expect("PCM fallback announced");
    assert!(reason.contains("DSP"), "{reason}");
    // DoP would be 24-in-32; the converted stream is plain 24-bit PCM at 176.4 kHz.
    assert_eq!(
        control.sessions()[0].format.container,
        SampleContainer::S24Packed
    );
}

#[test]
fn per_device_dsd_mode_overrides_the_default() {
    let expected = dsd_bytes(4_000, 8);
    let track = dsf("per-device.dsf", &expected);
    let mut config = PlayerConfig::new(NullBackend::device_id());
    config.default_dsd_mode = DsdMode::Dop;
    config
        .dsd_modes
        .insert(NullBackend::device_id(), DsdMode::Pcm);
    let (player, events, control) = start_with(config, dsd_device());
    player.enqueue(vec![track]);
    player.play();
    let seen = wait_end(&events);

    assert!(seen.iter().any(|e| matches!(
        e,
        PlayerEvent::DsdMode {
            mode: DsdMode::Pcm,
            ..
        }
    )));
    assert_eq!(control.sessions()[0].format.rate, 176_400);
    assert_eq!(
        control.sessions()[0].format.container,
        SampleContainer::S24Packed
    );
}

fn dsd_modes(events: &[PlayerEvent]) -> Vec<(DsdMode, Option<String>)> {
    events
        .iter()
        .filter_map(|e| match e {
            PlayerEvent::DsdMode { mode, reason } => Some((*mode, reason.clone())),
            _ => None,
        })
        .collect()
}

/// An exclusive device answers "busy" to format probes while our stream holds it; the next
/// DoP track must still reuse the open DoP stream instead of falling back to PCM.
#[test]
fn gapless_dop_survives_a_device_busy_with_our_own_stream() {
    let first = dsd_bytes(12_000, 9);
    let second = dsd_bytes(10_000, 10);
    let a = dsf("dop-busy-a.dsf", &first);
    let b = dsf("dop-busy-b.dsf", &second);
    let (player, events, control) = start(dsd_device().with_busy_probes());
    player.enqueue(vec![a, b]);
    player.play();
    let seen = wait_end(&events);

    let modes = dsd_modes(&seen);
    assert_eq!(modes, vec![(DsdMode::Dop, None); 2], "events: {seen:#?}");
    let sessions = control.sessions();
    assert_eq!(sessions.len(), 1, "same-rate DSD stays on one DoP stream");
    let (payload, idle) = decode_dop(&sessions[0].bytes, 2);
    let mut expected = first;
    expected.extend_from_slice(&second);
    assert_eq!(payload, expected, "idle frames: {idle}");
}

#[test]
fn seek_within_a_dop_track_keeps_the_dop_stream() {
    let source = dsd_bytes(200_000, 11);
    let track = dsf("dop-busy-seek.dsf", &source);
    let (player, events, control) = start(dsd_device().with_busy_probes());
    player.enqueue(vec![track]);
    player.play();
    wait_for(
        &events,
        |e| matches!(e, PlayerEvent::Position { elapsed, .. } if !elapsed.is_zero()),
    );
    player.seek(Duration::from_millis(400));
    let seen = wait_end(&events);

    let modes = dsd_modes(&seen);
    assert!(
        !modes.is_empty() && modes.iter().all(|m| *m == (DsdMode::Dop, None)),
        "events: {seen:#?}"
    );
    let sessions = control.sessions();
    assert_eq!(sessions.len(), 1, "a seek must not reopen the device");
    // The output ends with the source from the seek target, still as DoP.
    let (payload, _) = decode_dop(&sessions[0].bytes, 2);
    let target = 2 * (DSD64 as usize / 8) * 400 / 1000;
    assert!(payload.ends_with(&source[target..]));
}

/// The DoP answer is unknown while a PCM stream holds the device; once that stream drains,
/// the engine must learn the device rejects DoP and convert the track instead of skipping it.
#[test]
fn dsd_after_pcm_on_a_busy_device_falls_back_to_pcm_once_known() {
    let pcm = wav("busy-before-dsd.wav", 44_100, 16, 20_000, 12);
    let dsd = dsf("busy-after-pcm.dsf", &dsd_bytes(8_000, 13));
    let backend = dsd_device()
        .with_formats(vec![
            DeviceFormat {
                rate: 44_100,
                container: SampleContainer::S16,
                channels: 2,
            },
            DeviceFormat {
                rate: 176_400,
                container: SampleContainer::S24Packed,
                channels: 2,
            },
        ])
        .with_busy_probes();
    let (player, events, control) = start(backend);
    player.enqueue(vec![pcm, dsd]);
    player.play();
    let seen = wait_end(&events);

    assert!(
        !seen
            .iter()
            .any(|e| matches!(e, PlayerEvent::TrackFailed { .. })),
        "events: {seen:#?}"
    );
    assert!(
        matches!(dsd_modes(&seen).last(), Some((DsdMode::Pcm, Some(_)))),
        "events: {seen:#?}"
    );
    let sessions = control.sessions();
    assert_eq!(sessions.len(), 2);
    assert_eq!(sessions[1].format.rate, 176_400);
    assert!(!sessions[1].bytes.is_empty());
}

/// Changing the DSD mode while a DSD track plays switches it from the current position.
#[test]
fn set_dsd_switches_a_playing_dop_track_to_pcm() {
    let track = dsf("dsd-switch.dsf", &dsd_bytes(200_000, 14));
    let (player, events, control) = start(dsd_device());
    player.enqueue(vec![track]);
    player.play();
    wait_for(
        &events,
        |e| matches!(e, PlayerEvent::Position { elapsed, .. } if !elapsed.is_zero()),
    );
    player.set_dsd(
        DsdMode::Pcm,
        std::collections::HashMap::new(),
        DsdPcmRate::R352_8,
    );
    let seen = wait_end(&events);

    assert!(
        matches!(dsd_modes(&seen).last(), Some((DsdMode::Pcm, None))),
        "events: {seen:#?}"
    );
    let formats: Vec<_> = control.sessions().iter().map(|s| s.format).collect();
    assert_eq!(formats.len(), 2, "{formats:?}");
    assert_eq!(formats[0].container, SampleContainer::S24In32);
    assert_eq!(formats[1].rate, 352_800);
    assert_eq!(formats[1].container, SampleContainer::S24Packed);
}

/// The same settings sent again do not restart playback.
#[test]
fn unchanged_dsd_settings_are_a_noop() {
    let track = dsf("dsd-noop.dsf", &dsd_bytes(100_000, 15));
    let (player, events, control) = start(dsd_device());
    player.enqueue(vec![track]);
    player.play();
    wait_for(
        &events,
        |e| matches!(e, PlayerEvent::Position { elapsed, .. } if !elapsed.is_zero()),
    );
    player.set_dsd(
        DsdMode::Dop,
        std::collections::HashMap::new(),
        DsdPcmRate::R176_4,
    );
    let seen = wait_end(&events);
    assert!(dsd_modes(&seen).is_empty(), "events: {seen:#?}");
    assert_eq!(control.sessions().len(), 1);
}

/// A change that does not reach the playing DoP track (another device's mode, the PCM rate)
/// leaves it alone.
#[test]
fn dsd_changes_for_other_devices_or_the_pcm_rate_keep_dop_playing() {
    let track = dsf("dsd-unrelated.dsf", &dsd_bytes(100_000, 16));
    let (player, events, control) = start(dsd_device());
    player.enqueue(vec![track]);
    player.play();
    wait_for(
        &events,
        |e| matches!(e, PlayerEvent::Position { elapsed, .. } if !elapsed.is_zero()),
    );
    let other = std::collections::HashMap::from([(
        sonder_output::DeviceId("another".into()),
        DsdMode::Pcm,
    )]);
    player.set_dsd(DsdMode::Dop, other, DsdPcmRate::R352_8);
    let seen = wait_end(&events);
    assert!(dsd_modes(&seen).is_empty(), "events: {seen:#?}");
    assert_eq!(control.sessions().len(), 1);
}

/// A PCM track's resampler belongs to that track: the DSD track after it still plays DoP.
#[test]
fn dsd_after_a_resampled_pcm_track_keeps_dop() {
    let pcm = wav("resampled-before-dsd.wav", 44_100, 16, 4_000, 21);
    let expected = dsd_bytes(8_000, 22);
    let dsd = dsf("dop-after-resampled.dsf", &expected);
    let mut config = PlayerConfig::new(NullBackend::device_id());
    config.src.target = sonder_engine::SrcTarget::Fixed(48_000);
    let (player, events, control) = start_with(config, dsd_device());
    player.enqueue(vec![pcm, dsd]);
    player.play();
    let seen = wait_end(&events);

    assert_eq!(
        dsd_modes(&seen),
        vec![(DsdMode::Dop, None)],
        "events: {seen:#?}"
    );
    let sessions = control.sessions();
    assert_eq!(
        sessions[0].format.rate, 48_000,
        "the PCM track was resampled"
    );
    let (payload, _) = decode_dop(&sessions.last().unwrap().bytes, 2);
    assert_eq!(payload, expected);
}

/// A volume change in the middle of a track changes the signal path, so it is reported again.
#[test]
fn a_mid_track_volume_change_reports_the_new_signal_path() {
    let a = wav("path-volume.wav", 44_100, 16, 132_300, 23);
    let (player, events, _) = start(slow());
    player.enqueue(vec![a]);
    player.play();
    let (_, before) = wait_for(
        &events,
        |e| matches!(e, PlayerEvent::Position { elapsed, .. } if !elapsed.is_zero()),
    );
    player.set_volume(Some(-6.0));
    let after = wait_end(&events);

    let paths: Vec<_> = before
        .iter()
        .chain(&after)
        .filter_map(|e| match e {
            PlayerEvent::SignalPath(path) => Some(path.clone()),
            _ => None,
        })
        .collect();
    assert!(paths[0].bit_perfect, "{paths:#?}");
    let last = paths.last().unwrap();
    assert!(
        !last.bit_perfect && last.stages.iter().any(|s| s.name == "Volume"),
        "{paths:#?}"
    );
}

/// Levelling values for every path: one quiet album, fixed gains.
struct FixedGains;

impl sonder_engine::LoudnessSource for FixedGains {
    fn gain(&self, _path: &Path, _target_lufs: f64) -> Option<sonder_engine::TrackGain> {
        Some(sonder_engine::TrackGain {
            track: sonder_dsp::Gain {
                gain_db: -6.0,
                peak: 1.0,
            },
            album: sonder_dsp::Gain {
                gain_db: -9.0,
                peak: 1.0,
            },
            album_id: Some(7),
        })
    }
}

fn shaped_dsp() -> DspConfig {
    DspConfig {
        peq: Some(sonder_dsp::Peq {
            preamp_db: -3.0,
            bands: vec![sonder_dsp::PeqBand {
                kind: sonder_dsp::BiquadKind::Peak,
                freq_hz: 1_000.0,
                gain_db: 6.0,
                q: 1.0,
                enabled: true,
            }],
        }),
        leveling: sonder_dsp::LevelingMode::Track,
        leveling_target_lufs: -18.0,
        dither: true,
    }
}

#[test]
fn dsp_stages_run_in_roon_order() {
    let a = wav("order.wav", 44_100, 16, 4_000, 21);
    let backend = NullBackend::new().with_formats(vec![
        DeviceFormat {
            rate: 44_100,
            container: SampleContainer::S16,
            channels: 2,
        },
        DeviceFormat {
            rate: 48_000,
            container: SampleContainer::S16,
            channels: 2,
        },
    ]);
    let control = backend.control();
    let mut config = PlayerConfig::new(NullBackend::device_id());
    config.device_poll = Duration::from_millis(20);
    config.src.target = sonder_engine::SrcTarget::Fixed(48_000);
    config.dsp = shaped_dsp();
    config.loudness = std::sync::Arc::new(FixedGains);
    let (player, events) = Player::spawn(Box::new(backend), config).unwrap();
    player.set_volume(Some(-2.0));
    player.enqueue(vec![a.clone()]);
    player.play();
    let seen = wait_end(&events);

    let path = seen
        .iter()
        .find_map(|event| match event {
            PlayerEvent::SignalPath(path) => Some(path.clone()),
            _ => None,
        })
        .expect("signal path");
    let names: Vec<&str> = path.stages.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "Headroom",
            "Resampler",
            "PEQ",
            "Levelling",
            "Volume",
            "Dither"
        ],
        "{path:#?}"
    );
    assert!(!path.bit_perfect);
    assert_eq!(path.output_bits, 16);
    let dither = path.stages.last().unwrap();
    assert!(dither.detail.contains("16-bit"), "{}", dither.detail);
    assert!(path.stages[3].detail.contains("-6.0 dB"), "{path:#?}");
    // Every stage changed the samples: what reached the device is not the source.
    assert_ne!(
        control.played_bytes(),
        packed(SampleContainer::S16, &decoded(&a))
    );
}

#[test]
fn removing_every_dsp_stage_restores_the_bit_perfect_path() {
    let a = wav("shaped.wav", 44_100, 16, 8_000, 22);
    let backend = NullBackend::new();
    let control = backend.control();
    let mut config = PlayerConfig::new(NullBackend::device_id());
    config.device_poll = Duration::from_millis(20);
    config.dsp = shaped_dsp();
    config.loudness = std::sync::Arc::new(FixedGains);
    let (player, events) = Player::spawn(Box::new(backend), config).unwrap();

    player.enqueue(vec![a.clone()]);
    player.play();
    wait_end(&events);
    assert_ne!(
        control.played_bytes(),
        packed(SampleContainer::S16, &decoded(&a))
    );

    player.set_dsp(DspConfig::default());
    player.set_volume(None);
    player.play();
    wait_end(&events);
    assert_eq!(
        control.sessions().last().unwrap().bytes,
        packed(SampleContainer::S16, &decoded(&a))
    );
    let sessions = control.sessions();
    assert_eq!(
        sessions.last().unwrap().format.container,
        SampleContainer::S16
    );
}

#[test]
fn dither_engages_only_when_the_device_narrows_the_word_length() {
    let a = wav("dither.wav", 44_100, 24, 8_000, 23);
    let backend = NullBackend::new().with_formats(vec![DeviceFormat {
        rate: 44_100,
        container: SampleContainer::S16,
        channels: 2,
    }]);
    let control = backend.control();
    let mut config = PlayerConfig::new(NullBackend::device_id());
    config.device_poll = Duration::from_millis(20);
    config.dsp = DspConfig::default();
    let (player, events) = Player::spawn(Box::new(backend), config).unwrap();
    player.set_volume(Some(-6.0));
    player.enqueue(vec![a.clone()]);
    player.play();
    let seen = wait_end(&events);

    let path = seen
        .iter()
        .find_map(|event| match event {
            PlayerEvent::SignalPath(path) => Some(path.clone()),
            _ => None,
        })
        .expect("signal path");
    let names: Vec<&str> = path.stages.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["Volume", "Dither"], "{path:#?}");
    assert_eq!(path.output_bits, 16);

    let sessions = control.sessions();
    let session = sessions.last().unwrap();
    assert_eq!(session.format.container, SampleContainer::S16);
    let mut out = vec![0i32; session.bytes.len() / 2];
    unpack(SampleContainer::S16, &session.bytes, &mut out);
    assert_eq!(out.len(), decoded(&a).len());
    // Dither noise keeps every sample within two 16-bit LSBs of the scaled source.
    for (o, s) in out.iter().zip(decoded(&a)) {
        let expected = f64::from(s) * 0.501_187_2;
        assert!(
            (f64::from(*o) - expected).abs() <= 2.0 * f64::from(1 << 16),
            "{o} vs {expected}"
        );
    }
}

/// Dither quantises before the final conversion, so it has to report what it clamps: a boost
/// on a 16-bit device must still raise the clipping notice.
#[test]
fn clipping_is_reported_on_a_dithered_path() {
    let a = wav("dither-clip.wav", 44_100, 24, 8_000, 24);
    let backend = NullBackend::new().with_formats(vec![DeviceFormat {
        rate: 44_100,
        container: SampleContainer::S16,
        channels: 2,
    }]);
    let mut config = PlayerConfig::new(NullBackend::device_id());
    config.device_poll = Duration::from_millis(20);
    config.dsp = DspConfig {
        peq: Some(sonder_dsp::Peq {
            preamp_db: 12.0,
            bands: Vec::new(),
        }),
        ..DspConfig::default()
    };
    let (player, events) = Player::spawn(Box::new(backend), config).unwrap();
    player.enqueue(vec![a]);
    player.play();
    let seen = wait_end(&events);

    let path = seen
        .iter()
        .find_map(|event| match event {
            PlayerEvent::SignalPath(path) => Some(path.clone()),
            _ => None,
        })
        .expect("signal path");
    assert!(path.stages.iter().any(|s| s.name == "Dither"), "{path:#?}");
    assert!(
        seen.iter()
            .any(|e| matches!(e, PlayerEvent::Clipping(count) if *count > 0)),
        "events: {seen:#?}"
    );
}

/// Levelling only costs native DSD when the track has a gain: DSD is never measured and rarely
/// tagged, so a 0 dB levelling stage leaves DoP alone, while a real gain converts to PCM.
#[test]
fn levelling_keeps_dop_unless_the_dsd_track_has_a_gain() {
    let expected = dsd_bytes(8_000, 25);
    let track = dsf("levelled-dop.dsf", &expected);

    let mut config = PlayerConfig::new(NullBackend::device_id());
    config.dsp.leveling = sonder_dsp::LevelingMode::Track;
    let (player, events, control) = start_with(config, dsd_device());
    player.enqueue(vec![track.clone()]);
    player.play();
    let seen = wait_end(&events);
    assert_eq!(
        dsd_modes(&seen),
        vec![(DsdMode::Dop, None)],
        "events: {seen:#?}"
    );
    let (payload, _) = decode_dop(&control.sessions().last().unwrap().bytes, 2);
    assert_eq!(payload, expected);

    let mut config = PlayerConfig::new(NullBackend::device_id());
    config.dsp.leveling = sonder_dsp::LevelingMode::Track;
    config.loudness = std::sync::Arc::new(FixedGains);
    let (player, events, control) = start_with(config, dsd_device());
    player.enqueue(vec![track]);
    player.play();
    let seen = wait_end(&events);
    let modes = dsd_modes(&seen);
    assert_eq!(modes.len(), 1, "events: {seen:#?}");
    assert_eq!(modes[0].0, DsdMode::Pcm, "events: {seen:#?}");
    assert_eq!(
        control.sessions()[0].format.container,
        SampleContainer::S24Packed
    );
}

// ----- PCM→DSD -----

/// A 1 kHz tone, so the modulated stream can be decoded and measured.
fn tone_wav(name: &str, rate: u32, bits: u16, frames: usize) -> PathBuf {
    let path = temp_dir().join(name);
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: rate,
        bits_per_sample: bits,
        sample_format: hound::SampleFormat::Int,
    };
    let max = f64::from((1i32 << (bits - 1)) - 1);
    let mut w = hound::WavWriter::create(&path, spec).unwrap();
    for frame in 0..frames {
        let sample = (0.5
            * (2.0 * std::f64::consts::PI * 1000.0 * frame as f64 / f64::from(rate)).sin()
            * max) as i32;
        w.write_sample(sample).unwrap();
        w.write_sample(sample).unwrap();
    }
    w.finalize().unwrap();
    path
}

fn signal_path(seen: &[PlayerEvent]) -> &sonder_engine::SignalPath {
    seen.iter()
        .find_map(|event| match event {
            PlayerEvent::SignalPath(path) => Some(path),
            _ => None,
        })
        .expect("signal path")
}

#[test]
fn pcm_to_dsd64_reaches_the_device_over_dop() {
    let track = tone_wav("sdm-dop.wav", 44_100, 16, 48_000);
    let mut config = PlayerConfig::new(NullBackend::device_id());
    config.sdm.mode = sonder_engine::SdmMode::Dsd64;
    let (player, events, control) = start_with(config, dsd_device());
    player.enqueue(vec![track]);
    player.play();
    let seen = wait_end(&events);

    let sessions = control.sessions();
    assert_eq!(sessions.len(), 1, "one DoP stream");
    assert_eq!(
        sessions[0].format,
        DeviceFormat {
            rate: 176_400,
            container: SampleContainer::S24In32,
            channels: 2,
        }
    );
    let (payload, _idle) = decode_dop(&sessions[0].bytes, 2);
    assert!(!payload.is_empty(), "the modulator produced no payload");
    // 1.088 s at DSD64 is ~384k bytes per channel.
    assert!(
        payload.len() > 300_000,
        "payload {} bytes is short",
        payload.len()
    );

    // The payload decodes to the source tone, at the SACD 6 dB headroom.
    let mut converter = sonder_dsp::DsdToPcm::new(2_822_400, 176_400, 2).unwrap();
    let mut pcm = Vec::new();
    converter.process(&payload, &mut pcm);
    let body = &pcm[pcm.len() / 2..];
    let rms = (body.iter().map(|s| s * s).sum::<f64>() / body.len() as f64).sqrt();
    assert!(rms > 0.03, "decoded tone RMS {rms} is too low");

    let path = signal_path(&seen);
    assert_eq!(path.mode, sonder_engine::OutputMode::Dop);
    assert_eq!(path.output_rate, 176_400);
    assert!(!path.bit_perfect);
    assert!(
        path.stages.iter().any(|s| s.name == "DSD modulator"),
        "signal path: {path:#?}"
    );
    assert!(
        path.stages.iter().any(|s| s.name == "DoP"),
        "signal path: {path:#?}"
    );
}

#[test]
fn pcm_to_dsd128_and_dsd256_use_the_wider_carriers() {
    for (mode, carrier) in [
        (sonder_engine::SdmMode::Dsd128, 352_800),
        (sonder_engine::SdmMode::Dsd256, 705_600),
    ] {
        let track = tone_wav(&format!("sdm-{carrier}.wav"), 48_000, 24, 24_000);
        let mut config = PlayerConfig::new(NullBackend::device_id());
        config.sdm.mode = mode;
        let (player, events, control) = start_with(config, dsd_device());
        player.enqueue(vec![track]);
        player.play();
        let seen = wait_end(&events);
        let sessions = control.sessions();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].format.rate, carrier);
        assert!(
            signal_path(&seen)
                .stages
                .iter()
                .any(|s| s.detail.contains(mode.as_str())),
            "events: {seen:#?}"
        );
    }
}

#[test]
fn pcm_to_dsd_uses_native_words_when_the_device_prefers_them() {
    let track = tone_wav("sdm-native.wav", 44_100, 16, 24_000);
    let native = DeviceFormat {
        rate: 88_200,
        container: SampleContainer::DsdU32Be,
        channels: 2,
    };
    let mut config = PlayerConfig::new(NullBackend::device_id());
    config.sdm.mode = sonder_engine::SdmMode::Dsd64;
    config.default_dsd_mode = DsdMode::Native;
    let (player, events, control) =
        start_with(config, NullBackend::new().with_formats(vec![native]));
    player.enqueue(vec![track]);
    player.play();
    let seen = wait_end(&events);

    let sessions = control.sessions();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].format, native);
    assert!(!sessions[0].bytes.is_empty());
    let path = signal_path(&seen);
    assert_eq!(path.mode, sonder_engine::OutputMode::NativeDsd);
    assert_eq!(path.output_rate, 88_200);
}

#[test]
fn pcm_to_dsd_stays_pcm_when_the_device_cannot_carry_it() {
    let track = tone_wav("sdm-no-device.wav", 44_100, 16, 24_000);
    let pcm_only = DeviceFormat {
        rate: 44_100,
        container: SampleContainer::S16,
        channels: 2,
    };
    let mut config = PlayerConfig::new(NullBackend::device_id());
    config.sdm.mode = sonder_engine::SdmMode::Dsd64;
    let (player, events, control) =
        start_with(config, NullBackend::new().with_formats(vec![pcm_only]));
    player.enqueue(vec![track]);
    player.play();
    let seen = wait_end(&events);

    assert!(
        seen.iter().any(|event| matches!(
            event,
            PlayerEvent::Warning(message) if message.contains("DoP or native DSD")
        )),
        "events: {seen:#?}"
    );
    let sessions = control.sessions();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].format, pcm_only);
    let path = signal_path(&seen);
    assert_eq!(path.mode, sonder_engine::OutputMode::Pcm);
    assert!(path.bit_perfect);
}

#[test]
fn pcm_to_dsd_native_stays_gapless_across_tracks() {
    let a = tone_wav("sdm-native-a.wav", 44_100, 16, 12_000);
    let b = tone_wav("sdm-native-b.wav", 44_100, 16, 12_000);
    let native = DeviceFormat {
        rate: 88_200,
        container: SampleContainer::DsdU32Be,
        channels: 2,
    };
    let mut config = PlayerConfig::new(NullBackend::device_id());
    config.sdm.mode = sonder_engine::SdmMode::Dsd64;
    config.default_dsd_mode = DsdMode::Native;
    let (player, events, control) =
        start_with(config, NullBackend::new().with_formats(vec![native]));
    player.enqueue(vec![a, b]);
    player.play();
    wait_end(&events);

    let sessions = control.sessions();
    assert_eq!(
        sessions.len(),
        1,
        "native DSD should stay on one stream: {sessions:#?}"
    );
    assert_eq!(sessions[0].format, native);
}

/// Native DSD is requested but the device only takes DoP. While the first track's DoP stream
/// holds the device, the native probe can only answer "busy"; the open stream already proves
/// DoP works, so the next track must reuse it instead of reopening for native and re-decoding.
#[test]
fn pcm_to_dsd_reuses_the_open_dop_stream_when_native_cannot_be_probed() {
    let a = tone_wav("sdm-busy-a.wav", 44_100, 16, 12_000);
    let b = tone_wav("sdm-busy-b.wav", 44_100, 16, 12_000);
    let dop = DeviceFormat {
        rate: 176_400,
        container: SampleContainer::S24In32,
        channels: 2,
    };
    let mut config = PlayerConfig::new(NullBackend::device_id());
    config.sdm.mode = sonder_engine::SdmMode::Dsd64;
    config.default_dsd_mode = DsdMode::Native;
    let (player, events, control) = start_with(
        config,
        dsd_device().with_formats(vec![dop]).with_busy_probes(),
    );
    player.enqueue(vec![a, b]);
    player.play();
    wait_end(&events);

    let sessions = control.sessions();
    assert_eq!(
        sessions.len(),
        1,
        "gapless DoP should stay on one stream: {sessions:#?}"
    );
    assert_eq!(sessions[0].format, dop);
}

#[test]
fn pcm_to_dsd_warns_once_when_the_device_cannot_carry_it() {
    let a = tone_wav("sdm-warn-a.wav", 44_100, 16, 6_000);
    let b = tone_wav("sdm-warn-b.wav", 44_100, 16, 6_000);
    let pcm_only = DeviceFormat {
        rate: 44_100,
        container: SampleContainer::S16,
        channels: 2,
    };
    let mut config = PlayerConfig::new(NullBackend::device_id());
    config.sdm.mode = sonder_engine::SdmMode::Dsd64;
    let (player, events, _control) =
        start_with(config, NullBackend::new().with_formats(vec![pcm_only]));
    player.enqueue(vec![a, b]);
    player.play();
    let seen = wait_end(&events);

    let warnings = seen
        .iter()
        .filter(|event| {
            matches!(event, PlayerEvent::Warning(message) if message.contains("DoP or native DSD"))
        })
        .count();
    assert_eq!(warnings, 1, "events: {seen:#?}");
}

/// Levelling with no gain for a DSD track keeps DoP, so nothing touches the bits and the
/// signal path must say so.
#[test]
fn dop_with_idle_levelling_reports_bit_perfect() {
    let track = dsf("dop-leveling.dsf", &dsd_bytes(20_000, 12));
    let mut config = PlayerConfig::new(NullBackend::device_id());
    config.dsp.leveling = sonder_dsp::LevelingMode::Track;
    let (player, events, _control) = start_with(config, dsd_device());
    player.enqueue(vec![track]);
    player.play();
    let seen = wait_end(&events);

    assert_eq!(
        dsd_modes(&seen),
        vec![(DsdMode::Dop, None)],
        "events: {seen:#?}"
    );
    let path = signal_path(&seen);
    assert_eq!(path.mode, sonder_engine::OutputMode::Dop);
    assert!(path.stages.is_empty(), "signal path: {path:#?}");
    assert!(path.bit_perfect, "signal path: {path:#?}");
}
