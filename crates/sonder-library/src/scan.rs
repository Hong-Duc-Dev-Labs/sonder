//! Incremental scanning.
//!
//! A scan reconciles a set of paths (whole roots, or the files and folders a watcher saw
//! change) with the database in four steps:
//!
//! 1. Walk the paths in parallel and stat every audio file.
//! 2. Diff against the indexed tracks under the same paths: unchanged `(mtime, size)` is
//!    skipped, new and changed files are queued, indexed files no longer on disk are removed.
//! 3. Read tags and resolve cover art for the queued files in parallel.
//! 4. Apply everything in one transaction and refresh the affected albums.
//!
//! A root folder that is missing (an unplugged drive) or a folder that cannot be read is never
//! taken as "everything inside was deleted": its tracks are kept and the problem is reported.

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::io::ErrorKind;
use std::path::{MAIN_SEPARATOR, Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant, UNIX_EPOCH};

use rayon::prelude::*;
use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params};
use walkdir::WalkDir;

use crate::art::{ArtImage, ArtResolver};
use crate::group::album_identity;
use crate::query::Track;
use crate::tags::{FileMeta, TagError, is_audio_file, read_file};
use crate::{Library, LibraryError, Root, path_str, unix_now};

/// Display album artist for albums whose tracks name several artists and no album artist.
const VARIOUS_ARTISTS: &str = "Various Artists";

/// What a scan changed.
#[derive(Debug, Clone, Default)]
pub struct ScanReport {
    pub added: usize,
    pub updated: usize,
    pub removed: usize,
    pub unchanged: usize,
    /// Files and folders that could not be read; their previous index state is kept for
    /// unreadable folders and dropped for unreadable files.
    pub failed: Vec<ScanFailure>,
    pub elapsed: Duration,
}

impl ScanReport {
    /// Whether the index changed.
    pub fn changed(&self) -> bool {
        self.added + self.updated + self.removed > 0
    }
}

/// How far a running scan is, reported while it runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanProgress {
    /// Listing files under the scanned folders.
    Walking,
    /// Reading tags and cover art of new and changed files.
    Reading { done: usize, total: usize },
    /// Writing the changes to the index.
    Writing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanFailure {
    pub path: PathBuf,
    pub reason: String,
}

#[derive(Debug)]
struct FoundFile {
    path: PathBuf,
    key: String,
    mtime: i64,
    size: i64,
}

#[derive(Debug, Clone, Copy)]
struct Indexed {
    id: i64,
    mtime: i64,
    size: i64,
    album_id: i64,
}

/// A path to reconcile and the root it belongs to.
struct Scope<'a> {
    root: &'a Root,
    path: PathBuf,
}

#[derive(Default)]
struct WalkOutput {
    files: Vec<FoundFile>,
    failures: Vec<ScanFailure>,
    /// Folders whose content is unknown; indexed tracks under them are kept.
    unreadable: Vec<PathBuf>,
}

impl WalkOutput {
    fn merge(mut self, other: WalkOutput) -> WalkOutput {
        self.files.extend(other.files);
        self.failures.extend(other.failures);
        self.unreadable.extend(other.unreadable);
        self
    }

    fn fail(&mut self, path: &Path, reason: impl ToString, unreadable: bool) {
        self.failures.push(ScanFailure {
            path: path.to_path_buf(),
            reason: reason.to_string(),
        });
        if unreadable {
            self.unreadable.push(path.to_path_buf());
        }
    }
}

struct Pending {
    root_id: i64,
    file: FoundFile,
    indexed: Option<Indexed>,
}

/// Tags and resolved cover of a queued file, or why it could not be read.
type ReadResult = Result<(FileMeta, Option<ArtImage>), TagError>;

impl Library {
    /// Rescans every root.
    pub fn scan(&mut self) -> Result<ScanReport, LibraryError> {
        self.scan_with_progress(&|_| {})
    }

    /// Rescans every root, calling `progress` (from any thread) as the scan advances.
    pub fn scan_with_progress(
        &mut self,
        progress: &(dyn Fn(ScanProgress) + Sync),
    ) -> Result<ScanReport, LibraryError> {
        let paths: Vec<PathBuf> = self.roots()?.into_iter().map(|r| r.path).collect();
        self.scan_paths_with_progress(&paths, progress)
    }

