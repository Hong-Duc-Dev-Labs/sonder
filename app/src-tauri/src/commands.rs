//! Commands the web UI calls. They adapt the engine and library to IPC and hold no logic
//! of their own beyond keeping the UI's queue mirror in step with the engine.
//!
//! Every command is `async` so it runs on the async runtime's threads, never on the main
//! (UI) thread: library queries, device probing and dialogs must not freeze the window.

use std::path::PathBuf;
use std::sync::LazyLock;
use std::time::Duration;

use sonder_output::DeviceId;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;
use tauri_specta::Event;

use crate::dto::{
    Album, AlbumInfo, AlbumPage, AppError, AppResult, Artist, ArtistInfo, AutoEqResult, CoverImage,
    Device, FoundLyrics, ImpulseResponseInfo, LookupQuery, Lyrics, MetadataCandidate, PeqImport,
    PlayerSnapshot, Playlist, PlaylistInfo, QueueItem, RepeatMode, RootFolder, SortOrder,
    TrackEdit, TrackItem, TrackTagsInfo, export_file_name,
};
use crate::events::{LibraryChangedEvent, QueueChangedEvent};
use crate::settings::{PeqSetting, Settings};
use crate::state::{AppState, lock};

const SEARCH_LIMIT: usize = 200;

static LYRICS_ONLINE: LazyLock<sonder_lyrics::Online> =
    LazyLock::new(|| sonder_lyrics::Online::new(concat!("Sonder/", env!("CARGO_PKG_VERSION"))));

/// How Sonder names itself and a way to reach its authors, as MusicBrainz asks clients to.
pub const USER_AGENT: &str = concat!(
    "Sonder/",
    env!("CARGO_PKG_VERSION"),
    " ( https://github.com/Hong-Duc-Dev-Labs/sonder )"
);

static METADATA_ONLINE: LazyLock<sonder_metadata::Online> =
    LazyLock::new(|| sonder_metadata::Online::new(USER_AGENT));

/// AutoEQ results listed for one search.
const AUTOEQ_LIMIT: usize = 50;
const MAX_PAGE: u32 = 1000;

// ----- library -----

#[tauri::command]
#[specta::specta]
pub async fn library_albums(
    state: State<'_, AppState>,
    sort: SortOrder,
    offset: u32,
    limit: u32,
) -> AppResult<AlbumPage> {
    let library = lock(&state.library);
    let total = library.album_count()? as u32;
    let albums = library
        .albums_page(sort.into(), offset as usize, limit.min(MAX_PAGE) as usize)?
        .into_iter()
        .map(Album::from)
        .collect();
    Ok(AlbumPage {
        total,
        offset,
        albums,
    })
}

#[tauri::command]
#[specta::specta]
pub async fn library_album(state: State<'_, AppState>, id: i64) -> AppResult<Option<AlbumInfo>> {
    Ok(lock(&state.library).album(id)?.map(Into::into))
}

#[tauri::command]
#[specta::specta]
pub async fn library_artists(state: State<'_, AppState>) -> AppResult<Vec<Artist>> {
    Ok(lock(&state.library)
        .artists()?
        .into_iter()
        .map(Into::into)
        .collect())
}

#[tauri::command]
#[specta::specta]
pub async fn library_artist(state: State<'_, AppState>, id: i64) -> AppResult<Option<ArtistInfo>> {
    Ok(lock(&state.library).artist(id)?.map(Into::into))
}

/// Every track in the library, by title.
#[tauri::command]
#[specta::specta]
pub async fn library_tracks(state: State<'_, AppState>) -> AppResult<Vec<TrackItem>> {
    Ok(lock(&state.library)
        .tracks()?
        .into_iter()
        .map(Into::into)
        .collect())
}

#[tauri::command]
#[specta::specta]
pub async fn library_search(
    state: State<'_, AppState>,
    query: String,
) -> AppResult<Vec<TrackItem>> {
    Ok(lock(&state.library)
        .search(&query, SEARCH_LIMIT)?
        .into_iter()
        .map(Into::into)
        .collect())
}

#[tauri::command]
#[specta::specta]
pub async fn library_roots(state: State<'_, AppState>) -> AppResult<Vec<RootFolder>> {
    Ok(lock(&state.library)
        .roots()?
        .into_iter()
        .map(|r| RootFolder {
            path: r.path.to_string_lossy().into_owned(),
            follow_symlinks: r.follow_symlinks,
        })
        .collect())
}

