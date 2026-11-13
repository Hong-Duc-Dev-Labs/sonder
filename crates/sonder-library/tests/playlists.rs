//! Playlists over a scanned fixture library: editing, ordering, and entries whose file
//! leaves and returns.

mod fixtures;

use std::fs;
use std::path::PathBuf;

use fixtures::{Meta, flac};
use sonder_library::{AlbumSort, Library, LibraryError};
use tempfile::TempDir;

struct Env {
    tmp: TempDir,
    music: PathBuf,
}

impl Env {
    /// One album of three one-second tracks, A, B and C.
    fn new() -> Env {
        let tmp = tempfile::tempdir().unwrap();
        let music = tmp.path().join("music");
        for (no, title) in [(1, "A"), (2, "B"), (3, "C")] {
            flac(
                &music.join("Album").join(format!("{no:02}.flac")),
                44_100,
                16,
                2,
                1,
                &Meta {
                    title: Some(title),
                    album: Some("Album"),
                    album_artist: Some("Artist"),
                    track: Some(no),
                    ..Default::default()
                },
            );
        }
        Env { tmp, music }
    }

    fn open(&self) -> Library {
        let mut lib = Library::open(
            self.tmp.path().join("library.db"),
            self.tmp.path().join("art"),
        )
        .unwrap();
        lib.add_root(&self.music, false).unwrap();
        lib.scan().unwrap();
        lib
    }
}

/// Track ids of A, B and C.
fn ids(lib: &Library) -> [i64; 3] {
    let album = lib.albums(AlbumSort::Title).unwrap()[0].id;
    let tracks = lib.album(album).unwrap().unwrap().tracks;
    [tracks[0].id, tracks[1].id, tracks[2].id]
}

/// Titles of the playlist's present entries, in order, with their positions.
fn entries(lib: &Library, id: i64) -> Vec<(u32, String)> {
    lib.playlist(id)
        .unwrap()
        .unwrap()
        .items
        .into_iter()
        .map(|i| (i.position, i.track.title))
        .collect()
}

fn titles(list: &[(u32, &str)]) -> Vec<(u32, String)> {
    list.iter().map(|&(p, t)| (p, t.to_owned())).collect()
}

#[test]
fn create_rename_list_and_delete() {
    let env = Env::new();
    let mut lib = env.open();
    let [a, b, _] = ids(&lib);

    let road = lib.create_playlist("  Road trip ").unwrap();
    assert_eq!(road.name, "Road trip");
    assert_eq!((road.item_count, road.duration_ms), (0, 0));
    let chill = lib.create_playlist("chill").unwrap();
    assert!(matches!(
        lib.create_playlist("   "),
        Err(LibraryError::EmptyPlaylistName)
    ));

    // By name, ignoring case.
    let names: Vec<String> = lib
        .playlists()
        .unwrap()
        .into_iter()
        .map(|p| p.name)
        .collect();
    assert_eq!(names, ["chill", "Road trip"]);

    assert_eq!(lib.add_to_playlist(road.id, &[a, b, 999_999]).unwrap(), 2);
    let summary = lib.playlist(road.id).unwrap().unwrap().playlist;
    assert_eq!(summary.item_count, 2);
    assert_eq!(summary.duration_ms, 2_000);

    assert!(lib.rename_playlist(chill.id, "Zen").unwrap());
    assert!(!lib.rename_playlist(12_345, "x").unwrap());
    assert!(matches!(
        lib.rename_playlist(chill.id, ""),
        Err(LibraryError::EmptyPlaylistName)
    ));
    assert!(matches!(
        lib.add_to_playlist(12_345, &[a]),
        Err(LibraryError::PlaylistNotFound(12_345))
    ));

    assert!(lib.delete_playlist(road.id).unwrap());
    assert!(!lib.delete_playlist(road.id).unwrap());
    assert!(lib.playlist(road.id).unwrap().is_none());
    let names: Vec<String> = lib
        .playlists()
        .unwrap()
        .into_iter()
        .map(|p| p.name)
        .collect();
    assert_eq!(names, ["Zen"]);
}

