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
- **Updates** — `Settings → Updates` shows the running version and checks the project's GitHub
  releases; **Check on launch** (on by default) does the same once at start-up and raises a
  notice when a newer version is out. **Install … and restart** downloads the installer,
  verifies its minisign signature against the public key built into the app, installs it and
  restarts. Nothing is downloaded or installed without that click, and a check sends no library
  data.

The Now Playing view shows the signal path stage by stage (the icon next to the device line).

## Playlists

Playlists live in the library database (schema version 4; an older database is backed up as
`library.db.bak-3` before it is upgraded). Create one on the Playlists page or from any track's
⋯ menu → **Add to playlist** → **New playlist…**; a playlist page plays, shuffles, plays next or
queues the whole list, and each row can be dragged by its grip (or moved with Up/Down on the
grip) and removed from the ⋯ menu. Renaming and deleting are in the page's ⋯ menu; deleting
asks first and never touches the tracks.

Entries point at the file path, not the library's track id, so they survive rescans. A file that
is moved or deleted stays in the playlist as "not in library" and comes back when a rescan finds
it at the same path again.

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
2. The `release` workflow builds Windows, macOS and Linux installers, signs them with the
   `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` repository secrets and
   attaches them with `latest.json` to a draft release. Publishing the draft ships it.
3. Without the workflow, build on each OS with the key in the environment, then write the
   manifest and attach it with the installers and their `.sig` files:

   ```sh
   export TAURI_SIGNING_PRIVATE_KEY="$(cat ~/.tauri/sonder-updater.key)"
   export TAURI_SIGNING_PRIVATE_KEY_PASSWORD=""
   (cd app && pnpm tauri build)
   node scripts/updater-manifest.mjs v0.2.0 notes.md > latest.json
   gh release create v0.2.0 latest.json target/release/bundle/nsis/*-setup.exe* --notes-file notes.md
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
crates/sonder-cli      device listing, bit-perfect playback harness, library scanner
crates/sonder-verify   acceptance/verification suite
app                    Svelte UI + Tauri shell
tools                  SDM table tool, bundle check
plans                  phase plans and evidence
```
