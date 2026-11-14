//! Listening history: the play tracker fed by engine events, and the Stats page's commands.
//!
//! A play counts once the track has actually sounded for 30 s or half its length, whichever is
//! shorter. "Actually sounded" is the sum of small forward steps between position reports
//! while playing, so a seek (a jump), a pause or a skip adds nothing.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sonder_engine::{PlaybackState, PlayerEvent};
use sonder_library::{ListeningStats, RecentPlay, StatsPeriod};
use specta::Type;
use tauri::{AppHandle, Manager, State};
use tauri_specta::Event;

use crate::dto::{AppResult, TrackItem};
use crate::state::{AppState, lock};

/// Heard time that always counts as a play.
const PLAY_THRESHOLD_MS: u64 = 30_000;
/// Position steps longer than this are seeks, not listening (reports come every ~100 ms).
const MAX_STEP_MS: u64 = 1_500;
/// Entries in each top list.
const TOP_LIMIT: u32 = 10;

/// A play the tracker decided to keep.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Play {
    pub path: PathBuf,
    /// Unix seconds when the track started.
    pub played_at: i64,
    pub listened_ms: u64,
}

#[derive(Debug)]
struct Current {
    path: PathBuf,
    started_at: i64,
    duration_ms: Option<u64>,
    heard_ms: u64,
    last_elapsed_ms: Option<u64>,
}

impl Current {
    fn into_play(self) -> Option<Play> {
        let needed = self
            .duration_ms
            .map_or(PLAY_THRESHOLD_MS, |d| PLAY_THRESHOLD_MS.min(d / 2));
        (self.heard_ms > 0 && self.heard_ms >= needed).then_some(Play {
            path: self.path,
            played_at: self.started_at,
            listened_ms: self.heard_ms,
        })
    }
}

/// Follows the engine's events and yields a [`Play`] when a track that was heard enough ends.
#[derive(Debug, Default)]
pub struct PlayTracker {
    current: Option<Current>,
    playing: bool,
}

impl PlayTracker {
    /// Folds one engine event in; `now` is Unix seconds.
    pub fn on_event(&mut self, event: &PlayerEvent, now: i64) -> Option<Play> {
        match event {
            PlayerEvent::TrackChanged { path, duration, .. } => {
                let finished = self.finish();
                self.current = Some(Current {
                    path: path.clone(),
                    started_at: now,
                    duration_ms: duration.map(|d| d.as_millis() as u64),
                    heard_ms: 0,
                    last_elapsed_ms: None,
                });
                finished
            }
            PlayerEvent::Position { elapsed, .. } => {
                let elapsed = elapsed.as_millis() as u64;
                if let Some(current) = &mut self.current {
                    if self.playing
                        && let Some(last) = current.last_elapsed_ms
                        && elapsed > last
                        && elapsed - last <= MAX_STEP_MS
                    {
                        current.heard_ms += elapsed - last;
                    }
                    current.last_elapsed_ms = Some(elapsed);
                }
                None
            }
            PlayerEvent::State(state) => {
                self.playing = *state == PlaybackState::Playing;
                if *state == PlaybackState::Stopped {
                    self.finish()
                } else {
                    None
                }
            }
            PlayerEvent::EndOfQueue => self.finish(),
            _ => None,
        }
    }

    fn finish(&mut self) -> Option<Play> {
        self.current.take().and_then(Current::into_play)
    }
}

/// Unix seconds now.
pub fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// A play was recorded: the Stats page reloads.
#[derive(Debug, Clone, Serialize, Type, Event)]
pub struct HistoryChangedEvent;

