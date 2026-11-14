---
title: Fix macOS crossfade test timeout and duplicate Windows manifest
date: 2026-11-14
summary: Faster null backend for the 44 s crossfade test; shadow WinLibs default-manifest.o on windows-gnu
---

# Fix macOS crossfade test timeout and duplicate Windows manifest

## What happened
- #18: `spawn_clamps_the_crossfade_length` timed out on macOS CI. It plays two 22 s tracks
  (~1.94M frames) through `NullBackend::new()`, which pulls 512 frames per `sleep(1ms)`:
  ~3 800 sleeps. Locally 4.85 s (~1.27 ms/period); coarse timers on a loaded runner push it past
  the 20 s `TIMEOUT`. The tracks must stay > 20 s because the fade is capped at half the shorter track.
- #17: WinLibs GCC's spec links `default-manifest.o` (`%{!shared:%:if-exists(default-manifest.o%s)}`).
  WinLibs builds it with language 0x409, not neutral, so ld cannot drop it as the default manifest
  and leaves two RT_MANIFEST entries (id 1, lang 0x409) in sonder.exe: Tauri's Common-Controls v6
  one and the toolchain's longPathAware/asInvoker one.

## Decision
- Test uses `with_period(8_192, 1ms)` (~240 sleeps); 2.16 s locally.
- `app/src-tauri/build.rs` on windows-gnu writes an empty 20-byte COFF object as
  `OUT_DIR/no-default-manifest/default-manifest.o` and passes `-B<dir>`, so gcc picks it up first.
  Verified: no linker warning, a single RT_MANIFEST (Tauri's), app launches.

## Next steps
- Commit and confirm the next macOS CI run passes.

> Historical work record — not durable authority. Prefer docs/specs/ADRs for current decisions.