    /// Rescans the given files and folders (and everything below the folders). Paths that no
    /// longer exist remove what was indexed at or below them; paths outside every root are
    /// ignored.
    pub fn scan_paths(&mut self, paths: &[PathBuf]) -> Result<ScanReport, LibraryError> {
        self.scan_paths_with_progress(paths, &|_| {})
    }

    /// [`scan_paths`](Self::scan_paths), calling `progress` (from any thread) as it advances.
    pub fn scan_paths_with_progress(
        &mut self,
        paths: &[PathBuf],
        progress: &(dyn Fn(ScanProgress) + Sync),
    ) -> Result<ScanReport, LibraryError> {
        let start = Instant::now();
        progress(ScanProgress::Walking);
        let roots = self.roots()?;
        let mut report = ScanReport::default();
        let scopes = scopes(&roots, paths, &mut report);

        // 1. Walk.
        let walked: Vec<WalkOutput> = scopes
            .par_iter()
            .map(|s| {
                let follow = s.root.follow_symlinks;
                // The root itself is the user's choice even when it is a link; anything
                // linked inside it is followed only on opt-in.
                walk(&s.path, follow || s.path == s.root.path, follow)
            })
            .collect();

        // 2. Diff.
        let mut pending: Vec<Pending> = Vec::new();
        let mut removed: Vec<Indexed> = Vec::new();
        for (scope, output) in scopes.iter().zip(walked) {
            report.failed.extend(output.failures);
            let mut indexed = self.indexed_under(&scope.path)?;
            for file in output.files {
                match indexed.remove(&file.key) {
                    Some(old) if old.mtime == file.mtime && old.size == file.size => {
                        report.unchanged += 1;
                    }
                    old => pending.push(Pending {
                        root_id: scope.root.id,
                        file,
                        indexed: old,
                    }),
                }
            }
            removed.extend(
                indexed
                    .into_iter()
                    .filter(|(path, _)| {
                        !output
                            .unreadable
                            .iter()
                            .any(|dir| Path::new(path).starts_with(dir))
                    })
                    .map(|(_, old)| old),
            );
        }

        // 3. Read tags and art.
        let resolver = ArtResolver::new(&self.art, self.known_art()?);
        let total = pending.len();
        let done = AtomicUsize::new(0);
        progress(ScanProgress::Reading { done: 0, total });
        let read: Vec<(Pending, ReadResult)> = pending
            .into_par_iter()
            .map(|p| {
                let _done = CountDone {
                    done: &done,
                    total,
                    progress,
                };
                let result = read_file(&p.file.path).map(|mut meta| {
                    let cover = meta.tags.cover.take();
                    let art = resolver.resolve(cover.as_deref(), &p.file.path);
                    (meta, art)
                });
                (p, result)
            })
            .collect();
        drop(resolver);

        // 4. Write (nothing to do when every file was unchanged).
        if read.is_empty() && removed.is_empty() {
            report.elapsed = start.elapsed();
            return Ok(report);
        }
        progress(ScanProgress::Writing);
        let now = unix_now();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut touched: BTreeSet<i64> = BTreeSet::new();
        for old in &removed {
            tx.execute("DELETE FROM tracks WHERE id = ?1", [old.id])?;
            touched.insert(old.album_id);
        }
        report.removed += removed.len();
        for (p, result) in read {
            match result {
                Ok((meta, art)) => {
                    let album_id = write_track(&tx, &p, &meta, art.as_ref(), now)?;
                    touched.insert(album_id);
                    match p.indexed {
                        Some(old) => {
                            touched.insert(old.album_id);
                            report.updated += 1;
                        }
                        None => report.added += 1,
                    }
                }
                Err(err) => {
                    report.failed.push(ScanFailure {
                        path: p.file.path.clone(),
                        reason: err.to_string(),
                    });
                    if let Some(old) = p.indexed {
                        tx.execute("DELETE FROM tracks WHERE id = ?1", [old.id])?;
                        touched.insert(old.album_id);
                        report.removed += 1;
                    }
                }
            }
        }
        // A changed album holds a different mix of tracks: drop its album loudness so the next
        // analysis pass rebuilds it from the tracks it has now.
        crate::loudness::invalidate_album_loudness(&tx, &touched)?;
        refresh_albums(&tx, touched)?;
        let orphan_art = delete_orphans(&tx)?;
        tx.commit()?;
        for hash in orphan_art {
            self.art.remove(&hash);
        }

        report.elapsed = start.elapsed();
        Ok(report)
    }

