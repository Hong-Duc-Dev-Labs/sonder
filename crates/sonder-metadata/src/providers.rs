//! The catalogues. Requests live on [`Online`]; reading each reply is in [`reply`], apart from
//! the requests, so it can be tested offline.

use std::error::Error;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::{Candidate, Query, is_cover_url};

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

const ITUNES: &str = "https://itunes.apple.com/search";
const DEEZER: &str = "https://api.deezer.com";
const MUSICBRAINZ: &str = "https://musicbrainz.org/ws/2/recording";
const COVER_ART_ARCHIVE: &str = "https://coverartarchive.org/release";

/// Candidates read from each catalogue's reply.
const PER_SOURCE: usize = 5;
/// Deezer's search leaves out track and disc numbers, year and genre; each of its first
/// candidates costs two more requests to fill them in.
const DEEZER_DETAILED: usize = 3;
/// MusicBrainz serves one request a second per client and answers 503 to more.
const MUSICBRAINZ_INTERVAL: Duration = Duration::from_millis(1100);
/// Largest cover image accepted.
const MAX_COVER_BYTES: u64 = 20 * 1024 * 1024;

pub struct Online {
    agent: ureq::Agent,
    /// When MusicBrainz was last asked.
    musicbrainz_last: Mutex<Option<Instant>>,
}

impl Online {
    /// `user_agent` names the app and how to reach its authors: MusicBrainz asks for that.
    pub fn new(user_agent: &str) -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(10)))
            .user_agent(user_agent)
            .http_status_as_error(false)
            .build()
            .into();
        Online {
            agent,
            musicbrainz_last: Mutex::new(None),
        }
    }

    /// Asks every catalogue at once. Candidates whose length matches the track come first,
    /// then the catalogues in the order iTunes, Deezer, MusicBrainz. An error only when
    /// every catalogue failed, so "nothing found" and "could not ask" stay apart.
    pub fn search(&self, q: &Query) -> std::result::Result<Vec<Candidate>, String> {
        let results: Vec<(&str, Result<Vec<Candidate>>)> = std::thread::scope(|s| {
            let jobs = [
                ("iTunes", s.spawn(|| self.itunes(q))),
                ("Deezer", s.spawn(|| self.deezer(q))),
                ("MusicBrainz", s.spawn(|| self.musicbrainz(q))),
            ];
            jobs.into_iter()
                .map(|(name, job)| {
                    let got = job
                        .join()
                        .unwrap_or_else(|_| Err("the request panicked".into()));
                    (name, got)
                })
                .collect()
        });
        let mut errors = Vec::new();
        let mut all = Vec::new();
        for (name, got) in results {
            match got {
                Ok(found) => all.extend(found),
                Err(err) => {
                    eprintln!("sonder: metadata from {name}: {err}");
                    errors.push(format!("{name}: {err}"));
                }
            }
        }
        if all.is_empty() && errors.len() == 3 {
            return Err(errors.join("; "));
        }
        // Stable: within each group the catalogue order stays.
        all.sort_by_key(|c| !c.matches_length(q.duration_ms));
        Ok(all)
    }

    /// The image at `url`, which must be on a catalogue's image server. `None` when the
    /// server has none there (Cover Art Archive answers 404 for a release without art).
    pub fn cover(&self, url: &str) -> Result<Option<Vec<u8>>> {
        if !is_cover_url(url) {
            return Err(format!("not a catalogue image: {url}").into());
        }
        let mut resp = self.agent.get(url).call()?;
        match resp.status().as_u16() {
            200 => Ok(Some(
                resp.body_mut()
                    .with_config()
                    .limit(MAX_COVER_BYTES)
                    .read_to_vec()?,
            )),
            404 => Ok(None),
            code => Err(format!("HTTP {code}").into()),
        }
    }

    fn get(&self, url: &str, params: &[(&str, String)]) -> Result<Option<String>> {
        let mut req = self.agent.get(url);
        for (k, v) in params {
            req = req.query(*k, v);
        }
        let mut resp = req.call()?;
        match resp.status().as_u16() {
            200 => Ok(Some(resp.body_mut().read_to_string()?)),
            404 => Ok(None),
            code => Err(format!("HTTP {code}").into()),
        }
    }

    fn itunes(&self, q: &Query) -> Result<Vec<Candidate>> {
        let params = [
            ("term", words(q)),
            ("media", "music".to_string()),
            ("entity", "song".to_string()),
            ("limit", PER_SOURCE.to_string()),
        ];
        match self.get(ITUNES, &params)? {
            Some(body) => reply::itunes(&body),
            None => Ok(Vec::new()),
        }
    }

    fn deezer(&self, q: &Query) -> Result<Vec<Candidate>> {
        let params = [("q", words(q)), ("limit", PER_SOURCE.to_string())];
        let Some(body) = self.get(&format!("{DEEZER}/search"), &params)? else {
            return Ok(Vec::new());
        };
        let mut found = reply::deezer_search(&body)?;
        // The ones most likely to be picked get the details: matching length first.
        found.sort_by_key(|(c, _, _)| !c.matches_length(q.duration_ms));
        std::thread::scope(|s| {
            for (candidate, track_id, album_id) in found.iter_mut().take(DEEZER_DETAILED) {
                s.spawn(move || {
                    if let Ok(Some(body)) = self.get(&format!("{DEEZER}/track/{track_id}"), &[]) {
                        let _ = reply::deezer_track(&body, candidate);
                    }
                    if let Ok(Some(body)) = self.get(&format!("{DEEZER}/album/{album_id}"), &[]) {
                        let _ = reply::deezer_album(&body, candidate);
                    }
                });
            }
        });
        Ok(found.into_iter().map(|(c, _, _)| c).collect())
    }

    fn musicbrainz(&self, q: &Query) -> Result<Vec<Candidate>> {
        let mut query = format!("recording:{}", lucene_phrase(&q.title));
        if let Some(artist) = &q.artist {
            query.push_str(&format!(" AND artist:{}", lucene_phrase(artist)));
        }
        let params = [
            ("query", query),
            ("fmt", "json".to_string()),
            ("limit", PER_SOURCE.to_string()),
        ];
        {
            // Held while waiting, so two searches at once queue up instead of both sleeping
            // the same amount and asking together.
            let mut last = self
                .musicbrainz_last
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(wait) = last.and_then(|t| MUSICBRAINZ_INTERVAL.checked_sub(t.elapsed())) {
                std::thread::sleep(wait);
            }
            *last = Some(Instant::now());
        }
        match self.get(MUSICBRAINZ, &params)? {
            Some(body) => reply::musicbrainz(&body, q.album.as_deref()),
            None => Ok(Vec::new()),
        }
    }
}

