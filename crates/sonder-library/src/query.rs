//! Browse and search queries.

use std::path::PathBuf;

use rusqlite::{OptionalExtension, Row, params_from_iter, types::Value};

use crate::tags::{AudioInfo, Codec, ReplayGain};
use crate::{Library, LibraryError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlbumSort {
    /// Album artist, then year, then title.
    Artist,
    /// Title, then album artist.
    Title,
    /// Most recently added first.
    Added,
    /// Oldest first; albums without a year last.
    Year,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AlbumSummary {
    pub id: i64,
    pub title: String,
    /// Display album artist ("A; B", "Various Artists").
    pub artist: Option<String>,
    pub artist_id: Option<i64>,
    pub year: Option<i32>,
    pub track_count: u32,
    pub duration_ms: u64,
    /// Cover hash for [`ArtCache::thumbnail_path`](crate::ArtCache::thumbnail_path).
    pub art: Option<String>,
    /// Unix seconds.
    pub added_at: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    pub id: i64,
    pub path: PathBuf,
    pub title: String,
    pub artist: Option<String>,
    pub album_artist: Option<String>,
    pub album_id: i64,
    pub album: String,
    pub track_no: Option<u32>,
    pub disc_no: Option<u32>,
    pub year: Option<i32>,
    pub genre: Option<String>,
    pub audio: AudioInfo,
    pub replay_gain: ReplayGain,
    /// The track's own cover, else its album's.
    pub art: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AlbumDetail {
    pub album: AlbumSummary,
    pub mb_album_id: Option<String>,
    /// In disc and track order.
    pub tracks: Vec<Track>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtistSummary {
    pub id: i64,
    pub name: String,
    /// Albums with this artist as an album artist.
    pub album_count: u32,
    /// Tracks with this artist as a track artist.
    pub track_count: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ArtistDetail {
    pub artist: ArtistSummary,
    /// Albums with this artist as an album artist, oldest first.
    pub albums: Vec<AlbumSummary>,
    /// Other albums with tracks by this artist (compilations, guest spots).
    pub appears_on: Vec<AlbumSummary>,
}

const ALBUM_SELECT: &str = "
    SELECT a.id, a.title, a.artist, a.artist_id, a.year, count(t.id),
           coalesce(sum(t.duration_ms), 0), art.hash, a.added_at
    FROM albums a
    JOIN tracks t ON t.album_id = a.id
    LEFT JOIN art ON art.id = a.art_id";

const TRACK_SELECT: &str = "
    SELECT t.id, t.path, t.title, t.artist, t.album_artist, t.album_id, a.title, t.track_no,
           t.disc_no, t.year, t.genre, t.codec, t.rate, t.bits, t.channels, t.duration_ms,
           t.rg_track_gain, t.rg_track_peak, t.rg_album_gain, t.rg_album_peak, art.hash
    FROM tracks t
    JOIN albums a ON a.id = t.album_id
    LEFT JOIN art ON art.id = coalesce(t.art_id, a.art_id)";

const TRACK_ORDER: &str = "ORDER BY coalesce(t.disc_no, 1), t.track_no IS NULL, t.track_no, t.path";

/// Albums an artist is album artist of: primary (albums.artist_id) or any credited one.
const ARTIST_ALBUMS: &str = "
    SELECT id FROM albums WHERE artist_id = ?1
    UNION
    SELECT t.album_id FROM track_artists ta JOIN tracks t ON t.id = ta.track_id
    WHERE ta.artist_id = ?1 AND ta.role = 1";

fn album_row(row: &Row) -> rusqlite::Result<AlbumSummary> {
    Ok(AlbumSummary {
        id: row.get(0)?,
        title: row.get(1)?,
        artist: row.get(2)?,
        artist_id: row.get(3)?,
        year: row.get(4)?,
        track_count: row.get(5)?,
        duration_ms: row.get::<_, i64>(6)? as u64,
        art: row.get(7)?,
        added_at: row.get(8)?,
    })
}

fn track_row(row: &Row) -> rusqlite::Result<Track> {
    let codec: String = row.get(11)?;
    Ok(Track {
        id: row.get(0)?,
        path: PathBuf::from(row.get::<_, String>(1)?),
        title: row.get(2)?,
        artist: row.get(3)?,
        album_artist: row.get(4)?,
        album_id: row.get(5)?,
        album: row.get(6)?,
        track_no: row.get(7)?,
        disc_no: row.get(8)?,
        year: row.get(9)?,
        genre: row.get(10)?,
        audio: AudioInfo {
            codec: Codec::parse(&codec).ok_or_else(|| {
                rusqlite::Error::FromSqlConversionFailure(
                    11,
                    rusqlite::types::Type::Text,
                    format!("unknown codec {codec:?}").into(),
                )
            })?,
            rate: row.get(12)?,
            bits: row.get(13)?,
            channels: row.get(14)?,
            duration_ms: row.get::<_, i64>(15)? as u64,
        },
        replay_gain: ReplayGain {
            track_gain: row.get(16)?,
            track_peak: row.get(17)?,
            album_gain: row.get(18)?,
            album_peak: row.get(19)?,
        },
        art: row.get(20)?,
    })
}

impl Library {
    /// Every album with at least one track.
    pub fn albums(&self, sort: AlbumSort) -> Result<Vec<AlbumSummary>, LibraryError> {
        let order = match sort {
            AlbumSort::Artist => {
                "a.artist IS NULL, a.artist COLLATE NOCASE, a.year, a.title COLLATE NOCASE"
            }
            AlbumSort::Title => "a.title COLLATE NOCASE, a.artist COLLATE NOCASE",
            AlbumSort::Added => "a.added_at DESC, a.id DESC",
            AlbumSort::Year => {
                "a.year IS NULL, a.year, a.artist COLLATE NOCASE, a.title COLLATE NOCASE"
            }
        };
        let sql = format!("{ALBUM_SELECT} GROUP BY a.id ORDER BY {order}");
        let mut stmt = self.conn.prepare_cached(&sql)?;
        let albums = stmt.query_map([], album_row)?.collect::<Result<_, _>>()?;
        Ok(albums)
    }

    pub fn album(&self, id: i64) -> Result<Option<AlbumDetail>, LibraryError> {
        let sql = format!("{ALBUM_SELECT} WHERE a.id = ?1 GROUP BY a.id");
        let Some(album) = self
            .conn
            .prepare_cached(&sql)?
            .query_row([id], album_row)
            .optional()?
        else {
            return Ok(None);
        };
        let mb_album_id = self
            .conn
            .prepare_cached("SELECT mb_album_id FROM albums WHERE id = ?1")?
            .query_row([id], |r| r.get(0))?;
        let sql = format!("{TRACK_SELECT} WHERE t.album_id = ?1 {TRACK_ORDER}");
        let tracks = self
            .conn
            .prepare_cached(&sql)?
            .query_map([id], track_row)?
            .collect::<Result<_, _>>()?;
        Ok(Some(AlbumDetail {
            album,
            mb_album_id,
            tracks,
        }))
    }

    pub fn track(&self, id: i64) -> Result<Option<Track>, LibraryError> {
        let sql = format!("{TRACK_SELECT} WHERE t.id = ?1");
        Ok(self
            .conn
            .prepare_cached(&sql)?
            .query_row([id], track_row)
            .optional()?)
    }

    /// Every artist credited on a track or an album, by name.
    pub fn artists(&self) -> Result<Vec<ArtistSummary>, LibraryError> {
        let sql = format!(
            "SELECT ar.id, ar.name,
                    (SELECT count(*) FROM ({ARTIST_ALBUMS_FOR_OUTER})),
                    (SELECT count(*) FROM track_artists ta WHERE ta.artist_id = ar.id AND ta.role = 0)
             FROM artists ar
             ORDER BY ar.name COLLATE NOCASE",
            ARTIST_ALBUMS_FOR_OUTER = ARTIST_ALBUMS.replace("?1", "ar.id")
        );
        let mut stmt = self.conn.prepare_cached(&sql)?;
        let artists = stmt.query_map([], artist_row)?.collect::<Result<_, _>>()?;
        Ok(artists)
    }

    pub fn artist(&self, id: i64) -> Result<Option<ArtistDetail>, LibraryError> {
        let sql = format!(
            "SELECT ar.id, ar.name,
                    (SELECT count(*) FROM ({ARTIST_ALBUMS})),
                    (SELECT count(*) FROM track_artists ta WHERE ta.artist_id = ar.id AND ta.role = 0)
             FROM artists ar WHERE ar.id = ?1"
        );
        let Some(artist) = self
            .conn
            .prepare_cached(&sql)?
            .query_row([id], artist_row)
            .optional()?
        else {
            return Ok(None);
        };
        let albums_sql = format!(
            "{ALBUM_SELECT} WHERE a.id IN ({ARTIST_ALBUMS})
             GROUP BY a.id ORDER BY a.year IS NULL, a.year, a.title COLLATE NOCASE"
        );
        let albums = self
            .conn
            .prepare_cached(&albums_sql)?
            .query_map([id], album_row)?
            .collect::<Result<_, _>>()?;
        let appears_sql = format!(
            "{ALBUM_SELECT}
             WHERE a.id IN (SELECT t2.album_id FROM track_artists ta
                            JOIN tracks t2 ON t2.id = ta.track_id
                            WHERE ta.artist_id = ?1 AND ta.role = 0)
               AND a.id NOT IN ({ARTIST_ALBUMS})
             GROUP BY a.id ORDER BY a.year IS NULL, a.year, a.title COLLATE NOCASE"
        );
        let appears_on = self
            .conn
            .prepare_cached(&appears_sql)?
            .query_map([id], album_row)?
            .collect::<Result<_, _>>()?;
        Ok(Some(ArtistDetail {
            artist,
            albums,
            appears_on,
        }))
    }

    /// Tracks whose title, album or artists contain every whitespace-separated term of
    /// `query` (case- and diacritic-insensitive), best matches first.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<Track>, LibraryError> {
        let terms: Vec<&str> = query.split_whitespace().collect();
        if terms.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        // The trigram index answers MATCH for terms of three or more characters; shorter
        // terms fall back to LIKE on the same columns.
        let long: Vec<String> = terms
            .iter()
            .filter(|t| t.chars().count() >= 3)
            .map(|t| format!("\"{}\"", t.replace('"', "\"\"")))
            .collect();
        let mut conditions: Vec<String> = Vec::new();
        let mut values: Vec<Value> = Vec::new();
        if !long.is_empty() {
            values.push(Value::Text(long.join(" ")));
            conditions.push(format!("track_search MATCH ?{}", values.len()));
        }
        for term in terms.iter().filter(|t| t.chars().count() < 3) {
            values.push(Value::Text(format!("%{}%", escape_like(term))));
            let n = values.len();
            conditions.push(format!(
                "(track_search.title LIKE ?{n} ESCAPE '\\' OR track_search.album LIKE ?{n} ESCAPE '\\'
                  OR track_search.artist LIKE ?{n} ESCAPE '\\')"
            ));
        }
        values.push(Value::Integer(limit.min(i64::MAX as usize) as i64));
        let order = if long.is_empty() {
            "t.title COLLATE NOCASE"
        } else {
            "track_search.rank"
        };
        let sql = format!(
            "{TRACK_SELECT} JOIN track_search ON track_search.rowid = t.id
             WHERE {} ORDER BY {order} LIMIT ?{}",
            conditions.join(" AND "),
            values.len()
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let tracks = stmt
            .query_map(params_from_iter(values), track_row)?
            .collect::<Result<_, _>>()?;
        Ok(tracks)
    }
}

fn artist_row(row: &Row) -> rusqlite::Result<ArtistSummary> {
    Ok(ArtistSummary {
        id: row.get(0)?,
        name: row.get(1)?,
        album_count: row.get(2)?,
        track_count: row.get(3)?,
    })
}

fn escape_like(term: &str) -> String {
    let mut out = String::with_capacity(term.len());
    for c in term.chars() {
        if matches!(c, '%' | '_' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}