/// Stores a play from the event bridge and tells the UI.
pub fn record(app: &AppHandle, play: Play) {
    let state = app.state::<AppState>();
    let stored = lock(&state.library).record_play(&play.path, play.played_at, play.listened_ms);
    match stored {
        Ok(()) => {
            let _ = HistoryChangedEvent.emit(app);
        }
        Err(err) => eprintln!("cannot record a play of {}: {err}", play.path.display()),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum StatsPeriodSetting {
    Week,
    Month,
    Year,
    All,
}

impl From<StatsPeriodSetting> for StatsPeriod {
    fn from(p: StatsPeriodSetting) -> Self {
        match p {
            StatsPeriodSetting::Week => StatsPeriod::Week,
            StatsPeriodSetting::Month => StatsPeriod::Month,
            StatsPeriodSetting::Year => StatsPeriod::Year,
            StatsPeriodSetting::All => StatsPeriod::All,
        }
    }
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TopTrackItem {
    pub track: TrackItem,
    pub plays: u32,
    pub listened_ms: u64,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TopArtistItem {
    pub id: i64,
    pub name: String,
    pub art: Option<String>,
    pub plays: u32,
    pub listened_ms: u64,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TopAlbumItem {
    pub id: i64,
    pub title: String,
    pub artist: Option<String>,
    pub art: Option<String>,
    pub plays: u32,
    pub listened_ms: u64,
}

/// One bar of the chart: a local day (`YYYY-MM-DD`) or month (`YYYY-MM`).
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct StatsBar {
    pub label: String,
    pub listened_ms: u64,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ListeningStatsInfo {
    pub plays: u32,
    pub listened_ms: u64,
    pub tracks: u32,
    pub top_tracks: Vec<TopTrackItem>,
    pub top_artists: Vec<TopArtistItem>,
    pub top_albums: Vec<TopAlbumItem>,
    pub bars: Vec<StatsBar>,
    pub recent: Vec<RecentPlayItem>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RecentPlayItem {
    pub track: TrackItem,
    /// Unix seconds.
    pub played_at: i64,
}

impl From<RecentPlay> for RecentPlayItem {
    fn from(r: RecentPlay) -> Self {
        Self {
            track: r.track.into(),
            played_at: r.played_at,
        }
    }
}

fn info(stats: ListeningStats, recent: Vec<RecentPlay>) -> ListeningStatsInfo {
    ListeningStatsInfo {
        plays: stats.plays,
        listened_ms: stats.listened_ms,
        tracks: stats.tracks,
        top_tracks: stats
            .top_tracks
            .into_iter()
            .map(|t| TopTrackItem {
                track: t.track.into(),
                plays: t.plays,
                listened_ms: t.listened_ms,
            })
            .collect(),
        top_artists: stats
            .top_artists
            .into_iter()
            .map(|a| TopArtistItem {
                id: a.id,
                name: a.name,
                art: a.art,
                plays: a.plays,
                listened_ms: a.listened_ms,
            })
            .collect(),
        top_albums: stats
            .top_albums
            .into_iter()
            .map(|a| TopAlbumItem {
                id: a.id,
                title: a.title,
                artist: a.artist,
                art: a.art,
                plays: a.plays,
                listened_ms: a.listened_ms,
            })
            .collect(),
        bars: stats
            .buckets
            .into_iter()
            .map(|b| StatsBar {
                label: b.label,
                listened_ms: b.listened_ms,
            })
            .collect(),
        recent: recent.into_iter().map(Into::into).collect(),
    }
}

/// Listening stats for `period`, plus the most recent plays.
#[tauri::command]
#[specta::specta]
pub async fn history_stats(
    state: State<'_, AppState>,
    period: StatsPeriodSetting,
) -> AppResult<ListeningStatsInfo> {
    let library = lock(&state.library);
    let stats = library.listening_stats(period.into(), unix_now(), TOP_LIMIT)?;
    let recent = library.recent_plays(20)?;
    Ok(info(stats, recent))
}

/// Deletes the whole listening history; returns how many plays were removed.
#[tauri::command]
#[specta::specta]
pub async fn history_clear(app: AppHandle) -> AppResult<u32> {
    let removed = lock(&app.state::<AppState>().library).clear_history()?;
    let _ = HistoryChangedEvent.emit(&app);
    Ok(u32::try_from(removed).unwrap_or(u32::MAX))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use sonder_decode::SourceFormat;

    fn changed(path: &str, secs: u64) -> PlayerEvent {
        PlayerEvent::TrackChanged {
            index: 0,
            path: path.into(),
            source: SourceFormat {
                rate: 44_100,
                bits: 16,
                channels: 2,
            },
            duration: Some(Duration::from_secs(secs)),
        }
    }

    fn at(ms: u64) -> PlayerEvent {
        PlayerEvent::Position {
            index: 0,
            elapsed: Duration::from_millis(ms),
        }
    }

    /// Plays from `from` to `to` ms in 100 ms reports.
    fn play(t: &mut PlayTracker, from: u64, to: u64) -> Vec<Play> {
        (from..=to)
            .step_by(100)
            .filter_map(|ms| t.on_event(&at(ms), 0))
            .collect()
    }

    fn start(t: &mut PlayTracker, path: &str, secs: u64, now: i64) -> Option<Play> {
        let finished = t.on_event(&changed(path, secs), now);
        t.on_event(&PlayerEvent::State(PlaybackState::Playing), now);
        finished
    }

    #[test]
    fn thirty_seconds_of_a_long_track_count() {
        let mut t = PlayTracker::default();
        start(&mut t, "a.flac", 300, 100);
        play(&mut t, 0, 30_000);
        let done = start(&mut t, "b.flac", 300, 200).unwrap();
        assert_eq!(done.path, PathBuf::from("a.flac"));
        assert_eq!(done.played_at, 100);
        assert_eq!(done.listened_ms, 30_000);
    }

    #[test]
    fn a_skip_before_the_threshold_records_nothing() {
        let mut t = PlayTracker::default();
        start(&mut t, "a.flac", 300, 0);
        play(&mut t, 0, 29_000);
        assert_eq!(start(&mut t, "b.flac", 300, 0), None);
    }

    #[test]
    fn half_of_a_short_track_is_enough() {
        let mut t = PlayTracker::default();
        start(&mut t, "jingle.flac", 20, 0);
        play(&mut t, 0, 10_000);
        assert!(t.on_event(&PlayerEvent::EndOfQueue, 0).is_some());
    }

    #[test]
    fn seeks_and_pauses_add_nothing() {
        let mut t = PlayTracker::default();
        start(&mut t, "a.flac", 300, 0);
        play(&mut t, 0, 10_000);
        // Seek forward to 4 min and back to 1 min: jumps, not listening.
        t.on_event(&at(240_000), 0);
        t.on_event(&at(60_000), 0);
        play(&mut t, 60_000, 65_000);
        // Paused: reports (if any) do not count.
        t.on_event(&PlayerEvent::State(PlaybackState::Paused), 0);
        play(&mut t, 65_000, 120_000);
        assert_eq!(
            t.on_event(&PlayerEvent::State(PlaybackState::Stopped), 0),
            None
        );

        let mut t = PlayTracker::default();
        start(&mut t, "a.flac", 300, 0);
        play(&mut t, 0, 20_000);
        t.on_event(&PlayerEvent::State(PlaybackState::Paused), 0);
        t.on_event(&PlayerEvent::State(PlaybackState::Playing), 0);
        play(&mut t, 20_000, 31_000);
        let done = t
            .on_event(&PlayerEvent::State(PlaybackState::Stopped), 0)
            .unwrap();
        assert_eq!(done.listened_ms, 31_000);
    }

    #[test]
    fn a_track_replayed_counts_twice() {
        let mut t = PlayTracker::default();
        start(&mut t, "a.flac", 60, 0);
        play(&mut t, 0, 60_000);
        assert!(start(&mut t, "a.flac", 60, 60).is_some());
        play(&mut t, 0, 60_000);
        assert!(t.on_event(&PlayerEvent::EndOfQueue, 120).is_some());
        assert_eq!(t.on_event(&PlayerEvent::EndOfQueue, 120), None);
    }
}
