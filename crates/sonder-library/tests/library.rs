//! End-to-end scans of generated fixture trees: format detection, grouping, art, incremental
//! rescans, root handling and queries.

mod fixtures;

use std::fs;
use std::path::{Path, PathBuf};

use fixtures::{DSD64, Meta, aiff, dff, dsf, flac, png, wav};
use sonder_library::{AlbumSort, Codec, Library, LibraryError, THUMB_SIZES};
use tempfile::TempDir;

struct Env {
    _tmp: TempDir,
    music: PathBuf,
    db: PathBuf,
    art: PathBuf,
}

impl Env {
    fn new() -> Env {
        let tmp = tempfile::tempdir().unwrap();
        let music = tmp.path().join("music");
        fs::create_dir_all(&music).unwrap();
        Env {
            db: tmp.path().join("data").join("library.db"),
            art: tmp.path().join("art"),
            music,
            _tmp: tmp,
        }
    }

    fn open(&self) -> Library {
        Library::open(&self.db, &self.art).unwrap()
    }

    fn path(&self, rel: &str) -> PathBuf {
        rel.split('/').fold(self.music.clone(), |p, c| p.join(c))
    }
}

/// Mixed formats across four albums, plus files that must be skipped or reported.
fn build_tree(env: &Env) {
    let kob = Meta {
        artist: Some("Miles Davis"),
        album: Some("Kind of Blue"),
        album_artist: Some("Miles Davis"),
        year: Some("1959"),
        ..Default::default()
    };
    flac(
        &env.path("Miles Davis/Kind of Blue/01 So What.flac"),
        44_100,
        16,
        2,
        5,
        &Meta {
            title: Some("So What"),
            track: Some(1),
            track_gain: Some("-6.54 dB"),
            album_gain: Some("-7.10 dB"),
            cover: Some(png(640, 640, 10)),
            ..kob.clone()
        },
    );
    wav(
        &env.path("Miles Davis/Kind of Blue/02 Freddie.wav"),
        44_100,
        &Meta {
            title: Some("Freddie Freeloader"),
            track: Some(2),
            cover: Some(png(640, 640, 10)),
            ..kob.clone()
        },
    );

    // Compilation without album artist; cover only as a folder image.
    aiff(
        &env.path("Compilations/Now Jazz/01.aiff"),
        48_000,
        24,
        &Meta {
            title: Some("Take Five"),
            artist: Some("Dave Brubeck"),
            album: Some("Now Jazz"),
            track: Some(1),
            year: Some("2001"),
            ..Default::default()
        },
    );
    flac(
        &env.path("Compilations/Now Jazz/02.flac"),
        96_000,
        24,
        2,
        3,
        &Meta {
            title: Some("Halo"),
            artist: Some("Beyoncé"),
            album: Some("Now Jazz"),
            track: Some(2),
            ..Default::default()
        },
    );
    fs::write(
        env.path("Compilations/Now Jazz/cover.png"),
        png(200, 200, 99),
    )
    .unwrap();

    // DSD album split over a disc folder.
    let dsd = Meta {
        artist: Some("Artist D"),
        album: Some("DSD Album"),
        album_artist: Some("Artist D"),
        ..Default::default()
    };
    dsf(
        &env.path("DSD/DSD Album/01.dsf"),
        DSD64,
        3,
        &Meta {
            title: Some("First"),
            track: Some(1),
            disc: Some(1),
            ..dsd.clone()
        },
    );
    dff(
        &env.path("DSD/DSD Album/CD2/01.dff"),
        DSD64 * 2,
        500,
        Some(&Meta {
            title: Some("Second"),
            track: Some(1),
            disc: Some(2),
            ..dsd.clone()
        }),
        None,
    );

    // Untagged DFF: native DIIN title, folder name as album.
    dff(
        &env.path("Bootleg/CD1/03 - Untitled.dff"),
        DSD64,
        250,
        None,
        Some("Live Take"),
    );

    fs::write(env.path("Bootleg/notes.txt"), "not audio").unwrap();
    fs::write(env.path("Bootleg/._03 - Untitled.dff"), "AppleDouble").unwrap();
    fs::write(env.path("Bootleg/broken.flac"), "not a flac file").unwrap();
}

