//! Track metadata from online catalogues, for the tag editor to fill its fields from.
//!
//! The editor asks on the user's request only, with the title, artist and album typed in it
//! and the track's length. iTunes, Deezer and MusicBrainz are asked at once; each answers with
//! a few candidates, and the ones whose length matches the track come first. Any failure
//! (offline, timeout, unexpected reply) of one catalogue only drops its candidates.
//!
//! Cover images are fetched separately, and only from the catalogues' own image hosts.

mod providers;

pub use providers::Online;

/// What the catalogues are asked with. This is everything that leaves the machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    /// The track's length, to rank candidates; 0 when unknown.
    pub duration_ms: u64,
}

/// One catalogue's idea of the track. Fields it does not know are empty.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Candidate {
    /// The catalogue's name: "iTunes", "Deezer" or "MusicBrainz".
    pub source: String,
    pub title: String,
    pub artists: Vec<String>,
    pub album: Option<String>,
    pub album_artists: Vec<String>,
    pub track_no: Option<u32>,
    pub disc_no: Option<u32>,
    pub year: Option<i32>,
    pub genre: Option<String>,
    pub duration_ms: Option<u64>,
    /// A large front cover (500 px or more).
    pub cover_url: Option<String>,
    /// A small front cover for a list of candidates.
    pub thumb_url: Option<String>,
}

/// How far a candidate's length may be from the track's and still be the same recording.
pub const LENGTH_SLACK_MS: u64 = 3000;

impl Candidate {
    /// The candidate's length is the track's, give or take [`LENGTH_SLACK_MS`].
    pub fn matches_length(&self, duration_ms: u64) -> bool {
        duration_ms > 0
            && self
                .duration_ms
                .is_some_and(|d| d.abs_diff(duration_ms) <= LENGTH_SLACK_MS)
    }
}

/// The hosts cover images are fetched from: the catalogues' image servers.
const COVER_HOSTS: &[&str] = &["mzstatic.com", "dzcdn.net", "coverartarchive.org"];

/// Whether `url` points at one of the catalogues' image servers over HTTPS.
pub fn is_cover_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    // A user name or port in the URL is not something the catalogues hand out.
    if host.contains(['@', ':']) {
        return false;
    }
    let host = host.to_ascii_lowercase();
    COVER_HOSTS
        .iter()
        .any(|h| host == *h || host.ends_with(&format!(".{h}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn covers_come_only_from_the_catalogue_image_hosts() {
        assert!(is_cover_url(
            "https://is1-ssl.mzstatic.com/image/thumb/a.jpg/600x600bb.jpg"
        ));
        assert!(is_cover_url(
            "https://cdn-images.dzcdn.net/images/cover/x/1000x1000.jpg"
        ));
        assert!(is_cover_url(
            "https://coverartarchive.org/release/id/front-500"
        ));
        assert!(!is_cover_url("http://coverartarchive.org/release/id/front"));
        assert!(!is_cover_url("https://evil.com/mzstatic.com/a.jpg"));
        assert!(!is_cover_url("https://notmzstatic.com/a.jpg"));
        assert!(!is_cover_url("https://user@mzstatic.com/a.jpg"));
        assert!(!is_cover_url("https://localhost:8080/a.jpg"));
        assert!(!is_cover_url("file:///c:/a.jpg"));
    }

    #[test]
    fn length_match_needs_both_lengths() {
        let c = Candidate {
            duration_ms: Some(200_000),
            ..Default::default()
        };
        assert!(c.matches_length(202_000));
        assert!(!c.matches_length(204_000));
        assert!(!c.matches_length(0));
        assert!(!Candidate::default().matches_length(200_000));
    }
}
