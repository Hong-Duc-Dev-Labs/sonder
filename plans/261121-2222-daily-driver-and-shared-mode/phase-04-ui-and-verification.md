# Phase 4 — Frontend UI, Bilingual i18n & Full Verification

## Goal

Expose all Wave 1 features cleanly in the Svelte 5 Cosmic Glass UI with full English/Vietnamese localization, regenerated TypeScript bindings, `reducedMotion()` compliance, and full desktop breakpoint verification.

## Changes

- `app/src/lib/bindings.ts`: regenerated via `SONDER_UPDATE_BINDINGS=1 cargo test -p sonder-app typescript_bindings_are_up_to_date`.
- `app/src/lib/locales/{en,vi}.ts`: complete parity for Shared Mode, Queue actions, Favorites, Home shelves, Reveal in Folder, and File Drop overlay.
- `app/src/lib/{app.svelte.ts,lyrics.ts}` & `app/src/components/{Icon,SettingsView,TransportBar,FormatBadges,SignalPathPanel,QueuePanel,FavoritesView,TrackList,RowMenu,QualityView,HomeView,NowPlayingView,LyricsPanel,LyricStage}.svelte` & `app/src/App.svelte`.

## Verification

- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo fmt --all -- --check`
- `pnpm --dir app check`
- `pnpm --dir app test`
- `pnpm --dir app build`
- `bash scripts/check-forbidden-files.sh`