#[test]
fn scan_builds_albums_from_mixed_formats() {
    let env = Env::new();
    build_tree(&env);
    let mut lib = env.open();
    lib.add_root(&env.music, false).unwrap();
    let report = lib.scan().unwrap();

    assert_eq!(report.added, 7, "{report:?}");
    assert_eq!(
        (report.updated, report.removed, report.unchanged),
        (0, 0, 0)
    );
    assert_eq!(report.failed.len(), 1, "{:?}", report.failed);
    assert!(report.failed[0].path.ends_with("broken.flac"));

    let albums = lib.albums(AlbumSort::Title).unwrap();
    let summary: Vec<(&str, Option<&str>, u32)> = albums
        .iter()
        .map(|a| (a.title.as_str(), a.artist.as_deref(), a.track_count))
        .collect();
    assert_eq!(
        summary,
        vec![
            ("Bootleg", None, 1),
            ("DSD Album", Some("Artist D"), 2),
            ("Kind of Blue", Some("Miles Davis"), 2),
            ("Now Jazz", Some("Various Artists"), 2),
        ]
    );

    // Formats.
    let kob = albums.iter().find(|a| a.title == "Kind of Blue").unwrap();
    assert_eq!(kob.year, Some(1959));
    let detail = lib.album(kob.id).unwrap().unwrap();
    let so_what = &detail.tracks[0];
    assert_eq!(so_what.title, "So What");
    assert_eq!(so_what.audio.codec, Codec::Flac);
    assert_eq!(
        (
            so_what.audio.rate,
            so_what.audio.bits,
            so_what.audio.channels
        ),
        (44_100, Some(16), 2)
    );
    assert_eq!(so_what.audio.duration_ms, 5_000);
    assert_eq!(so_what.replay_gain.track_gain, Some(-6.54));
    assert_eq!(so_what.replay_gain.album_gain, Some(-7.10));
    assert_eq!(detail.tracks[1].title, "Freddie Freeloader");
    assert_eq!(detail.tracks[1].audio.codec, Codec::Wav);

    let now = albums.iter().find(|a| a.title == "Now Jazz").unwrap();
    let now = lib.album(now.id).unwrap().unwrap();
    assert_eq!(now.tracks[0].audio.codec, Codec::Aiff);
    assert_eq!(
        (now.tracks[0].audio.rate, now.tracks[0].audio.bits),
        (48_000, Some(24))
    );
    assert_eq!(now.album.year, Some(2001));

    let dsd = albums.iter().find(|a| a.title == "DSD Album").unwrap();
    let dsd = lib.album(dsd.id).unwrap().unwrap();
    let formats: Vec<_> = dsd
        .tracks
        .iter()
        .map(|t| {
            (
                t.audio.codec,
                t.audio.rate,
                t.audio.bits,
                t.audio.duration_ms,
                t.disc_no,
            )
        })
        .collect();
    assert_eq!(
        formats,
        vec![
            (Codec::Dsf, DSD64, Some(1), 3_000, Some(1)),
            (Codec::Dff, DSD64 * 2, Some(1), 500, Some(2)),
        ]
    );

    let bootleg = albums.iter().find(|a| a.title == "Bootleg").unwrap();
    let bootleg = lib.album(bootleg.id).unwrap().unwrap();
    assert_eq!(bootleg.tracks[0].title, "Live Take");
    assert_eq!(bootleg.tracks[0].track_no, Some(3));
    assert_eq!(bootleg.tracks[0].audio.duration_ms, 250);

    // Art: embedded for Kind of Blue (one cover shared by both tracks), folder image for
    // Now Jazz, none elsewhere.
    let kob_art = kob.art.clone().expect("embedded cover");
    assert!(
        detail
            .tracks
            .iter()
            .all(|t| t.art.as_ref() == Some(&kob_art))
    );
    let now_art = now.album.art.clone().expect("folder cover");
    assert_ne!(now_art, kob_art);
    assert_eq!(dsd.album.art, None);
    for hash in [&kob_art, &now_art] {
        for size in THUMB_SIZES {
            assert!(lib.art_cache().thumbnail_path(hash, size).is_file());
        }
    }
    let thumb = image::open(lib.art_cache().thumbnail_path(&kob_art, 300)).unwrap();
    assert_eq!((thumb.width(), thumb.height()), (300, 300));
}

