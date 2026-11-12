//! Local music library: scans user-chosen folders into a SQLite index of tracks, albums,
//! artists and cover art, keeps it current with incremental rescans and a file watcher, and
//! answers the browse and search queries the UI needs.
//!
//! ```no_run
//! # fn main() -> Result<(), sonder_library::LibraryError> {
//! use sonder_library::{AlbumSort, Library};
//!
//! let mut library = Library::open("library.db", "art-cache")?;
//! library.add_root("D:/Music", false)?;
//! let report = library.scan()?;
//! println!("{} added, {} unchanged", report.added, report.unchanged);
//! for album in library.albums(AlbumSort::Artist)? {
//!     println!("{} - {}", album.artist.unwrap_or_default(), album.title);
//! }
//! # Ok(())
//! # }
//! ```

mod art;
mod group;
mod query;
mod scan;
mod schema;
mod tags;
mod watch;

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

pub use art::{ArtCache, ArtImage, MAX_COVER_COLORS, THUMB_SIZES, cover_colors};
pub use query::{AlbumDetail, AlbumSort, AlbumSummary, ArtistDetail, ArtistSummary, Track};
pub use scan::{ScanFailure, ScanProgress, ScanReport};
pub use schema::SCHEMA_VERSION;
pub use tags::{AUDIO_EXTENSIONS, AudioInfo, Codec, FileMeta, ReplayGain, TagError, TrackTags};
pub use tags::{is_audio_file, read_file};
pub use watch::{LibraryWatcher, WatchEvent};

#[derive(Debug, thiserror::Error)]
pub enum LibraryError {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("library database is at schema version {found}, newer than this build ({supported})")]
    SchemaTooNew { found: u32, supported: u32 },
    #[error("cannot enable WAL journaling on the library database (mode is {0})")]
    NoWal(String),
    #[error("path is not valid UTF-8: {}", .0.display())]
    NonUtf8Path(PathBuf),
    #[error("not a folder: {}", .0.display())]
    NotADirectory(PathBuf),
    #[error("{} overlaps the library folder {}", .path.display(), .existing.display())]
    RootOverlaps { path: PathBuf, existing: PathBuf },
    #[error("file watcher error: {0}")]
    Watch(#[from] notify_debouncer_mini::notify::Error),
}

/// A folder the library indexes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Root {
    pub id: i64,
    pub path: PathBuf,
    /// Follow symbolic links while scanning (they may lead outside the folder).
    pub follow_symlinks: bool,
}

/// One connection to the library database plus the art cache it fills.
///
/// Several `Library` values (for example the UI's and the watcher's) may share one database
/// file; SQLite's WAL mode lets them read concurrently and serialises writes.
pub struct Library {
    conn: Connection,
    art: ArtCache,
}

impl Library {
    /// Opens or creates the database at `db_path` (migrating it, with a backup, if it is
    /// older) and uses `art_dir` for cover thumbnails.
    pub fn open(
        db_path: impl AsRef<Path>,
        art_dir: impl Into<PathBuf>,
    ) -> Result<Self, LibraryError> {
        let db_path = db_path.as_ref();
        if let Some(parent) = db_path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        Ok(Library {
            conn: schema::open(db_path)?,
            art: ArtCache::new(art_dir),
        })
    }

    pub fn art_cache(&self) -> &ArtCache {
        &self.art
    }

    /// Adds a folder to index; call [`scan`](Self::scan) afterwards. Adding a folder that is
    /// already a root updates its symlink setting. Nested roots are refused because every
    /// track must belong to exactly one root.
    pub fn add_root(
        &mut self,
        path: impl AsRef<Path>,
        follow_symlinks: bool,
    ) -> Result<Root, LibraryError> {
        let path = std::path::absolute(path.as_ref())?;
        if !path.is_dir() {
            return Err(LibraryError::NotADirectory(path));
        }
        let path_str = path_str(&path)?;
        for root in self.roots()? {
            if root.path != path && (root.path.starts_with(&path) || path.starts_with(&root.path)) {
                return Err(LibraryError::RootOverlaps {
                    path,
                    existing: root.path,
                });
            }
        }
        let id = self.conn.query_row(
            "INSERT INTO roots (path, follow_symlinks, added_at) VALUES (?1, ?2, ?3)
             ON CONFLICT (path) DO UPDATE SET follow_symlinks = excluded.follow_symlinks
             RETURNING id",
            params![path_str, follow_symlinks, unix_now()],
            |row| row.get(0),
        )?;
        Ok(Root {
            id,
            path,
            follow_symlinks,
        })
    }

    /// Removes a root and everything indexed under it. Returns whether it existed.
    pub fn remove_root(&mut self, path: impl AsRef<Path>) -> Result<bool, LibraryError> {
        let path = std::path::absolute(path.as_ref())?;
        let path_str = path_str(&path)?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let Some(root_id): Option<i64> = tx
            .query_row("SELECT id FROM roots WHERE path = ?1", [path_str], |r| {
                r.get(0)
            })
            .optional()?
        else {
            return Ok(false);
        };
        let albums: Vec<i64> = {
            let mut stmt = tx.prepare("SELECT DISTINCT album_id FROM tracks WHERE root_id = ?1")?;
            stmt.query_map([root_id], |r| r.get(0))?
                .collect::<Result<_, _>>()?
        };
        tx.execute("DELETE FROM roots WHERE id = ?1", [root_id])?;
        scan::refresh_albums(&tx, albums)?;
        let orphan_art = scan::delete_orphans(&tx)?;
        tx.commit()?;
        for hash in orphan_art {
            self.art.remove(&hash);
        }
        Ok(true)
    }

    pub fn roots(&self) -> Result<Vec<Root>, LibraryError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, path, follow_symlinks FROM roots ORDER BY path")?;
        let roots = stmt
            .query_map([], |row| {
                Ok(Root {
                    id: row.get(0)?,
                    path: PathBuf::from(row.get::<_, String>(1)?),
                    follow_symlinks: row.get(2)?,
                })
            })?
            .collect::<Result<_, _>>()?;
        Ok(roots)
    }

    /// Works out the colours of covers stored before colours were recorded, from their
    /// cached 300 px thumbnail. Covers whose thumbnail is missing or unreadable are left for
    /// the next rescan. Returns how many covers were updated.
    pub fn backfill_art_colors(&mut self) -> Result<usize, LibraryError> {
        let pending: Vec<String> = self
            .conn
            .prepare("SELECT hash FROM art WHERE colors IS NULL")?
            .query_map([], |row| row.get(0))?
            .collect::<Result<_, _>>()?;
        let mut updated = 0;
        for hash in pending {
            let path = self.art.thumbnail_path(&hash, THUMB_SIZES[0]);
            let Ok(image) = image::open(&path) else {
                continue;
            };
            self.conn.execute(
                "UPDATE art SET colors = ?1 WHERE hash = ?2",
                params![art::cover_colors(&image), hash],
            )?;
            updated += 1;
        }
        Ok(updated)
    }
}

pub(crate) fn path_str(path: &Path) -> Result<&str, LibraryError> {
    path.to_str()
        .ok_or_else(|| LibraryError::NonUtf8Path(path.to_path_buf()))
}

pub(crate) fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}
