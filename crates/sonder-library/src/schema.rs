//! Database open, pragmas and versioned migrations.
//!
//! The schema version lives in `PRAGMA user_version`. Before any migration touches an existing
//! database, a consistent copy is written next to it as `<file>.bak-<old version>`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, TransactionBehavior};

use crate::LibraryError;

/// Migration scripts in order; script `i` upgrades the schema from version `i` to `i + 1`.
const MIGRATIONS: &[&str] = &[
    include_str!("migrations/0001_initial.sql"),
    include_str!("migrations/0002_art_colors.sql"),
    include_str!("migrations/0003_loudness.sql"),
    include_str!("migrations/0004_playlists.sql"),
    include_str!("migrations/0005_quality.sql"),
    include_str!("migrations/0006_history.sql"),
    include_str!("migrations/0007_favorites.sql"),
];

/// Schema version this build creates and understands.
pub const SCHEMA_VERSION: u32 = MIGRATIONS.len() as u32;

/// Opens (creating if needed) the library database and brings its schema up to date.
pub(crate) fn open(path: &Path) -> Result<Connection, LibraryError> {
    let existed = path.metadata().is_ok_and(|m| m.len() > 0);
    let mut conn = Connection::open(path)?;
    configure(&conn)?;
    migrate(&mut conn, path, existed)?;
    Ok(conn)
}

fn configure(conn: &Connection) -> Result<(), LibraryError> {
    // The scanner and the watcher may write from different connections.
    conn.busy_timeout(Duration::from_secs(10))?;
    let mode: String =
        conn.pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get(0))?;
    if !mode.eq_ignore_ascii_case("wal") {
        return Err(LibraryError::NoWal(mode));
    }
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", true)?;
    Ok(())
}

fn migrate(conn: &mut Connection, path: &Path, existed: bool) -> Result<(), LibraryError> {
    let version: u32 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version > SCHEMA_VERSION {
        return Err(LibraryError::SchemaTooNew {
            found: version,
            supported: SCHEMA_VERSION,
        });
    }
    if version == SCHEMA_VERSION {
        return Ok(());
    }
    if existed {
        backup(conn, &backup_path(path, version))?;
    }
    for (index, script) in MIGRATIONS.iter().enumerate().skip(version as usize) {
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute_batch(script)?;
        tx.pragma_update(None, "user_version", index as u32 + 1)?;
        tx.commit()?;
    }
    Ok(())
}

/// `library.db` at version 1 backs up to `library.db.bak-1`.
pub(crate) fn backup_path(path: &Path, version: u32) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".bak-{version}"));
    path.with_file_name(name)
}

/// Writes a consistent copy of the database (WAL content included) to `dest`.
///
/// An existing backup for the same version is kept: migrations run in transactions, so a
/// backup left by an earlier failed attempt still holds the untouched pre-migration data.
fn backup(conn: &Connection, dest: &Path) -> Result<(), LibraryError> {
    if dest.exists() {
        return Ok(());
    }
    let dest_str = dest
        .to_str()
        .ok_or_else(|| LibraryError::NonUtf8Path(dest.to_path_buf()))?;
    conn.execute("VACUUM INTO ?1", [dest_str])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_database_gets_current_schema_without_backup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.db");
        let conn = open(&path).unwrap();
        let version: u32 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
        assert!(!backup_path(&path, 0).exists());
        let mode: String = conn
            .pragma_query_value(None, "journal_mode", |r| r.get(0))
            .unwrap();
        assert_eq!(mode, "wal");
    }

    #[test]
    fn existing_database_is_backed_up_before_migrating() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.db");
        {
            // A database at schema version 0 that already holds data.
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch("CREATE TABLE keep (v TEXT); INSERT INTO keep VALUES ('x');")
                .unwrap();
        }
        open(&path).unwrap();
        let bak = backup_path(&path, 0);
        assert!(bak.exists());
        let copy = Connection::open(&bak).unwrap();
        let v: String = copy
            .query_row("SELECT v FROM keep", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, "x");
        let version: u32 = copy
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(version, 0);
    }

    #[test]
    fn upgrade_from_loudness_schema_keeps_data_and_adds_playlists() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.db");
        {
            // A library as the previous release left it: schema version 3 with one track.
            let conn = Connection::open(&path).unwrap();
            for script in &MIGRATIONS[..3] {
                conn.execute_batch(script).unwrap();
            }
            conn.pragma_update(None, "user_version", 3).unwrap();
            conn.execute_batch(
                "INSERT INTO roots (id, path, added_at) VALUES (1, '/m', 0);
                 INSERT INTO albums (id, group_key, title, added_at) VALUES (1, 'k', 'A', 0);
                 INSERT INTO tracks (root_id, path, mtime, size, album_id, title, codec, rate,
                                     channels, duration_ms, added_at, loudness_lufs)
                 VALUES (1, '/m/a.flac', 0, 0, 1, 'T', 'flac', 44100, 2, 1000, 0, -14.0);",
            )
            .unwrap();
        }
        let conn = open(&path).unwrap();
        let (title, lufs): (String, f64) = conn
            .query_row("SELECT title, loudness_lufs FROM tracks", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!((title.as_str(), lufs), ("T", -14.0));
        conn.execute(
            "INSERT INTO playlists (name, created_at, updated_at) VALUES ('p', 0, 0)",
            [],
        )
        .unwrap();
        let copy = Connection::open(backup_path(&path, 3)).unwrap();
        let version: u32 = copy
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(version, 3);
        let tracks: i64 = copy
            .query_row("SELECT count(*) FROM tracks", [], |r| r.get(0))
            .unwrap();
        assert_eq!(tracks, 1);
    }

    #[test]
    fn newer_schema_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.db");
        {
            let conn = Connection::open(&path).unwrap();
            conn.pragma_update(None, "user_version", SCHEMA_VERSION + 1)
                .unwrap();
        }
        assert!(matches!(
            open(&path),
            Err(LibraryError::SchemaTooNew { .. })
        ));
    }

    #[test]
    fn reopening_current_schema_is_a_no_op() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.db");
        drop(open(&path).unwrap());
        drop(open(&path).unwrap());
        assert!(!backup_path(&path, SCHEMA_VERSION).exists());
    }

    #[test]
    fn fts5_trigram_tokenizer_is_available() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE VIRTUAL TABLE t USING fts5(x, tokenize = 'trigram remove_diacritics 1');
             INSERT INTO t VALUES ('Beyoncé – Lemonade');",
        )
        .unwrap();
        let n: i64 = conn
            .query_row(
                "SELECT count(*) FROM t WHERE t MATCH '\"beyonce\"'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1);
    }
}
