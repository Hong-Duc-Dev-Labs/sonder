# Brainstorm — Sonder v0.2.x Roadmap: Daily Driver, Shared Mode, Extended Formats & Integrations

Date: 2026-11-21. Scope: Product roadmap for the next delivery waves building on `v0.1.4` (`f650da3`).

---

## Outcome

Turn Sonder from an audiophile-grade lossless/DSD engine into a frictionless **daily-driver desktop music player** while adding opt-in **Shared Mode** output, broader source format support (with transparent lossy badges), and opt-in social/scrobbling integrations — without diluting its default bit-perfect guarantee.

---

## Constraints

- **Bit-perfect by default:** Exclusive mode + DSP off + software volume off remains the untouched default path and must keep passing 100% of `crates/sonder-verify`.
- **Desktop-first offline:** Native Windows/macOS/Linux Tauri 2 app following [`DESIGN.md`](file:///D:/Dev/Sonder/DESIGN.md) tokens (`Momo Trust Sans` + `Momo Signature`, critically damped springs $\zeta = 0.58$, `reducedMotion()`, 3-breakpoint desktop snapping `640px` / `900px` / `1280px+`).
- **Offline sandbox / zero speculative bloat:** No cloud streaming accounts, no mobile-web SEO surfaces inside the desktop bundle; backward-compatible `settings.toml` and SQLite schema migrations (`library.db.bak-*`).
- **Real-time audio safety:** Zero allocations, locks, or network I/O on the audio render thread.

---

## Non-goals

- Cloud streaming services (Tidal, Qobuz, Spotify, Google Drive, Dropbox, WebDAV).
- Multi-room network audio servers/renderers (RAAT, UPnP/DLNA, AirPlay) — deferred as an independent networking study.
- Format transcoding/ripping tools or mobile/PWA targets.

---

## Agreed Milestones

### Milestone 1 — Daily Driver Polish *(Wave 1 — Immediate)*
1. **Session & Queue Persistence:** Atomically persist `session.json` (queue file paths, current index, elapsed ms, repeat mode) and restore on launch (paused/stopped at exact position).
2. **Queue Ergonomics:** Drag-grip & keyboard (`ArrowUp`/`ArrowDown`) queue reordering (`queue_move`), live upcoming queue shuffle (`queue_shuffle`), and Save Queue as Playlist (`playlist_create_from_queue`).
3. **OS File Open, Drag-and-Drop & Reveal in Folder:** Open files/folders via OS file associations (`flac`, `m4a`, `wav`, `aiff`, `aif`, `dsf`, `dff`), startup CLI paths, and native window drag-and-drop overlay (`player_open_paths`); add `reveal_in_folder` in `RowMenu` and `QualityView`.
4. **Favorites (❤️) & Home Upgrade:** Path-keyed `favorites` table (SQLite schema v7), `FavoritesView`, heart toggles across track rows / transport / Now Playing / row menu, plus **Recently played** album shelf and **Favorites** shortcut on `HomeView`.
5. **Full Desktop Keyboard Shortcuts:** Media seek (`ArrowLeft`/`ArrowRight`, `Shift` for 30s), prev/next track (`Ctrl/⌘+Left/Right`), software volume step when active (`Ctrl/⌘+Up/Down`), inspector/view toggles (`L`, `Q`, `N`), and fullscreen (`F11` / `Ctrl+⌘+F`).

### Milestone 2 — Shared Mode Output (Opt-in) *(Wave 1 — Immediate)*
- **Settings → Output Mode:** `Exclusive` (default) vs `Shared` toggle (`OutputShareMode`).
- **Cross-platform mixer support:** WASAPI Shared (`StreamMode::EventsShared` with mix format on Windows), CoreAudio non-hog (`kAudioStreamPropertyVirtualFormat` on macOS), and ALSA `default` (Linux), plus `NullBackend` simulation.
- **Explicit Signal Path honesty:** Forces DSD → PCM (and disables PCM → DSD) when `Shared` is active, resamples to OS mixer rate when needed, appends an `"OS Mixer — Shared mode (not bit-perfect)"` stage, and marks `bitPerfect = false` with amber indicators across Settings, TransportBar, FormatBadges, and SignalPathPanel.

### Milestone 3 — Extended Formats *(Wave 2 — Planned Next)*
- **CUE Sheets (`.cue` + single-file FLAC/WAV):** Virtual track model (`start_frame` / `end_frame`) across `sonder-decode`, `sonder-library`, `sonder-engine`, and `sonder-quality`.
- **WavPack (`.wv`) & Common Lossy Formats (`.mp3`, `.aac`, `.ogg`, `.opus`):** Support playback & scanning while explicitly labeling lossy tracks with a `"Lossy"` badge in track lists, Now Playing, and Quality Check.

### Milestone 4 — Opt-in Integrations *(Wave 3 — Planned Next)*
- **Discord Rich Presence:** Local IPC presence showing playing track, artist, album, elapsed/remaining timestamps (off by default).
- **ListenBrainz & Last.fm Scrobbling:** Reuse `PlayTracker` (already filters ≥30s or 50% actual listening, ignoring seeks/pauses) with an offline retry queue and opt-in token/auth settings.

---

## Trade-offs & Phasing Decision

| Option | Load-bearing assumption | Fails first when | Cost to reverse |
|---|---|---|---|
| **Wave 1 (M1 + M2) first, M3 & M4 in separate waves (chosen)** | Daily ergonomics + Shared Mode unlock everyday desktop use immediately without destabilizing the decoder or adding offline HTTP/IPC complexity | Users primarily rely on `.cue`/`.wv` archives today | **Low** — clean modular boundaries |
| **Ship M1–M4 all together** | All 4 cross-cutting boundaries (UI/SQLite, OS audio backends, decoder virtual tracks, external RPC/HTTP) land cleanly at once | `.cue` virtual-track offsets complicate `Track`/queue/quality changes before M1 stabilizes | **High** — tangled diff across 8+ crates |

Better approaches: none — splitting Wave 1 (`M1 + M2`) from `M3` (virtual-track decoder refactor) and `M4` (IPC/HTTP integrations) delivers the highest daily-use value with minimal regression risk to bit-perfect playback.

---

## Acceptance Criteria (Wave 1)

1. `cargo test --workspace` (including all `sonder-verify` bit-perfect checks + new `ShareMode`, `Queue`, `Favorites`, and `Session` tests) passes.
2. `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --all -- --check` pass cleanly.
3. `pnpm --dir app check`, `pnpm --dir app test`, and `pnpm --dir app build` exit 0 with zero warnings (`en.ts` / `vi.ts` key parity enforced).
4. `scripts/check-forbidden-files.sh` passes; all UI changes honor `DESIGN.md` tokens, `reducedMotion()`, and `640px`–`1280px+` desktop responsiveness.
