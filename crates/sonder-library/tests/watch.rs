//! The watcher picks up added and deleted files and stops cleanly.

mod fixtures;

use std::fs;
use std::time::{Duration, Instant};

use crossbeam_channel::Receiver;
use fixtures::{Meta, flac};
use sonder_library::{AlbumSort, Library, LibraryWatcher, WatchEvent};

/// Waits until `done` holds for the index, draining watcher events meanwhile.
fn wait_until(events: &Receiver<WatchEvent>, lib: &Library, done: impl Fn(&Library) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !done(lib) {
        let left = deadline.saturating_duration_since(Instant::now());
        assert!(!left.is_zero(), "watcher did not update the index in time");
        match events.recv_timeout(left.min(Duration::from_millis(500))) {
            Ok(WatchEvent::Error(e)) => panic!("watcher error: {e}"),
            Ok(WatchEvent::Scanned(_)) | Err(_) => {}
        }
    }
}

#[test]
fn watcher_indexes_added_and_deleted_files() {
    let tmp = tempfile::tempdir().unwrap();
    let music = tmp.path().join("music");
    fs::create_dir_all(&music).unwrap();
    let db = tmp.path().join("library.db");
    let art = tmp.path().join("art");

    let mut lib = Library::open(&db, &art).unwrap();
    lib.add_root(&music, false).unwrap();
    lib.scan().unwrap();

    let (tx, rx) = crossbeam_channel::unbounded();
    let watcher = LibraryWatcher::start(Library::open(&db, &art).unwrap(), tx).unwrap();

    let file = music.join("Artist").join("Album").join("01.flac");
    flac(
        &file,
        44_100,
        16,
        2,
        1,
        &Meta {
            title: Some("Watched"),
            album: Some("Album"),
            ..Default::default()
        },
    );
    wait_until(&rx, &lib, |l| l.search("watched", 5).unwrap().len() == 1);

    fs::remove_dir_all(music.join("Artist")).unwrap();
    wait_until(&rx, &lib, |l| {
        l.albums(AlbumSort::Title).unwrap().is_empty()
    });

    let start = Instant::now();
    drop(watcher);
    assert!(
        start.elapsed() < Duration::from_secs(10),
        "stopping the watcher hung"
    );
}
