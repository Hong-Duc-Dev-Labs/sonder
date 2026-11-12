//! Online lyrics services. Each is asked with the track's title, artist, album and length
//! only, and any failure (offline, timeout, unexpected reply) counts as "not found".
//!
//! LRCLIB goes first: it is open, fast and matches on length. When it has no synced
//! lyrics, the others are asked at once and the first synced answer in this order wins:
//! BetterLyrics, LyricsPlus, Kugou.

use std::error::Error;
use std::time::Duration;

use base64::Engine as _;
use serde::Deserialize;

use crate::{Lyrics, Query, lrc, ttml};

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

/// How far a candidate's length may be from the track's and still be the same recording.
const LENGTH_SLACK_MS: u64 = 3000;

const LRCLIB: &str = "https://lrclib.net/api";
const BETTER_LYRICS: &str = "https://lyrics-api.boidu.dev/getLyrics";
const LYRICS_PLUS: [&str; 2] = [
    "https://lyricsplus.binimum.org/v2/lyrics/get",
    "https://lyricsplus.prjktla.my.id/v2/lyrics/get",
];
const KUGOU_SEARCH: &str = "https://lyrics.kugou.com/search";
const KUGOU_DOWNLOAD: &str = "https://lyrics.kugou.com/download";

pub struct Online {
    agent: ureq::Agent,
}

