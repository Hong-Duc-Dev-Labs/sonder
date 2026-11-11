//! Drives the player against the null backend and checks the exact bytes it would have
//! written to the device.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crossbeam_channel::Receiver;
use sonder_decode::Decoder;
use sonder_engine::{PlaybackState, Player, PlayerConfig, PlayerEvent};
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
    let control = backend.control();
    let mut config = PlayerConfig::new(NullBackend::device_id());
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
    let (player, events, control) = start(NullBackend::new().with_formats(vec![only]));
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
