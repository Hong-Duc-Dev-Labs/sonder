//! Album identity: which tracks belong to the same album.
//!
//! All grouping heuristics live here so they can be tuned against the table-driven tests below.
//!
//! 1. A MusicBrainz release id wins outright.
//! 2. Otherwise album artist + album title (disc suffixes such as "(Disc 2)" removed).
//! 3. Without an album artist, the album title within its folder, so two unrelated
//!    "Greatest Hits" never merge while a compilation with many track artists stays whole.
//! 4. Without an album title, the folder itself is the album.
//!
//! Disc folders (`CD1`, `Disc 2`, ...) count as their parent folder.

use std::path::Path;

/// Separator inside group keys; it cannot appear in tags after normalisation.
const SEP: char = '\u{1f}';

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AlbumIdentity {
    /// Stable key stored in `albums.group_key`.
    pub key: String,
    /// Display title for a newly created album.
    pub title: String,
}

/// Computes the album a track at `path` belongs to.
pub(crate) fn album_identity(
    album: Option<&str>,
    album_artist: Option<&str>,
    mb_album_id: Option<&str>,
    path: &Path,
) -> AlbumIdentity {
    let dir = album_dir(path);
    let album = album.map(str::trim).filter(|s| !s.is_empty());
    let title = match album {
        Some(t) => strip_disc_suffix(t).to_string(),
        None => dir
            .and_then(|d| d.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Unknown Album".to_string()),
    };

    if let Some(mbid) = mb_album_id.map(str::trim).filter(|s| !s.is_empty()) {
        return AlbumIdentity {
            key: format!("mb{SEP}{}", mbid.to_lowercase()),
            title,
        };
    }
    let album_artist = album_artist.map(str::trim).filter(|s| !s.is_empty());
    let key = match (album, album_artist) {
        (Some(_), Some(artist)) => {
            format!("aa{SEP}{}{SEP}{}", normalize(artist), normalize(&title))
        }
        _ => {
            let dir = dir
                .map(|d| d.to_string_lossy().into_owned())
                .unwrap_or_default();
            format!("dir{SEP}{}{SEP}{dir}", normalize(&title))
        }
    };
    AlbumIdentity { key, title }
}

/// The folder that represents a track's album: its parent, or the grandparent when the
/// parent is a disc folder.
pub(crate) fn album_dir(path: &Path) -> Option<&Path> {
    let parent = path.parent()?;
    match parent.file_name() {
        Some(name) if is_disc_folder(&name.to_string_lossy()) => parent.parent().or(Some(parent)),
        _ => Some(parent),
    }
}

/// `CD1`, `cd 2`, `Disc 01`, `Disk-3`, `CD2 - Bonus`.
pub(crate) fn is_disc_folder(name: &str) -> bool {
    let lower = name.trim().to_lowercase();
    match disc_token_len(&lower) {
        Some(len) => lower[len..]
            .chars()
            .next()
            .is_none_or(|c| !c.is_alphanumeric()),
        None => false,
    }
}

/// Removes a trailing disc marker: "Album (Disc 2)", "Album [CD1]", "Album - Disc 1 of 2",
/// "Album CD2".
pub(crate) fn strip_disc_suffix(title: &str) -> &str {
    let trimmed = title.trim_end();
    let lower = trimmed.to_lowercase();
    // Lowercasing can change byte lengths for some scripts; only strip when it does not.
    if lower.len() != trimmed.len() {
        return trimmed;
    }

    // Bracketed: "(...)" or "[...]" whose content is a disc marker.
    if let Some(close) = lower.chars().last().filter(|c| *c == ')' || *c == ']') {
        let open = if close == ')' { '(' } else { '[' };
        if let Some(start) = lower.rfind(open) {
            let inner = lower[start + 1..lower.len() - 1].trim();
            if disc_token_len(inner) == Some(inner.len()) {
                let rest = trimmed[..start].trim_end();
                return rest.trim_end_matches(['-', ':', ',']).trim_end();
            }
        }
        return trimmed;
    }

    // Bare: the marker starts after a space and runs to the end.
    for (pos, _) in lower.match_indices(' ') {
        let tail = &lower[pos + 1..];
        if disc_token_len(tail) == Some(tail.len()) {
            let rest = trimmed[..pos]
                .trim_end()
                .trim_end_matches(['-', ':', ','])
                .trim_end();
            if !rest.is_empty() {
                return rest;
            }
        }
    }
    trimmed
}

/// Length of a disc marker at the start of `s` (already lowercase):
/// `(cd|disc|disk) [-_. ]* digits ( (of|/) digits )?`.
fn disc_token_len(s: &str) -> Option<usize> {
    let prefix = ["disc", "disk", "cd"]
        .into_iter()
        .find(|p| s.starts_with(p))?;
    let mut i = prefix.len();
    let bytes = s.as_bytes();
    while i < bytes.len() && matches!(bytes[i], b' ' | b'-' | b'_' | b'.') {
        i += 1;
    }
    let digits = count_digits(&bytes[i..]);
    if digits == 0 {
        return None;
    }
    i += digits;
    // Optional "of N" / "/N".
    let mut j = i;
    while j < bytes.len() && bytes[j] == b' ' {
        j += 1;
    }
    let sep = if bytes[j..].starts_with(b"of") {
        2
    } else if bytes[j..].starts_with(b"/") {
        1
    } else {
        0
    };
    if sep > 0 {
        let mut k = j + sep;
        while k < bytes.len() && bytes[k] == b' ' {
            k += 1;
        }
        let total = count_digits(&bytes[k..]);
        if total > 0 {
            i = k + total;
        }
    }
    Some(i)
}

fn count_digits(bytes: &[u8]) -> usize {
    bytes.iter().take_while(|b| b.is_ascii_digit()).count()
}

/// Case- and whitespace-insensitive form used inside keys.
fn normalize(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
        .replace(SEP, " ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s.replace('/', std::path::MAIN_SEPARATOR_STR))
    }

    #[test]
    fn disc_folders() {
        let cases = [
            ("CD1", true),
            ("cd 2", true),
            ("Disc 01", true),
            ("Disk-3", true),
            ("CD2 - Bonus", true),
            ("disc_4", true),
            ("CDs", false),
            ("Discography", false),
            ("Disco", false),
            ("CD", false),
            ("Abbey Road", false),
        ];
        for (name, expected) in cases {
            assert_eq!(is_disc_folder(name), expected, "{name}");
        }
    }

    #[test]
    fn disc_suffixes() {
        let cases = [
            ("Mellon Collie (Disc 1)", "Mellon Collie"),
            ("Mellon Collie [CD2]", "Mellon Collie"),
            ("Mellon Collie - Disc 1 of 2", "Mellon Collie"),
            ("Mellon Collie CD2", "Mellon Collie"),
            ("Mellon Collie (disc 2/2)", "Mellon Collie"),
            ("Disco Inferno", "Disco Inferno"),
            ("Discovery", "Discovery"),
            ("CD1", "CD1"),
            ("Live (Remastered)", "Live (Remastered)"),
            ("Album (Disc Two)", "Album (Disc Two)"),
            ("Mùa Thu (CD 1)", "Mùa Thu"),
        ];
        for (title, expected) in cases {
            assert_eq!(strip_disc_suffix(title), expected, "{title}");
        }
    }

    /// Each case: (album, album artist, mbid, path) for two tracks and whether they group.
    #[test]
    fn grouping_table() {
        type Track<'a> = (Option<&'a str>, Option<&'a str>, Option<&'a str>, &'a str);
        let cases: &[(&str, Track, Track, bool)] = &[
            (
                "same album artist and title in different folders",
                (
                    Some("Kind of Blue"),
                    Some("Miles Davis"),
                    None,
                    "/m/a/01.flac",
                ),
                (
                    Some("Kind of Blue"),
                    Some("Miles Davis"),
                    None,
                    "/m/b/02.flac",
                ),
                true,
            ),
            (
                "album artist case and spacing differ",
                (
                    Some("Kind of Blue"),
                    Some("Miles  Davis"),
                    None,
                    "/m/a/01.flac",
                ),
                (
                    Some("kind of blue"),
                    Some("miles davis"),
                    None,
                    "/m/a/02.flac",
                ),
                true,
            ),
            (
                "disc folders of one album",
                (
                    Some("Mellon Collie"),
                    None,
                    None,
                    "/m/SP/Mellon/CD1/01.flac",
                ),
                (
                    Some("Mellon Collie"),
                    None,
                    None,
                    "/m/SP/Mellon/CD2/01.flac",
                ),
                true,
            ),
            (
                "disc suffix in the album tag",
                (
                    Some("Mellon Collie (Disc 1)"),
                    Some("SP"),
                    None,
                    "/m/x/01.flac",
                ),
                (
                    Some("Mellon Collie (Disc 2)"),
                    Some("SP"),
                    None,
                    "/m/x/02.flac",
                ),
                true,
            ),
            (
                "same title without album artist in different folders",
                (Some("Greatest Hits"), None, None, "/m/queen/01.flac"),
                (Some("Greatest Hits"), None, None, "/m/abba/01.flac"),
                false,
            ),
            (
                "compilation without album artist in one folder",
                (Some("Now 50"), None, None, "/m/now50/01.flac"),
                (Some("Now 50"), None, None, "/m/now50/02.flac"),
                true,
            ),
            (
                "different album artists, same title",
                (Some("Greatest Hits"), Some("Queen"), None, "/m/x/01.flac"),
                (Some("Greatest Hits"), Some("ABBA"), None, "/m/x/02.flac"),
                false,
            ),
            (
                "mbid overrides differing titles",
                (Some("Album"), Some("A"), Some("1234-ABCD"), "/m/x/01.flac"),
                (
                    Some("Album (Deluxe)"),
                    Some("A"),
                    Some("1234-abcd"),
                    "/m/y/01.flac",
                ),
                true,
            ),
            (
                "different mbids, same title",
                (Some("Album"), Some("A"), Some("1111"), "/m/x/01.flac"),
                (Some("Album"), Some("A"), Some("2222"), "/m/x/02.flac"),
                false,
            ),
            (
                "untagged files in one folder",
                (None, None, None, "/m/Bootleg/01.flac"),
                (None, None, None, "/m/Bootleg/02.flac"),
                true,
            ),
            (
                "untagged files in disc folders",
                (None, None, None, "/m/Bootleg/CD1/01.flac"),
                (None, None, None, "/m/Bootleg/CD2/01.flac"),
                true,
            ),
            (
                "untagged files in sibling folders",
                (None, None, None, "/m/One/01.flac"),
                (None, None, None, "/m/Two/01.flac"),
                false,
            ),
        ];
        for (name, a, b, expected) in cases {
            let ka = album_identity(a.0, a.1, a.2, &p(a.3)).key;
            let kb = album_identity(b.0, b.1, b.2, &p(b.3)).key;
            assert_eq!(ka == kb, *expected, "{name}: {ka:?} vs {kb:?}");
        }
    }

    #[test]
    fn titles() {
        let id = album_identity(Some("X (Disc 1)"), None, None, &p("/m/x/01.flac"));
        assert_eq!(id.title, "X");
        let id = album_identity(None, None, None, &p("/m/Bootleg/CD1/01.flac"));
        assert_eq!(id.title, "Bootleg");
    }
}