impl Online {
    pub fn new(user_agent: &str) -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(8)))
            .user_agent(user_agent)
            .http_status_as_error(false)
            .build()
            .into();
        Online { agent }
    }

    /// Asks the services in turn (see the module docs). Blocks for at most a few timeouts.
    pub fn find(&self, q: &Query) -> Option<Lyrics> {
        let first = self.lrclib(q).unwrap_or_else(|e| log("LRCLIB", e));
        if first.as_ref().is_some_and(|l| l.synced || l.instrumental) {
            return first;
        }
        let others: Vec<Option<Lyrics>> = std::thread::scope(|s| {
            let jobs = [
                s.spawn(|| {
                    self.better_lyrics(q)
                        .unwrap_or_else(|e| log("BetterLyrics", e))
                }),
                s.spawn(|| self.lyrics_plus(q).unwrap_or_else(|e| log("LyricsPlus", e))),
                s.spawn(|| self.kugou(q).unwrap_or_else(|e| log("Kugou", e))),
            ];
            jobs.into_iter().map(|j| j.join().ok().flatten()).collect()
        });
        let all: Vec<Lyrics> = std::iter::once(first).chain(others).flatten().collect();
        let pick = all.iter().position(|l| l.synced).unwrap_or(0);
        all.into_iter().nth(pick)
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

    fn lrclib(&self, q: &Query) -> Result<Option<Lyrics>> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Record {
            #[serde(default)]
            instrumental: bool,
            plain_lyrics: Option<String>,
            synced_lyrics: Option<String>,
            duration: Option<f64>,
        }
        fn lyrics(r: &Record) -> Option<Lyrics> {
            if r.instrumental {
                return Some(Lyrics::instrumental("LRCLIB"));
            }
            r.synced_lyrics
                .as_deref()
                .and_then(|t| lrc::parse(t, "LRCLIB"))
                .or_else(|| {
                    r.plain_lyrics
                        .as_deref()
                        .and_then(|t| Lyrics::plain("LRCLIB", t))
                })
        }

        let mut params = vec![("track_name", q.title.clone())];
        if let Some(a) = &q.artist {
            params.push(("artist_name", a.clone()));
        }
        // The exact lookup wants every field; without an artist only search can match.
        if q.artist.is_some() {
            let mut exact = params.clone();
            if let Some(al) = &q.album {
                exact.push(("album_name", al.clone()));
            }
            exact.push(("duration", (q.duration_ms / 1000).to_string()));
            if let Some(body) = self.get(&format!("{LRCLIB}/get"), &exact)? {
                let found = lyrics(&serde_json::from_str(&body)?);
                if found.is_some() {
                    return Ok(found);
                }
            }
        }
        let Some(body) = self.get(&format!("{LRCLIB}/search"), &params)? else {
            return Ok(None);
        };
        let records: Vec<Record> = serde_json::from_str(&body)?;
        let matching: Vec<&Record> = records
            .iter()
            .filter(|r| r.duration.is_some_and(|d| near(q, (d * 1000.0) as u64)))
            .collect();
        Ok(matching
            .iter()
            .find(|r| r.synced_lyrics.is_some())
            .or(matching.first())
            .and_then(|r| lyrics(r)))
    }

    fn better_lyrics(&self, q: &Query) -> Result<Option<Lyrics>> {
        #[derive(Deserialize)]
        struct Reply {
            ttml: Option<String>,
        }
        let Some(artist) = &q.artist else {
            return Ok(None);
        };
        let mut params = vec![
            ("s", q.title.clone()),
            ("a", artist.clone()),
            ("d", (q.duration_ms / 1000).to_string()),
        ];
        if let Some(al) = &q.album {
            params.push(("al", al.clone()));
        }
        let Some(body) = self.get(BETTER_LYRICS, &params)? else {
            return Ok(None);
        };
        let reply: Reply = serde_json::from_str(&body)?;
        Ok(reply
            .ttml
            .as_deref()
            .and_then(|t| ttml::parse(t, "BetterLyrics")))
    }

    fn lyrics_plus(&self, q: &Query) -> Result<Option<Lyrics>> {
        #[derive(Deserialize)]
        struct Reply {
            #[serde(default)]
            lyrics: Vec<Item>,
        }
        #[derive(Deserialize)]
        struct Item {
            #[serde(default)]
            time: u32,
            #[serde(default)]
            text: String,
        }
        let Some(artist) = &q.artist else {
            return Ok(None);
        };
        let mut params = vec![
            ("title", q.title.clone()),
            ("artist", artist.clone()),
            ("duration", (q.duration_ms / 1000).to_string()),
        ];
        if let Some(al) = &q.album {
            params.push(("album", al.clone()));
        }
        // Two mirrors of one service: the second is only asked when the first fails.
        let mut last_err = None;
        for base in LYRICS_PLUS {
            match self.get(base, &params) {
                Ok(None) => return Ok(None),
                Ok(Some(body)) => {
                    let reply: Reply = serde_json::from_str(&body)?;
                    let lines: Vec<crate::Line> = reply
                        .lyrics
                        .into_iter()
                        .map(|i| crate::Line {
                            start_ms: i.time,
                            text: i.text.trim().to_string(),
                        })
                        .collect();
                    if lines.is_empty() {
                        return Ok(None);
                    }
                    if lines.iter().all(|l| l.start_ms == 0) {
                        let text: Vec<&str> = lines.iter().map(|l| l.text.as_str()).collect();
                        return Ok(Lyrics::plain("LyricsPlus", &text.join("\n")));
                    }
                    return Ok(Some(Lyrics::synced("LyricsPlus", lines)));
                }
                Err(e) => last_err = Some(e),
            }
        }
        Err(last_err.unwrap_or_else(|| "no mirror answered".into()))
    }

    fn kugou(&self, q: &Query) -> Result<Option<Lyrics>> {
        #[derive(Deserialize)]
        struct Search {
            #[serde(default)]
            candidates: Vec<Candidate>,
        }
        #[derive(Deserialize)]
        struct Candidate {
            id: String,
            accesskey: String,
            #[serde(default)]
            duration: u64,
        }
        #[derive(Deserialize)]
        struct Download {
            content: Option<String>,
        }

        let keyword = match &q.artist {
            Some(a) => format!("{a} - {}", q.title),
            None => q.title.clone(),
        };
        let params = [
            ("ver", "1".to_string()),
            ("man", "yes".to_string()),
            ("client", "pc".to_string()),
            ("keyword", keyword),
            ("duration", q.duration_ms.to_string()),
        ];
        let Some(body) = self.get(KUGOU_SEARCH, &params)? else {
            return Ok(None);
        };
        let search: Search = serde_json::from_str(&body)?;
        let Some(best) = search
            .candidates
            .iter()
            .filter(|c| near(q, c.duration))
            .min_by_key(|c| c.duration.abs_diff(q.duration_ms))
        else {
            return Ok(None);
        };
        let params = [
            ("fmt", "lrc".to_string()),
            ("charset", "utf8".to_string()),
            ("client", "pc".to_string()),
            ("ver", "1".to_string()),
            ("id", best.id.clone()),
            ("accesskey", best.accesskey.clone()),
        ];
        let Some(body) = self.get(KUGOU_DOWNLOAD, &params)? else {
            return Ok(None);
        };
        let download: Download = serde_json::from_str(&body)?;
        let Some(content) = download.content else {
            return Ok(None);
        };
        let bytes = base64::engine::general_purpose::STANDARD.decode(content.trim())?;
        let text = String::from_utf8_lossy(&bytes);
        Ok(lrc::parse(&text, "Kugou").map(|l| drop_credits(l, q)))
    }
}

