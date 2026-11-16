# Feature Comparison: Running Lyrics Effect & Stage Visualizer

## Source: egoist/mygo-music (commit `298c88a`)
## Local Project: Sonder (Tauri 2 + Svelte 5 + Rust)

## Head-to-Head

| Aspect | Source (`mygo-music`) | Local Project (`Sonder`) | Recommendation |
|---|---|---|---|
| **Language & GUI** | Go + MyGo custom immediate-mode 2D GUI canvas | TypeScript + Svelte 5 (Runes) + CSS Transitions / GPU Compositor | Port math & algorithms to idiomatic TypeScript/Svelte. Use CSS custom properties and GPU transforms for 60fps rendering. |
| **Lyrics Input** | Standard LRC `[mm:ss.xx]` + custom word-timed JSON from plugins | Standard LRC & TTML via `crates/sonder-lyrics` (`start_ms`, `text`) | Re-use existing Rust provider pipeline without breaking changes. |
| **Word Timing Synthesis** | `SpreadWords` algorithm: 85% singing time, 15% breath pause, proportional character weight, CJK/spaced handling | None (lines are monolithic strings) | **Port `spreadWords` to `app/src/lib/lyrics.ts`**. Allows every `.lrc` file to enjoy running karaoke without needing syllable-level provider tags. |
| **In-column Lyrics Display** | `timedLines`: words rendered as individual spans with progressive brightness at 25fps (`ui.RichText`) | `LyricsPanel.svelte`: active line highlights statically as a whole line (`opacity: 1`) | **Upgrade `LyricsPanel.svelte`**: render words with dynamic running fill/glow. |
| **Scroll Kinetics** | `lyricGlide`: custom critically-damped spring (`omega = 10`) calculating DIP lag | `scroller.scrollTo({ top, behavior: "smooth" })` with 4s user-drag hold (`heldUntil`) | Keep Sonder's robust `heldUntil` hold-scroll logic, enhance vertical centering offset to ~38%. |
| **Interlude Handling** | `interlude`: 3 breathing dots pulsing with `sin(time / 420)` during breaks > 4s | Blank lines or empty space | **Port 3 breathing dots indicator** for breaks > 4s. |
| **Visualizer Stage Mode** | Dedicated Now Playing "Stage" tab with Folia's Flow, Steps, and Tilt styles + Sparks | Turntable (Vinyl) or Square cover in `NowPlayingView.svelte` | **Add Lyric Stage visualizer mode** to `NowPlayingView.svelte` as an alternative hero display alongside Vinyl and Square. |

---

## Architectural Adaptation Notes

1. **Deterministic Pseudo-Random Layout**:
   Folia & MyGo use `sin(seed + offset) * 10000 % 1` seeded by `line.startMs`. This ensures that every line has an organic, non-uniform, scattered layout, yet produces the *exact same frame* when seeking forward or backward. We port this exact function to `stageMotion.ts`.

2. **Critically-Damped Spring Dynamics**:
   The formula `1 - exp(-10t) * (cos(10t) + sin(10t))` provides natural spring feel without oscillating endlessly. It reaches within 0.5px of resting state in ~500ms.

3. **Accessibility**:
   All animations gracefully degrade when `reducedMotion()` is true, using instant or linear fades instead of transforms.