#[test]
fn deleting_a_playlist_removes_its_entries() {
    let env = Env::new();
    let mut lib = env.open();
    let [a, b, c] = ids(&lib);
    let p = lib.create_playlist("p").unwrap();
    lib.add_to_playlist(p.id, &[a, b, c]).unwrap();
    lib.delete_playlist(p.id).unwrap();
    drop(lib);

    let db = rusqlite::Connection::open(env.tmp.path().join("library.db")).unwrap();
    let left: i64 = db
        .query_row("SELECT count(*) FROM playlist_items", [], |r| r.get(0))
        .unwrap();
    assert_eq!(left, 0);
}

#[test]
fn positions_stay_dense_through_remove_and_move() {
    let env = Env::new();
    let mut lib = env.open();
    let [a, b, c] = ids(&lib);
    let p = lib.create_playlist("p").unwrap().id;

    // The same track may appear twice; appends keep the given order.
    lib.add_to_playlist(p, &[a, b]).unwrap();
    lib.add_to_playlist(p, &[c, a]).unwrap();
    assert_eq!(
        entries(&lib, p),
        titles(&[(0, "A"), (1, "B"), (2, "C"), (3, "A")])
    );

    // Down, then up.
    assert!(lib.move_in_playlist(p, 0, 2).unwrap());
    assert_eq!(
        entries(&lib, p),
        titles(&[(0, "B"), (1, "C"), (2, "A"), (3, "A")])
    );
    assert!(lib.move_in_playlist(p, 3, 0).unwrap());
    assert_eq!(
        entries(&lib, p),
        titles(&[(0, "A"), (1, "B"), (2, "C"), (3, "A")])
    );
    assert!(lib.move_in_playlist(p, 1, 1).unwrap());
    assert!(!lib.move_in_playlist(p, 0, 4).unwrap());
    assert!(!lib.move_in_playlist(p, 4, 0).unwrap());

    assert!(lib.remove_from_playlist(p, 1).unwrap());
    assert_eq!(entries(&lib, p), titles(&[(0, "A"), (1, "C"), (2, "A")]));
    assert!(!lib.remove_from_playlist(p, 3).unwrap());
    assert!(!lib.remove_from_playlist(12_345, 0).unwrap());
    assert!(lib.remove_from_playlist(p, 2).unwrap());
    assert!(lib.remove_from_playlist(p, 0).unwrap());
    assert_eq!(entries(&lib, p), titles(&[(0, "C")]));
}

#[test]
fn entries_survive_their_file_leaving_and_returning() {
    let env = Env::new();
    let mut lib = env.open();
    let [a, b, c] = ids(&lib);
    let p = lib.create_playlist("p").unwrap().id;
    lib.add_to_playlist(p, &[a, b, c]).unwrap();

    // B leaves the library: hidden and counted as missing, with positions unchanged.
    let b_path = env.music.join("Album").join("02.flac");
    let parked = env.tmp.path().join("parked.flac");
    fs::rename(&b_path, &parked).unwrap();
    lib.scan().unwrap();
    let detail = lib.playlist(p).unwrap().unwrap();
    assert_eq!(detail.missing, 1);
    assert_eq!(detail.playlist.item_count, 3);
    assert_eq!(detail.playlist.duration_ms, 2_000);
    assert_eq!(entries(&lib, p), titles(&[(0, "A"), (2, "C")]));

    // Editing around the missing entry keeps it in place.
    assert!(lib.move_in_playlist(p, 2, 0).unwrap());
    assert_eq!(entries(&lib, p), titles(&[(0, "C"), (1, "A")]));

    // B comes back at the same path (as a new track row) and shows up again.
    fs::rename(&parked, &b_path).unwrap();
    lib.scan().unwrap();
    let detail = lib.playlist(p).unwrap().unwrap();
    assert_eq!(detail.missing, 0);
    assert_eq!(entries(&lib, p), titles(&[(0, "C"), (1, "A"), (2, "B")]));
    assert_eq!(detail.items[2].track.path, b_path);
}
