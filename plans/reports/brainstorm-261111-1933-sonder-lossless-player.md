# Brainstorm — Sonder: lossless music player in Rust

Date: 2026-11-11 · Status: accepted direction, ready for planning

## Evidence

- `D:\Dev\Sonder` is empty (not a git repo yet). Rust 1.98.1 / cargo installed.
- `D:\Dev\reverse\roon` holds a static analysis of Roon 2.65: decompiled C# (6005 files),
  `DSP-NOTES.md` (DSP chain order, block inventory), and 76 polyphase SRC filter tables
  extracted from `Roon.Audio.resource` (`extracted/filters/*.csv`, `filters_manifest.json`).
- Roon's DSP chain (from `DspConfig.Build()`): headroom → SRC → user DSP (convolution,
  PEQ, crossfeed…) → speaker setup → PCM→DSD (SDM) → bit-depth + TPDF dither → channel map.

## Contract

**Outcome:** a desktop app "Sonder" that scans a local music library, browses it with an
album-art-first UI, and plays lossless files bit-perfect to a DAC, with an optional
Roon-style DSP chain (upsampling, PEQ, ReplayGain/volume levelling, PCM→DSD).

**Constraints**
- Rust core; Tauri 2 shell with a web UI (TypeScript).
- Formats: FLAC, ALAC, WAV, AIFF up to 32-bit/768 kHz; DSD64–DSD256 (DSF/DFF) via DoP
  and native DSD where the driver allows, plus DSD→PCM.
- Platforms: Windows (WASAPI exclusive; ASIO optional), macOS (CoreAudio hog/integer
  mode), Linux (ALSA `hw:` direct).
- Bit-perfect path must be provable: with DSP off, output samples equal decoded samples.
- Audio thread is real-time safe: no allocation, locks or I/O in the render callback.
- **Roon filter tables (user decision):** Sonder may load the extracted Roon polyphase
  filters. They are Roon Labs' copyrighted data, so they are loaded at runtime from a
  user-configured local path and are never committed to the repo or bundled in a build.
  Sonder ships its own designed filters (Kaiser-windowed sinc, linear + minimum phase) as
  the default, so the app works and stays distributable without the Roon tables.
- No decompiled Roon C# is copied into Sonder; Roon's code is reference for behaviour only.

**Non-goals (this delivery)**
- Core/remote split, phone remote, multi-zone, RAAT/AirPlay/Chromecast/UPnP output.
- Streaming services (Qobuz/Tidal), internet radio, Roon Radio-style recommendations.
- Online metadata matching (MusicBrainz/Discogs); tags + embedded/folder art only.
- MQA decoding, convolution room correction, multichannel speaker setup (later phases).

**Acceptance criteria**
1. Scanning a folder of mixed FLAC/ALAC/WAV/AIFF/DSF/DFF files builds a library browsable
   by album/artist/track, with embedded or folder cover art.
2. With DSP off, a capture/loopback or null test shows bit-identical output for 16/44.1,
   24/96 and 24/192 files on Windows WASAPI exclusive (and on macOS/Linux equivalents).
3. Gapless playback between tracks of the same format; correct device rate switching.
4. DSD files play as DoP on a DoP-capable DAC and as DSD→PCM otherwise.
5. Upsampling to a selected rate/rate family using either Sonder's own filters or, when
   configured, the Roon tables; output passes a sine/sweep test with no clipping and the
   expected image rejection. PCM→DSD (SDM order 5/7) produces playable DoP.
6. PEQ, ReplayGain/levelling and TPDF dither when reducing bit depth are selectable.
7. The signal path is visible in the UI (source format → each DSP stage → device format).
8. Roon filter files are absent from git and from release bundles.

## Approaches compared

| | A. Rust crates + Sonder DSP (recommended) | B. Wrap native libs (FFmpeg/libsoxr/BASS) | C. Write everything from scratch |
|---|---|---|---|
| Decode | `symphonia` (FLAC/ALAC/WAV/AIFF); own DSF/DFF parser | FFmpeg | own decoders |
| Output | `cpal` for shared mode + thin per-OS exclusive backends (`wasapi` crate, CoreAudio, `alsa`) | BASS/PortAudio | raw OS APIs |
| DSP | own polyphase resampler (loads Sonder or Roon tables), biquads, SDM, dither | libsoxr | same as A |
| Depends most on | `cpal` lacking exclusive/hog mode → need own backends | C toolchain + licensing (FFmpeg LGPL, BASS paid) | time |
| Fails first when | a DAC needs ASIO/native-DSD quirks | cross-compile on 3 OSes, bit-perfect hard to prove through wrappers | decoder edge cases eat the schedule |

**Trade-offs:** A depends on writing three small exclusive-mode output backends ourselves,
since `cpal` does not expose WASAPI exclusive or CoreAudio hog mode; that is the riskiest
piece and should be the first spike. B is fastest to "sound" but hides the bit-perfect
path and adds licensing; C has the worst worst-case.

**Better approaches:** none — recommended direction matches the request (Rust core; the
reverse-engineered notes map cleanly onto a pure-Rust DSP chain, and the filter tables are
plain coefficient arrays a polyphase FIR can consume).

**Evolutionary seams (evidenced, not speculative)**
- `FilterSource` trait: `SonderDesigned` and `RoonTables(path)` — two real variants now.
- `OutputBackend` trait: WASAPI-exclusive, CoreAudio-hog, ALSA-hw — three real variants.
- DSP stages as a `Vec<Box<dyn Stage>>` built in a fixed order mirroring the chain above.
- Remote/core split is deferred: keep the player engine a library crate behind Tauri
  commands so a network API can be added later without moving code.

## Proposed crate layout

```
sonder/
  crates/sonder-decode   # symphonia + DSF/DFF
  crates/sonder-dsp      # resampler, filters, biquad/PEQ, SDM, dither, levelling
  crates/sonder-output   # backends + device format negotiation
  crates/sonder-engine   # queue, gapless, signal-path model, real-time ring buffer
  crates/sonder-library  # scanner, tags, art, SQLite index
  app/                   # Tauri 2 + web UI
```

## Suggested phase order

1. Spike: decode FLAC → WASAPI exclusive bit-perfect on Windows (proves the hardest risk).
2. Engine: queue, gapless, ring buffer, rate switching; CoreAudio + ALSA backends.
3. Library scanner + SQLite + Tauri UI (albums, now playing, queue).
4. DSD: DSF/DFF, DoP, DSD→PCM.
5. DSP: polyphase resampler + `FilterSource` (Sonder + Roon tables), PEQ, dither, levelling.
6. PCM→DSD (SDM), signal-path UI, verification suite (null/sweep tests).

## Unresolved questions

- ASIO on Windows: needed for native DSD on many DACs; it requires the Steinberg SDK
  (licence terms) — include in MVP or defer?
- UI framework inside Tauri: React or Svelte?
- Where Roon filter tables live by default (e.g. a settings path pointing at
  `D:\Dev\reverse\roon\extracted\filters`).