    /// Re-reads one indexed file and updates its row, even when its size and modification time
    /// look unchanged: a tag write can leave both as they were. Returns the track as it is now
    /// indexed, or `None` when the file is gone or no longer readable.
    ///
    /// Loudness measured for the audio is kept, because the audio did not change; a track that
    /// moved to another album leaves its album-level values to a later analysis.
    pub fn reindex_track(&mut self, id: i64) -> Result<Option<Track>, LibraryError> {
        let Some((path, root_id, album_id, mtime, size)) = self
            .conn
            .query_row(
                "SELECT path, root_id, album_id, mtime, size FROM tracks WHERE id = ?1",
                [id],
                |row| {
                    Ok((
                        PathBuf::from(row.get::<_, String>(0)?),
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, i64>(4)?,
                    ))
                },
            )
            .optional()?
        else {
            return Ok(None);
        };
        let Some((new_mtime, new_size)) = fs::metadata(&path).ok().as_ref().map(stamp) else {
            return Ok(None);
        };
        let mut meta = read_file(&path)?;
        let cover = meta.tags.cover.take();
        let art = ArtResolver::new(&self.art, self.known_art()?).resolve(cover.as_deref(), &path);
        let loudness: (Option<f64>, Option<f64>, Option<f64>, Option<f64>) = self
            .conn
            .prepare_cached(
                "SELECT loudness_lufs, loudness_peak, album_lufs, album_peak FROM tracks
                 WHERE id = ?1",
            )?
            .query_row([id], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })?;

        let pending = Pending {
            root_id,
            file: FoundFile {
                key: path_str(&path)?.to_string(),
                path,
                mtime: new_mtime,
                size: new_size,
            },
            indexed: Some(Indexed {
                id,
                mtime,
                size,
                album_id,
            }),
        };
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let new_album = write_track(&tx, &pending, &meta, art.as_ref(), unix_now())?;
        // The decode did not change, so the track's own measurements still hold. Album values
        // describe the album's track set, which only stays valid when the album did.
        let moved = new_album != album_id;
        tx.prepare_cached(
            "UPDATE tracks SET loudness_lufs = ?1, loudness_peak = ?2, album_lufs = ?3,
             album_peak = ?4 WHERE id = ?5",
        )?
        .execute(params![
            loudness.0,
            loudness.1,
            if moved { None } else { loudness.2 },
            if moved { None } else { loudness.3 },
            id
        ])?;
        if moved {
            crate::loudness::invalidate_album_loudness(
                &tx,
                &BTreeSet::from([album_id, new_album]),
            )?;
        }
        refresh_albums(&tx, [album_id, new_album])?;
        let orphan_art = delete_orphans(&tx)?;
        tx.commit()?;
        for hash in orphan_art {
            self.art.remove(&hash);
        }
        self.track(id)
    }

    /// Indexed tracks at `path` or below it, keyed by path.
    fn indexed_under(&self, path: &Path) -> Result<HashMap<String, Indexed>, LibraryError> {
        let exact = path_str(path)?;
        let (lo, hi) = prefix_range(exact);
        let mut stmt = self.conn.prepare_cached(
            "SELECT path, id, mtime, size, album_id FROM tracks
             WHERE path = ?1 OR (path > ?2 AND path < ?3)",
        )?;
        let rows = stmt.query_map(params![exact, lo, hi], |row| {
            Ok((
                row.get::<_, String>(0)?,
                Indexed {
                    id: row.get(1)?,
                    mtime: row.get(2)?,
                    size: row.get(3)?,
                    album_id: row.get(4)?,
                },
            ))
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    fn known_art(&self) -> Result<HashMap<String, ArtImage>, LibraryError> {
        let mut stmt = self
            .conn
            .prepare("SELECT hash, width, height, colors FROM art")?;
        let rows = stmt.query_map([], |row| {
            let hash: String = row.get(0)?;
            Ok((
                hash.clone(),
                ArtImage {
                    hash,
                    width: row.get(1)?,
                    height: row.get(2)?,
                    colors: row.get(3)?,
                },
            ))
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }
}

/// Counts a read file as done when dropped, after its tags and art were handled.
struct CountDone<'a> {
    done: &'a AtomicUsize,
    total: usize,
    progress: &'a (dyn Fn(ScanProgress) + Sync),
}

impl Drop for CountDone<'_> {
    fn drop(&mut self) {
        let done = self.done.fetch_add(1, Ordering::Relaxed) + 1;
        (self.progress)(ScanProgress::Reading {
            done,
            total: self.total,
        });
    }
}

/// Maps each path to its root, drops paths outside every root and paths inside another
/// requested path, and skips roots that are not reachable.
fn scopes<'a>(roots: &'a [Root], paths: &[PathBuf], report: &mut ScanReport) -> Vec<Scope<'a>> {
    let mut sorted: Vec<PathBuf> = paths
        .iter()
        .filter_map(|p| std::path::absolute(p).ok())
        .collect();
    sorted.sort();
    sorted.dedup();

    let mut scopes: Vec<Scope> = Vec::new();
    let mut offline: Vec<&Root> = Vec::new();
    for path in sorted {
        if scopes.iter().any(|s| path.starts_with(&s.path)) {
            continue;
        }
        let Some(root) = roots.iter().find(|r| path.starts_with(&r.path)) else {
            continue;
        };
        if offline.iter().any(|r| r.id == root.id) {
            continue;
        }
        if !root.path.is_dir() {
            report.failed.push(ScanFailure {
                path: root.path.clone(),
                reason: "library folder is not available; its tracks are kept".into(),
            });
            offline.push(root);
            continue;
        }
        scopes.push(Scope { root, path });
    }
    scopes
}