/// Artist and title as free search words.
fn words(q: &Query) -> String {
    match &q.artist {
        Some(artist) => format!("{artist} {}", q.title),
        None => q.title.clone(),
    }
}

/// `text` as a quoted Lucene phrase for MusicBrainz's search.
fn lucene_phrase(text: &str) -> String {
    let escaped = text.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

/// The year at the start of a date like "1959-08-17T07:00:00Z" or "1959".
fn year_of(date: &str) -> Option<i32> {
    date.get(..4)
        .filter(|y| y.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|y| y.parse().ok())
        .filter(|y| *y > 0)
}

/// `Some` when the text is not blank.
fn text(value: Option<String>) -> Option<String> {
    value
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

pub(crate) mod reply {
    use super::*;

    pub fn itunes(body: &str) -> Result<Vec<Candidate>> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Reply {
            #[serde(default)]
            results: Vec<Song>,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Song {
            track_name: Option<String>,
            artist_name: Option<String>,
            collection_name: Option<String>,
            collection_artist_name: Option<String>,
            track_number: Option<u32>,
            disc_number: Option<u32>,
            release_date: Option<String>,
            primary_genre_name: Option<String>,
            track_time_millis: Option<u64>,
            artwork_url100: Option<String>,
        }
        let reply: Reply = serde_json::from_str(body)?;
        Ok(reply
            .results
            .into_iter()
            .filter_map(|s| {
                let title = text(s.track_name)?;
                let artist = text(s.artist_name);
                let album_artist = text(s.collection_artist_name).or_else(|| artist.clone());
                // The artwork URL names its size; the same image comes in any size up to the
                // original, so the large one is asked for by name.
                let thumb = text(s.artwork_url100);
                let cover = thumb
                    .as_deref()
                    .filter(|u| u.contains("/100x100"))
                    .map(|u| u.replace("/100x100bb.", "/1200x1200bb."));
                Some(Candidate {
                    source: "iTunes".into(),
                    title,
                    artists: artist.into_iter().collect(),
                    album: text(s.collection_name),
                    album_artists: album_artist.into_iter().collect(),
                    track_no: s.track_number.filter(|n| *n > 0),
                    disc_no: s.disc_number.filter(|n| *n > 0),
                    year: s.release_date.as_deref().and_then(year_of),
                    genre: text(s.primary_genre_name),
                    duration_ms: s.track_time_millis.filter(|d| *d > 0),
                    cover_url: cover.or_else(|| thumb.clone()),
                    thumb_url: thumb,
                })
            })
            .collect())
    }

    #[derive(Deserialize)]
    struct Named {
        name: Option<String>,
    }

    /// Search results, each with its Deezer track and album ids for the detail requests.
    pub fn deezer_search(body: &str) -> Result<Vec<(Candidate, u64, u64)>> {
        #[derive(Deserialize)]
        struct Reply {
            #[serde(default)]
            data: Vec<Track>,
            error: Option<serde_json::Value>,
        }
        #[derive(Deserialize)]
        struct Track {
            id: u64,
            title: Option<String>,
            duration: Option<u64>,
            artist: Option<Named>,
            album: Option<Album>,
        }
        #[derive(Deserialize)]
        struct Album {
            id: u64,
            title: Option<String>,
            cover_medium: Option<String>,
            cover_xl: Option<String>,
        }
        let reply: Reply = serde_json::from_str(body)?;
        if let Some(error) = reply.error {
            return Err(format!("Deezer error: {error}").into());
        }
        Ok(reply
            .data
            .into_iter()
            .filter_map(|t| {
                let album = t.album?;
                let candidate = Candidate {
                    source: "Deezer".into(),
                    title: text(t.title)?,
                    artists: text(t.artist.and_then(|a| a.name)).into_iter().collect(),
                    album: text(album.title),
                    duration_ms: t.duration.filter(|d| *d > 0).map(|d| d * 1000),
                    cover_url: text(album.cover_xl),
                    thumb_url: text(album.cover_medium),
                    ..Default::default()
                };
                Some((candidate, t.id, album.id))
            })
            .collect())
    }

    /// `/track/{id}`: every credited artist, track and disc number.
    pub fn deezer_track(body: &str, c: &mut Candidate) -> Result<()> {
        #[derive(Deserialize)]
        struct Track {
            #[serde(default)]
            contributors: Vec<Named>,
            track_position: Option<u32>,
            disk_number: Option<u32>,
            release_date: Option<String>,
        }
        let t: Track = serde_json::from_str(body)?;
        let names: Vec<String> = t
            .contributors
            .into_iter()
            .filter_map(|n| text(n.name))
            .collect();
        if !names.is_empty() {
            c.artists = names;
        }
        c.track_no = t.track_position.filter(|n| *n > 0);
        c.disc_no = t.disk_number.filter(|n| *n > 0);
        c.year = c.year.or(t.release_date.as_deref().and_then(year_of));
        Ok(())
    }

    /// `/album/{id}`: album artist, release year and genre.
    pub fn deezer_album(body: &str, c: &mut Candidate) -> Result<()> {
        #[derive(Deserialize)]
        struct Album {
            artist: Option<Named>,
            release_date: Option<String>,
            genres: Option<Genres>,
        }
        #[derive(Deserialize)]
        struct Genres {
            #[serde(default)]
            data: Vec<Named>,
        }
        let a: Album = serde_json::from_str(body)?;
        if let Some(name) = text(a.artist.and_then(|n| n.name)) {
            c.album_artists = vec![name];
        }
        if let Some(year) = a.release_date.as_deref().and_then(year_of) {
            c.year = Some(year);
        }
        c.genre = a
            .genres
            .and_then(|g| g.data.into_iter().find_map(|n| text(n.name)));
        Ok(())
    }

    /// Recordings, each with the release that best fits: the one named like `album`, else
    /// an official album that is not a compilation, else the first.
    pub fn musicbrainz(body: &str, album: Option<&str>) -> Result<Vec<Candidate>> {
        #[derive(Deserialize)]
        struct Reply {
            #[serde(default)]
            recordings: Vec<Recording>,
        }
        #[derive(Deserialize)]
        struct Recording {
            title: Option<String>,
            length: Option<u64>,
            #[serde(rename = "artist-credit", default)]
            artist_credit: Vec<Named>,
            #[serde(rename = "first-release-date")]
            first_release_date: Option<String>,
            #[serde(default)]
            releases: Vec<Release>,
        }
        #[derive(Deserialize)]
        struct Release {
            id: String,
            title: Option<String>,
            status: Option<String>,
            date: Option<String>,
            #[serde(rename = "artist-credit", default)]
            artist_credit: Vec<Named>,
            #[serde(rename = "release-group")]
            release_group: Option<Group>,
            #[serde(default)]
            media: Vec<Medium>,
        }
        #[derive(Deserialize)]
        struct Group {
            #[serde(rename = "primary-type")]
            primary_type: Option<String>,
            #[serde(rename = "secondary-types", default)]
            secondary_types: Vec<String>,
        }
        #[derive(Deserialize)]
        struct Medium {
            position: Option<u32>,
            #[serde(default)]
            track: Vec<MediumTrack>,
        }
        #[derive(Deserialize)]
        struct MediumTrack {
            number: Option<String>,
        }

        fn names(credit: Vec<Named>) -> Vec<String> {
            credit.into_iter().filter_map(|n| text(n.name)).collect()
        }

        let reply: Reply = serde_json::from_str(body)?;
        let wanted = album.map(str::to_lowercase);
        Ok(reply
            .recordings
            .into_iter()
            .filter_map(|r| {
                let title = text(r.title)?;
                let mut releases = r.releases;
                let pick = releases
                    .iter()
                    .position(|rel| {
                        wanted.is_some() && rel.title.as_deref().map(str::to_lowercase) == wanted
                    })
                    .or_else(|| {
                        releases.iter().position(|rel| {
                            rel.status.as_deref() == Some("Official")
                                && rel.release_group.as_ref().is_some_and(|g| {
                                    g.primary_type.as_deref() == Some("Album")
                                        && g.secondary_types.is_empty()
                                })
                        })
                    })
                    .unwrap_or(0);
                let release = (pick < releases.len()).then(|| releases.swap_remove(pick));
                let mut c = Candidate {
                    source: "MusicBrainz".into(),
                    title,
                    artists: names(r.artist_credit),
                    duration_ms: r.length.filter(|d| *d > 0),
                    year: r.first_release_date.as_deref().and_then(year_of),
                    ..Default::default()
                };
                if let Some(rel) = release {
                    c.album = text(rel.title);
                    c.album_artists = names(rel.artist_credit);
                    if let Some(year) = rel.date.as_deref().and_then(year_of) {
                        c.year = Some(year);
                    }
                    if let Some(medium) = rel.media.into_iter().next() {
                        c.disc_no = medium.position.filter(|n| *n > 0);
                        c.track_no = medium
                            .track
                            .into_iter()
                            .next()
                            .and_then(|t| t.number)
                            .and_then(|n| n.trim().parse().ok())
                            .filter(|n| *n > 0);
                    }
                    c.cover_url = Some(format!("{COVER_ART_ARCHIVE}/{}/front-500", rel.id));
                    c.thumb_url = Some(format!("{COVER_ART_ARCHIVE}/{}/front-250", rel.id));
                }
                Some(c)
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Replies below are trimmed copies of what each catalogue returned for a real track, so a
    // change in their shape fails here rather than silently finding nothing.

    #[test]
    fn itunes_reply_reads_every_field_and_asks_for_the_large_cover() {
        let body = r#"{"resultCount":1,"results":[{"wrapperType":"track","kind":"song",
            "artistName":"Miles Davis","collectionName":"Kind of Blue","trackName":"So What",
            "artworkUrl100":"https://is1-ssl.mzstatic.com/image/thumb/Music/7f/mzi.jpg/100x100bb.jpg",
            "releaseDate":"1959-08-17T07:00:00Z","discNumber":1,"trackNumber":1,
            "trackTimeMillis":547983,"primaryGenreName":"Jazz"}]}"#;
        let found = reply::itunes(body).unwrap();
        assert_eq!(
            found,
            vec![Candidate {
                source: "iTunes".into(),
                title: "So What".into(),
                artists: vec!["Miles Davis".into()],
                album: Some("Kind of Blue".into()),
                album_artists: vec!["Miles Davis".into()],
                track_no: Some(1),
                disc_no: Some(1),
                year: Some(1959),
                genre: Some("Jazz".into()),
                duration_ms: Some(547_983),
                cover_url: Some(
                    "https://is1-ssl.mzstatic.com/image/thumb/Music/7f/mzi.jpg/1200x1200bb.jpg"
                        .into()
                ),
                thumb_url: Some(
                    "https://is1-ssl.mzstatic.com/image/thumb/Music/7f/mzi.jpg/100x100bb.jpg"
                        .into()
                ),
            }]
        );
        assert!(
            reply::itunes(r#"{"resultCount":0,"results":[]}"#)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn deezer_replies_fill_in_the_details() {
        let search = r#"{"data":[{"id":2711778,"title":"So What","duration":562,
            "artist":{"id":1910,"name":"Miles Davis"},
            "album":{"id":262078,"title":"Kind Of Blue",
                "cover_medium":"https://cdn-images.dzcdn.net/images/cover/fb/250x250.jpg",
                "cover_xl":"https://cdn-images.dzcdn.net/images/cover/fb/1000x1000.jpg"}}],"total":1}"#;
        let mut found = reply::deezer_search(search).unwrap();
        assert_eq!(found.len(), 1);
        let (c, track_id, album_id) = &mut found[0];
        assert_eq!((*track_id, *album_id), (2_711_778, 262_078));
        assert_eq!(c.duration_ms, Some(562_000));

        let track = r#"{"id":2711778,"track_position":1,"disk_number":1,"release_date":"2009-01-19",
            "contributors":[{"id":1910,"name":"Miles Davis"},{"id":2,"name":"John Coltrane"}]}"#;
        reply::deezer_track(track, c).unwrap();
        let album = r#"{"id":262078,"release_date":"1959-08-17",
            "genres":{"data":[{"id":129,"name":"Jazz"}]},"artist":{"id":1910,"name":"Miles Davis"}}"#;
        reply::deezer_album(album, c).unwrap();
        assert_eq!(c.artists, ["Miles Davis", "John Coltrane"]);
        assert_eq!(c.album_artists, ["Miles Davis"]);
        assert_eq!(
            (c.track_no, c.disc_no, c.year),
            (Some(1), Some(1), Some(1959))
        );
        assert_eq!(c.genre.as_deref(), Some("Jazz"));

        let quota = r#"{"error":{"type":"Exception","message":"Quota limit exceeded","code":4}}"#;
        assert!(reply::deezer_search(quota).is_err());
    }

    #[test]
    fn musicbrainz_reply_picks_the_fitting_release() {
        let body = r#"{"count":2,"recordings":[{"id":"r1","title":"So What","length":547000,
            "artist-credit":[{"name":"Miles Davis","joinphrase":""}],
            "first-release-date":"1959-08-17",
            "releases":[
              {"id":"comp","title":"Smash Hits","status":"Official",
               "artist-credit":[{"name":"Various Artists"}],
               "release-group":{"primary-type":"Album","secondary-types":["Compilation"]},
               "media":[{"position":1,"track":[{"number":"16"}]}]},
              {"id":"kob","title":"Kind of Blue","status":"Official","date":"1997",
               "artist-credit":[{"name":"Miles Davis"}],
               "release-group":{"primary-type":"Album"},
               "media":[{"position":1,"track":[{"number":"1"}]}]}
            ]}]}"#;
        let found = reply::musicbrainz(body, None).unwrap();
        let c = &found[0];
        assert_eq!(c.album.as_deref(), Some("Kind of Blue"));
        assert_eq!(c.album_artists, ["Miles Davis"]);
        assert_eq!(
            (c.track_no, c.disc_no, c.year),
            (Some(1), Some(1), Some(1997))
        );
        assert_eq!(
            c.cover_url.as_deref(),
            Some("https://coverartarchive.org/release/kob/front-500")
        );

        // The album typed in the editor wins over the album heuristic.
        let found = reply::musicbrainz(body, Some("smash hits")).unwrap();
        assert_eq!(found[0].track_no, Some(16));
        assert_eq!(found[0].album_artists, ["Various Artists"]);
    }

    #[test]
    fn malformed_replies_are_errors_not_panics() {
        assert!(reply::itunes("<html>").is_err());
        assert!(reply::deezer_search("error code: 1033").is_err());
        assert!(reply::musicbrainz("{", None).is_err());
    }

    #[test]
    fn helpers() {
        assert_eq!(year_of("1959-08-17T07:00:00Z"), Some(1959));
        assert_eq!(year_of("19"), None);
        assert_eq!(lucene_phrase(r#"Say "Hi" \o/"#), r#""Say \"Hi\" \\o/""#);
    }

    /// Asks the real catalogues; run with `cargo test -p sonder-metadata -- --ignored`.
    #[test]
    #[ignore = "network"]
    fn live_catalogues_answer() {
        let online =
            Online::new("Sonder-tests/0.1 ( https://github.com/Hong-Duc-Dev-Labs/sonder )");
        let q = Query {
            title: "So What".into(),
            artist: Some("Miles Davis".into()),
            album: Some("Kind of Blue".into()),
            duration_ms: 562_000,
        };
        for (name, got) in [
            ("iTunes", online.itunes(&q)),
            ("Deezer", online.deezer(&q)),
            ("MusicBrainz", online.musicbrainz(&q)),
        ] {
            let found = got.unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(!found.is_empty(), "{name}: nothing");
            eprintln!("{name}: {:#?}", found[0]);
        }
        let all = online.search(&q).unwrap();
        let cover = all.iter().find_map(|c| c.cover_url.clone()).unwrap();
        let bytes = online.cover(&cover).unwrap().expect("a cover");
        assert!(bytes.len() > 1000);
    }
}