#[test]
fn queries_artists_and_search() {
    let env = Env::new();
    build_tree(&env);
    let mut lib = env.open();
    lib.add_root(&env.music, false).unwrap();
    lib.scan().unwrap();

    let names: Vec<String> = lib.artists().unwrap().into_iter().map(|a| a.name).collect();
    assert_eq!(
        names,
        [
            "Artist D",
            "Beyoncé",
            "Dave Brubeck",
            "Miles Davis",
            "Various Artists"
        ]
    );

    let artists = lib.artists().unwrap();
    let miles = artists.iter().find(|a| a.name == "Miles Davis").unwrap();
    assert_eq!((miles.album_count, miles.track_count), (1, 2));
    let detail = lib.artist(miles.id).unwrap().unwrap();
    assert_eq!(detail.albums.len(), 1);
    assert_eq!(detail.albums[0].title, "Kind of Blue");
    assert!(detail.appears_on.is_empty());

    let brubeck = artists.iter().find(|a| a.name == "Dave Brubeck").unwrap();
    let detail = lib.artist(brubeck.id).unwrap().unwrap();
    assert!(detail.albums.is_empty());
    assert_eq!(detail.appears_on.len(), 1);
    assert_eq!(detail.appears_on[0].title, "Now Jazz");

    let titles = |q: &str| -> Vec<String> {
        let mut t: Vec<String> = lib
            .search(q, 50)
            .unwrap()
            .into_iter()
            .map(|t| t.title)
            .collect();
        t.sort();
        t
    };
    assert_eq!(titles("freddie"), ["Freddie Freeloader"]);
    assert_eq!(titles("MILES"), ["Freddie Freeloader", "So What"]);
    assert_eq!(titles("kind blue"), ["Freddie Freeloader", "So What"]);
    assert_eq!(titles("ind of bl"), ["Freddie Freeloader", "So What"]);
    assert_eq!(titles("beyonce"), ["Halo"]);
    assert_eq!(titles("so"), ["So What"]);
    assert_eq!(titles("jazz five"), ["Take Five"]);
    assert_eq!(titles("bootleg"), ["Live Take"]);
    assert!(titles("zzzz").is_empty());
    assert!(titles("  ").is_empty());
    assert!(titles("\"").is_empty());
    assert_eq!(lib.search("miles", 1).unwrap().len(), 1);

    let by_added = lib.albums(AlbumSort::Added).unwrap();
    assert_eq!(by_added.len(), 4);
    let by_year: Vec<Option<i32>> = lib
        .albums(AlbumSort::Year)
        .unwrap()
        .iter()
        .map(|a| a.year)
        .collect();
    assert_eq!(by_year, [Some(1959), Some(2001), None, None]);
    let by_artist: Vec<String> = lib
        .albums(AlbumSort::Artist)
        .unwrap()
        .into_iter()
        .map(|a| a.title)
        .collect();
    assert_eq!(
        by_artist,
        ["DSD Album", "Kind of Blue", "Now Jazz", "Bootleg"]
    );
}

#[test]
fn rescan_applies_only_the_delta() {
    let env = Env::new();
    build_tree(&env);
    let mut lib = env.open();
    lib.add_root(&env.music, false).unwrap();
    lib.scan().unwrap();

    // Nothing changed: every file is skipped by (mtime, size).
    let report = lib.scan().unwrap();
    assert_eq!(
        (
            report.added,
            report.updated,
            report.removed,
            report.unchanged
        ),
        (0, 0, 0, 7)
    );
    assert!(!report.changed());

    // Edit one file's tags, add one file, delete a whole album.
    let freddie = env.path("Miles Davis/Kind of Blue/02 Freddie.wav");
    wav(
        &freddie,
        44_100,
        &Meta {
            title: Some("Freddie Freeloader (Alternate Take)"),
            artist: Some("Miles Davis"),
            album: Some("Kind of Blue"),
            album_artist: Some("Miles Davis"),
            track: Some(2),
            ..Default::default()
        },
    );
    flac(
        &env.path("Miles Davis/Kind of Blue/03 Blue in Green.flac"),
        44_100,
        16,
        2,
        2,
        &Meta {
            title: Some("Blue in Green"),
            artist: Some("Miles Davis"),
            album: Some("Kind of Blue"),
            album_artist: Some("Miles Davis"),
            track: Some(3),
            ..Default::default()
        },
    );
    let now_art = {
        let a = lib.albums(AlbumSort::Title).unwrap();
        a.into_iter()
            .find(|a| a.title == "Now Jazz")
            .unwrap()
            .art
            .unwrap()
    };
    fs::remove_dir_all(env.path("Compilations")).unwrap();

    let report = lib.scan().unwrap();
    assert_eq!(
        (
            report.added,
            report.updated,
            report.removed,
            report.unchanged
        ),
        (1, 1, 2, 4),
        "{report:?}"
    );

    let albums = lib.albums(AlbumSort::Title).unwrap();
    assert!(albums.iter().all(|a| a.title != "Now Jazz"));
    let kob = albums.iter().find(|a| a.title == "Kind of Blue").unwrap();
    let titles: Vec<String> = lib
        .album(kob.id)
        .unwrap()
        .unwrap()
        .tracks
        .into_iter()
        .map(|t| t.title)
        .collect();
    assert_eq!(
        titles,
        [
            "So What",
            "Freddie Freeloader (Alternate Take)",
            "Blue in Green"
        ]
    );
    // The edited file no longer embeds the cover, so it falls back to the album's.
    assert!(lib.search("alternate", 5).unwrap()[0].art.is_some());

    // Orphans are gone: compilation artists, their art row and its thumbnails.
    let names: Vec<String> = lib.artists().unwrap().into_iter().map(|a| a.name).collect();
    assert!(
        !names
            .iter()
            .any(|n| n == "Beyoncé" || n == "Various Artists"),
        "{names:?}"
    );
    for size in THUMB_SIZES {
        assert!(!lib.art_cache().thumbnail_path(&now_art, size).exists());
    }
    assert!(lib.search("beyonce", 5).unwrap().is_empty());
}