/// `(lo, hi)` such that `lo < p < hi` holds exactly for paths strictly below `dir`.
fn prefix_range(dir: &str) -> (String, String) {
    let base = dir.strip_suffix(MAIN_SEPARATOR).unwrap_or(dir);
    let after = char::from_u32(MAIN_SEPARATOR as u32 + 1).expect("separator is ASCII");
    (format!("{base}{MAIN_SEPARATOR}"), format!("{base}{after}"))
}

/// Lists audio files at or below `path`, walking its subfolders in parallel. `follow_top`
/// applies to `path` itself, `follow_links` to every link below it.
fn walk(path: &Path, follow_top: bool, follow_links: bool) -> WalkOutput {
    let mut out = WalkOutput::default();
    let meta = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == ErrorKind::NotFound => return out,
        Err(e) => {
            out.fail(path, e, true);
            return out;
        }
    };
    let meta = if meta.file_type().is_symlink() {
        if !follow_top {
            return out;
        }
        match fs::metadata(path) {
            Ok(m) => m,
            Err(e) => {
                out.fail(path, e, true);
                return out;
            }
        }
    } else {
        meta
    };
    if meta.is_file() {
        add_file(&mut out, path, &meta);
        return out;
    }
    if !meta.is_dir() {
        return out;
    }
    let children: Vec<fs::DirEntry> = match fs::read_dir(path) {
        Ok(entries) => entries.flatten().collect(),
        Err(e) => {
            out.fail(path, e, true);
            return out;
        }
    };
    children
        .into_par_iter()
        .map(|child| walk_tree(&child.path(), follow_links))
        .reduce(WalkOutput::default, WalkOutput::merge)
        .merge(out)
}

