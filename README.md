# Sonder

A local lossless music player: Rust core, Tauri UI, bit-perfect PCM and DSD playback through
exclusive-mode output on Windows, macOS and Linux, with a Roon-style DSP chain (headroom → SRC
→ convolution → PEQ → crossfeed → levelling → volume → dither, plus PCM→DSD).

## What it plays

- FLAC, ALAC (M4A), WAV, AIFF, DSF and DFF, up to 32-bit/384 kHz and DSD256.
- Exclusive mode on WASAPI (Windows), CoreAudio hog mode (macOS) and ALSA `hw:` (Linux).
- DSD as DoP, as native DSD words where the driver exposes them (ALSA), or converted to PCM.
- PCM → DSD64/128/256 with a 5th- or 7th-order single-bit modulator, carried by DoP or native
  DSD when the device takes it.

With every DSP option off and software volume off, the bytes written to the device are the
decoded samples (`crates/sonder-verify` proves it per format; `sonder-cli --verify-hash` proves
it on a real DAC).

## Requirements

- Rust 1.98 (see `rust-toolchain.toml`), Node 22 + pnpm 12, and the Tauri 2 prerequisites for
  your OS ([tauri.app](https://tauri.app/start/prerequisites/)).
- Linux additionally needs the ALSA headers (`libasound2-dev`) and WebKitGTK.
- A DAC is only needed for exclusive-mode playback; the test suite runs on the null backend.

## Build and run

```sh
# development (Vite + Tauri, hot reload)
cd app && pnpm install && pnpm tauri dev

# release installer for this OS
cd app && pnpm tauri build
```

The installer is written to `target/release/bundle/`. The core works without the UI:

```sh
cargo run -p sonder-cli -- devices
cargo run -p sonder-cli -- play ~/Music/album
cargo run -p sonder-cli -- play ~/Music/album --pcm-to-dsd dsd128 --sdm-order 5
cargo run -p sonder-cli -- play ~/Music/album --verify-hash
```

## Settings worth knowing

- **Output device** — exclusive mode is requested for every device; the list shows the formats
  each one accepts.
- **Volume** — off means bit-perfect. Software volume inserts a stage and marks the path
  "not bit-perfect".
- **DSD playback** — DoP, native DSD (Linux) or always PCM; the DSD→PCM rate is 176.4 or
  352.8 kHz.
- **PCM to DSD** — Off, DSD64, DSD128 or DSD256, with a 5th- or 7th-order modulator. 0 dBFS
  PCM maps to 50 % modulation (the SACD convention), so the DSD stream is 6 dB below full
  scale. On a 2022 laptop CPU the whole PCM→DSD256 path (resampling plus a 7th-order
  modulator) takes 0.4–0.7 of a core per stereo stream, DSD128 about half that; pick DSD128 if
  DSD256 drops out on a slower machine (see `docs/verification.md`).
- **Upsampling** — off, the device maximum, the source's rate family, or a fixed rate, with
  Sonder-designed filters (linear or minimum phase) or a folder of Roon tables you own.
- **Roon filter folder** — `Settings → Upsampling → Filter source: Roon` and pick the folder
  that holds Roon's `polyphase_*` tables. Sonder reads them from there at runtime and never
  copies them into its own files. A missing table for one ratio falls back to Sonder's filter
  and the signal path says which source was used. The tables are not distributed with Sonder
  (see `.gitignore` and `scripts/check-forbidden-files.sh`).
- **Convolution** — `Settings → Convolution → Choose…` takes a WAV impulse response (16/24/32-bit
  integer or 32-bit float, up to 2 s): one channel for every output channel, or one channel per
  output channel. It runs after upsampling, resampled once per stream rate; when the converter
  feeds the DSD modulator above 384 kHz it runs at the source rate instead. Settings shows the
  response's peak gain; a positive value is how much to lower the EQ preamp or headroom. A file
  that cannot be read turns convolution off with a notice; playback continues.
- **Parametric EQ** — up to 32 bands (peak, low/high shelf, low/high pass, band pass, notch)
  behind a preamp, saved as named presets. **Import…** reads an Equalizer APO parametric file —
  the `ParametricEQ.txt` AutoEQ publishes per headphone — and applies it as a preset named after
  the file, minus a trailing ` ParametricEQ`; an existing name asks before it is replaced. Lines
  meant for other APO stages and values Sonder clamps are listed in one warning notice, and a
  file that is not a parametric EQ is rejected without changing anything. **Export…** writes the
  current EQ in the same format (`Preamp:` then `Filter N:` lines) for Equalizer APO; importing
  that file gives the same EQ back. Types read: `PK`/`PEQ`, `LSC`/`LS`, `HSC`/`HS`, `LP`/`LPQ`,
  `HP`/`HPQ`, `BP`, `NO`; `BW Oct` is converted to Q and a shelf or pass without Q uses 0.707.
- **Crossfeed** — bs2b headphone crossfeed for stereo tracks, after the EQ, with libbs2b's
  presets: Default (700 Hz, 4.5 dB), Chu Moy (700 Hz, 6 dB) and Jan Meier (650 Hz, 9.5 dB).
  Centred sounds keep their level in the bass and dip 1–1.75 dB above the cut-off, as in bs2b.
- **Crossfade** — an equal-power fade (1–10 s, default 4) from one queue entry into the next,
  and on Next while playing. It is skipped between consecutive tracks of the same album, across
  different output formats, next to DSD files and under repeat-one; the signal path names the
  reason ("crossfade skipped: same album"). With crossfade off the transition is the existing
  gapless join, byte for byte.
- **Repeat** — the button next to Next cycles off → all → one. Repeat-one replays the entry
  gaplessly and manual Next still moves on; repeat-all wraps from the last entry to the first
  without a gap; turning it off keeps the entry already decoded ahead.
- **Mini player** — `Settings → Mini player` (on by default): closing the main window while a
  track is loaded hides it and opens a small always-on-top window in the screen's bottom-right
  corner with the cover, a spinning record, and previous / play-pause / next on hover. Drag it
  anywhere; double-click it or use its expand button to bring the main window back; its close
  button quits Sonder. With the setting off, or nothing loaded, closing the window quits as
  before.
- **Updates** — `Settings → Updates` shows the running version and checks the project's GitHub
  releases; **Check on launch** (on by default) does the same once at start-up and raises a
  notice when a newer version is out. **Install … and restart** downloads the installer,
  verifies its minisign signature against the public key built into the app, installs it and
  restarts. Nothing is downloaded or installed without that click, and a check sends no library
  data.
- **Language** — `Settings → Appearance → Language`: English, Tiếng Việt, or System (the
  first of the OS's preferred languages Sonder has, else English). The interface switches at
  once; messages that come from the audio engine or the OS (device errors, signal-path stage
  names) stay in English. Catalogs live in `app/src/lib/locales/`; `vi.ts` is typed against
  `en.ts`, so a missing or extra key fails `pnpm check`.

The Now Playing view shows the signal path stage by stage (the icon next to the device line).

## Playlists

Playlists live in the library database (schema version 4 or later; an older database is backed up as
`library.db.bak-3` before it is upgraded). Create one on the Playlists page or from any track's
⋯ menu → **Add to playlist** → **New playlist…**; a playlist page plays, shuffles, plays next or
queues the whole list, and each row can be dragged by its grip (or moved with Up/Down on the
grip) and removed from the ⋯ menu. Renaming and deleting are in the page's ⋯ menu; deleting
asks first and never touches the tracks.

Entries point at the file path, not the library's track id, so they survive rescans. A file that
is moved or deleted stays in the playlist as "not in library" and comes back when a rescan finds
it at the same path again.

## Editing metadata

Any track's ⋯ menu → **Edit metadata** shows the tags stored in the file itself — title,
artists, album, album artists, track and disc number, year, genre, the embedded cover and the
lyrics — and **Save** writes them
back into that file, then re-indexes the track in place (its album follows the new tags). FLAC,
ALAC (M4A), WAV and AIFF are written through lofty; DSF and DFF get their ID3v2 tag replaced in
the container, and a tag a DFF file carries after its form (as sacd_extract writes one) is
rewritten where it is. ReplayGain, comments and every other tag are left untouched; the cover
and lyrics change only when you change them. A new cover (JPEG or PNG) becomes the front cover
and other pictures stay; removing the cover removes every embedded picture. Lyrics are stored as
typed — plain text, or LRC with `[mm:ss.xx]` times, which the lyrics panel then follows. Several artists are typed separated by semicolons and stored as separate values, so
each one is its own artist in the library and in other players. Every tag the file carries gets
the edit (a WAV's RIFF INFO next to its ID3v2 tag, for example), so a cleared field stays
cleared. The file is rebuilt through a temporary file beside it that keeps the original's
permissions, so a failed write leaves the original as it was, and a read-only file or folder
reports the error. The editor shows what the file holds: a track number the library guesses
from the file name is not offered for saving. A tag edit keeps the loudness already measured for that audio; a
track whose album changed gets its new album's value on the next analysis pass.

Nothing in the editor goes online on its own. **Find info online** sends the title, artist and
album as typed to iTunes, Deezer and MusicBrainz at once and lists what they know, the ones
whose length matches the track first; picking one fills in the fields it knows and downloads
its cover (from the catalogues' own image servers and Cover Art Archive only). **Find lyrics
online** asks the same services as the lyrics panel (LRCLIB, BetterLyrics, LyricsPlus, Kugou)
with the typed fields, even when online lyrics are off in Settings, and puts the answer in the
lyrics box. Either way nothing is written until **Save**.

## Quality check

A track's ⋯ menu → **Check quality** decodes the whole file and says whether it is the
lossless file it claims to be: **Genuine lossless**, **Suspect**, **Transcoded from lossy**,
**Fake hi-res (upsampled)**, **Fake bit depth** or **Inconclusive** (too short, too quiet or too
little treble; not a pass), each with a confidence. The report lists the evidence behind it,
row by row:

- **Spectral cutoff, slope and stability** — MP3/AAC encoders low-pass at a fixed 16–20.5 kHz,
  so a transcode has a steep, flat "lid" across the whole track; real recordings reach above
  21.5 kHz or fade out gradually with a top edge that moves with the music.
- **Spectral holes** and **high-frequency stereo** — treble bands that drop out from moment to
  moment, and treble that collapses towards mono (joint stereo), are lossy fingerprints.
- **Real bit depth** — read exactly from the samples: a 24-bit file whose low byte is always
  zero is padded 16-bit.
- **Hi-res content** — a 88.2 kHz-or-higher file with a wall at 20–24 kHz was upsampled.
- **DSD source** — a DSD file with a CD-rate wall under its modulator noise is reported as
  suspect, never fake (analog transfers can look the same).
- **FLAC MD5** — proves the file decodes to what was encoded; it says nothing about the source.

It also shows the average spectrum with the detected cutoff and a spectrum-over-time map. A whole
album (the spectrum button on its page), a playlist (its ⋯ menu) or every song (the Songs page)
can be checked as a batch, one track at a time, with progress and Cancel in the sidebar; a
track whose cached result is still valid is not decoded again. Results are stored in the library
database (schema version 5; a version-4 database is backed up as `library.db.bak-4` first), keyed
by path and dropped when the file changes, and show as a small badge in track lists. Nothing is
uploaded and no file is changed. It is a spectral estimate: thresholds were tuned on synthetic
signals and a sample of real albums, and an encoder with an unusual low-pass can still read as
genuine.

```sh
cargo run --release -p sonder-cli -- check ~/Music/album          # text report per file
cargo run --release -p sonder-cli -- check track.flac --json      # the full report as JSON
```

## Verification

```sh
cargo test --workspace          # unit + integration, includes the acceptance suite
cargo clippy --workspace --all-targets -- -D warnings
cargo bench -p sonder-dsp --bench sdm
cargo bench -p sonder-dsp --bench convolver
bash scripts/check-forbidden-files.sh
pwsh tools/check_bundle.ps1     # add -Build to build the installer first
python tools/design_sdm.py      # modulator stability and noise shaping
```

`crates/sonder-verify/tests/verify.rs` is the acceptance suite: byte-exact playback per format,
the device tap the `--verify-hash` hook uses, resampler image rejection, PEQ response against the
RBJ formulas, dither, DoP round-trip, the SDM round-trip and the signal-path model.
`docs/verification.md` records what has been run on real hardware and what is still open.

## Releasing

Releases are what the in-app updater follows: it reads `latest.json` from the newest published
(not draft, not pre-release) release. Every installer must be signed with the updater key whose
public half is `plugins.updater.pubkey` in `app/src-tauri/tauri.conf.json`. Losing that private
key means installed copies can no longer be updated, so keep a backup of it.

1. Bump `version` in the root `Cargo.toml` (the app takes the workspace version) and in
   `app/package.json`, merge to `main` and push a tag such as `v0.2.0`.
2. The `release` workflow checks that the tag matches both versions, builds Windows, universal
   macOS and Linux installers, signs them with the `TAURI_SIGNING_PRIVATE_KEY` and
   `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` repository secrets and attaches them with `latest.json`
   to a draft release. Before publishing the draft, open its `latest.json` and confirm it lists
   every platform (the build jobs run in parallel and can overwrite each other's entries). The in-app release notes are
   copied into `latest.json` at build time, so editing the draft's text does not change them.
3. Without the workflow, build on each OS with the key in the environment, then write the
   manifest; on a second OS pass the first manifest with `--merge` so one file lists both.
   The script prints each installer and `.sig` to attach as an `attach:` line:

   ```sh
   export TAURI_SIGNING_PRIVATE_KEY="$(cat ~/.tauri/sonder-updater.key)"
   export TAURI_SIGNING_PRIVATE_KEY_PASSWORD=""
   (cd app && pnpm tauri build)
   node scripts/updater-manifest.mjs v0.2.0 --notes notes.md > latest.json
   gh release create v0.2.0 latest.json <every attach: path> --notes-file notes.md
   ```

The updater downloads release assets without credentials, so it only works while the
repository's releases are publicly readable.

## Repository layout

```
crates/sonder-decode   FLAC/ALAC/WAV/AIFF via symphonia; DSF/DFF readers; DSD bit plumbing
crates/sonder-output   exclusive-mode backends, renderer, DoP markers, null backend for tests
crates/sonder-dsp      FIR core, resamplers, PEQ, levelling, volume, dither, DSD→PCM, SDM
crates/sonder-engine   player thread, gapless queue, DSP chain order, signal path, DoP packing
crates/sonder-library  SQLite library, scanner, art cache, loudness analysis, playlists
crates/sonder-lyrics   LRC/TTML and online lyrics with a disk cache
crates/sonder-metadata track info and covers from iTunes, Deezer and MusicBrainz
crates/sonder-quality  lossless quality check: spectral cutoff, lossy fingerprints, real bit depth
crates/sonder-cli      device listing, bit-perfect playback harness, library scanner, quality check
crates/sonder-verify   acceptance/verification suite
app                    Svelte UI + Tauri shell
tools                  SDM table tool, bundle check
plans                  phase plans and evidence
```
