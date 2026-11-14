# Feature Comparison: Synq 1.5.1 vs Sonder

## Source: `D:\Dev\reverse\Synq_Offline_Music_1.5.1` (com.synqmusic.app, Flutter AOT, proprietary)
## Local project: Sonder (Rust core + Tauri/Svelte, desktop)

Evidence: `docs/README.md` and `dart/app_source_files.txt` of the reverse-engineered APK
(file and string names only; no Dart source exists). Nothing can be copied; every item below
would be Sonder's own implementation of the idea.

## Head-to-head

| Synq feature | Sonder today | Value for Sonder | Effort | Recommendation |
|---|---|---|---|---|
| Lossless check (spectrum + bit depth) | none | High | ~3 d | **Planned**: `plans/261114-1600-lossless-quality-check` |
| AutoEQ catalog (6,017 headphones bundled, search by name, `headphone_match`) | imports one `ParametricEQ.txt` file | High: one click from headphone name to EQ | ~1.5 d (fetch AutoEQ index on demand, no bundling) | **Add next** |
| Listening log + "Wrapped" stats (top artists/albums/tracks, time listened) | no play history | High: local-only, uses existing library DB | ~2 d | **Add** |
| Sleep timer (also in Music Check) | none | Medium, trivial | ~0.5 d | **Add** (stop after N min or end of track/album) |
| M3U/M3U8 playlist import/export | playlists by path, no file I/O | Medium: move playlists in/out of other players | ~0.5 d | **Add** |
| Favourites | none | Medium | ~0.5 d | Optional, pairs with stats |
| Purist / bit-perfect one-tap switch | DSP stages toggled one by one; signal path shows bit-perfect | Low–Medium: convenience | ~0.5 d | Optional: a "Purist" toggle that bypasses all DSP and remembers previous settings |
| Hearing profile + hearing test wizard | none | Low: uncalibrated headphones make it unreliable | 2–3 d | Skip |
| Cloud streaming (Dropbox / Drive / OneDrive / WebDAV) | local files | Low–Medium: conflicts with local bit-perfect focus, OAuth upkeep | 1–2 wk | Later; WebDAV/SMB via mounted drives already works |
| Chromecast + local HTTP media server | none | Medium for some users; UPnP/DLNA renderer output fits a Roon-like player better | 1–2 wk | Later, as a separate output backend study |
| Wi-Fi transfer server | n/a | Low on desktop | — | Skip |
| Synced lyrics (`lrc_parser`, lrclib) | already (`sonder-lyrics`, LRC + TTML) | — | — | Have |
| ReplayGain / normalization | already (tags + R128 analysis, levelling) | — | — | Have |
| DSD/DoP bit-perfect USB | already (WASAPI/CoreAudio/ALSA, DoP, native DSD) | — | — | Have |
| Artwork search (iTunes) | metadata editor online lookup | — | — | Have |
| Achievements, theme store, IAP, ads, social sign-in | — | None | — | Skip |

## Recommendation

1. Lossless quality check (planned now).
2. AutoEQ search by headphone name — extends the PEQ import already shipped.
3. Listening history + stats, with favourites.
4. Sleep timer and M3U import/export — small, can ship together.