fn walk_tree(path: &Path, follow_links: bool) -> WalkOutput {
    let mut out = WalkOutput::default();
    // `path` is a child of the walked folder, so a link there is no more trusted than one
    // deeper down (walkdir follows a link passed as its root unless told otherwise).
    let walker = WalkDir::new(path)
        .follow_links(follow_links)
        .follow_root_links(follow_links);
    for entry in walker {
        match entry {
            Ok(entry) => {
                if !entry.file_type().is_file() {
                    continue;
                }
                match entry.metadata() {
                    Ok(meta) => add_file(&mut out, entry.path(), &meta),
                    Err(e) => out.fail(entry.path(), e, false),
                }
            }
            Err(e) => {
                let at = e.path().unwrap_or(path).to_path_buf();
                // A loop or unreadable folder: keep what is indexed below it.
                out.fail(&at, e, true);
            }
        }
    }
    out
}

/// `(modification time, size)` as the index stores them.
fn stamp(meta: &fs::Metadata) -> (i64, i64) {
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos() as i64);
    (mtime, meta.len() as i64)
}

fn add_file(out: &mut WalkOutput, path: &Path, meta: &fs::Metadata) {
    if !is_audio_file(path) {
        return;
    }
    // macOS AppleDouble companions ("._song.flac") are not audio.
    if path
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with("._"))
    {
        return;
    }
    let Some(key) = path.to_str() else {
        out.fail(path, "path is not valid UTF-8", false);
        return;
    };
    let (mtime, size) = stamp(meta);
    out.files.push(FoundFile {
        path: path.to_path_buf(),
        key: key.to_string(),
        mtime,
        size,
    });
}

fn joined(values: &[String]) -> Option<String> {
    (!values.is_empty()).then(|| values.join("; "))
}

/// Inserts or updates one track and returns its album id.
fn write_track(
    tx: &Transaction,
    p: &Pending,
    meta: &FileMeta,
    art: Option<&ArtImage>,
    now: i64,
) -> Result<i64, LibraryError> {
    let tags = &meta.tags;
    let audio = &meta.audio;
    let artist = joined(&tags.artists);
    let album_artist = joined(&tags.album_artists);
    let identity = album_identity(
        tags.album.as_deref(),
        album_artist.as_deref(),
        tags.mb_album_id.as_deref(),
        &p.file.path,
    );
    let album_id: i64 = tx
        .prepare_cached(
            "INSERT INTO albums (group_key, title, mb_album_id, added_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (group_key) DO UPDATE SET group_key = excluded.group_key
             RETURNING id",
        )?
        .query_row(
            params![identity.key, identity.title, tags.mb_album_id, now],
            |r| r.get(0),
        )?;
    let art_id: Option<i64> = match art {
        Some(a) => Some(
            tx.prepare_cached(
                "INSERT INTO art (hash, width, height, colors) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (hash) DO UPDATE SET colors = coalesce(excluded.colors, art.colors)
                 RETURNING id",
            )?
            .query_row(params![a.hash, a.width, a.height, a.colors], |r| r.get(0))?,
        ),
        None => None,
    };
    let title = tags.title.clone().unwrap_or_default();
    let rg = &tags.replay_gain;
    let values = params![
        p.root_id,
        p.file.key,
        p.file.mtime,
        p.file.size,
        album_id,
        title,
        artist,
        album_artist,
        tags.compilation,
        tags.track_no,
        tags.disc_no,
        tags.year,
        tags.genre,
        audio.codec.as_str(),
        audio.rate,
        audio.bits,
        audio.channels,
        audio.duration_ms as i64,
        rg.track_gain,
        rg.track_peak,
        rg.album_gain,
        rg.album_peak,
        art_id,
        now,
    ];
    let track_id: i64 =
        match p.indexed {
            Some(old) => {
                tx.prepare_cached(
                "UPDATE tracks SET root_id = ?1, path = ?2, mtime = ?3, size = ?4, album_id = ?5,
                 title = ?6, artist = ?7, album_artist = ?8, compilation = ?9, track_no = ?10,
                 disc_no = ?11, year = ?12, genre = ?13, codec = ?14, rate = ?15, bits = ?16,
                 channels = ?17, duration_ms = ?18, rg_track_gain = ?19, rg_track_peak = ?20,
                 rg_album_gain = ?21, rg_album_peak = ?22, art_id = ?23,
                 loudness_lufs = NULL, loudness_peak = NULL, album_lufs = NULL, album_peak = NULL
                 WHERE id = ?24",
            )?
            .execute(rusqlite::params_from_iter(
                // Same columns as the insert; added_at is kept and the id selects the row.
                values[..23].iter().copied().chain([&old.id as &dyn rusqlite::ToSql]),
            ))?;
                tx.prepare_cached("DELETE FROM track_artists WHERE track_id = ?1")?
                    .execute([old.id])?;
                tx.prepare_cached("DELETE FROM track_search WHERE rowid = ?1")?
                    .execute([old.id])?;
                old.id
            }
            None => tx
                .prepare_cached(
                    "INSERT INTO tracks (root_id, path, mtime, size, album_id, title, artist,
                 album_artist, compilation, track_no, disc_no, year, genre, codec, rate, bits,
                 channels, duration_ms, rg_track_gain, rg_track_peak, rg_album_gain,
                 rg_album_peak, art_id, added_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16,
                 ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24)
                 RETURNING id",
                )?
                .query_row(values, |r| r.get(0))?,
        };

    for (role, names) in [(0, &tags.artists), (1, &tags.album_artists)] {
        for (position, name) in names.iter().enumerate() {
            let artist_id = artist_id(tx, name)?;
            tx.prepare_cached(
                "INSERT OR IGNORE INTO track_artists (track_id, artist_id, role, position)
                 VALUES (?1, ?2, ?3, ?4)",
            )?
            .execute(params![track_id, artist_id, role, position as i64])?;
        }
    }

    let search_artists = tags
        .artists
        .iter()
        .chain(&tags.album_artists)
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(" ");
    tx.prepare_cached(
        "INSERT INTO track_search (rowid, title, album, artist) VALUES (?1, ?2, ?3, ?4)",
    )?
    .execute(params![track_id, title, identity.title, search_artists])?;
    Ok(album_id)
}