fn near(q: &Query, candidate_ms: u64) -> bool {
    q.duration_ms == 0 || candidate_ms.abs_diff(q.duration_ms) <= LENGTH_SLACK_MS
}

/// Kugou opens with a "Title - Artist" line and "Lyrics by: …" style credits; they are not
/// lyrics, so the leading run of them is dropped.
fn drop_credits(mut l: Lyrics, q: &Query) -> Lyrics {
    let title = q.title.to_lowercase();
    let lead = l
        .lines
        .iter()
        .take_while(|line| {
            let t = line.text.to_lowercase();
            t.is_empty()
                || t.contains('：')
                || t.contains(':')
                || t.starts_with(&format!("{title} - "))
        })
        .count();
    if lead < l.lines.len() {
        l.lines.drain(..lead);
    }
    l
}

fn log<T>(service: &str, err: Box<dyn Error + Send + Sync>) -> Option<T> {
    eprintln!("sonder: lyrics from {service}: {err}");
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Line;

    fn query() -> Query {
        Query {
            title: "Song".into(),
            artist: Some("Artist".into()),
            album: None,
            duration_ms: 200_000,
        }
    }

    #[test]
    fn length_match_allows_a_few_seconds() {
        assert!(near(&query(), 202_900));
        assert!(!near(&query(), 204_000));
    }

    #[test]
    fn kugou_credits_are_dropped() {
        let line = |ms, t: &str| Line {
            start_ms: ms,
            text: t.into(),
        };
        let l = Lyrics::synced(
            "Kugou",
            vec![
                line(0, "Song - Artist"),
                line(10, "Lyrics by：Someone"),
                line(20, "Composer: X"),
                line(900, "First real line"),
                line(950, "Time: 3:00"),
            ],
        );
        let texts: Vec<String> = drop_credits(l, &query())
            .lines
            .into_iter()
            .map(|l| l.text)
            .collect();
        assert_eq!(texts, vec!["First real line", "Time: 3:00"]);
    }

    /// Asks the real services; run with `cargo test -p sonder-lyrics -- --ignored`.
    #[test]
    #[ignore = "network"]
    fn live_services_answer() {
        let online = Online::new("Sonder-tests/0.1");
        let q = Query {
            title: "Shape of You".into(),
            artist: Some("Ed Sheeran".into()),
            album: None,
            duration_ms: 233_700,
        };
        for (name, got) in [
            ("LRCLIB", online.lrclib(&q)),
            ("BetterLyrics", online.better_lyrics(&q)),
            ("LyricsPlus", online.lyrics_plus(&q)),
            ("Kugou", online.kugou(&q)),
        ] {
            let l = got
                .unwrap_or_else(|e| panic!("{name}: {e}"))
                .unwrap_or_else(|| panic!("{name}: none"));
            assert!(l.synced, "{name} not synced");
            assert!(l.lines.len() > 20, "{name}: {} lines", l.lines.len());
            eprintln!(
                "{name}: {} lines, first {:?}",
                l.lines.len(),
                l.lines.first()
            );
        }
        assert!(online.find(&q).unwrap().synced);
    }
}
