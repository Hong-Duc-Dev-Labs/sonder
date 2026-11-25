//! Sonder desktop app. The engine and library stay plain crates; this binary only adapts
//! them to Tauri commands, events and the `sonder-art` scheme.

// No console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod art;
mod commands;
mod dto;
mod events;
mod history;
mod loudness;
mod media;
mod mini;
mod quality;
mod scan;
mod session;
mod settings;
mod sleep;
mod state;
mod update;

use tauri::Manager;
use tauri_specta::{Builder, collect_commands, collect_events};

use crate::state::{AppPaths, AppState};

/// Path of the generated TypeScript bindings, relative to this crate.
#[cfg(any(debug_assertions, test))]
const BINDINGS: &str = "../src/lib/bindings.ts";

fn bindings() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new()
        .commands(collect_commands![
            commands::library_albums,
            commands::library_album,
            commands::library_artists,
            commands::library_artist,
            commands::library_search,
            commands::library_roots,
            commands::library_add_root,
            commands::library_remove_root,
            commands::library_rescan,
            commands::library_is_scanning,
            commands::library_playlists,
            commands::library_playlist,
            commands::playlist_create,
            commands::playlist_rename,
            commands::playlist_delete,
            commands::playlist_add_tracks,
            commands::playlist_remove_entry,
            commands::playlist_move_entry,
            commands::player_play_album,
            commands::player_play_tracks,
            commands::player_enqueue_album,
            commands::player_enqueue_tracks,
            commands::player_play_index,
            commands::player_play_next_tracks,
            commands::player_play_next_album,
            commands::queue_remove,
            commands::library_artist_tracks,
            commands::track_tags,
            commands::track_update_tags,
            commands::track_lookup,
            commands::cover_download,
            commands::track_find_lyrics,
            commands::library_tracks,
            commands::queue_items,
            commands::queue_clear,
            commands::player_play,
            commands::player_pause,
            commands::player_toggle,
            commands::player_stop,
            commands::player_next,
            commands::player_previous,
            commands::player_seek,
            commands::player_set_volume,
            commands::player_set_repeat,
            commands::player_snapshot,
            commands::devices_list,
            commands::device_select,
            commands::settings_get,
            commands::settings_set,
            commands::settings_pick_roon_folder,
            commands::settings_pick_impulse_response,
            commands::impulse_response_info,
            commands::peq_import,
            commands::peq_export,
            commands::autoeq_search,
            commands::autoeq_import,
            commands::library_analyze_loudness,
            commands::library_loudness_pending,
            commands::library_is_analyzing,
            commands::lyrics_get,
            update::app_version,
            update::update_pending,
            update::update_check,
            update::update_install,
            mini::window_show_main,
            mini::app_quit,
            quality::quality_check_track,
            quality::quality_cached,
            quality::quality_pick_file,
            quality::quality_check_file,
            quality::quality_check_batch,
            quality::quality_cancel_check,
            quality::quality_cancel_batch,
            quality::quality_batch_status,
            history::history_stats,
            history::history_clear,
            sleep::sleep_timer_set,
            sleep::sleep_timer_get,
            commands::playlist_import_m3u,
            commands::playlist_export_m3u,
            commands::queue_move,
            commands::queue_shuffle,
            commands::playlist_create_from_queue,
            commands::library_toggle_favorite,
            commands::library_favorites,
            commands::library_favorite_paths,
            commands::library_recent_albums,
            commands::lyrics_get_path,
            commands::player_open_paths,
            commands::reveal_in_folder,
        ])
        .events(collect_events![
            events::PlayerStateEvent,
            events::QueueChangedEvent,
            events::FavoritesChangedEvent,
            events::ScanProgressEvent,
            events::ScanFinishedEvent,
            events::LoudnessProgressEvent,
            events::LoudnessFinishedEvent,
            events::LibraryChangedEvent,
            events::DevicesChangedEvent,
            events::NoticeEvent,
            update::UpdateAvailableEvent,
            update::UpdateProgressEvent,
            quality::QualityProgressEvent,
            quality::QualityCheckedEvent,
            quality::QualityBatchEvent,
            history::HistoryChangedEvent,
            sleep::SleepTimerEvent,
        ])
        // Ids and millisecond counts stay far below 2^53.
        .dangerously_cast_bigints_to_number()
        // One type per Rust type; the UI only reads what Rust serializes.
        .disable_serde_phases()
}

