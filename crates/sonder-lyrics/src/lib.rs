//! Lyrics for a track.
//!
//! Sources, best first: lyrics embedded in the file's tags, a `.lrc` file beside it with the
//! same name, then (only when the caller allows it) online lyrics services. Synced lyrics
//! from any source beat plain text from any other. Online results, including "none found",
//! are cached on disk so a track is looked up once.

mod cache;
pub mod lrc;
mod providers;
pub mod ttml;

use std::path::Path;

use serde::{Deserialize, Serialize};

pub use cache::Cache;
pub use providers::Online;

/// One line of lyrics. `start_ms` is 0 for every line of plain (unsynced) lyrics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Line {
    pub start_ms: u32,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lyrics {
    /// Where they came from: "Embedded", "LRC file" or the service's name.
    pub source: String,
    /// Lines carry start times.
    pub synced: bool,
    /// The service says the track has no vocals; `lines` is then empty.
    #[serde(default)]
    pub instrumental: bool,
    pub lines: Vec<Line>,
}

impl Lyrics {
    pub fn synced(source: &str, lines: Vec<Line>) -> Self {
        Lyrics {
            source: source.to_string(),
            synced: true,
            instrumental: false,
            lines,
        }
    }

    /// Plain text split into lines, outer blank lines dropped. `None` when it is all blank.
    pub fn plain(source: &str, text: &str) -> Option<Self> {
        let lines: Vec<Line> = text
            .trim_matches(|c: char| c.is_whitespace())
            .lines()
            .map(|l| Line {
                start_ms: 0,
                text: l.trim().to_string(),
            })
            .collect();
        (!lines.iter().all(|l| l.text.is_empty())).then(|| Lyrics {
            source: source.to_string(),
            synced: false,
            instrumental: false,
            lines,
        })
    }

    pub fn instrumental(source: &str) -> Self {
        Lyrics {
            source: source.to_string(),
            synced: false,
            instrumental: true,
            lines: Vec::new(),
        }
    }
}

/// What online services are asked with. This is everything that leaves the machine.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Query {
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration_ms: u64,
}

/// Lyrics stored with the file: tags first, then a same-named `.lrc` beside it. Synced
/// beats plain between the two.
pub fn local(audio: &Path, embedded: Option<&str>) -> Option<Lyrics> {
    let tag = embedded.and_then(|t| lrc::parse(t, "Embedded"));
    if tag.as_ref().is_some_and(|l| l.synced) {
        return tag;
    }
    let sidecar = read_text(&audio.with_extension("lrc")).and_then(|t| lrc::parse(&t, "LRC file"));
    if sidecar.as_ref().is_some_and(|l| l.synced) {
        return sidecar;
    }
    tag.or(sidecar)
}

/// A text file as UTF-8, or Latin-1 when it is not (older .lrc files).
fn read_text(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    Some(match String::from_utf8(bytes) {
        Ok(s) => s,
        Err(e) => e.into_bytes().iter().map(|&b| char::from(b)).collect(),
    })
}

/// Picks the lyrics to show: local synced, else online synced, else any local, else online.
pub fn choose(local: Option<Lyrics>, online: impl FnOnce() -> Option<Lyrics>) -> Option<Lyrics> {
    if local.as_ref().is_some_and(|l| l.synced) {
        return local;
    }
    let online = online();
    if online.as_ref().is_some_and(|l| l.synced) {
        return online;
    }
    local.or(online)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sidecar_lrc_beats_plain_embedded_text() {
        let dir = tempfile::tempdir().unwrap();
        let audio = dir.path().join("01 Song.flac");
        std::fs::write(dir.path().join("01 Song.lrc"), "[00:01.00]Hi\n").unwrap();
        let l = local(&audio, Some("Hi")).unwrap();
        assert_eq!(l.source, "LRC file");
        assert!(l.synced);
    }

    #[test]
    fn synced_embedded_lyrics_win_and_latin1_sidecars_read() {
        let dir = tempfile::tempdir().unwrap();
        let audio = dir.path().join("a.flac");
        assert_eq!(
            local(&audio, Some("[00:02.00]Tag")).unwrap().source,
            "Embedded"
        );
        std::fs::write(dir.path().join("a.lrc"), b"[00:01.00]Caf\xe9\n").unwrap();
        assert_eq!(local(&audio, None).unwrap().lines[0].text, "Café");
        assert!(local(&dir.path().join("none.flac"), None).is_none());
    }

    #[test]
    fn choose_prefers_synced_and_skips_online_when_local_is_synced() {
        let synced = Lyrics::synced(
            "a",
            vec![Line {
                start_ms: 0,
                text: "x".into(),
            }],
        );
        let plain = Lyrics::plain("b", "x").unwrap();
        let mut asked = false;
        let got = choose(Some(synced.clone()), || {
            asked = true;
            None
        });
        assert_eq!(got, Some(synced.clone()));
        assert!(!asked);
        assert_eq!(
            choose(Some(plain.clone()), || Some(synced.clone())),
            Some(synced)
        );
        assert_eq!(choose(Some(plain.clone()), || None), Some(plain.clone()));
        assert_eq!(choose(None, || Some(plain.clone())), Some(plain));
    }
}