#[test]
fn scan_paths_reconciles_only_the_given_paths() {
    let env = Env::new();
    build_tree(&env);
    let mut lib = env.open();
    lib.add_root(&env.music, false).unwrap();
    lib.scan().unwrap();

    // A deleted file elsewhere is not noticed by a scan of another folder.
    fs::remove_file(env.path("Bootleg/CD1/03 - Untitled.dff")).unwrap();
    let report = lib.scan_paths(&[env.path("DSD")]).unwrap();
    assert_eq!((report.removed, report.unchanged), (0, 2));
    assert_eq!(lib.albums(AlbumSort::Title).unwrap().len(), 4);

    // The deleted file's own path removes it; its now-empty folder works too.
    let report = lib
        .scan_paths(&[env.path("Bootleg/CD1/03 - Untitled.dff")])
        .unwrap();
    assert_eq!(report.removed, 1);
    assert_eq!(lib.albums(AlbumSort::Title).unwrap().len(), 3);

    // A sibling folder whose name extends another's is not included.
    let report = lib.scan_paths(&[env.path("DSD/DSD Album/CD")]).unwrap();
    assert_eq!((report.removed, report.unchanged), (0, 0));

    // Paths outside every root are ignored.
    let report = lib.scan_paths(std::slice::from_ref(&env.db)).unwrap();
    assert_eq!((report.added, report.removed, report.unchanged), (0, 0, 0));
}

#[test]
fn unavailable_root_keeps_its_tracks() {
    let env = Env::new();
    build_tree(&env);
    let mut lib = env.open();
    lib.add_root(&env.music, false).unwrap();
    lib.scan().unwrap();

    let moved = env.music.with_file_name("unplugged");
    fs::rename(&env.music, &moved).unwrap();
    let report = lib.scan().unwrap();
    assert_eq!(report.removed, 0);
    assert_eq!(report.failed.len(), 1);
    assert_eq!(lib.albums(AlbumSort::Title).unwrap().len(), 4);

    fs::rename(&moved, &env.music).unwrap();
    let report = lib.scan().unwrap();
    assert_eq!((report.removed, report.unchanged), (0, 7));
}

#[test]
fn roots_are_managed() {
    let env = Env::new();
    build_tree(&env);
    let mut lib = env.open();
    let root = lib.add_root(&env.music, false).unwrap();
    assert_eq!(lib.roots().unwrap(), vec![root.clone()]);

    // Re-adding updates the symlink setting; nested or enclosing folders are refused.
    let again = lib.add_root(&env.music, true).unwrap();
    assert_eq!((again.id, again.follow_symlinks), (root.id, true));
    assert!(matches!(
        lib.add_root(env.path("DSD"), false),
        Err(LibraryError::RootOverlaps { .. })
    ));
    assert!(matches!(
        lib.add_root(env.music.parent().unwrap(), false),
        Err(LibraryError::RootOverlaps { .. })
    ));
    assert!(matches!(
        lib.add_root(env.path("missing"), false),
        Err(LibraryError::NotADirectory(_))
    ));

    lib.scan().unwrap();
    let art_hashes: Vec<String> = lib
        .albums(AlbumSort::Title)
        .unwrap()
        .into_iter()
        .filter_map(|a| a.art)
        .collect();
    assert!(lib.remove_root(&env.music).unwrap());
    assert!(!lib.remove_root(&env.music).unwrap());
    assert!(lib.roots().unwrap().is_empty());
    assert!(lib.albums(AlbumSort::Title).unwrap().is_empty());
    assert!(lib.artists().unwrap().is_empty());
    assert!(lib.search("miles", 5).unwrap().is_empty());
    for hash in art_hashes {
        assert!(
            !lib.art_cache()
                .thumbnail_path(&hash, THUMB_SIZES[0])
                .exists()
        );
    }
}

