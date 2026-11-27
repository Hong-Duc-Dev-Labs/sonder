//! Favorites (schema v7) and recently played albums queries.

mod fixtures;

use std::path::PathBuf;

use fixtures::{Meta, flac};
use rusqlite::Connection;
use sonder_library::{Library, SCHEMA_VERSION};

const NOW: i64 = 1_794_657_600;

struct Env {
    tmp: tempfile::TempDir,
    music: PathBuf,
    lib: Library,
    a: PathBuf,
    b: PathBuf,
    c: PathBuf,
}

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
        tmp,
        music,
        lib,
        a,
        b,
        c,
    }
}

#[test]
fn favorites_toggle_list_and_survive_rescan() {
    let Env {
        mut lib,
        music,
        a,
        b,
        ..
    } = env();

    assert!(lib.favorite_tracks().unwrap().is_empty());
    assert!(lib.favorite_paths().unwrap().is_empty());

    assert!(lib.toggle_favorite(&a).unwrap());
    assert!(lib.track_by_path(&a).unwrap().unwrap().favorite);
    assert!(!lib.track_by_path(&b).unwrap().unwrap().favorite);

    lib.set_favorite(&b, true).unwrap();
    lib.set_favorite(&b, true).unwrap(); // idempotent
    let fav_paths = lib.favorite_paths().unwrap();
    assert_eq!(fav_paths.len(), 2);

    // Removing and re-adding the root assigns new track IDs; path-keyed favorites survive.
    lib.remove_root(&music).unwrap();
    lib.add_root(&music, false).unwrap();
    lib.scan().unwrap();

    let fav_tracks = lib.favorite_tracks().unwrap();
    assert_eq!(fav_tracks.len(), 2);
    assert!(fav_tracks.iter().all(|t| t.favorite));

    assert!(!lib.toggle_favorite(&a).unwrap());
    assert!(!lib.track_by_path(&a).unwrap().unwrap().favorite);
    assert_eq!(lib.favorite_tracks().unwrap().len(), 1);
}

#[test]
fn recent_albums_order_distinct_albums_without_inflating_counts() {
    let Env {
        mut lib, a, b, c, ..
    } = env();

    lib.record_play(&a, NOW - 300, 1_000).unwrap();
    lib.record_play(&b, NOW - 200, 1_000).unwrap();
    lib.record_play(&c, NOW - 100, 1_000).unwrap();
    lib.record_play(&a, NOW - 50, 1_000).unwrap(); // First is now most recent, played 3x total.

    let recent = lib.recent_albums(10).unwrap();
    assert_eq!(recent.len(), 2);
    assert_eq!(recent[0].title, "First");
    assert_eq!(recent[0].track_count, 2, "must not multiply by play count");
    assert_eq!(recent[1].title, "Second");
    assert_eq!(recent[1].track_count, 1);

    let limited = lib.recent_albums(1).unwrap();
    assert_eq!(limited.len(), 1);
    assert_eq!(limited[0].title, "First");
}

#[test]
fn opening_v6_database_creates_backup_and_migrates_to_v7() {
    let Env {
        tmp, music, lib, a, ..
    } = env();
    drop(lib);
    drop(music);

    // Clone the populated library DB via VACUUM INTO, then roll it back to v6 without `favorites`.
    let db_path = tmp.path().join("v6.db");
    {
        let src = Connection::open(tmp.path().join("library.db")).unwrap();
        src.execute("VACUUM INTO ?1", [db_path.to_str().unwrap()])
            .unwrap();
    }
    {
        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "DROP TABLE favorites;
             DROP INDEX IF EXISTS roots_source_kind;
             DROP INDEX IF EXISTS roots_remote_folder;
             DROP INDEX IF EXISTS tracks_source_kind;
             DROP INDEX IF EXISTS tracks_remote_file;
             ALTER TABLE roots DROP COLUMN source_kind;
             ALTER TABLE roots DROP COLUMN remote_folder_id;
             ALTER TABLE roots DROP COLUMN display_name;
             ALTER TABLE tracks DROP COLUMN source_kind;
             ALTER TABLE tracks DROP COLUMN remote_file_id;
             ALTER TABLE tracks DROP COLUMN remote_folder_id;
             ALTER TABLE tracks DROP COLUMN remote_modified_time;
             ALTER TABLE tracks DROP COLUMN remote_size;",
        )
        .unwrap();
        conn.pragma_update(None, "user_version", 6).unwrap();
    }

    let mut migrated = Library::open(&db_path, tmp.path().join("art-v6")).unwrap();
    assert!(db_path.with_file_name("v6.db.bak-6").exists());
    let conn = Connection::open(&db_path).unwrap();
    let version: u32 = conn
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(version, SCHEMA_VERSION);

    assert!(migrated.toggle_favorite(&a).unwrap());
    assert!(migrated.track_by_path(&a).unwrap().unwrap().favorite);
}
