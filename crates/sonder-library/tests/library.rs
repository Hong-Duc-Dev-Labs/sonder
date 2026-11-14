//! End-to-end scans of generated fixture trees: format detection, grouping, art, incremental
//! rescans, root handling and queries.

mod fixtures;

use std::fs;
use std::path::{Path, PathBuf};

use fixtures::{DSD64, Meta, aiff, dff, dsf, flac, png, wav};
use sonder_library::{AlbumSort, Codec, Library, LibraryError, ScanProgress, THUMB_SIZES};
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

#[test]
fn albums_page_through_the_sorted_list_and_scan_reports_progress() {
    let env = Env::new();
    build_tree(&env);
    let mut lib = env.open();
    lib.add_root(&env.music, false).unwrap();
    let seen = std::sync::Mutex::new(Vec::new());
    let report = lib
        .scan_with_progress(&|p| seen.lock().unwrap().push(p))
        .unwrap();

    let seen = seen.into_inner().unwrap();
    let read = report.added + 1; // the broken file is read too, then reported
    assert_eq!(seen.first(), Some(&ScanProgress::Walking));
    assert_eq!(seen.last(), Some(&ScanProgress::Writing));
    let reading: Vec<usize> = seen
        .iter()
        .filter_map(|p| match p {
            ScanProgress::Reading { done, total } => {
                assert_eq!(*total, read);
                Some(*done)
            }
            _ => None,
        })
        .collect();
    let mut sorted = reading.clone();
    sorted.sort_unstable();
    assert_eq!(sorted, (0..=read).collect::<Vec<_>>());

    let all = lib.albums(AlbumSort::Title).unwrap();
    assert_eq!(lib.album_count().unwrap(), all.len());
    let mut paged = lib.albums_page(AlbumSort::Title, 0, 3).unwrap();
    assert_eq!(paged.len(), 3);
    paged.extend(lib.albums_page(AlbumSort::Title, 3, 3).unwrap());
    assert_eq!(paged, all);
    assert!(
        lib.albums_page(AlbumSort::Title, all.len(), 3)
            .unwrap()
            .is_empty()
    );
}