#[test]
fn reopening_keeps_the_index_and_two_connections_share_it() {
    let env = Env::new();
    build_tree(&env);
    {
        let mut lib = env.open();
        lib.add_root(&env.music, false).unwrap();
        lib.scan().unwrap();
    }
    let mut a = env.open();
    let b = env.open();
    assert_eq!(b.albums(AlbumSort::Title).unwrap().len(), 4);
    fs::remove_dir_all(env.path("Bootleg")).unwrap();
    a.scan().unwrap();
    assert_eq!(b.albums(AlbumSort::Title).unwrap().len(), 3);
}

/// Creates a directory symlink; `false` when the OS refuses (Windows without Developer Mode).
fn link_dir(target: &Path, link: &Path) -> bool {
    fs::create_dir_all(link.parent().unwrap()).unwrap();
    #[cfg(unix)]
    let result = std::os::unix::fs::symlink(target, link);
    #[cfg(windows)]
    let result = std::os::windows::fs::symlink_dir(target, link);
    match result {
        Ok(()) => true,
        Err(e) if cfg!(windows) => {
            eprintln!("skipping: cannot create symlinks here ({e})");
            false
        }
        Err(e) => panic!("symlink: {e}"),
    }
}

#[test]
fn symlinks_are_followed_only_on_opt_in() {
    let env = Env::new();
    // One link directly in the root and one in a subfolder, both leading outside it.
    for (name, link) in [("outside-a", "link"), ("outside-b", "Album/link")] {
        let outside = env.music.with_file_name(name);
        flac(
            &outside.join("x.flac"),
            44_100,
            16,
            2,
            1,
            &Meta {
                title: Some(name),
                ..Default::default()
            },
        );
        if !link_dir(&outside, &env.path(link)) {
            return;
        }
    }
    let mut lib = env.open();
    lib.add_root(&env.music, false).unwrap();
    assert_eq!(lib.scan().unwrap().added, 0);
    lib.add_root(&env.music, true).unwrap();
    assert_eq!(lib.scan().unwrap().added, 2);
}

#[test]
fn missing_paths_do_not_need_to_exist_to_be_ignored() {
    let env = Env::new();
    let mut lib = env.open();
    lib.add_root(&env.music, false).unwrap();
    let report = lib
        .scan_paths(&[Path::new("does/not/exist").to_path_buf()])
        .unwrap();
    assert!(!report.changed());
}

#[test]
fn alac_with_a_padded_sample_entry_is_indexed() {
    let env = Env::new();
    let path = env.path("Album/01 Padded.m4a");
    fixtures::alac_m4a_padded(&path, 96_000, 24, 2);
    // The case being covered: lofty reads the file but cannot name its codec.
    assert_eq!(lofty_mp4_codec(&path), None);
    let meta = sonder_library::read_file(&path).unwrap();
    assert_eq!(meta.audio.codec, Codec::Alac);
    assert_eq!(
        (meta.audio.rate, meta.audio.bits, meta.audio.channels),
        (96_000, Some(24), 2)
    );
    // The file has no samples, so the duration comes from the track header.
    assert_eq!(meta.audio.duration_ms, 2_000);
    assert_eq!(meta.tags.title.as_deref(), Some("Padded"));
}

#[test]
fn pcm_in_mp4_is_not_indexed_as_alac() {
    let env = Env::new();
    let path = env.path("Album/01 Pcm.m4a");
    fixtures::pcm_m4a(&path, 48_000, 1);
    assert_eq!(lofty_mp4_codec(&path), None);
    let err = sonder_library::read_file(&path).unwrap_err();
    assert!(err.to_string().contains("not ALAC"), "{err}");
}

fn lofty_mp4_codec(path: &Path) -> Option<lofty::mp4::Mp4Codec> {
    use lofty::file::AudioFile;
    let mut file = fs::File::open(path).unwrap();
    lofty::mp4::Mp4File::read_from(&mut file, lofty::config::ParseOptions::new())
        .unwrap()
        .properties()
        .codec()
}
