//! User playlists.
//!
//! An item names its track by path, so it survives a rescan that deletes and re-adds the
//! track under a new id. While the file is not indexed the item stays in the playlist,
//! hidden from [`PlaylistDetail::items`] and counted in [`PlaylistDetail::missing`].

use rusqlite::{OptionalExtension, Row, Transaction, TransactionBehavior, params};

use crate::query::{TRACK_COLUMN_COUNT, TRACK_COLUMNS, TRACK_FROM, track_row};
use crate::{Library, LibraryError, Track, unix_now};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistSummary {
    pub id: i64,
    pub name: String,
    /// Every entry, including those whose file is not in the library.
    pub item_count: u32,
    /// Total length of the entries whose file is in the library.
    pub duration_ms: u64,
    /// Cover of the first entry in the library, if any.
    pub art: Option<String>,
    /// Unix seconds.
    pub created_at: i64,
    /// Unix seconds of the last change to the name or the entries.
    pub updated_at: i64,
}

/// An entry whose file is in the library.
#[derive(Debug, Clone, PartialEq)]
pub struct PlaylistItem {
    /// Index among all entries (missing ones included); what remove and move take.
    pub position: u32,
    pub track: Track,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlaylistDetail {
    pub playlist: PlaylistSummary,
    /// Entries whose file is in the library, in playlist order.
    pub items: Vec<PlaylistItem>,
    /// Entries whose file is not in the library (moved, deleted or not yet rescanned).
    pub missing: u32,
}

const SUMMARY_SELECT: &str = "
    SELECT p.id, p.name, p.created_at, p.updated_at,
           (SELECT count(*) FROM playlist_items i WHERE i.playlist_id = p.id),
           (SELECT coalesce(sum(t.duration_ms), 0) FROM playlist_items i
            JOIN tracks t ON t.path = i.path WHERE i.playlist_id = p.id),
           (SELECT art.hash FROM playlist_items i
            JOIN tracks t ON t.path = i.path
            JOIN albums a ON a.id = t.album_id
            JOIN art ON art.id = coalesce(t.art_id, a.art_id)
            WHERE i.playlist_id = p.id ORDER BY i.position LIMIT 1)
    FROM playlists p";

fn summary_row(row: &Row) -> rusqlite::Result<PlaylistSummary> {
    Ok(PlaylistSummary {
        id: row.get(0)?,
        name: row.get(1)?,
        created_at: row.get(2)?,
        updated_at: row.get(3)?,
        item_count: row.get(4)?,
        duration_ms: row.get::<_, i64>(5)? as u64,
        art: row.get(6)?,
    })
}

/// A playlist name with surrounding whitespace removed; empty names are refused.
fn clean_name(name: &str) -> Result<&str, LibraryError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(LibraryError::EmptyPlaylistName);
    }
    Ok(name)
}

impl Library {
    /// Every playlist, by name.
    pub fn playlists(&self) -> Result<Vec<PlaylistSummary>, LibraryError> {
        let sql = format!("{SUMMARY_SELECT} ORDER BY p.name COLLATE NOCASE, p.id");
        let playlists = self
            .conn
            .prepare_cached(&sql)?
            .query_map([], summary_row)?
            .collect::<Result<_, _>>()?;
        Ok(playlists)
    }

    /// A playlist with its entries that are in the library.
    pub fn playlist(&self, id: i64) -> Result<Option<PlaylistDetail>, LibraryError> {
        // One read transaction, so the summary and the items describe the same state.
        let tx = self.conn.unchecked_transaction()?;
        let Some(playlist) = summary(&tx, id)? else {
            return Ok(None);
        };
        let sql = format!(
            "SELECT {TRACK_COLUMNS}, i.position {TRACK_FROM}
             JOIN playlist_items i ON i.path = t.path
             WHERE i.playlist_id = ?1 ORDER BY i.position"
        );
        let items: Vec<PlaylistItem> = tx
            .prepare_cached(&sql)?
            .query_map([id], |row| {
                Ok(PlaylistItem {
                    track: track_row(row)?,
                    position: row.get(TRACK_COLUMN_COUNT)?,
                })
            })?
            .collect::<Result<_, _>>()?;
        let missing = playlist.item_count - items.len() as u32;
        Ok(Some(PlaylistDetail {
            playlist,
            items,
            missing,
        }))
    }

    pub fn create_playlist(&mut self, name: &str) -> Result<PlaylistSummary, LibraryError> {
        let name = clean_name(name)?;
        let now = unix_now();
        let id: i64 = self.conn.query_row(
            "INSERT INTO playlists (name, created_at, updated_at) VALUES (?1, ?2, ?2)
             RETURNING id",
            params![name, now],
            |row| row.get(0),
        )?;
        Ok(PlaylistSummary {
            id,
            name: name.to_owned(),
            item_count: 0,
            duration_ms: 0,
            art: None,
            created_at: now,
            updated_at: now,
        })
    }

