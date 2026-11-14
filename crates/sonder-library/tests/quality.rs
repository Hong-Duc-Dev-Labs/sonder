//! Cached quality check results: stored per file, shown on track rows, dropped when the file
//! changes or the analyzer moves on.

mod fixtures;

use fixtures::{Meta, flac};
use sonder_library::{AlbumSort, Library, QualityResult, TrackQuality};

fn result(version: u32, verdict: &str) -> QualityResult {
    QualityResult {
        analyzer_version: version,
        verdict: verdict.into(),
        confidence: "high".into(),
        report_json: format!("{{\"verdict\":\"{verdict}\"}}"),
    }
}

#[test]
fn results_follow_the_file_and_the_analyzer_version() {
    let tmp = tempfile::tempdir().unwrap();
    let music = tmp.path().join("music");
    let file = music.join("Album").join("01.flac");
    let meta = Meta {
        title: Some("A"),
        album: Some("Album"),
        ..Default::default()
    };
    flac(&file, 44_100, 16, 2, 1, &meta);
    let mut lib = Library::open(tmp.path().join("library.db"), tmp.path().join("art")).unwrap();
    lib.add_root(&music, false).unwrap();
    lib.scan().unwrap();
    let track = |lib: &Library| {
        let album = lib.albums(AlbumSort::Title).unwrap()[0].id;
        lib.album(album).unwrap().unwrap().tracks.remove(0)
    };

    let id = track(&lib).id;
    assert_eq!(track(&lib).quality, None);
    assert_eq!(lib.quality(id).unwrap(), None);

    let subject = lib.quality_subject(id).unwrap().unwrap();
    assert_eq!(subject.path, file);
    // A file picked from disk is recognised as the library track stored at that path.
    assert_eq!(lib.track_id_at(&file).unwrap(), Some(id));
    assert_eq!(
        lib.track_id_at(&music.join("elsewhere.flac")).unwrap(),
        None
    );
    lib.save_quality(&subject, &result(1, "transcoded"))
        .unwrap();
    assert_eq!(lib.quality(id).unwrap(), Some(result(1, "transcoded")));
    assert_eq!(
        track(&lib).quality,
        Some(TrackQuality {
            verdict: "transcoded".into(),
            confidence: "high".into()
        })
    );

    // A second check replaces the first.
    lib.save_quality(&subject, &result(1, "genuine")).unwrap();
    assert_eq!(lib.quality(id).unwrap().unwrap().verdict, "genuine");

    // A newer analyzer drops older results.
    assert_eq!(lib.remove_quality_before(1).unwrap(), 0);
    assert_eq!(lib.remove_quality_before(2).unwrap(), 1);
    assert_eq!(lib.quality(id).unwrap(), None);

    // A changed file no longer matches its result.
    lib.save_quality(&subject, &result(2, "genuine")).unwrap();
    flac(&file, 44_100, 16, 2, 2, &meta);
    lib.scan().unwrap();
    let id = track(&lib).id;
    assert_eq!(lib.quality(id).unwrap(), None);
    assert_eq!(track(&lib).quality, None);

    assert_eq!(lib.quality_subject(i64::MAX).unwrap(), None);
}