/// Asks for a folder with the native picker, adds it and scans. Returns `None` when the user
/// cancels. The picker runs here, not in the webview, so the webview needs no dialog rights.
#[tauri::command]
#[specta::specta]
pub async fn library_add_root(
    app: AppHandle,
    state: State<'_, AppState>,
    follow_symlinks: bool,
) -> AppResult<Option<RootFolder>> {
    let Some(picked) = app.dialog().file().blocking_pick_folder() else {
        return Ok(None);
    };
    let path = picked.into_path().map_err(AppError::from)?;
    let root = lock(&state.library).add_root(&path, follow_symlinks)?;
    if let Some(watcher) = lock(&state.watcher).as_mut() {
        // Watching is best effort; the scan below still indexes the folder.
        let _ = watcher.watch_root(&root.path);
    }
    crate::scan::start(&app);
    Ok(Some(RootFolder {
        path: root.path.to_string_lossy().into_owned(),
        follow_symlinks: root.follow_symlinks,
    }))
}

#[tauri::command]
#[specta::specta]
pub async fn library_remove_root(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> AppResult<()> {
    let path = PathBuf::from(path);
    if let Some(watcher) = lock(&state.watcher).as_mut() {
        let _ = watcher.unwatch_root(&path);
    }
    if lock(&state.library).remove_root(&path)? {
        let _ = LibraryChangedEvent.emit(&app);
    }
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn library_rescan(app: AppHandle) -> AppResult<()> {
    crate::scan::start(&app);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn library_is_scanning(state: State<'_, AppState>) -> AppResult<bool> {
    Ok(state.scan.running.load(std::sync::atomic::Ordering::SeqCst))
}

// ----- playlists -----
//
// Playing a playlist goes through the player commands with its entries' track ids.

#[tauri::command]
#[specta::specta]
pub async fn library_playlists(state: State<'_, AppState>) -> AppResult<Vec<Playlist>> {
    Ok(lock(&state.library)
        .playlists()?
        .into_iter()
        .map(Into::into)
        .collect())
}

#[tauri::command]
#[specta::specta]
pub async fn library_playlist(
    state: State<'_, AppState>,
    id: i64,
) -> AppResult<Option<PlaylistInfo>> {
    Ok(lock(&state.library).playlist(id)?.map(Into::into))
}

#[tauri::command]
#[specta::specta]
pub async fn playlist_create(state: State<'_, AppState>, name: String) -> AppResult<Playlist> {
    Ok(lock(&state.library).create_playlist(&name)?.into())
}

#[tauri::command]
#[specta::specta]
pub async fn playlist_rename(state: State<'_, AppState>, id: i64, name: String) -> AppResult<()> {
    lock(&state.library).rename_playlist(id, &name)?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn playlist_delete(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    lock(&state.library).delete_playlist(id)?;
    Ok(())
}

/// Appends tracks to a playlist; returns how many were added.
#[tauri::command]
#[specta::specta]
pub async fn playlist_add_tracks(
    state: State<'_, AppState>,
    id: i64,
    track_ids: Vec<i64>,
) -> AppResult<u32> {
    let added = lock(&state.library).add_to_playlist(id, &track_ids)?;
    Ok(u32::try_from(added).unwrap_or(u32::MAX))
}

#[tauri::command]
#[specta::specta]
pub async fn playlist_remove_entry(
    state: State<'_, AppState>,
    id: i64,
    position: u32,
) -> AppResult<()> {
    lock(&state.library).remove_from_playlist(id, position)?;
    Ok(())
}

/// Moves the entry at position `from` to position `to`.
#[tauri::command]
#[specta::specta]
pub async fn playlist_move_entry(
    state: State<'_, AppState>,
    id: i64,
    from: u32,
    to: u32,
) -> AppResult<()> {
    lock(&state.library).move_in_playlist(id, from, to)?;
    Ok(())
}

// ----- queue and transport -----

fn album_tracks(state: &AppState, album_id: i64) -> AppResult<Vec<TrackItem>> {
    let album = lock(&state.library)
        .album(album_id)?
        .ok_or_else(|| AppError("album not found".into()))?;
    Ok(album.tracks.into_iter().map(Into::into).collect())
}

fn tracks_by_id(state: &AppState, ids: &[i64]) -> AppResult<Vec<TrackItem>> {
    let library = lock(&state.library);
    let mut tracks = Vec::with_capacity(ids.len());
    for &id in ids {
        if let Some(track) = library.track(id)? {
            tracks.push(track.into());
        }
    }
    Ok(tracks)
}

/// Appends `tracks` to the engine queue and the mirror, optionally replacing the queue and
/// starting playback at `play_at` (an index into `tracks`).
fn enqueue(app: &AppHandle, tracks: Vec<TrackItem>, replace: bool, play_at: Option<u32>) {
    let state = app.state::<AppState>();
    let mut queue = lock(&state.queue);
    if replace {
        state.player.clear();
        queue.clear();
    }
    let base = queue.len();
    queue.extend(
        tracks
            .iter()
            .map(|t| QueueItem::new(state.next_entry_id(), t)),
    );
    state
        .player
        .enqueue(tracks.iter().map(|t| PathBuf::from(&t.path)).collect());
    if let Some(at) = play_at.filter(|&i| (i as usize) < tracks.len()) {
        state.player.play_index(base + at as usize);
    }
    let _ = QueueChangedEvent(queue.clone()).emit(app);
}

/// Replaces the queue with the album and plays from track `start`.
#[tauri::command]
#[specta::specta]
pub async fn player_play_album(
    app: AppHandle,
    state: State<'_, AppState>,
    album_id: i64,
    start: u32,
) -> AppResult<()> {
    let tracks = album_tracks(&state, album_id)?;
    enqueue(&app, tracks, true, Some(start));
    Ok(())
}

/// Replaces the queue with these tracks and plays from `start`.
#[tauri::command]
#[specta::specta]
pub async fn player_play_tracks(
    app: AppHandle,
    state: State<'_, AppState>,
    track_ids: Vec<i64>,
    start: u32,
) -> AppResult<()> {
    let tracks = tracks_by_id(&state, &track_ids)?;
    enqueue(&app, tracks, true, Some(start));
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn player_enqueue_album(
    app: AppHandle,
    state: State<'_, AppState>,
    album_id: i64,
) -> AppResult<()> {
    let tracks = album_tracks(&state, album_id)?;
    enqueue(&app, tracks, false, None);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn player_enqueue_tracks(
    app: AppHandle,
    state: State<'_, AppState>,
    track_ids: Vec<i64>,
) -> AppResult<()> {
    let tracks = tracks_by_id(&state, &track_ids)?;
    enqueue(&app, tracks, false, None);
    Ok(())
}

/// Inserts `tracks` right after the playing entry (at the end when nothing is current), so
/// they play next.
fn play_next(app: &AppHandle, tracks: Vec<TrackItem>) {
    let state = app.state::<AppState>();
    let mut queue = lock(&state.queue);
    let at = state
        .snapshot()
        .index
        .map_or(queue.len(), |i| i as usize + 1)
        .min(queue.len());
    let items: Vec<QueueItem> = tracks
        .iter()
        .map(|t| QueueItem::new(state.next_entry_id(), t))
        .collect();
    queue.splice(at..at, items);
    state
        .player
        .insert(at, tracks.iter().map(|t| PathBuf::from(&t.path)).collect());
    let _ = QueueChangedEvent(queue.clone()).emit(app);
}

#[tauri::command]
#[specta::specta]
pub async fn player_play_next_tracks(
    app: AppHandle,
    state: State<'_, AppState>,
    track_ids: Vec<i64>,
) -> AppResult<()> {
    let tracks = tracks_by_id(&state, &track_ids)?;
    play_next(&app, tracks);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn player_play_next_album(
    app: AppHandle,
    state: State<'_, AppState>,
    album_id: i64,
) -> AppResult<()> {
    let tracks = album_tracks(&state, album_id)?;
    play_next(&app, tracks);
    Ok(())
}

/// Removes one queue entry. Removing the playing entry moves on to the next one.
#[tauri::command]
#[specta::specta]
pub async fn queue_remove(
    app: AppHandle,
    state: State<'_, AppState>,
    entry_id: u32,
) -> AppResult<()> {
    let mut queue = lock(&state.queue);
    let Some(index) = queue.iter().position(|q| q.entry_id == entry_id) else {
        // Already gone (a double click, or the queue was replaced meanwhile).
        return Ok(());
    };
    queue.remove(index);
    state.player.remove(index);
    let _ = QueueChangedEvent(queue.clone()).emit(&app);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn library_artist_tracks(
    state: State<'_, AppState>,
    id: i64,
) -> AppResult<Vec<TrackItem>> {
    Ok(lock(&state.library)
        .artist_tracks(id)?
        .into_iter()
        .map(Into::into)
        .collect())
}

// ----- tags -----

/// The metadata stored in a track's own file, for the editor to show. Read fresh from the
/// file rather than from the index, and without the track number the library guesses from
/// the file name, so Save never writes a value the user did not see come from the file. A
/// missing title is still filled from the file name, since a track needs one.
#[tauri::command]
#[specta::specta]
pub async fn track_tags(
    state: State<'_, AppState>,
    track_id: i64,
) -> AppResult<Option<TrackTagsInfo>> {
    let Some(track) = lock(&state.library).track(track_id)? else {
        return Ok(None);
    };
    let mut tags =
        tauri::async_runtime::spawn_blocking(move || sonder_library::read_file_tags(&track.path))
            .await??;
    // A picture in a format the tags cannot take back (GIF, BMP) is not offered as the cover;
    // Save keeps it unless the user replaces or removes it.
    let cover = tags.cover.take().and_then(|c| CoverImage::from_bytes(&c));
    Ok(Some(TrackTagsInfo {
        tags: tags.into(),
        cover,
    }))
}

/// Writes edited metadata into the track's file itself, then updates the index and tells the
/// UI. Returns the track as it is indexed now; `None` when it has left the library.
#[tauri::command]
#[specta::specta]
pub async fn track_update_tags(
    app: AppHandle,
    state: State<'_, AppState>,
    track_id: i64,
    edit: TrackEdit,
) -> AppResult<Option<TrackItem>> {
    let Some(track) = lock(&state.library).track(track_id)? else {
        return Ok(None);
    };
    let path = track.path;
    let library_edit: sonder_library::TagEdit = edit.try_into()?;
    tauri::async_runtime::spawn_blocking(move || sonder_library::write_file(&path, &library_edit))
        .await??;
    let updated: Option<TrackItem> = lock(&state.library)
        .reindex_track(track_id)?
        .map(Into::into);
    if let Some(item) = &updated {
        refresh_queue(&app, item);
    }
    let _ = LibraryChangedEvent.emit(&app);
    // A tag edit keeps the measured loudness, but a track that changed album needs its new
    // album's value; an analysis pass with nothing pending returns at once.
    crate::loudness::start_analysis(&app);
    Ok(updated)
}

/// Asks iTunes, Deezer and MusicBrainz about the track, with the fields as the editor has
/// them and the track's length. Only on the user's request, so the lyrics setting does not
/// apply. Candidates whose length matches come first.
#[tauri::command]
#[specta::specta]
pub async fn track_lookup(
    state: State<'_, AppState>,
    track_id: i64,
    query: LookupQuery,
) -> AppResult<Vec<MetadataCandidate>> {
    let Some(track) = lock(&state.library).track(track_id)? else {
        return Ok(Vec::new());
    };
    let duration_ms = track.audio.duration_ms;
    let query = sonder_metadata::Query {
        title: query.title.trim().to_string(),
        artist: filled(&query.artist),
        album: filled(&query.album),
        duration_ms,
    };
    if query.title.is_empty() {
        return Err(AppError("a title is needed to search".into()));
    }
    let found =
        tauri::async_runtime::spawn_blocking(move || METADATA_ONLINE.search(&query)).await?;
    Ok(found
        .map_err(AppError)?
        .into_iter()
        .map(|c| MetadataCandidate::new(c, duration_ms))
        .collect())
}

/// Fetches a cover image an online catalogue named, for the editor to show and save. `None`
/// when the catalogue has no image there.
#[tauri::command]
#[specta::specta]
pub async fn cover_download(url: String) -> AppResult<Option<CoverImage>> {
    let bytes = tauri::async_runtime::spawn_blocking(move || METADATA_ONLINE.cover(&url))
        .await?
        .map_err(|e| AppError(format!("cannot download the cover: {e}")))?;
    match bytes {
        None => Ok(None),
        Some(bytes) => CoverImage::from_bytes(&bytes)
            .map(Some)
            .ok_or_else(|| AppError("the cover is not a JPEG or PNG image".into())),
    }
}

/// Asks the online lyrics services with the fields as the editor has them, for the user to
/// review and save into the file. Only on the user's request, so the lyrics setting does not
/// apply; the answer is cached like any other lookup.
#[tauri::command]
#[specta::specta]
pub async fn track_find_lyrics(
    state: State<'_, AppState>,
    track_id: i64,
    query: LookupQuery,
) -> AppResult<Option<FoundLyrics>> {
    let Some(track) = lock(&state.library).track(track_id)? else {
        return Ok(None);
    };
    let query = sonder_lyrics::Query {
        title: query.title.trim().to_string(),
        artist: filled(&query.artist),
        album: filled(&query.album),
        duration_ms: track.audio.duration_ms,
    };
    if query.title.is_empty() {
        return Err(AppError("a title is needed to search".into()));
    }
    let cache = sonder_lyrics::Cache::new(state.paths.lyrics.clone());
    let found = tauri::async_runtime::spawn_blocking(move || {
        let found = LYRICS_ONLINE.find(&query);
        cache.put(&query, found.as_ref());
        found
    })
    .await?;
    Ok(found.map(Into::into))
}

/// The first of several names typed with `;` between them, `None` when blank: the online
/// services match on one artist.
fn filled(text: &str) -> Option<String> {
    text.split(';')
        .map(str::trim)
        .find(|t| !t.is_empty())
        .map(str::to_string)
}

/// Replaces the metadata of every queue entry holding `track`, so Now Playing and the queue
/// show an edited title or cover without a restart. The engine only holds paths.
fn refresh_queue(app: &AppHandle, track: &TrackItem) {
    let state = app.state::<AppState>();
    let mut queue = lock(&state.queue);
    let mut touched = false;
    for entry in queue.iter_mut().filter(|e| e.track_id == track.id) {
        *entry = QueueItem::new(entry.entry_id, track);
        touched = true;
    }
    if touched {
        let _ = QueueChangedEvent(queue.clone()).emit(app);
    }
}

#[tauri::command]
#[specta::specta]
pub async fn player_play_index(state: State<'_, AppState>, index: u32) -> AppResult<()> {
    state.player.play_index(index as usize);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn queue_items(state: State<'_, AppState>) -> AppResult<Vec<QueueItem>> {
    Ok(lock(&state.queue).clone())
}

#[tauri::command]
#[specta::specta]
pub async fn queue_clear(app: AppHandle, state: State<'_, AppState>) -> AppResult<()> {
    let mut queue = lock(&state.queue);
    state.player.clear();
    queue.clear();
    let _ = QueueChangedEvent(Vec::new()).emit(&app);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn player_play(state: State<'_, AppState>) -> AppResult<()> {
    state.player.play();
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn player_pause(state: State<'_, AppState>) -> AppResult<()> {
    state.player.pause();
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn player_toggle(state: State<'_, AppState>) -> AppResult<()> {
    state.toggle();
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn player_stop(state: State<'_, AppState>) -> AppResult<()> {
    state.player.stop();
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn player_next(state: State<'_, AppState>) -> AppResult<()> {
    state.player.next();
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn player_previous(state: State<'_, AppState>) -> AppResult<()> {
    state.player.previous();
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn player_seek(state: State<'_, AppState>, position_ms: u32) -> AppResult<()> {
    state
        .player
        .seek(Duration::from_millis(u64::from(position_ms)));
    Ok(())
}

/// Sets software volume in dB, or `None` for bit-perfect output, and remembers it.
#[tauri::command]
#[specta::specta]
pub async fn player_set_volume(state: State<'_, AppState>, db: Option<f64>) -> AppResult<()> {
    let settings = state.update_settings(|s| s.volume_db = db)?;
    apply_volume(&state, settings.volume_db);
    Ok(())
}

fn apply_volume(state: &AppState, db: Option<f64>) {
    let mut snap = state.snapshot();
    if snap.volume_db != db {
        snap.volume_db = db;
        state.player.set_volume(db);
    }
}

/// Sets what plays after the current track ends (off, all, one) and remembers it for this run.
#[tauri::command]
#[specta::specta]
pub async fn player_set_repeat(state: State<'_, AppState>, mode: RepeatMode) -> AppResult<()> {
    state.snapshot().repeat = mode;
    state.player.set_repeat(mode.into());
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn player_snapshot(state: State<'_, AppState>) -> AppResult<PlayerSnapshot> {
    Ok(state.snapshot().clone())
}

// ----- devices -----

#[tauri::command]
#[specta::specta]
pub async fn devices_list(state: State<'_, AppState>) -> AppResult<Vec<Device>> {
    state.device_list()
}

/// Switches output to `id` (playback continues there) and remembers the choice.
#[tauri::command]
#[specta::specta]
pub async fn device_select(state: State<'_, AppState>, id: String) -> AppResult<()> {
    state.update_settings(|s| s.output_device = Some(id.clone()))?;
    apply_device(&state, DeviceId(id));
    Ok(())
}

fn apply_device(state: &AppState, id: DeviceId) {
    let mut current = lock(&state.device);
    if *current != id {
        *current = id.clone();
        state.player.set_device(id);
    }
}

// ----- settings -----

#[tauri::command]
#[specta::specta]
pub async fn settings_get(state: State<'_, AppState>) -> AppResult<Settings> {
    Ok(lock(&state.settings).clone())
}

/// Saves `settings` and applies the device, volume, DSD, DSP, crossfade and rate-conversion
/// settings in them.
#[tauri::command]
#[specta::specta]
pub async fn settings_set(state: State<'_, AppState>, settings: Settings) -> AppResult<Settings> {
    let saved = state.update_settings(|s| *s = settings)?;
    // DSD first: a device switch then opens the new device with the new DSD mode.
    saved.apply_dsd(&state.player);
    if let Some(device) = &saved.output_device {
        apply_device(&state, DeviceId(device.clone()));
    }
    apply_volume(&state, saved.volume_db);
    saved.apply_dsp(&state.player);
    saved.apply_src(&state.player);
    saved.apply_sdm(&state.player);
    saved.apply_crossfade(&state.player);
    Ok(saved)
}

// ----- lyrics -----

/// Lyrics for a library track: its own (tags, then a `.lrc` beside it), else online when the
/// setting allows, through the on-disk cache. `None` when there are none anywhere.
#[tauri::command]
#[specta::specta]
pub async fn lyrics_get(state: State<'_, AppState>, track_id: i64) -> AppResult<Option<Lyrics>> {
    let Some(track) = lock(&state.library).track(track_id)? else {
        return Ok(None);
    };
    let online = lock(&state.settings).lyrics_online;
    let cache = sonder_lyrics::Cache::new(state.paths.lyrics.clone());
    let found = tauri::async_runtime::spawn_blocking(move || {
        let embedded = sonder_library::read_file(&track.path)
            .ok()
            .and_then(|m| m.tags.lyrics);
        let local = sonder_lyrics::local(&track.path, embedded.as_deref());
        let query = sonder_lyrics::Query {
            title: track.title,
            artist: track.artist.or(track.album_artist),
            album: Some(track.album).filter(|a| !a.is_empty()),
            duration_ms: track.audio.duration_ms,
        };
        sonder_lyrics::choose(local, || {
            // A cached answer is used even with lookups off: it sends nothing.
            if let Some(cached) = cache.get(&query) {
                return cached;
            }
            if !online {
                return None;
            }
            let found = LYRICS_ONLINE.find(&query);
            cache.put(&query, found.as_ref());
            found
        })
    })
    .await?;
    Ok(found.map(Into::into))
}

// ----- DSP and loudness -----

/// Asks for the folder that holds Roon's filter tables. Returns `None` when the user cancels.
#[tauri::command]
#[specta::specta]
pub async fn settings_pick_roon_folder(app: AppHandle) -> AppResult<Option<String>> {
    let Some(picked) = app.dialog().file().blocking_pick_folder() else {
        return Ok(None);
    };
    let path = picked.into_path().map_err(AppError::from)?;
    Ok(Some(path.to_string_lossy().into_owned()))
}

/// Asks for an impulse response WAV file and describes it. Returns `None` when the user
/// cancels; a file Sonder cannot use is an error naming the reason.
#[tauri::command]
#[specta::specta]
pub async fn settings_pick_impulse_response(
    app: AppHandle,
) -> AppResult<Option<ImpulseResponseInfo>> {
    let Some(picked) = app
        .dialog()
        .file()
        .add_filter("WAV impulse response", &["wav"])
        .blocking_pick_file()
    else {
        return Ok(None);
    };
    let path = picked.into_path().map_err(AppError::from)?;
    let path = path.to_string_lossy().into_owned();
    let info =
        tauri::async_runtime::spawn_blocking(move || ImpulseResponseInfo::describe(&path)).await?;
    Ok(Some(info?))
}

/// Describes the impulse response at `path`, or says why it cannot be used.
#[tauri::command]
#[specta::specta]
pub async fn impulse_response_info(path: String) -> AppResult<ImpulseResponseInfo> {
    let info =
        tauri::async_runtime::spawn_blocking(move || ImpulseResponseInfo::describe(&path)).await?;
    Ok(info?)
}

/// Picks an Equalizer APO parametric file (the `ParametricEQ.txt` AutoEQ publishes) and returns
/// its EQ. Nothing is saved here, so a cancelled dialog changes nothing.
#[tauri::command]
#[specta::specta]
pub async fn peq_import(app: AppHandle) -> AppResult<Option<PeqImport>> {
    let Some(picked) = app
        .dialog()
        .file()
        .add_filter("Equalizer APO parametric EQ", &["txt"])
        .blocking_pick_file()
    else {
        return Ok(None);
    };
    let path = picked.into_path().map_err(AppError::from)?;
    let import = tauri::async_runtime::spawn_blocking(move || PeqImport::read(&path)).await??;
    Ok(Some(import))
}

/// Headphones in AutoEQ matching `query`, best first. The first search downloads AutoEQ's index
/// (kept on disk for a week); the query itself is matched locally and never sent anywhere.
#[tauri::command]
#[specta::specta]
pub async fn autoeq_search(
    state: State<'_, AppState>,
    query: String,
) -> AppResult<Vec<AutoEqResult>> {
    let found = tauri::async_runtime::spawn_blocking({
        let query = query.trim().to_owned();
        let autoeq = state.autoeq.clone();
        move || autoeq.search(&query, AUTOEQ_LIMIT)
    })
    .await??;
    Ok(found.into_iter().map(AutoEqResult::from).collect())
}

/// Downloads the AutoEQ parametric EQ of `result` and returns it as an import named after the
/// headphone. Nothing is saved here.
#[tauri::command]
#[specta::specta]
pub async fn autoeq_import(
    state: State<'_, AppState>,
    result: AutoEqResult,
) -> AppResult<PeqImport> {
    let autoeq = state.autoeq.clone();
    let name = result.name.clone();
    let text = tauri::async_runtime::spawn_blocking(move || autoeq.parametric_eq(&result.into()))
        .await??;
    PeqImport::parse(name, &text)
}

/// Saves `peq` as Equalizer APO parametric text, defaulting to `<name> ParametricEQ.txt`, and
/// returns the saved path. Nothing is written when the dialog is cancelled.
#[tauri::command]
#[specta::specta]
pub async fn peq_export(
    app: AppHandle,
    peq: PeqSetting,
    name: String,
) -> AppResult<Option<String>> {
    let Some(target) = app
        .dialog()
        .file()
        .add_filter("Equalizer APO parametric EQ", &["txt"])
        .set_file_name(export_file_name(&name))
        .blocking_save_file()
    else {
        return Ok(None);
    };
    let path = target.into_path().map_err(AppError::from)?;
    // The round trip is documented for a sanitized EQ; clamp here too, so a caller cannot
    // write a file this app would read back differently.
    let text = sonder_dsp::Peq::from(peq).sanitized().to_apo_text();
    let saved = path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        std::fs::write(&path, text)
            .map_err(|e| AppError(format!("cannot write {}: {e}", path.display())))
    })
    .await??;
    Ok(Some(saved.to_string_lossy().into_owned()))
}

/// Tracks still waiting for a loudness measurement.
#[tauri::command]
#[specta::specta]
pub async fn library_loudness_pending(state: State<'_, AppState>) -> AppResult<u32> {
    let pending = lock(&state.loudness).loudness_pending()?;
    Ok(u32::try_from(pending).unwrap_or(u32::MAX))
}

/// Starts measuring every track that has no ReplayGain tags, or queues a pass behind the one
/// running. Progress and the result arrive as `LoudnessProgressEvent` and
/// `LoudnessFinishedEvent`.
#[tauri::command]
#[specta::specta]
pub async fn library_analyze_loudness(app: AppHandle) -> AppResult<()> {
    crate::loudness::start_analysis(&app);
    Ok(())
}

/// Whether a loudness analysis pass is running.
#[tauri::command]
#[specta::specta]
pub async fn library_is_analyzing(state: State<'_, AppState>) -> AppResult<bool> {
    Ok(state
        .analysis
        .running
        .load(std::sync::atomic::Ordering::SeqCst))
}