fn artist_id(tx: &Transaction, name: &str) -> Result<i64, LibraryError> {
    Ok(tx
        .prepare_cached(
            "INSERT INTO artists (name) VALUES (?1)
             ON CONFLICT (name) DO UPDATE SET name = artists.name
             RETURNING id",
        )?
        .query_row([name], |r| r.get(0))?)
}

/// Recomputes album artist, year and cover from the album's tracks; deletes empty albums.
pub(crate) fn refresh_albums(
    tx: &Transaction,
    albums: impl IntoIterator<Item = i64>,
) -> Result<(), LibraryError> {
    for album_id in albums {
        let tracks: i64 = tx
            .prepare_cached("SELECT count(*) FROM tracks WHERE album_id = ?1")?
            .query_row([album_id], |r| r.get(0))?;
        if tracks == 0 {
            tx.prepare_cached("DELETE FROM albums WHERE id = ?1")?
                .execute([album_id])?;
            continue;
        }

        // Most common tagged album artist; else the single track artist; else "Various".
        let tagged: Option<String> = tx
            .prepare_cached(
                "SELECT album_artist FROM tracks WHERE album_id = ?1 AND album_artist IS NOT NULL
                 GROUP BY album_artist ORDER BY count(*) DESC, album_artist LIMIT 1",
            )?
            .query_row([album_id], |r| r.get(0))
            .or_else(none_if_no_rows)?;
        let display = match tagged {
            Some(name) => Some(name),
            None => {
                let artists: Vec<String> = tx
                    .prepare_cached(
                        "SELECT DISTINCT artist FROM tracks
                         WHERE album_id = ?1 AND artist IS NOT NULL LIMIT 2",
                    )?
                    .query_map([album_id], |r| r.get(0))?
                    .collect::<Result<_, _>>()?;
                let compilation: bool = tx
                    .prepare_cached(
                        "SELECT EXISTS (SELECT 1 FROM tracks WHERE album_id = ?1 AND compilation)",
                    )?
                    .query_row([album_id], |r| r.get(0))?;
                if compilation || artists.len() > 1 {
                    Some(VARIOUS_ARTISTS.to_string())
                } else {
                    artists.into_iter().next()
                }
            }
        };
        let primary = match &display {
            Some(name) => {
                let first = name.split("; ").next().unwrap_or(name);
                Some(artist_id(tx, first)?)
            }
            None => None,
        };
        tx.prepare_cached(
            "UPDATE albums SET
                artist = ?2,
                artist_id = ?3,
                year = (SELECT min(year) FROM tracks WHERE album_id = ?1),
                art_id = (SELECT art_id FROM tracks WHERE album_id = ?1 AND art_id IS NOT NULL
                          ORDER BY coalesce(disc_no, 0), coalesce(track_no, 0), path LIMIT 1)
             WHERE id = ?1",
        )?
        .execute(params![album_id, display, primary])?;
    }
    Ok(())
}

