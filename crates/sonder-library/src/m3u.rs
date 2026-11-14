//! M3U / M3U8 playlists: read entries into paths, write a playlist out.
//!
//! Reading accepts what other players write: UTF-8 with or without a BOM, or Latin-1 (old
//! `.m3u` files); absolute, relative (to the playlist's folder) and `file://` entries, with
//! either slash; `#EXTM3U`/`#EXTINF` and other `#` lines are ignored. Web URLs cannot be
//! library entries and are reported as skipped. Paths are normalised (`.`/`..` resolved,
//! native separators) so they compare equal to the paths the library stores.
//!
//! Writing produces extended M3U8: UTF-8, `#EXTM3U`, then `#EXTINF:<seconds>,<artist> -
//! <title>` before each absolute path when the track is in the library.

use std::path::{Component, Path, PathBuf};

use crate::PlaylistEntry;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct M3uImport {
    /// Entries in playlist order.
    pub paths: Vec<PathBuf>,
    /// Entries that are not local files (web streams), as written.
    pub skipped: Vec<String>,
}

/// Parses an M3U file's bytes; relative entries are taken from `base` (the file's folder).
pub fn parse_m3u(bytes: &[u8], base: &Path) -> M3uImport {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let text = match std::str::from_utf8(bytes) {
        Ok(text) => text.to_owned(),
        // Not UTF-8: old `.m3u` files are Latin-1 (every byte is one code point).
        Err(_) => bytes.iter().map(|&b| char::from(b)).collect(),
    };
    let mut out = M3uImport::default();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        match entry_path(line, base) {
            Some(path) => out.paths.push(path),
            None => out.skipped.push(line.to_owned()),
        }
    }
    out
}

/// The local path an entry names, or `None` for a URL of another scheme.
fn entry_path(entry: &str, base: &Path) -> Option<PathBuf> {
    let raw = if let Some(rest) = strip_prefix_ignore_case(entry, "file://") {
        // file:///C:/x on Windows, file:///home/x elsewhere; a host part is not supported.
        let decoded = percent_decode(rest.strip_prefix("localhost").unwrap_or(rest));
        let windows_drive = decoded.len() > 3
            && decoded.starts_with('/')
            && decoded.as_bytes()[2] == b':'
            && decoded.as_bytes()[1].is_ascii_alphabetic();
        if windows_drive {
            decoded[1..].to_owned()
        } else {
            decoded
        }
    } else if has_scheme(entry) {
        return None;
    } else {
        entry.to_owned()
    };
    let native = if cfg!(windows) {
        raw.replace('/', "\\")
    } else {
        raw.replace('\\', "/")
    };
    let path = PathBuf::from(native);
    let joined = if path.has_root() || path.is_absolute() {
        path
    } else {
        base.join(path)
    };
    Some(normalize(&joined))
}

/// `http://…`, `https://…` and other `scheme://` URLs (a Windows drive `C:\` is not one).
fn has_scheme(entry: &str) -> bool {
    entry.split_once("://").is_some_and(|(scheme, _)| {
        scheme.len() > 1
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    })
}

fn strip_prefix_ignore_case<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    let head = s.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &s[prefix.len()..])
}

