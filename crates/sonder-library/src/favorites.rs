//! User favorite tracks, keyed by file path so stars survive rescans.

use std::path::Path;

use rusqlite::{TransactionBehavior, params};

use crate::query::{TRACK_COLUMNS, TRACK_FROM, track_row};
use crate::{Library, LibraryError, Track, path_str, unix_now};

impl Library {
    /// Toggles the favorite mark for `path` and returns its new state (`true` when favorited).
    pub fn toggle_favorite(&mut self, path: &Path) -> Result<bool, LibraryError> {
        let path = path_str(path)?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let deleted = tx.execute("DELETE FROM favorites WHERE path = ?1", [path])?;
        let favorited = if deleted > 0 {
            false
        } else {
            tx.execute(
                "INSERT INTO favorites (path, added_at) VALUES (?1, ?2)",
                params![path, unix_now()],
            )?;
            true
        };
        tx.commit()?;
        Ok(favorited)
    }

    /// Sets or clears the favorite mark for `path`.
    pub fn set_favorite(&mut self, path: &Path, favorite: bool) -> Result<(), LibraryError> {
        let path = path_str(path)?;
        if favorite {
            self.conn.execute(
                "INSERT OR IGNORE INTO favorites (path, added_at) VALUES (?1, ?2)",
                params![path, unix_now()],
            )?;
        } else {
            self.conn
                .execute("DELETE FROM favorites WHERE path = ?1", [path])?;
        }
        Ok(())
    }

    /// Every favorited track still indexed in the library, newest favorite first.
    pub fn favorite_tracks(&self) -> Result<Vec<Track>, LibraryError> {
        let sql = format!(
            "SELECT {TRACK_COLUMNS} {TRACK_FROM}
             WHERE f.path IS NOT NULL
             ORDER BY f.added_at DESC, t.title COLLATE NOCASE"
        );
        let mut stmt = self.conn.prepare_cached(&sql)?;
        let tracks = stmt.query_map([], track_row)?.collect::<Result<_, _>>()?;
        Ok(tracks)
    }

    /// Every favorited file path, including files currently outside the scanned library roots.
    pub fn favorite_paths(&self) -> Result<Vec<String>, LibraryError> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT path FROM favorites ORDER BY added_at DESC")?;
        let paths = stmt
            .query_map([], |row| row.get(0))?
            .collect::<Result<_, _>>()?;
        Ok(paths)
    }
}
