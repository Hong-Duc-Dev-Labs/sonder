//! Listening history over a scanned fixture library, and playlists by path (what M3U import
//! and export use).

mod fixtures;

use std::path::PathBuf;

use fixtures::{Meta, flac};
use sonder_library::{Library, StatsPeriod, format_m3u, parse_m3u};

const DAY: i64 = 86_400;
/// A fixed "now" (2026-11-14 12:00 UTC), so periods do not depend on the clock.
const NOW: i64 = 1_794_657_600;

struct Env {
    _tmp: tempfile::TempDir,
    lib: Library,
    a: PathBuf,
    b: PathBuf,
    c: PathBuf,
}

/// Two albums: "First" by Ann (A, B) and "Second" by Bo (C).
fn env() -> Env {
    let tmp = tempfile::tempdir().unwrap();
    let music = tmp.path().join("music");
    let file = |album: &str, no: u32| music.join(album).join(format!("{no:02}.flac"));
    let (a, b, c) = (file("First", 1), file("First", 2), file("Second", 1));
    for (path, title, artist, album, no) in [
        (&a, "A", "Ann", "First", 1),
        (&b, "B", "Ann", "First", 2),
        (&c, "C", "Bo", "Second", 1),
    ] {
        flac(
            path,
            44_100,
            16,
            2,
            1,
            &Meta {
                title: Some(title),
                artist: Some(artist),
                album: Some(album),
                album_artist: Some(artist),
                track: Some(no),
                ..Default::default()
            },
        );
    }
    let mut lib = Library::open(tmp.path().join("library.db"), tmp.path().join("art")).unwrap();
    lib.add_root(&music, false).unwrap();
    lib.scan().unwrap();
    Env {
        _tmp: tmp,
        lib,
        a,
        b,
        c,
    }
}

#[test]
fn stats_count_plays_inside_the_period() {
    let Env {
        mut lib, a, b, c, ..
    } = env();
    // This week: A three times, C once. Twenty days ago: B twice. Two years ago: C.
    for at in [NOW - 3600, NOW - 2 * DAY, NOW - 5 * DAY] {
        lib.record_play(&a, at, 180_000).unwrap();
    }
    lib.record_play(&c, NOW - DAY, 60_000).unwrap();
    lib.record_play(&b, NOW - 20 * DAY, 200_000).unwrap();
    lib.record_play(&b, NOW - 20 * DAY + 600, 200_000).unwrap();
    lib.record_play(&c, NOW - 730 * DAY, 90_000).unwrap();
    // A file that left the library still counts in the totals.
    lib.record_play(&PathBuf::from("/gone/x.flac"), NOW - 60, 30_000)
        .unwrap();

    let week = lib.listening_stats(StatsPeriod::Week, NOW, 10).unwrap();
    assert_eq!(week.plays, 5);
    assert_eq!(week.listened_ms, 3 * 180_000 + 60_000 + 30_000);
    assert_eq!(week.tracks, 3);
    let titles: Vec<&str> = week
        .top_tracks
        .iter()
        .map(|t| t.track.title.as_str())
        .collect();
    assert_eq!(titles, ["A", "C"]);
    assert_eq!(week.top_tracks[0].plays, 3);
    assert_eq!(week.top_artists[0].name, "Ann");
    assert_eq!(week.top_albums[0].title, "First");
    assert_eq!(week.buckets.len(), 7);
    let charted: u64 = week.buckets.iter().map(|b| b.listened_ms).sum();
    assert_eq!(charted, week.listened_ms);

    let month = lib.listening_stats(StatsPeriod::Month, NOW, 10).unwrap();
    assert_eq!(month.plays, 7);
    assert_eq!(month.buckets.len(), 30);
    // B's 400 s across two plays outranks C's single play.
    let titles: Vec<&str> = month
        .top_tracks
        .iter()
        .map(|t| t.track.title.as_str())
        .collect();
    assert_eq!(titles, ["A", "B", "C"]);

    let year = lib.listening_stats(StatsPeriod::Year, NOW, 10).unwrap();
    assert_eq!(year.plays, 7);
    assert_eq!(year.buckets.len(), 12);
    assert!(
        year.buckets.iter().all(|b| b.label.len() == 7),
        "{:?}",
        year.buckets
    );

    let all = lib.listening_stats(StatsPeriod::All, NOW, 10).unwrap();
    assert_eq!(all.plays, 8);
    assert!(all.buckets.len() >= 24, "monthly bars since the first play");
    let charted: u64 = all.buckets.iter().map(|b| b.listened_ms).sum();
    assert_eq!(charted, all.listened_ms);
    assert_eq!(
        lib.listening_stats(StatsPeriod::All, NOW, 1)
            .unwrap()
            .top_tracks
            .len(),
        1
    );

    let recent = lib.recent_plays(2).unwrap();
    let titles: Vec<&str> = recent.iter().map(|r| r.track.title.as_str()).collect();
    assert_eq!(
        titles,
        ["A", "C"],
        "newest first; the missing file is left out"
    );

    assert_eq!(lib.clear_history().unwrap(), 8);
    let empty = lib.listening_stats(StatsPeriod::Week, NOW, 10).unwrap();
    assert_eq!(empty.plays, 0);
    assert!(empty.top_tracks.is_empty());
    assert_eq!(empty.buckets.len(), 7);
    assert!(
        lib.listening_stats(StatsPeriod::All, NOW, 10)
            .unwrap()
            .buckets
            .is_empty()
    );
}

#[test]
fn playlists_take_paths_and_export_round_trips() {
    let Env { mut lib, a, c, .. } = env();
    let list = lib.create_playlist("Imported").unwrap();
    let outside = a.parent().unwrap().join("not-scanned.flac");
    let added = lib
        .add_paths_to_playlist(list.id, &[c.clone(), outside.clone(), a.clone()])
        .unwrap();
    assert_eq!(added, 3);

    let detail = lib.playlist(list.id).unwrap().unwrap();
    assert_eq!(detail.items.len(), 2);
    assert_eq!(detail.missing, 1);

    let entries = lib.playlist_entries(list.id).unwrap().unwrap();
    let paths: Vec<&PathBuf> = entries.iter().map(|e| &e.path).collect();
    assert_eq!(paths, [&c, &outside, &a]);
    assert!(entries[1].track.is_none());

    let text = format_m3u(&entries);
    assert!(text.starts_with("#EXTM3U\n#EXTINF:1,Bo - C\n"), "{text}");
    let back = parse_m3u(text.as_bytes(), std::path::Path::new("/unused"));
    assert_eq!(back.paths, [c, outside, a]);

    assert!(lib.playlist_entries(i64::MAX).unwrap().is_none());
}