/// Decodes `%XX` escapes; invalid escapes stay as written.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(hex) = s.get(i + 1..i + 3)
            && let Ok(byte) = u8::from_str_radix(hex, 16)
        {
            out.push(byte);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Resolves `.` and `..` without touching the file system.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Writes `entries` as extended M3U8 text.
pub fn format_m3u(entries: &[PlaylistEntry]) -> String {
    let mut out = String::from("#EXTM3U\n");
    for entry in entries {
        if let Some(track) = &entry.track {
            let seconds = (track.audio.duration_ms + 500) / 1000;
            let title = clean(&track.title);
            match track.artist.as_deref().or(track.album_artist.as_deref()) {
                Some(artist) => {
                    out.push_str(&format!("#EXTINF:{seconds},{} - {title}\n", clean(artist)));
                }
                None => out.push_str(&format!("#EXTINF:{seconds},{title}\n")),
            }
        }
        out.push_str(&entry.path.to_string_lossy());
        out.push('\n');
    }
    out
}

/// A tag value on one line.
fn clean(s: &str) -> String {
    s.replace(['\r', '\n'], " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> PathBuf {
        if cfg!(windows) {
            PathBuf::from(r"D:\Music\Lists")
        } else {
            PathBuf::from("/music/lists")
        }
    }

    #[test]
    fn relative_entries_resolve_against_the_playlist_folder() {
        let m = parse_m3u(
            b"#EXTM3U\n#EXTINF:200,A - B\n../Album/01 Song.flac\n\nsub/02.flac\n",
            &base(),
        );
        let expected: Vec<PathBuf> = if cfg!(windows) {
            vec![
                r"D:\Music\Album\01 Song.flac".into(),
                r"D:\Music\Lists\sub\02.flac".into(),
            ]
        } else {
            vec![
                "/music/Album/01 Song.flac".into(),
                "/music/lists/sub/02.flac".into(),
            ]
        };
        assert_eq!(m.paths, expected);
        assert!(m.skipped.is_empty());
    }

    #[test]
    fn either_slash_and_absolute_paths_are_kept() {
        let text: &[u8] = if cfg!(windows) {
            b"C:/Music/a.flac\r\nC:\\Music\\b.flac\r\n\\\\nas\\share\\c.flac\r\n"
        } else {
            b"/home/u/a.flac\n/home/u/./x/../b.flac\nMusic\\c.flac\n"
        };
        let m = parse_m3u(text, &base());
        let expected: Vec<PathBuf> = if cfg!(windows) {
            vec![
                r"C:\Music\a.flac".into(),
                r"C:\Music\b.flac".into(),
                r"\\nas\share\c.flac".into(),
            ]
        } else {
            vec![
                "/home/u/a.flac".into(),
                "/home/u/b.flac".into(),
                "/music/lists/Music/c.flac".into(),
            ]
        };
        assert_eq!(m.paths, expected);
    }

    #[test]
    fn file_urls_are_decoded_and_web_urls_skipped() {
        let text: &[u8] = if cfg!(windows) {
            b"file:///C:/My%20Music/a%C3%A9.flac\nhttps://radio.example/stream\nFILE:///C:/b.flac\n"
        } else {
            b"file:///home/u/My%20Music/a%C3%A9.flac\nhttps://radio.example/stream\nFILE:///home/b.flac\n"
        };
        let m = parse_m3u(text, &base());
        let expected: Vec<PathBuf> = if cfg!(windows) {
            vec![r"C:\My Music\aé.flac".into(), r"C:\b.flac".into()]
        } else {
            vec!["/home/u/My Music/aé.flac".into(), "/home/b.flac".into()]
        };
        assert_eq!(m.paths, expected);
        assert_eq!(m.skipped, ["https://radio.example/stream"]);
    }

    #[test]
    fn bom_and_latin1_files_read() {
        let utf8 = parse_m3u(b"\xEF\xBB\xBFsong.flac\n", &base());
        assert_eq!(utf8.paths, [base().join("song.flac")]);
        // "Café.flac" in Latin-1: 0xE9 is é and not valid UTF-8 on its own.
        let latin1 = parse_m3u(b"Caf\xE9.flac\n", &base());
        assert_eq!(latin1.paths, [base().join("Café.flac")]);
    }

    #[test]
    fn written_entries_carry_extinf_only_for_library_tracks() {
        let entries = vec![PlaylistEntry {
            path: base().join("missing.flac"),
            track: None,
        }];
        let text = format_m3u(&entries);
        assert_eq!(
            text,
            format!("#EXTM3U\n{}\n", base().join("missing.flac").display())
        );
        // Reading it back gives the same paths.
        assert_eq!(
            parse_m3u(text.as_bytes(), Path::new("/elsewhere")).paths,
            [base().join("missing.flac")]
        );
    }
}