    /// Returns whether the playlist exists.
    pub fn rename_playlist(&mut self, id: i64, name: &str) -> Result<bool, LibraryError> {
        let name = clean_name(name)?;
        let changed = self.conn.execute(
            "UPDATE playlists SET name = ?1, updated_at = ?2 WHERE id = ?3",
            params![name, unix_now(), id],
        )?;
        Ok(changed > 0)
    }

    /// Deletes a playlist and its entries. Returns whether it existed.
    pub fn delete_playlist(&mut self, id: i64) -> Result<bool, LibraryError> {
        let changed = self
            .conn
            .execute("DELETE FROM playlists WHERE id = ?1", [id])?;
        Ok(changed > 0)
    }

    /// Appends tracks, in the given order, to the end of a playlist. Ids not in the library
    /// are skipped; the same track may appear more than once. Returns how many were added.
    pub fn add_to_playlist(&mut self, id: i64, track_ids: &[i64]) -> Result<usize, LibraryError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if !touch(&tx, id)? {
            return Err(LibraryError::PlaylistNotFound(id));
        }
        let mut next: u32 = tx.query_row(
            "SELECT count(*) FROM playlist_items WHERE playlist_id = ?1",
            [id],
            |r| r.get(0),
        )?;
        let mut added = 0;
        {
            let mut path_of = tx.prepare_cached("SELECT path FROM tracks WHERE id = ?1")?;
            let mut insert = tx.prepare_cached(
                "INSERT INTO playlist_items (playlist_id, position, path) VALUES (?1, ?2, ?3)",
            )?;
            for &track in track_ids {
                let Some(path): Option<String> =
                    path_of.query_row([track], |r| r.get(0)).optional()?
                else {
                    continue;
                };
                insert.execute(params![id, next, path])?;
                next += 1;
                added += 1;
            }
        }
        tx.commit()?;
        Ok(added)
    }

    /// Removes the entry at `position`; later entries move up. Returns whether it existed.
    pub fn remove_from_playlist(&mut self, id: i64, position: u32) -> Result<bool, LibraryError> {
        self.edit_items(id, |paths| {
            let index = position as usize;
            if index >= paths.len() {
                return false;
            }
            paths.remove(index);
            true
        })
    }

    /// Moves the entry at `from` so it ends up at position `to`, shifting the entries
    /// between. Returns whether both positions exist.
    pub fn move_in_playlist(&mut self, id: i64, from: u32, to: u32) -> Result<bool, LibraryError> {
        self.edit_items(id, |paths| {
            let (from, to) = (from as usize, to as usize);
            if from >= paths.len() || to >= paths.len() {
                return false;
            }
            let path = paths.remove(from);
            paths.insert(to, path);
            true
        })
    }

    /// Rewrites a playlist's entries after `edit` changes their paths in order. Rewriting
    /// keeps positions dense without shuffling them under the `(playlist, position)` key.
    fn edit_items(
        &mut self,
        id: i64,
        edit: impl FnOnce(&mut Vec<String>) -> bool,
    ) -> Result<bool, LibraryError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut paths: Vec<String> = tx
            .prepare_cached(
                "SELECT path FROM playlist_items WHERE playlist_id = ?1 ORDER BY position",
            )?
            .query_map([id], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        if !edit(&mut paths) || !touch(&tx, id)? {
            return Ok(false);
        }
        tx.execute("DELETE FROM playlist_items WHERE playlist_id = ?1", [id])?;
        {
            let mut insert = tx.prepare_cached(
                "INSERT INTO playlist_items (playlist_id, position, path) VALUES (?1, ?2, ?3)",
            )?;
            for (position, path) in paths.iter().enumerate() {
                insert.execute(params![id, position as u32, path])?;
            }
        }
        tx.commit()?;
        Ok(true)
    }
}

fn summary(tx: &Transaction, id: i64) -> Result<Option<PlaylistSummary>, LibraryError> {
    let sql = format!("{SUMMARY_SELECT} WHERE p.id = ?1");
    Ok(tx
        .prepare_cached(&sql)?
        .query_row([id], summary_row)
        .optional()?)
}

/// Marks a playlist as changed now. Returns whether it exists.
fn touch(tx: &Transaction, id: i64) -> Result<bool, LibraryError> {
    let changed = tx.execute(
        "UPDATE playlists SET updated_at = ?1 WHERE id = ?2",
        params![unix_now(), id],
    )?;
    Ok(changed > 0)
}
