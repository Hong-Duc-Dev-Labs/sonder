//! Media keys and the OS now-playing panel (SMTC on Windows, MPNowPlayingInfoCenter on
//! macOS, MPRIS on Linux) through `souvlaki`.
//!
//! The controls live in a main-thread local: macOS requires the main thread and Windows ties
//! them to the window. Other threads update them through [`update`], which hops onto the main
//! thread.

use std::cell::RefCell;
use std::time::Duration;

use souvlaki::{
    MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig,
    SeekDirection,
};
use tauri::{AppHandle, Manager, WebviewWindow};

use crate::state::AppState;

/// Seek step for media keys that do not say how far.
const SEEK_STEP: Duration = Duration::from_secs(10);

thread_local! {
    static CONTROLS: RefCell<Option<MediaControls>> = const { RefCell::new(None) };
}

/// What the OS panel shows.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NowPlaying {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    /// `file://` URL of the cover thumbnail.
    pub cover_url: Option<String>,
    pub duration: Option<Duration>,
    pub playback: Playback,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum Playback {
    #[default]
    Stopped,
    Playing(Duration),
    Paused(Duration),
}

/// Creates the controls on the main thread and routes their events to the player.
/// Failure only disables media keys; the app keeps running.
pub fn init(app: &AppHandle, window: &WebviewWindow) {
    #[cfg(windows)]
    let hwnd = match window.hwnd() {
        Ok(hwnd) => Some(hwnd.0),
        Err(err) => {
            eprintln!("sonder: media keys disabled, no window handle: {err}");
            return;
        }
    };
    #[cfg(not(windows))]
    let hwnd = {
        let _ = window;
        None
    };
    let config = PlatformConfig {
        display_name: "Sonder",
        dbus_name: "sonder",
        hwnd,
    };
    let mut controls = match MediaControls::new(config) {
        Ok(c) => c,
        Err(err) => {
            eprintln!("sonder: media keys disabled: {err:?}");
            return;
        }
    };
    let handle = app.clone();
    if let Err(err) = controls.attach(move |event| on_event(&handle, event)) {
        eprintln!("sonder: media keys disabled: {err:?}");
        return;
    }
    CONTROLS.with(|c| *c.borrow_mut() = Some(controls));
}

fn on_event(app: &AppHandle, event: MediaControlEvent) {
    let state = app.state::<AppState>();
    let player = &state.player;
    match event {
        MediaControlEvent::Play => player.play(),
        MediaControlEvent::Pause => player.pause(),
        MediaControlEvent::Toggle => state.toggle(),
        MediaControlEvent::Next => player.next(),
        MediaControlEvent::Previous => player.previous(),
        MediaControlEvent::Stop => player.stop(),
        MediaControlEvent::SetPosition(MediaPosition(to)) => player.seek(to),
        MediaControlEvent::Seek(direction) => state.seek_by(direction, SEEK_STEP),
        MediaControlEvent::SeekBy(direction, by) => state.seek_by(direction, by),
        _ => {}
    }
}

/// Publishes `now` to the OS panel.
pub fn update(app: &AppHandle, now: NowPlaying) {
    let _ = app.run_on_main_thread(move || {
        CONTROLS.with(|c| {
            let mut c = c.borrow_mut();
            let Some(controls) = c.as_mut() else {
                return;
            };
            let _ = controls.set_metadata(MediaMetadata {
                title: now.title.as_deref(),
                album: now.album.as_deref(),
                artist: now.artist.as_deref(),
                cover_url: now.cover_url.as_deref(),
                duration: now.duration,
            });
            let playback = match now.playback {
                Playback::Stopped => MediaPlayback::Stopped,
                Playback::Playing(at) => MediaPlayback::Playing {
                    progress: Some(MediaPosition(at)),
                },
                Playback::Paused(at) => MediaPlayback::Paused {
                    progress: Some(MediaPosition(at)),
                },
            };
            let _ = controls.set_playback(playback);
        });
    });
}

pub fn seek_target(elapsed: Duration, direction: SeekDirection, by: Duration) -> Duration {
    match direction {
        SeekDirection::Forward => elapsed + by,
        SeekDirection::Backward => elapsed.saturating_sub(by),
    }
}
