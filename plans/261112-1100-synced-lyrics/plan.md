# Synced lyrics

Status: implemented, awaiting an in-app visual check.

## Outcome

Now Playing can show lyrics in the right-hand column beside the record (the record stays).
Synced lyrics follow playback, light the sung line and seek on click; plain lyrics scroll.

## Decisions (from the user)

- Sources: the file's own first (embedded tag, then a same-named `.lrc`), then online services
  "like Mochy-Music".
- Online lookup on by default, switchable in Settings, which states what is sent.
- Online results, including "none found", cached in the app data folder (`<data>/lyrics`).
- Display: lyrics column beside the disc, toggled from Now Playing's tools row.

## Sources

| Order | Source | Notes |
|---|---|---|
| 1 | Embedded tag | lofty `ItemKey::Lyrics`; LRC or plain |
| 2 | `<track>.lrc` | UTF-8, Latin-1 fallback |
| 3 | LRCLIB | `/api/get` with length, then `/api/search` matched within 3 s |
| 4 | BetterLyrics | TTML, asked together with 5 and 6 when LRCLIB has no synced lyrics |
| 5 | LyricsPlus | two mirrors, second only on failure |
| 6 | Kugou | candidate matched on length within 3 s; leading credit lines dropped |

Synced beats plain across all sources. Sent online: title, artist, album, length.

Not ported, with reasons:

- Paxsenix: Cloudflare answers 403 to non-app clients.
- Musixmatch: Mochy's code scrapes web pages and gets past the WAF with a hidden browser.
- SimpMusic: it needs a YouTube video id, and local files have none.
- Genius and Directlyrics: they are HTML scraping and give plain text only.

## Where

- `crates/sonder-lyrics`: LRC/TTML parsing, providers, cache, source choice. The live test is
  `cargo test -p sonder-lyrics -- --ignored`.
- `app/src-tauri/src/commands.rs` (`lyrics_get`), `dto.rs` (`Lyrics`), `settings.rs` (`lyricsOnline`).
- `app/src/components/LyricsPanel.svelte`, `app/src/lib/lyrics.ts`, Now Playing and Settings views.

## Acceptance

- [x] Unit tests for LRC, TTML, cache, local source order, line lookup.
- [x] Live provider test passes for all four services.
- [x] svelte-check, vitest, vite build, cargo tests, bindings drift test.
- [ ] Visual check in the running app (follow scroll, click-to-seek, narrow pane).
