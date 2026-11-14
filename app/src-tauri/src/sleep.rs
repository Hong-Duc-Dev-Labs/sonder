//! Sleep timer: pause playback after a set time or when the current track ends.
//!
//! It lives in the backend and is checked by the engine event bridge, so it fires with the
//! main window closed. "End of track" fires when the engine reports that the next track
//! started (gapless playback has no gap to stop in), or when the queue ends.

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use sonder_engine::PlayerEvent;
use specta::Type;
use tauri::{AppHandle, Manager, State};
use tauri_specta::Event;

use crate::dto::{AppError, AppResult};
use crate::state::{AppState, lock};

/// Longest timer accepted.
const MAX_MINUTES: u32 = 24 * 60;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Mode {
    #[default]
    Off,
    /// Fires at `at`; `ends_at_ms` is the same moment as Unix milliseconds for the UI.
    At {
        at: Instant,
        ends_at_ms: u64,
    },
    EndOfTrack,
}

#[derive(Debug, Default)]
pub struct SleepTimer {
    mode: Mode,
}

/// What the timer is set to, for the UI.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct SleepTimerEvent {
    /// When it fires, as Unix milliseconds; `None` when off or waiting for the track's end.
    pub ends_at_ms: Option<u64>,
    pub end_of_track: bool,
    /// Sent once when the timer has just paused playback.
    pub fired: bool,
}

impl SleepTimer {
    /// Fires `minutes` from `now` (`wall` is the same moment as Unix milliseconds).
    fn set_minutes(&mut self, minutes: u32, now: Instant, wall_ms: u64) {
        let after = Duration::from_secs(u64::from(minutes) * 60);
        self.mode = Mode::At {
            at: now + after,
            ends_at_ms: wall_ms + after.as_millis() as u64,
        };
    }

    fn set_end_of_track(&mut self) {
        self.mode = Mode::EndOfTrack;
    }

    fn cancel(&mut self) {
        self.mode = Mode::Off;
    }

    fn state(&self, fired: bool) -> SleepTimerEvent {
        match self.mode {
            Mode::Off => SleepTimerEvent {
                fired,
                ..SleepTimerEvent::default()
            },
            Mode::At { ends_at_ms, .. } => SleepTimerEvent {
                ends_at_ms: Some(ends_at_ms),
                end_of_track: false,
                fired,
            },
            Mode::EndOfTrack => SleepTimerEvent {
                ends_at_ms: None,
                end_of_track: true,
                fired,
            },
        }
    }

    /// Whether the timer fires now, given the clock and (optionally) an engine event. A timer
    /// that fires turns itself off.
    fn check(&mut self, now: Instant, event: Option<&PlayerEvent>) -> bool {
        let due = match self.mode {
            Mode::Off => false,
            Mode::At { at, .. } => now >= at,
            Mode::EndOfTrack => matches!(
                event,
                Some(PlayerEvent::TrackChanged { .. } | PlayerEvent::EndOfQueue)
            ),
        };
        if due {
            self.mode = Mode::Off;
        }
        due
    }
}

fn wall_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// Called by the event bridge for every engine event (`Some`) and on every tick (`None`).
pub fn check(app: &AppHandle, event: Option<&PlayerEvent>) {
    let state = app.state::<AppState>();
    let fired = lock(&state.sleep).check(Instant::now(), event);
    if fired {
        state.player.pause();
        let _ = lock(&state.sleep).state(true).emit(app);
    }
}

fn changed(app: &AppHandle) -> SleepTimerEvent {
    let state = app.state::<AppState>();
    let now = lock(&state.sleep).state(false);
    let _ = now.emit(app);
    now
}

/// Sets the timer: `minutes` from now, or the end of the current track, or off when neither
/// is given.
#[tauri::command]
#[specta::specta]
pub async fn sleep_timer_set(
    app: AppHandle,
    minutes: Option<u32>,
    end_of_track: bool,
) -> AppResult<SleepTimerEvent> {
    {
        let state = app.state::<AppState>();
        let mut timer = lock(&state.sleep);
        match (minutes, end_of_track) {
            (Some(m), false) if (1..=MAX_MINUTES).contains(&m) => {
                timer.set_minutes(m, Instant::now(), wall_ms());
            }
            (Some(_), false) => {
                return Err(AppError(format!(
                    "a sleep timer runs 1 to {MAX_MINUTES} minutes"
                )));
            }
            (None, true) => timer.set_end_of_track(),
            (None, false) => timer.cancel(),
            (Some(_), true) => {
                return Err(AppError("choose a time or the end of the track".into()));
            }
        }
    }
    Ok(changed(&app))
}

/// What the timer is set to.
#[tauri::command]
#[specta::specta]
pub async fn sleep_timer_get(state: State<'_, AppState>) -> AppResult<SleepTimerEvent> {
    Ok(lock(&state.sleep).state(false))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_timed_timer_fires_once_at_its_deadline() {
        let mut timer = SleepTimer::default();
        let now = Instant::now();
        timer.set_minutes(15, now, 1_000);
        assert_eq!(timer.state(false).ends_at_ms, Some(1_000 + 15 * 60_000));
        assert!(!timer.check(now + Duration::from_secs(899), None));
        assert!(!timer.check(now, Some(&PlayerEvent::EndOfQueue)));
        assert!(timer.check(now + Duration::from_secs(900), None));
        assert!(!timer.check(now + Duration::from_secs(901), None));
        assert_eq!(timer.state(false), SleepTimerEvent::default());
    }

    #[test]
    fn end_of_track_fires_on_the_next_track_or_the_end_of_the_queue() {
        let mut timer = SleepTimer::default();
        timer.set_end_of_track();
        let now = Instant::now();
        assert!(!timer.check(now + Duration::from_secs(3600), None));
        let position = PlayerEvent::Position {
            index: 0,
            elapsed: Duration::from_secs(1),
        };
        assert!(!timer.check(now, Some(&position)));
        assert!(timer.check(now, Some(&PlayerEvent::EndOfQueue)));
        assert!(!timer.state(false).end_of_track);
    }

    #[test]
    fn a_cancelled_timer_never_fires() {
        let mut timer = SleepTimer::default();
        let now = Instant::now();
        timer.set_minutes(1, now, 0);
        timer.cancel();
        assert!(!timer.check(now + Duration::from_secs(120), None));
    }
}