#[cfg(any(debug_assertions, test))]
fn export_bindings(builder: &Builder<tauri::Wry>, path: &std::path::Path) {
    builder
        .export(
            specta_typescript::Typescript::default().header(
                "// Generated by sonder-app from the Rust command and event types. Do not edit.\n",
            ),
            path,
        )
        .expect("cannot export TypeScript bindings");
}

fn main() {
    let builder = bindings();
    #[cfg(debug_assertions)]
    export_bindings(
        &builder,
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(BINDINGS),
    );

    let paths = match AppPaths::from_os() {
        Ok(p) => p,
        Err(err) => {
            eprintln!("sonder: {err}");
            std::process::exit(1);
        }
    };
    let art_cache = sonder_library::ArtCache::new(&paths.art);

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(update::PendingUpdate::default())
        .invoke_handler(builder.invoke_handler())
        .on_window_event(mini::on_window_event)
        .register_asynchronous_uri_scheme_protocol(art::SCHEME, move |_ctx, request, responder| {
            let cache = art_cache.clone();
            tauri::async_runtime::spawn_blocking(move || {
                responder.respond(art::respond(&cache, &request));
            });
        })
        .setup(move |app| {
            builder.mount_events(app);
            // Device probing uses COM in multithreaded mode, which the main (UI) thread
            // cannot enter; open everything on a helper thread.
            let opened = std::thread::spawn(move || AppState::open(paths))
                .join()
                .map_err(|_| "starting the app panicked")?;
            let (state, player_events) = opened.map_err(|e| e.0)?;
            app.manage(state);

            let handle = app.handle().clone();
            std::thread::Builder::new()
                .name("sonder-ui-events".into())
                .spawn(move || events::run_bridge(handle, player_events))?;
            scan::start_watcher(app.handle());
            scan::backfill_cover_colors(app.handle());
            update::check_on_launch(app.handle());
            #[cfg(target_os = "macos")]
            const HOST_OS: &str = "macos";
            #[cfg(target_os = "windows")]
            const HOST_OS: &str = "windows";
            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
            const HOST_OS: &str = "linux";

            let main_config = app
                .config()
                .app
                .windows
                .iter()
                .find(|w| w.label == mini::MAIN)
                .ok_or("missing main window config")?
                .clone();
            let mut main_builder =
                tauri::WebviewWindowBuilder::from_config(app.handle(), &main_config)?
                    .theme(Some(tauri::Theme::Dark))
                    .initialization_script(format!("window.__SONDER_OS__ = \"{HOST_OS}\";"));
            #[cfg(target_os = "macos")]
            {
                main_builder = main_builder
                    .decorations(true)
                    .title_bar_style(tauri::TitleBarStyle::Overlay)
                    .traffic_light_position(tauri::LogicalPosition::new(14.0, 18.0))
                    .hidden_title(true);
            }
            #[cfg(not(target_os = "macos"))]
            {
                main_builder = main_builder.decorations(false).shadow(true);
            }
            let window = main_builder.build()?;
            media::init(app.handle(), &window);
            let startup_paths: Vec<String> = std::env::args()
                .skip(1)
                .filter(|arg| !arg.starts_with('-'))
                .filter(|arg| std::path::Path::new(arg).exists())
                .collect();
            if !startup_paths.is_empty() {
                let _ = commands::open_paths_sync(app.handle(), startup_paths);
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("cannot start Sonder")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                let state = app.state::<AppState>();
                session::capture_and_save(&state);
                // Release the exclusive device before the process goes away.
                state.player.stop();
            }
        });
}

#[cfg(test)]
mod tests {
    /// The committed bindings must match the Rust types, or the UI would call commands that
    /// no longer exist. Run the app once in debug (or this test's output) to regenerate.
    #[test]
    fn typescript_bindings_are_up_to_date() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let fresh = std::env::temp_dir().join(format!("sonder-bindings-{}.ts", std::process::id()));
        super::export_bindings(&super::bindings(), &fresh);
        let fresh_text = std::fs::read_to_string(&fresh).unwrap();
        let _ = std::fs::remove_file(&fresh);
        let committed = std::fs::read_to_string(dir.join(super::BINDINGS)).unwrap_or_default();
        if std::env::var_os("SONDER_UPDATE_BINDINGS").is_some() {
            std::fs::write(dir.join(super::BINDINGS), &fresh_text).unwrap();
            return;
        }
        assert!(
            committed.replace("\r\n", "\n") == fresh_text.replace("\r\n", "\n"),
            "src/lib/bindings.ts is stale; rerun with SONDER_UPDATE_BINDINGS=1"
        );
    }
}