/// A PNG filled by `pixel(x, y)`.
fn painted_png(side: u32, pixel: impl Fn(u32, u32) -> [u8; 3]) -> Vec<u8> {
    let img = image::RgbImage::from_fn(side, side, |x, y| image::Rgb(pixel(x, y)));
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

fn rgb(hex: &str) -> (u8, u8, u8) {
    let v = u32::from_str_radix(hex.trim_start_matches('#'), 16).unwrap();
    ((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

/// Two albums: one with a mostly red, partly blue cover, one with a grey cover.
fn build_colour_tree(env: &Env) {
    let red_blue = painted_png(256, |x, _| {
        if x < 192 {
            [210, 40, 40]
        } else {
            [40, 60, 210]
        }
    });
    let grey = painted_png(256, |x, y| {
        let v = 60 + ((x + y) % 120) as u8;
        [v, v, v]
    });
    for (dir, album, cover) in [("Red", "Red Album", red_blue), ("Grey", "Grey Album", grey)] {
        flac(
            &env.path(&format!("{dir}/01.flac")),
            44_100,
            16,
            2,
            1,
            &Meta {
                title: Some("One"),
                artist: Some("Painter"),
                album: Some(album),
                album_artist: Some("Painter"),
                cover: Some(cover),
                ..Default::default()
            },
        );
    }
}

#[test]
fn cover_colours_are_stored_ranked_and_shared_with_tracks() {
    let env = Env::new();
    build_colour_tree(&env);
    let mut lib = env.open();
    lib.add_root(&env.music, false).unwrap();
    lib.scan().unwrap();

    let albums = lib.albums(AlbumSort::Title).unwrap();
    let red = albums.iter().find(|a| a.title == "Red Album").unwrap();
    let grey = albums.iter().find(|a| a.title == "Grey Album").unwrap();

    assert_eq!(red.art_colors.len(), 2, "{:?}", red.art_colors);
    let (r, g, b) = rgb(&red.art_colors[0]);
    assert!(
        r > 170 && g < 90 && b < 90,
        "dominant is red: {:?}",
        red.art_colors
    );
    let (r, _, b) = rgb(&red.art_colors[1]);
    assert!(b > 170 && r < 90, "second is blue: {:?}", red.art_colors);
    assert!(grey.art.is_some());
    assert!(grey.art_colors.is_empty(), "{:?}", grey.art_colors);

    let detail = lib.album(red.id).unwrap().unwrap();
    assert_eq!(detail.tracks[0].art_colors, red.art_colors);

    // An unchanged rescan keeps them.
    lib.scan().unwrap();
    let again = lib.album(red.id).unwrap().unwrap();
    assert_eq!(again.album.art_colors, red.art_colors);
}

#[test]
fn backfill_fills_colours_of_covers_stored_without_them() {
    let env = Env::new();
    build_colour_tree(&env);
    let mut lib = env.open();
    lib.add_root(&env.music, false).unwrap();
    lib.scan().unwrap();
    let before: Vec<Vec<String>> = lib
        .albums(AlbumSort::Title)
        .unwrap()
        .into_iter()
        .map(|a| a.art_colors)
        .collect();

    // As a database written before the column existed would be.
    rusqlite::Connection::open(&env.db)
        .unwrap()
        .execute("UPDATE art SET colors = NULL", [])
        .unwrap();
    assert!(
        lib.albums(AlbumSort::Title)
            .unwrap()
            .iter()
            .all(|a| a.art_colors.is_empty())
    );

    assert_eq!(lib.backfill_art_colors().unwrap(), 2);
    let after: Vec<Vec<String>> = lib
        .albums(AlbumSort::Title)
        .unwrap()
        .into_iter()
        .map(|a| a.art_colors)
        .collect();
    // Backfill reads the JPEG thumbnail, not the original, so channels may differ slightly.
    assert_eq!(after.len(), before.len());
    for (a, b) in after.iter().zip(&before) {
        assert_eq!(a.len(), b.len(), "{after:?} vs {before:?}");
        for (x, y) in a.iter().zip(b) {
            let (x, y) = (rgb(x), rgb(y));
            let close = |p: u8, q: u8| p.abs_diff(q) <= 8;
            assert!(
                close(x.0, y.0) && close(x.1, y.1) && close(x.2, y.2),
                "{after:?} vs {before:?}"
            );
        }
    }
    assert_eq!(
        lib.backfill_art_colors().unwrap(),
        0,
        "nothing left to fill"
    );
}

#[test]
fn artist_tracks_list_own_albums_by_year_then_guest_spots() {
    let env = Env::new();
    let own = |album: &'static str, year: &'static str| Meta {
        artist: Some("Solo"),
        album: Some(album),
        album_artist: Some("Solo"),
        year: Some(year),
        ..Default::default()
    };
    for (file, title, track, meta) in [
        ("Solo/Later/02.flac", "Later Two", 2, own("Later", "2005")),
        ("Solo/Later/01.flac", "Later One", 1, own("Later", "2005")),
        ("Solo/Early/01.flac", "Early One", 1, own("Early", "1995")),
    ] {
        flac(
            &env.path(file),
            44_100,
            16,
            2,
            1,
            &Meta {
                title: Some(title),
                track: Some(track),
                ..meta
            },
        );
    }
    flac(
        &env.path("Mix/01.flac"),
        44_100,
        16,
        2,
        1,
        &Meta {
            title: Some("Guest Spot"),
            artist: Some("Solo"),
            album: Some("Mix"),
            album_artist: Some("Someone Else"),
            track: Some(1),
            ..Default::default()
        },
    );
    let mut lib = env.open();
    lib.add_root(&env.music, false).unwrap();
    lib.scan().unwrap();

    let solo = lib
        .artists()
        .unwrap()
        .into_iter()
        .find(|a| a.name == "Solo")
        .unwrap();
    let titles: Vec<String> = lib
        .artist_tracks(solo.id)
        .unwrap()
        .into_iter()
        .map(|t| t.title)
        .collect();
    assert_eq!(
        titles,
        ["Early One", "Later One", "Later Two", "Guest Spot"]
    );
}

#[test]
fn tracks_list_the_whole_library_by_title() {
    let env = Env::new();
    for (file, title, album) in [
        ("A/02.flac", "beta", "Zeta"),
        ("A/01.flac", "Alpha", "Zeta"),
        ("B/01.flac", "Beta", "Alpha Album"),
    ] {
        flac(
            &env.path(file),
            44_100,
            16,
            2,
            1,
            &Meta {
                title: Some(title),
                artist: Some("Someone"),
                album: Some(album),
                ..Default::default()
            },
        );
    }
    let mut lib = env.open();
    lib.add_root(&env.music, false).unwrap();
    lib.scan().unwrap();

    let tracks: Vec<(String, String)> = lib
        .tracks()
        .unwrap()
        .into_iter()
        .map(|t| (t.title, t.album))
        .collect();
    // Case-insensitive by title; equal titles by album.
    assert_eq!(
        tracks,
        [
            ("Alpha".to_string(), "Zeta".to_string()),
            ("Beta".to_string(), "Alpha Album".to_string()),
            ("beta".to_string(), "Zeta".to_string()),
        ]
    );
}

#[test]
fn artists_carry_a_cover_from_their_own_albums_first() {
    let env = Env::new();
    build_tree(&env);
    let mut lib = env.open();
    lib.add_root(&env.music, false).unwrap();
    lib.scan().unwrap();
    let artists = lib.artists().unwrap();
    let miles = artists.iter().find(|a| a.name == "Miles Davis").unwrap();
    let kob = lib
        .albums(AlbumSort::Title)
        .unwrap()
        .into_iter()
        .find(|a| a.title == "Kind of Blue")
        .unwrap();
    assert_eq!(miles.art, kob.art);
    // A guest artist without albums of their own falls back to the album they appear on.
    let brubeck = artists.iter().find(|a| a.name == "Dave Brubeck").unwrap();
    assert!(brubeck.art.is_some());
    assert_eq!(lib.artist(miles.id).unwrap().unwrap().artist.art, miles.art);
}

#[test]
fn loudness_analysis_fills_untagged_tracks_and_their_album() {
    let env = Env::new();
    let quiet = env.path("Measured/01 Quiet.wav");
    let loud = env.path("Measured/02 Loud.wav");
    fixtures::wav_tone(
        &quiet,
        44_100,
        2.0,
        0.02,
        &Meta {
            title: Some("Quiet"),
            album: Some("Measured"),
            album_artist: Some("Artist"),
            track: Some(1),
            ..Default::default()
        },
    );
    fixtures::wav_tone(
        &loud,
        44_100,
        3.0,
        0.2,
        &Meta {
            title: Some("Loud"),
            album: Some("Measured"),
            album_artist: Some("Artist"),
            track: Some(2),
            ..Default::default()
        },
    );

    let mut lib = env.open();
    lib.add_root(&env.music, false).unwrap();
    lib.scan().unwrap();
    assert_eq!(lib.loudness_pending().unwrap(), 2);

    let seen = std::sync::Mutex::new(Vec::new());
    let report = lib
        .analyze_loudness_with_progress(&|p| seen.lock().unwrap().push(p))
        .unwrap();
    assert_eq!(report.tracks, 2);
    assert_eq!(report.albums, 1);
    assert_eq!(report.failed, 0);
    assert_eq!(
        seen.lock().unwrap().last().copied(),
        Some(sonder_library::LoudnessProgress {
            done: 2,
            total: 2,
            failed: 0,
        })
    );
    assert_eq!(lib.loudness_pending().unwrap(), 0);

    let quiet = lib.track_loudness(&quiet).unwrap().unwrap();
    let loud = lib.track_loudness(&loud).unwrap().unwrap();
    let quiet_loudness = quiet.loudness.unwrap();
    let loud_loudness = loud.loudness.unwrap();
    assert!(quiet_loudness.lufs < loud_loudness.lufs);
    assert!(quiet_loudness.peak < loud_loudness.peak);

    // The album value sits between its tracks and is repeated on both rows.
    let album = quiet.album.unwrap();
    assert_eq!(quiet.album, loud.album);
    assert!(
        album.lufs > quiet_loudness.lufs && album.lufs < loud_loudness.lufs,
        "{album:?}"
    );
    assert!((album.peak - loud_loudness.peak).abs() < 1e-9);

    // Track gains preserve the loudness difference, which is what album levelling relies on.
    let (quiet_gain, quiet_peak) = quiet.track_gain(-18.0).unwrap();
    let (loud_gain, _) = loud.track_gain(-18.0).unwrap();
    assert!((quiet_gain - loud_gain - (loud_loudness.lufs - quiet_loudness.lufs)).abs() < 1e-9);
    assert_eq!(quiet_peak, quiet_loudness.peak);
}

/// Duration-weighted energy average of two measured tracks, as the analysis computes it.
fn album_lufs(tracks: &[(f64, f64)]) -> f64 {
    let (energy, weight) = tracks.iter().fold((0.0, 0.0), |(e, w), &(lufs, seconds)| {
        (e + 10f64.powf((lufs + 0.691) / 10.0) * seconds, w + seconds)
    });
    -0.691 + 10.0 * (energy / weight).log10()
}

#[test]
fn rescans_remeasure_changed_tracks_and_rebuild_their_album_without_placeholders() {
    let env = Env::new();
    let meta = |title, track| Meta {
        title: Some(title),
        album: Some("Mix"),
        album_artist: Some("Artist"),
        track: Some(track),
        ..Default::default()
    };
    let loud = env.path("Mix/01 Loud.wav");
    let quiet = env.path("Mix/02 Quiet.wav");
    let silent = env.path("Mix/03 Silent.wav");
    fixtures::wav_tone(&loud, 44_100, 2.0, 0.2, &meta("Loud", 1));
    fixtures::wav_tone(&quiet, 44_100, 2.0, 0.02, &meta("Quiet", 2));
    // Long digital silence: stored as the placeholder, it must neither get a gain nor drag
    // the album value down.
    fixtures::wav_tone(&silent, 44_100, 10.0, 0.0, &meta("Silent", 3));

    let mut lib = env.open();
    lib.add_root(&env.music, false).unwrap();
    lib.scan().unwrap();
    lib.analyze_loudness().unwrap();

    let read = |lib: &Library, path: &Path| lib.track_loudness(path).unwrap().unwrap();
    let silent_row = read(&lib, &silent);
    assert!(!silent_row.loudness.unwrap().is_measured());
    assert_eq!(silent_row.track_gain(-18.0), None);
    let loud_lufs = read(&lib, &loud).loudness.unwrap().lufs;
    let quiet_lufs = read(&lib, &quiet).loudness.unwrap().lufs;
    let album = read(&lib, &loud).album.unwrap();
    let expected = album_lufs(&[(loud_lufs, 2.0), (quiet_lufs, 2.0)]);
    assert!(
        (album.lufs - expected).abs() < 0.01,
        "{album:?} vs {expected}"
    );
    assert_eq!(silent_row.album, Some(album));

    // Replace the quiet track with louder, longer audio: the rescan drops its measurement and
    // the album value, and the next pass measures it again.
    fixtures::wav_tone(&quiet, 44_100, 3.0, 0.2, &meta("Quiet", 2));
    let report = lib.scan().unwrap();
    assert_eq!(report.updated, 1);
    assert_eq!(read(&lib, &quiet).loudness, None);
    assert_eq!(read(&lib, &loud).album, None);
    assert_eq!(lib.loudness_pending().unwrap(), 1);
    lib.analyze_loudness().unwrap();
    let remeasured = read(&lib, &quiet).loudness.unwrap().lufs;
    assert!(
        remeasured > quiet_lufs + 15.0,
        "{remeasured} vs {quiet_lufs}"
    );
    let album = read(&lib, &loud).album.unwrap();
    let expected = album_lufs(&[(loud_lufs, 2.0), (remeasured, 3.0)]);
    assert!(
        (album.lufs - expected).abs() < 0.01,
        "{album:?} vs {expected}"
    );

    // Removing a track leaves nothing to decode, but the album value is still rebuilt.
    fs::remove_file(&loud).unwrap();
    lib.scan().unwrap();
    assert_eq!(lib.loudness_pending().unwrap(), 0);
    let report = lib.analyze_loudness().unwrap();
    assert_eq!((report.tracks, report.albums), (0, 1));
    let album = read(&lib, &quiet).album.unwrap();
    assert!(
        (album.lufs - remeasured).abs() < 0.01,
        "{album:?} vs {remeasured}"
    );
}

#[test]
fn a_new_cover_and_lyrics_saved_into_a_flac_reach_the_index() {
    let env = Env::new();
    let path = env.path("Album/01 - So What.flac");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    flac(
        &path,
        44_100,
        16,
        2,
        1,
        &Meta {
            title: Some("So What"),
            artist: Some("Miles Davis"),
            album: Some("Kind of Blue"),
            cover: Some(png(8, 8, 40)),
            ..Default::default()
        },
    );
    // Stand-in audio frames: in a real file the metadata never ends the file.
    let mut bytes = fs::read(&path).unwrap();
    bytes.extend_from_slice(&[0xff; 4096]);
    fs::write(&path, &bytes).unwrap();
    let mut lib = env.open();
    lib.add_root(&env.music, false).unwrap();
    lib.scan().unwrap();
    let track = lib.tracks().unwrap().into_iter().next().unwrap();
    let old_art = track.art.clone().expect("the embedded cover is indexed");

    let new_cover = png(8, 8, 200);
    let edit = sonder_library::TagEdit {
        title: "So What".into(),
        artists: vec!["Miles Davis".into()],
        album: "Kind of Blue".into(),
        lyrics: Some("[00:01.00]So what\n".into()),
        cover: sonder_library::CoverEdit::Set(new_cover.clone()),
        ..Default::default()
    };
    sonder_library::write_file(&path, &edit).unwrap();

    // FLAC keeps pictures in PICTURE blocks; the reader must find the new one there.
    let tags = sonder_library::read_file_tags(&path).unwrap();
    assert_eq!(tags.cover.as_deref(), Some(new_cover.as_slice()));
    assert_eq!(tags.lyrics.as_deref(), Some("[00:01.00]So what"));
    let updated = lib.reindex_track(track.id).unwrap().unwrap();
    assert_ne!(updated.art, Some(old_art));
    assert!(updated.art.is_some());

    let removed = sonder_library::TagEdit {
        cover: sonder_library::CoverEdit::Remove,
        lyrics: Some(String::new()),
        ..edit
    };
    sonder_library::write_file(&path, &removed).unwrap();
    let tags = sonder_library::read_file_tags(&path).unwrap();
    assert_eq!((tags.cover, tags.lyrics), (None, None));
    assert_eq!(lib.reindex_track(track.id).unwrap().unwrap().art, None);
}

#[test]
fn a_rewrite_that_does_not_read_back_leaves_the_original() {
    // A FLAC with no audio frames: lofty cuts it short when its metadata shrinks.
    let env = Env::new();
    let path = env.path("a.flac");
    flac(
        &path,
        44_100,
        16,
        2,
        1,
        &Meta {
            title: Some("T"),
            cover: Some(png(8, 8, 40)),
            ..Default::default()
        },
    );
    let before = fs::read(&path).unwrap();
    let edit = sonder_library::TagEdit {
        title: "T".into(),
        cover: sonder_library::CoverEdit::Remove,
        ..Default::default()
    };
    assert!(sonder_library::write_file(&path, &edit).is_err());
    assert_eq!(fs::read(&path).unwrap(), before);
}
