//! Google Drive URL and ID parser for shared folders and single audio files.

use url::Url;

use crate::error::GdriveError;

/// Whether a parsed Google Drive link points to a folder, a file, or an ambiguous ID
/// (such as `/open?id=...` or a raw ID) that should be auto-detected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriveLinkKind {
    Folder,
    File,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedDriveLink {
    pub id: String,
    pub kind: DriveLinkKind,
}

/// Parses a Google Drive share URL or raw Drive ID into its file/folder ID and kind.
///
/// Supported forms:
/// - `https://drive.google.com/drive/folders/<ID>`
/// - `https://drive.google.com/drive/u/0/folders/<ID>?usp=sharing`
/// - `https://drive.google.com/drive/mobile/folders/<ID>`
/// - `https://drive.google.com/embeddedfolderview?id=<ID>`
/// - `https://drive.google.com/file/d/<ID>/view?usp=sharing`
/// - `https://drive.google.com/file/u/0/d/<ID>/view`
/// - `https://drive.google.com/open?id=<ID>`
/// - `https://drive.google.com/uc?id=<ID>&export=download`
/// - `https://drive.usercontent.google.com/download?id=<ID>`
/// - Raw Drive IDs (10–128 ASCII alphanumeric, `-`, `_` characters)
pub fn parse_drive_link(input: &str) -> Result<ParsedDriveLink, GdriveError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(GdriveError::Protocol(
            "Please paste a Google Drive folder or file link".into(),
        ));
    }

    // Raw Drive ID (no URL scheme or slashes).
    if !trimmed.contains("://") && !trimmed.contains('/') && !trimmed.contains('?') {
        if is_valid_drive_id(trimmed) {
            return Ok(ParsedDriveLink {
                id: trimmed.to_owned(),
                kind: DriveLinkKind::Unknown,
            });
        }
        return Err(GdriveError::Protocol(
            "That does not look like a Google Drive link or ID".into(),
        ));
    }

    let normalized = if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        trimmed.to_owned()
    } else {
        format!("https://{trimmed}")
    };
    let url = Url::parse(&normalized)
        .map_err(|_| GdriveError::Protocol("Invalid Google Drive URL".into()))?;

    let host = url.host_str().unwrap_or("").to_ascii_lowercase();
    let is_google_drive = host == "drive.google.com"
        || host == "drive.usercontent.google.com"
        || host == "docs.google.com";
    if !is_google_drive {
        return Err(GdriveError::Protocol(
            "Only Google Drive links (drive.google.com) are supported".into(),
        ));
    }

    let segments: Vec<&str> = url
        .path_segments()
        .map(|s| s.filter(|p| !p.is_empty()).collect())
        .unwrap_or_default();

    // /drive/folders/<ID>, /drive/u/0/folders/<ID>, /drive/mobile/folders/<ID>
    if let Some(pos) = segments.iter().position(|&s| s == "folders")
        && let Some(&id) = segments.get(pos + 1)
        && is_valid_drive_id(id)
    {
        return Ok(ParsedDriveLink {
            id: id.to_owned(),
            kind: DriveLinkKind::Folder,
        });
    }

    // /file/d/<ID>/..., /file/u/0/d/<ID>/...
    if let Some(pos) = segments.iter().position(|&s| s == "d")
        && let Some(&id) = segments.get(pos + 1)
        && is_valid_drive_id(id)
    {
        return Ok(ParsedDriveLink {
            id: id.to_owned(),
            kind: DriveLinkKind::File,
        });
    }

    // Query parameter ?id=<ID>
    if let Some((_, id)) = url.query_pairs().find(|(k, _)| k == "id") {
        let id = id.trim();
        if is_valid_drive_id(id) {
            let kind = match segments.first().copied() {
                Some("embeddedfolderview") => DriveLinkKind::Folder,
                Some("uc") | Some("download") => DriveLinkKind::File,
                _ => DriveLinkKind::Unknown,
            };
            return Ok(ParsedDriveLink {
                id: id.to_owned(),
                kind,
            });
        }
    }

    Err(GdriveError::Protocol(
        "Could not find a Google Drive folder or file ID in that URL".into(),
    ))
}

pub(crate) fn is_valid_drive_id(id: &str) -> bool {
    (10..=128).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_folder_and_file_url_variants() {
        let cases = [
            (
                "https://drive.google.com/drive/folders/1AbCdEfGhIjKlMnOpQrStUvWxYz?usp=sharing",
                "1AbCdEfGhIjKlMnOpQrStUvWxYz",
                DriveLinkKind::Folder,
            ),
            (
                "https://drive.google.com/drive/u/0/folders/1AbCdEfGhIjKlMnOpQrStUvWxYz",
                "1AbCdEfGhIjKlMnOpQrStUvWxYz",
                DriveLinkKind::Folder,
            ),
            (
                "https://drive.google.com/drive/mobile/folders/1AbCdEfGhIjKlMnOpQrStUvWxYz?resourcekey=0-abc",
                "1AbCdEfGhIjKlMnOpQrStUvWxYz",
                DriveLinkKind::Folder,
            ),
            (
                "https://drive.google.com/embeddedfolderview?id=1AbCdEfGhIjKlMnOpQrStUvWxYz#list",
                "1AbCdEfGhIjKlMnOpQrStUvWxYz",
                DriveLinkKind::Folder,
            ),
            (
                "https://drive.google.com/file/d/1XyZ_9876543210-abcd/view?usp=drive_link",
                "1XyZ_9876543210-abcd",
                DriveLinkKind::File,
            ),
            (
                "https://drive.google.com/file/u/1/d/1XyZ_9876543210-abcd/preview",
                "1XyZ_9876543210-abcd",
                DriveLinkKind::File,
            ),
            (
                "https://drive.google.com/uc?id=1XyZ_9876543210-abcd&export=download",
                "1XyZ_9876543210-abcd",
                DriveLinkKind::File,
            ),
            (
                "https://drive.usercontent.google.com/download?id=1XyZ_9876543210-abcd&export=download&confirm=t",
                "1XyZ_9876543210-abcd",
                DriveLinkKind::File,
            ),
            (
                "https://drive.google.com/open?id=1AbCdEfGhIjKlMnOpQrStUvWxYz",
                "1AbCdEfGhIjKlMnOpQrStUvWxYz",
                DriveLinkKind::Unknown,
            ),
            (
                "1AbCdEfGhIjKlMnOpQrStUvWxYz",
                "1AbCdEfGhIjKlMnOpQrStUvWxYz",
                DriveLinkKind::Unknown,
            ),
        ];
        for (input, expected_id, expected_kind) in cases {
            let parsed = parse_drive_link(input).unwrap_or_else(|e| panic!("{input}: {e}"));
            assert_eq!(parsed.id, expected_id, "input: {input}");
            assert_eq!(parsed.kind, expected_kind, "input: {input}");
        }
    }

    #[test]
    fn rejects_invalid_or_non_drive_urls() {
        for bad in [
            "",
            "   ",
            "short",
            "https://example.com/drive/folders/1AbCdEfGhIjKlMnOpQrStUvWxYz",
            "https://drive.google.com/drive/my-drive",
            "https://drive.google.com/file/d/bad/view",
        ] {
            assert!(parse_drive_link(bad).is_err(), "expected error for {bad:?}");
        }
    }
}
