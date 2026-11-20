# Cross-Platform Cosmic Glass Custom Title Bar

**Date:** 2026-11-20  
**Tasks:**
- `/ak-cook Làm giúp tôi phần thanh tiều đề này đẹp hơn thân thiện phù hợp với cả 3 hệ điều hành`
- `/ak-code-review` & `/ak-fix sửa cái finding giúp tôi`

---

## 1. Goal & Design

Replace the plain OS-drawn title bar with an integrated, atmospheric `--titlebar-h: 40px` cosmic glass header that matches Sonder's `DESIGN.md` aesthetic while respecting the native window idioms of **macOS**, **Windows**, and **Linux**.

### Per-Platform Window Architecture
- **macOS (`TitleBarStyle::Overlay` + `hiddenTitle: true`):** Keeps native traffic lights (centered in the 40px bar via `.traffic_light_position(14.0, 18.0)`), native rounded corners, shadows, green-button tiling, and native Space fullscreen transitions. Left inset clears traffic lights (`78px` windowed, easing to `14px` in fullscreen) and uses macOS shortcuts (`⌘F`, `⌘B`, `⌘[`).
- **Windows (`decorations: false`, `shadow: true`, `Theme::Dark`):** Uses DWM rounded corners, dark frame border, and drop shadow (`transparent: false` for clean WebGL/backdrop-filter compositing), paired with flush `46px × 100%` Fluent 11 caption controls on the top-right (preserving Fitts's Law) and 8-way resize handles stopping clear of the caption cluster (`right: 142px`).
- **Linux (`decorations: false`, `Theme::Dark`):** Uses circular frosted GNOME/Adwaita-inspired caption buttons, a subtle `1px` inset hairline around unmaximized floating windows, and 8-way edge/corner resize handles via `startResizeDragging`.

### Friendly Title Bar Controls (`TitleBar.svelte`)
- **Left:** Spinning vinyl brand button (navigates Home), sidebar collapse toggle, and history Back button (removing the redundant sidebar `.brand` header and the floating in-page `.back` circle).
- **Center:** Cosmic-glass quick search pill displaying active query or localized placeholder plus OS-appropriate shortcut keycap (`⌘F` / `Ctrl F`), collapsing cleanly down to `640px` split-snap width.
- **Right:** Always-on-top Pin button + OS-idiomatic window controls (Windows Fluent / Linux Adwaita; omitted on macOS).

---

## 2. Review Findings Resolved (`/ak-code-review` → `/ak-fix`)

1. **OS-safe global shortcuts (`platform.ts`, `App.svelte`):** Extracted `resolveGlobalShortcut()` so macOS `⌥←` word-navigation inside inputs is never hijacked (using `⌘[` instead), shortcuts are gated by OS-appropriate primary modifiers (`⌘` vs `Ctrl`), modal dialogs (`app.dialog`, `app.editing`, `app.signalPathOpen`) block background shortcuts, and `Ctrl/Cmd+F` focuses the search input via `app.openSearch()`.
2. **Modal & Popover stacking (`NowPlayingView.svelte`, `Popover.svelte`, `app.svelte.ts`):**
   - Converted Signal Path modal from `z-index: 100` inside `main.full` to a native top-layer `<dialog>` clearing `TitleBar` (`z-index: 200`), and reset `app.signalPathOpen` when navigating away from `"now"`.
   - Clamped `Popover.svelte` `top` bounds below `--titlebar-h + 12px` with scrollable `max-height`.
3. **macOS overlay positioning, dark theme & fullscreen settle (`main.rs`, `TitleBar.svelte`):** Added `.theme(Some(Theme::Dark))`, `.traffic_light_position(14.0, 18.0)`, compile-time `window.__SONDER_OS__` injection, and macOS post-resize settle sync timers (`180ms`, `600ms`).
4. **Async window safety (`TitleBar.svelte`):** Awaited `startResizeDragging` inside `withWindow`, attached event listeners before `Promise.allSettled` hydration, serialized `toggleMaximize` through `syncBoundsState()`, and serialized `togglePin()` with `pinSeq` + `isAlwaysOnTop()` verification.
5. **Least-privilege capabilities (`capabilities/`):** Kept `default.json` minimal for `["main", "mini"]` (`core:event:default`, `core:window:default`, `core:window:allow-start-dragging`), and split `main-window.json` (`allow-set-always-on-top`) and `main-frameless.json` (`platforms: ["windows", "linux"]` for resize/min/max/close).
6. **UI polish & browser dev mock (`TitleBar.svelte`, `NowPlayingView.svelte`, `AlbumView.svelte`, `ArtistView.svelte`, `PlaylistView.svelte`, `main.ts`, `en.ts`, `vi.ts`):**
   - Fixed hover-opacity specificity inversion when unfocused on Now Playing, added dark top scrim for WCAG AA contrast over bright album covers, added inset `:focus-visible` ring on `.win-btn`, expanded Windows resize handles to 8-way, raised `NowPlayingView` vertical scroll fallback to `max-height: 720px`, trimmed stale `84px` hero top padding on detail views, fixed browser dev-mode `__TAURI_EVENT_PLUGIN_INTERNALS__` & `library_albums` mock shapes, and removed dead `nav.back` / `nav.expandSidebar` / `nav.collapseSidebar` locale keys.

---

## 3. Files Modified & Created

- **Rust / Tauri:**
  - `app/src-tauri/tauri.conf.json`
  - `app/src-tauri/src/main.rs`
  - `app/src-tauri/capabilities/default.json`
  - `app/src-tauri/capabilities/main-window.json` *(new)*
  - `app/src-tauri/capabilities/main-frameless.json` *(new)*
- **Frontend & Tests:**
  - `app/src/components/TitleBar.svelte` *(new)*
  - `app/src/lib/platform.ts` *(new)*
  - `app/src/lib/platform.test.ts` *(new)*
  - `app/src/App.svelte`, `app/src/app.css`, `app/src/main.ts`
  - `app/src/lib/app.svelte.ts`, `app/src/lib/shell.test.ts`
  - `app/src/components/NowPlayingView.svelte`, `Popover.svelte`, `SearchView.svelte`, `AlbumView.svelte`, `ArtistView.svelte`, `PlaylistView.svelte`
  - `app/src/lib/locales/en.ts`, `app/src/lib/locales/vi.ts`
- **Docs:**
  - `DESIGN.md`

---

## 4. Verification & Known Limitations

- `pnpm --dir app check` (`svelte-check --fail-on-warnings`): **0 errors, 0 warnings**.
- `pnpm --dir app test`: **14/14 test files, 89/89 tests passed** (including `platform.test.ts`, `shell.test.ts`, `i18n.test.ts`).
- `pnpm --dir app build`: **succeeded cleanly**.
- `cargo fmt --all -- --check`: **clean exit**.
- `cargo clippy -p sonder-app --all-targets -- -D warnings` & `cargo test -p sonder-app` (verified in an isolated `--target-dir` while the user's running `sonder.exe` PID `26432` held `D:\Dev\.cargo-target\debug\WebView2Loader.dll`): **0 warnings, 28/28 tests passed** (including `typescript_bindings_are_up_to_date`).
- **Remaining manual checks:** Live visual/interactive smoke testing on Windows 11 (Light/Dark theme border, edge resize, Snap Assist), macOS (traffic light alignment, native Space fullscreen), and Linux (Wayland/X11 8-way resize).