fn none_if_no_rows<T>(err: rusqlite::Error) -> Result<Option<T>, rusqlite::Error> {
    match err {
        rusqlite::Error::QueryReturnedNoRows => Ok(None),
        other => Err(other),
    }
}

/// Deletes albums without tracks, artists nothing refers to and unused art rows. Returns the
/// hashes of deleted art so the caller can drop their thumbnails after commit.
pub(crate) fn delete_orphans(tx: &Transaction) -> Result<Vec<String>, LibraryError> {
    tx.execute_batch(
        "DELETE FROM albums WHERE id NOT IN (SELECT album_id FROM tracks);
         DELETE FROM artists
          WHERE id NOT IN (SELECT artist_id FROM track_artists)
            AND id NOT IN (SELECT artist_id FROM albums WHERE artist_id IS NOT NULL);",
    )?;
    let hashes: Vec<String> = tx
        .prepare(
            "DELETE FROM art
              WHERE id NOT IN (SELECT art_id FROM tracks WHERE art_id IS NOT NULL)
                AND id NOT IN (SELECT art_id FROM albums WHERE art_id IS NOT NULL)
             RETURNING hash",
        )?
        .query_map([], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    Ok(hashes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AlbumSort;

    #[test]
    fn prefix_range_selects_only_descendants() {
        let sep = MAIN_SEPARATOR;
        let dir = format!("{sep}m{sep}a");
        let (lo, hi) = prefix_range(&dir);
        let inside = format!("{dir}{sep}x.flac");
        let deep = format!("{dir}{sep}b{sep}x.flac");
        let sibling = format!("{dir}b{sep}x.flac");
        let itself = dir.clone();
        let between = |p: &str| lo.as_str() < p && p < hi.as_str();
        assert!(between(&inside));
        assert!(between(&deep));
        assert!(!between(&sibling));
        assert!(!between(&itself));
        assert_eq!(prefix_range(&format!("{dir}{sep}")), (lo, hi));
    }

    fn wav(path: &Path) {
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 44_100,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(path, spec).unwrap();
        for i in 0..882 {
            writer.write_sample((i % 64) as i16).unwrap();
        }
        writer.finalize().unwrap();
    }

    #[test]
    fn reindex_picks_up_written_tags_and_moves_the_track_to_its_new_album() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("music");
        fs::create_dir_all(&root).unwrap();
        let path = root.join("01 - So What.wav");
        wav(&path);

        let mut library =
            Library::open(dir.path().join("library.db"), dir.path().join("art")).unwrap();
        library.add_root(&root, false).unwrap();
        library.scan().unwrap();
        let track = library.tracks().unwrap().into_iter().next().unwrap();
        let old_album = track.album_id;

        let edit = crate::tags::TagEdit {
            title: "So What".into(),
            artists: vec!["Miles Davis".into()],
            album: "Kind of Blue".into(),
            album_artists: vec!["Miles Davis".into()],
            track_no: Some(1),
            disc_no: Some(1),
            year: Some(1959),
            genre: "Jazz".into(),
        };
        crate::tags::write_file(&path, &edit).unwrap();
        let updated = library
            .reindex_track(track.id)
            .unwrap()
            .expect("still indexed");

        assert_eq!(updated.title, "So What");
        assert_eq!(updated.artist.as_deref(), Some("Miles Davis"));
        assert_eq!(updated.album, "Kind of Blue");
        assert_eq!(updated.year, Some(1959));
        assert_ne!(updated.album_id, old_album);
        assert_eq!(library.albums(AlbumSort::Artist).unwrap().len(), 1);
        assert!(library.album(old_album).unwrap().is_none());
    }
}
