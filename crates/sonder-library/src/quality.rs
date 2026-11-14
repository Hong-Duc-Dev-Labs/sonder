//! Cached lossless quality check results.
//!
//! The library stores what the analyzer (`sonder-quality`) reports without interpreting it:
//! a verdict and confidence word for track lists, and the report as JSON for reopening it.
//! A result is tied to the file's (mtime, size) when the check started; a changed file no
//! longer matches and reads as unchecked. Results from an older analyzer are removed with
//! [`Library::remove_quality_before`].

use std::path::{Path, PathBuf};

use rusqlite::{OptionalExtension, params};

use crate::{Library, LibraryError, unix_now};

/// The file a check is about, as the library knew it when the check started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QualitySubject {
    pub track_id: i64,
    pub path: PathBuf,
    pub mtime: i64,
    pub size: i64,
}

/// What a check concluded, as the analyzer words it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QualityResult {
    pub analyzer_version: u32,
    pub verdict: String,
    pub confidence: String,
    pub report_json: String,
}

impl Library {
    /// The file behind `track_id`, or `None` when the track is gone.
    pub fn quality_subject(&self, track_id: i64) -> Result<Option<QualitySubject>, LibraryError> {
        Ok(self
            .conn
            .query_row(
                "SELECT path, mtime, size FROM tracks WHERE id = ?1",
                params![track_id],
                |row| {
                    Ok(QualitySubject {
                        track_id,
                        path: PathBuf::from(row.get::<_, String>(0)?),
                        mtime: row.get(1)?,
                        size: row.get(2)?,
                    })
                },
            )
            .optional()?)
    }

    /// The library track stored at `path`, if the file is in the library.
    pub fn track_id_at(&self, path: &Path) -> Result<Option<i64>, LibraryError> {
        let Some(key) = path.to_str() else {
            return Ok(None);
        };
        Ok(self
            .conn
            .query_row("SELECT id FROM tracks WHERE path = ?1", [key], |row| {
                row.get(0)
            })
            .optional()?)
    }

    /// Stores the result of checking `subject`, replacing any earlier one for that path.
    pub fn save_quality(
        &mut self,
        subject: &QualitySubject,
        result: &QualityResult,
    ) -> Result<(), LibraryError> {
        self.conn.execute(
            "INSERT INTO quality_checks
                 (path, mtime, size, analyzer_version, verdict, confidence, report_json, checked_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT (path) DO UPDATE SET
                 mtime = excluded.mtime, size = excluded.size,
                 analyzer_version = excluded.analyzer_version, verdict = excluded.verdict,
                 confidence = excluded.confidence, report_json = excluded.report_json,
                 checked_at = excluded.checked_at",
            params![
                subject.path.to_string_lossy(),
                subject.mtime,
                subject.size,
                result.analyzer_version,
                result.verdict,
                result.confidence,
                result.report_json,
                unix_now(),
            ],
        )?;
        Ok(())
    }

    /// The cached result for `track_id`, if its file has not changed since the check.
    pub fn quality(&self, track_id: i64) -> Result<Option<QualityResult>, LibraryError> {
        Ok(self
            .conn
            .query_row(
                "SELECT q.analyzer_version, q.verdict, q.confidence, q.report_json
                 FROM tracks t
                 JOIN quality_checks q ON q.path = t.path AND q.mtime = t.mtime AND q.size = t.size
                 WHERE t.id = ?1",
                params![track_id],
                |row| {
                    Ok(QualityResult {
                        analyzer_version: row.get(0)?,
                        verdict: row.get(1)?,
                        confidence: row.get(2)?,
                        report_json: row.get(3)?,
                    })
                },
            )
            .optional()?)
    }

    /// Removes results an analyzer older than `version` produced; returns how many.
    pub fn remove_quality_before(&mut self, version: u32) -> Result<usize, LibraryError> {
        Ok(self.conn.execute(
            "DELETE FROM quality_checks WHERE analyzer_version < ?1",
            params![version],
        )?)
    }
}
