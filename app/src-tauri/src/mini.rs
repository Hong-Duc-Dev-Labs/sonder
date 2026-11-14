//! The mini player: a small always-on-top window that takes over when the main window is
//! closed while something is playing (if the setting allows). The main window is only hidden
//! then, so reopening it keeps its state; closing the mini player quits the app.

use tauri::{
    AppHandle, Manager, PhysicalPosition, Runtime, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
    Window, WindowEvent,
};

use crate::state::AppState;

pub const MAIN: &str = "main";
pub const MINI: &str = "mini";

/// Logical size of the mini player window (the card plus the record peeking out of it).
const WIDTH: f64 = 340.0;
const HEIGHT: f64 = 300.0;
/// Gap between the mini player and the edges of the screen's work area, in logical pixels.
const MARGIN: f64 = 24.0;

/// Routes window events: closing the main window may hand over to the mini player, closing
/// the mini player quits.
pub fn on_window_event<R: Runtime>(window: &Window<R>, event: &WindowEvent) {
    let app = window.app_handle();
    match (window.label(), event) {
        (MAIN, WindowEvent::CloseRequested { api, .. }) => {
            if wants_mini(app) {
                api.prevent_close();
                let _ = window.hide();
                if let Err(err) = show_mini(app) {
                    eprintln!("sonder: cannot open the mini player: {err}");
                    let _ = window.show();
                }
            } else if let Some(mini) = app.get_webview_window(MINI) {
                // The main window closes for good: a hidden mini player left from an earlier
                // close must not keep the app alive.
                let _ = mini.destroy();
            }
        }
        (MINI, WindowEvent::CloseRequested { api, .. }) => {
            api.prevent_close();
            app.exit(0);
        }
        _ => {}
    }
}

/// The setting is on and there is a track to show.
fn wants_mini<R: Runtime>(app: &AppHandle<R>) -> bool {
    let Some(state) = app.try_state::<AppState>() else {
        return false;
    };
    let enabled = crate::state::lock(&state.settings).mini_player;
    let playing = state.snapshot().index.is_some();
    enabled && playing
}

/// Shows the mini player, creating it in the bottom-right corner of the main window's screen
/// the first time.
fn show_mini<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let mini = match app.get_webview_window(MINI) {
        Some(window) => window,
        None => {
            let window = WebviewWindowBuilder::new(app, MINI, WebviewUrl::App("index.html".into()))
                .title("Sonder")
                .inner_size(WIDTH, HEIGHT)
                .resizable(false)
                .maximizable(false)
                .minimizable(false)
                .decorations(false)
                .transparent(true)
                .shadow(false)
                .always_on_top(true)
                .visible(false)
                .build()?;
            place_in_corner(app, &window);
            window
        }
    };
    mini.show()?;
    mini.set_focus()
}

fn place_in_corner<R: Runtime>(app: &AppHandle<R>, window: &WebviewWindow<R>) {
    let monitor = app
        .get_webview_window(MAIN)
        .and_then(|main| main.current_monitor().ok().flatten())
        .or_else(|| window.primary_monitor().ok().flatten());
    let Some(monitor) = monitor else {
        return;
    };
    let scale = monitor.scale_factor();
    let area = monitor.work_area();
    let width = ((WIDTH + MARGIN) * scale) as i32;
    let height = ((HEIGHT + MARGIN) * scale) as i32;
    let x = area.position.x + area.size.width as i32 - width;
    let y = area.position.y + area.size.height as i32 - height;
    let _ = window.set_position(PhysicalPosition::new(x, y));
}

/// Brings the main window back and puts the mini player away.
#[tauri::command]
#[specta::specta]
pub fn window_show_main(app: AppHandle) {
    if let Some(main) = app.get_webview_window(MAIN) {
        let _ = main.unminimize();
        let _ = main.show();
        let _ = main.set_focus();
    }
    if let Some(mini) = app.get_webview_window(MINI) {
        let _ = mini.hide();
    }
}

/// Quits Sonder (the mini player's close button).
#[tauri::command]
#[specta::specta]
pub fn app_quit(app: AppHandle) {
    app.exit(0);
}
