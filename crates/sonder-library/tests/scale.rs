//! Timing of a full scan and a no-change rescan over 10 000 generated tracks.
//!
//! Ignored by default (it writes ~1 000 cover images); run with
//! `cargo test --release -p sonder-library --test scale -- --ignored --nocapture`.

mod fixtures;

use std::time::Instant;

use fixtures::{Meta, flac, png};
use sonder_library::{AlbumSort, Library};

const ALBUMS: usize = 1_000;
const TRACKS_PER_ALBUM: usize = 10;

#[test]
#[ignore = "slow: generates 10k files"]
fn ten_thousand_tracks() {
    let tmp = tempfile::tempdir().unwrap();
    let music = tmp.path().join("music");

    let start = Instant::now();
    for a in 0..ALBUMS {
        let artist = format!("Artist {}", a / 10);
        let album = format!("Album {a}");
        let cover = png(600, 600, (a % 256) as u8);
        for t in 1..=TRACKS_PER_ALBUM {
            let path = music
                .join(&artist)
                .join(&album)
                .join(format!("{t:02} - Song {t}.flac"));
            flac(
                &path,
                44_100,
                16,
                2,
                240,
                &Meta {
                    title: Some(&format!("Song {a}-{t}")),
                    artist: Some(&artist),
                    album: Some(&album),
                    album_artist: Some(&artist),
                    track: Some(t as u32),
                    year: Some("2001"),
                    track_gain: Some("-7.5 dB"),
                    cover: Some(cover.clone()),
                    ..Default::default()
                },
            );
        }
    }
    println!(
        "generated {} files in {:.2?}",
        ALBUMS * TRACKS_PER_ALBUM,
        start.elapsed()
    );

    let mut lib = Library::open(tmp.path().join("library.db"), tmp.path().join("art")).unwrap();
    lib.add_root(&music, false).unwrap();

    let first = lib.scan().unwrap();
    assert_eq!(
        first.added,
        ALBUMS * TRACKS_PER_ALBUM,
        "{:?}",
        first.failed.first()
    );
    println!("full scan: {:.2?}", first.elapsed);

    let again = lib.scan().unwrap();
    assert_eq!(again.unchanged, ALBUMS * TRACKS_PER_ALBUM);
    println!("rescan, no changes: {:.2?}", again.elapsed);

    let t = Instant::now();
    let albums = lib.albums(AlbumSort::Artist).unwrap();
    assert_eq!(albums.len(), ALBUMS);
    println!("list albums: {:.2?}", t.elapsed());

    let t = Instant::now();
    let hits = lib.search("song 999", 100).unwrap();
    assert!(!hits.is_empty());
    println!("search: {:.2?}", t.elapsed());
}
