//! Listening history: plays recorded by the app's play tracker, and the stats built on them.
//!
//! A play is stored by path with the time it started and how long was actually heard. The
//! stats join plays to the library at query time, so a track's current tags are shown and a
//! file that left the library only counts in the totals.

use std::path::Path;

use rusqlite::{Row, params};

use crate::query::{
    ALBUM_SELECT, TRACK_COLUMN_COUNT, TRACK_COLUMNS, TRACK_FROM, album_row, track_row,
};
use crate::{AlbumSummary, Library, LibraryError, Track};

/// The span the stats cover, ending now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatsPeriod {
    Week,
    Month,
    Year,
    All,
}

impl StatsPeriod {
    /// First second of the period, or `None` for all time.
    fn since(self, now: i64) -> Option<i64> {
        let days = match self {
            Self::Week => 7,
            Self::Month => 30,
            Self::Year => 365,
            Self::All => return None,
        };
        Some(now - days * 86_400)
    }

    /// Whether the chart has one bar per day (else one per month).
    fn daily(self) -> bool {
        matches!(self, Self::Week | Self::Month)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TopTrack {
    pub track: Track,
    pub plays: u32,
    pub listened_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopArtist {
    pub id: i64,
    pub name: String,
    /// A cover from one of the artist's played albums.
    pub art: Option<String>,
    pub plays: u32,
    pub listened_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopAlbum {
    pub id: i64,
    pub title: String,
    pub artist: Option<String>,
    pub art: Option<String>,
    pub plays: u32,
    pub listened_ms: u64,
}

/// One bar of the listening-time chart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatsBucket {
    /// Local date `YYYY-MM-DD` for a day, `YYYY-MM` for a month.
    pub label: String,
    pub listened_ms: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RecentPlay {
    pub track: Track,
    /// Unix seconds.
    pub played_at: i64,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ListeningStats {
    pub plays: u32,
    pub listened_ms: u64,
    /// Different files played.
    pub tracks: u32,
    pub top_tracks: Vec<TopTrack>,
    pub top_artists: Vec<TopArtist>,
    pub top_albums: Vec<TopAlbum>,
    /// Every day (or month) of the period in order, empty ones included.
    pub buckets: Vec<StatsBucket>,
}

impl Library {
    /// Records a play of `path` that started at `played_at` (Unix seconds).
    pub fn record_play(
        &mut self,
        path: &Path,
        played_at: i64,
        listened_ms: u64,
    ) -> Result<(), LibraryError> {
        let path = path
            .to_str()
            .ok_or_else(|| LibraryError::NonUtf8Path(path.to_path_buf()))?;
        self.conn.execute(
            "INSERT INTO plays (path, played_at, listened_ms) VALUES (?1, ?2, ?3)",
            params![
                path,
                played_at,
                i64::try_from(listened_ms).unwrap_or(i64::MAX)
            ],
        )?;
        Ok(())
    }

    /// Totals, top lists of up to `limit` entries and the chart for `period`, ending at `now`.
    pub fn listening_stats(
        &self,
        period: StatsPeriod,
        now: i64,
        limit: u32,
    ) -> Result<ListeningStats, LibraryError> {
        let since = period.since(now).unwrap_or(i64::MIN);
        let (plays, listened_ms, tracks) = self.conn.query_row(
            "SELECT count(*), coalesce(sum(listened_ms), 0), count(DISTINCT path)
             FROM plays WHERE played_at >= ?1 AND played_at <= ?2",
            params![since, now],
            |r| Ok((r.get(0)?, r.get::<_, i64>(1)? as u64, r.get(2)?)),
        )?;
        if plays == 0 {
            return Ok(ListeningStats {
                buckets: self.buckets(period, since, now)?,
                ..ListeningStats::default()
            });
        }

        let mut stmt = self.conn.prepare(&format!(
            "SELECT {TRACK_COLUMNS}, count(*), sum(p.listened_ms) {TRACK_FROM}
             JOIN plays p ON p.path = t.path
             WHERE p.played_at >= ?1 AND p.played_at <= ?2
             GROUP BY t.id ORDER BY count(*) DESC, sum(p.listened_ms) DESC, t.title LIMIT ?3"
        ))?;
        let top_tracks = stmt
            .query_map(params![since, now, limit], |row| {
                Ok(TopTrack {
                    track: track_row(row)?,
                    plays: row.get(TRACK_COLUMN_COUNT)?,
                    listened_ms: row.get::<_, i64>(TRACK_COLUMN_COUNT + 1)? as u64,
                })
            })?
            .collect::<Result<_, _>>()?;

        let mut stmt = self.conn.prepare(
            "SELECT ar.id, ar.name, max(art.hash), count(*), sum(p.listened_ms)
             FROM plays p
             JOIN tracks t ON t.path = p.path
             JOIN track_artists ta ON ta.track_id = t.id AND ta.role = 0
             JOIN artists ar ON ar.id = ta.artist_id
             JOIN albums a ON a.id = t.album_id
             LEFT JOIN art ON art.id = coalesce(t.art_id, a.art_id)
             WHERE p.played_at >= ?1 AND p.played_at <= ?2
             GROUP BY ar.id ORDER BY count(*) DESC, sum(p.listened_ms) DESC, ar.name LIMIT ?3",
        )?;
        let top_artists = stmt
            .query_map(params![since, now, limit], |r| {
                Ok(TopArtist {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    art: r.get(2)?,
                    plays: r.get(3)?,
                    listened_ms: r.get::<_, i64>(4)? as u64,
                })
            })?
            .collect::<Result<_, _>>()?;

        let mut stmt = self.conn.prepare(
            "SELECT a.id, a.title, a.artist, art.hash, count(*), sum(p.listened_ms)
             FROM plays p
             JOIN tracks t ON t.path = p.path
             JOIN albums a ON a.id = t.album_id
             LEFT JOIN art ON art.id = a.art_id
             WHERE p.played_at >= ?1 AND p.played_at <= ?2
             GROUP BY a.id ORDER BY count(*) DESC, sum(p.listened_ms) DESC, a.title LIMIT ?3",
        )?;
        let top_albums = stmt
            .query_map(params![since, now, limit], |r| {
                Ok(TopAlbum {
                    id: r.get(0)?,
                    title: r.get(1)?,
                    artist: r.get(2)?,
                    art: r.get(3)?,
                    plays: r.get(4)?,
                    listened_ms: r.get::<_, i64>(5)? as u64,
                })
            })?
            .collect::<Result<_, _>>()?;

        Ok(ListeningStats {
            plays,
            listened_ms,
            tracks,
            top_tracks,
            top_artists,
            top_albums,
            buckets: self.buckets(period, since, now)?,
        })
    }

    /// The chart: every local day (or month) from the period's start (or the first play)
    /// through `now`, with the time heard in each.
    fn buckets(
        &self,
        period: StatsPeriod,
        since: i64,
        now: i64,
    ) -> Result<Vec<StatsBucket>, LibraryError> {
        let first: Option<i64> = if period == StatsPeriod::All {
            self.conn
                .query_row("SELECT min(played_at) FROM plays", [], |r| r.get(0))?
        } else {
            Some(since)
        };
        let Some(first) = first else {
            return Ok(Vec::new());
        };
        // Days run from the day after `since` so the chart has exactly 7 or 30 bars.
        let (start, step, key, label) = if period.daily() {
            (
                "date(?1, 'unixepoch', 'localtime', '+1 day')",
                "'+1 day'",
                "date(played_at, 'unixepoch', 'localtime')",
                "d",
            )
        } else {
            (
                "date(?1, 'unixepoch', 'localtime', 'start of month', iif(?3, '+0 days', '+1 month'))",
                "'+1 month'",
                "strftime('%Y-%m', played_at, 'unixepoch', 'localtime')",
                "strftime('%Y-%m', d)",
            )
        };
        let end = if period.daily() {
            "date(?2, 'unixepoch', 'localtime')"
        } else {
            "date(?2, 'unixepoch', 'localtime', 'start of month')"
        };
        let sql = format!(
            "WITH RECURSIVE span(d) AS (
                 SELECT {start}
                 UNION ALL SELECT date(d, {step}) FROM span WHERE d < {end}
             ),
             heard(k, ms) AS (
                 SELECT {key}, sum(listened_ms) FROM plays
                 WHERE played_at >= ?1 AND played_at <= ?2 GROUP BY 1
             )
             SELECT {label}, coalesce((SELECT ms FROM heard WHERE k = {label}), 0)
             FROM span ORDER BY d"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let row = |r: &Row| {
            Ok(StatsBucket {
                label: r.get(0)?,
                listened_ms: r.get::<_, i64>(1)? as u64,
            })
        };
        // Only the monthly chart asks whether it starts at the first play (all time).
        let rows = if period.daily() {
            stmt.query_map(params![first, now], row)?
                .collect::<Result<_, _>>()?
        } else {
            stmt.query_map(params![first, now, period == StatsPeriod::All], row)?
                .collect::<Result<_, _>>()?
        };
        Ok(rows)
    }

    /// The `limit` most recent plays of files still in the library, newest first.
    pub fn recent_plays(&self, limit: u32) -> Result<Vec<RecentPlay>, LibraryError> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {TRACK_COLUMNS}, p.played_at {TRACK_FROM}
             JOIN plays p ON p.path = t.path
             ORDER BY p.played_at DESC, p.id DESC LIMIT ?1"
        ))?;
        let rows = stmt
            .query_map([limit], |row: &Row| {
                Ok(RecentPlay {
                    track: track_row(row)?,
                    played_at: row.get(TRACK_COLUMN_COUNT)?,
                })
            })?
            .collect::<Result<_, _>>()?;
        Ok(rows)
    }

    /// The `limit` most recently played albums still in the library, newest play first.
    pub fn recent_albums(&self, limit: u32) -> Result<Vec<AlbumSummary>, LibraryError> {
        let sql = format!(
            "{ALBUM_SELECT}
             JOIN (
                 SELECT t.album_id, max(p.played_at) AS last_played, max(p.id) AS last_play_id
                 FROM plays p
                 JOIN tracks t ON t.path = p.path
                 GROUP BY t.album_id
             ) recent ON recent.album_id = a.id
             GROUP BY a.id
             ORDER BY recent.last_played DESC, recent.last_play_id DESC
             LIMIT ?1"
        );
        let mut stmt = self.conn.prepare_cached(&sql)?;
        let rows = stmt
            .query_map([limit], album_row)?
            .collect::<Result<_, _>>()?;
        Ok(rows)
    }

    /// Deletes the whole listening history; returns how many plays were removed.
    pub fn clear_history(&mut self) -> Result<usize, LibraryError> {
        Ok(self.conn.execute("DELETE FROM plays", [])?)
    }
}
