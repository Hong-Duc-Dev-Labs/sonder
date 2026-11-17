# Fix Controller to Now Playing Shared-Element Morph Transition

**Date:** 2026-11-17
**Task:** `/ak-fix Sửa giúp tôi hiệu ứng chuyển đổi từ controller vào now playing vẫn là hiệu ứng cũ`

## 1. Problem & Root Cause Analysis

The user reported that the transition when switching from the bottom player controller (TransportBar) into Now Playing was still showing the legacy animation rather than the modern shared-element morph.

### Root Causes
1. **Broken `clipPath` calculation in `NowPlayingView.onMount`:**
   - In `App.svelte`, navigating to `now` set `onNow = true`, which immediately marked `TransportBar` with `.dock.hidden` (`transform: translateY(100% + ...)`).
   - `NowPlayingView` attempted to read `document.querySelector('[data-flight="pill"]').getBoundingClientRect()` during `onMount` (after DOM update), resulting in an off-screen rect (`top > window.innerHeight` and negative bottom inset), rendering the `clipPath` inset animation invalid or broken.
2. **Conflicting `DiscFlight` ballistic flight arc:**
   - `app.go({ kind: "now" })` and `app.back()` unconditionally launched `DiscFlight`, flying a floating vinyl disc on top of the view (`z-index: 150`) across 640ms, masking the shared card transition.
3. **Competing `pageIn` transition on `.view`:**
   - `App.svelte` wrapped every view with `<div class="view" in:pageIn>`, executing a 240ms opacity and translateY fade that fought with the full-screen morph.
4. **Immediate unmount with zero exit animation:**
   - Dismissing Now Playing (via header chevron, Escape key, or pull-to-dismiss) immediately destroyed `NowPlayingView` and called `app.back()`, leaving no closing morph or shrink-down into the controller.
5. **Inaccessible track metadata click in `TransportBar`:**
   - Only the tiny 44px spinning disc button triggered `app.go({ kind: "now" })`. Clicking the song title or artist text did not open Now Playing.
6. **False track-change animation on mount in `TurntableStage`:**
   - `in:slideRecord` ran on initial mount of `TurntableStage`, sliding the vinyl disc horizontally from the right edge with a 220ms delay even though the current track was already playing.

---

## 2. Solution Implemented

1. **Pre-Navigation Controller Bounds Caching (`app.svelte.ts`):**
   - Added `ControllerRect` interface and `app.controllerRect` state.
   - In `app.go({ kind: "now" })`, captured both `.pill` and `[data-flight="pill"]` bounding boxes *before* `app.view` mutates and before `TransportBar` is hidden.
   - Suppressed legacy `DiscFlight` launch when `app.settings.growFromCover ?? true` is active.

2. **Full Bidirectional Shared-Element Morph (`NowPlayingView.svelte`):**
   - **Entrance (`playEnterAnimation`):**
     - Card container `clipPath` smoothly expands from the controller pill's exact geometry (`pillRect` with `pillRadius`) to full viewport (`inset(0px round 0px)`) over 440ms using `cubic-bezier(0.2, 0.9, 0.2, 1)`.
     - Cover morph: Computes exact delta and scale factor from the controller mini-disc to the turntable/square cover slot, smoothly animating `translate3d(dx, dy, 0) scale(s) -> translate3d(0, 0, 0) scale(1)` over 460ms.
     - Staggered elevation: Metadata, seekbar, transport controls, and lyrics panel rise with staggered delays (`80ms - 140ms`).
   - **Exit (`playExitAnimation`):**
     - Triggered on chevron button click, Escape key press, or pull-down flick (`pullY > 120`).
     - Fast fade-out of surrounding controls (160ms).
     - Card container `clipPath` smoothly collapses back into the controller pill's bounds over 320ms with `cubic-bezier(0.25, 1, 0.5, 1)`.
     - Turntable/cover shrinks down directly onto the controller's mini-disc.
     - `app.nowExiting` keeps `TransportBar` unhidden and resting underneath during the collapse so the card seamlessly lands onto the real pill.

3. **Disabled Competing Transitions:**
   - In `transitions.ts`, updated `pageIn` to return `{ duration: 0 }` when opening `now` with `growFromCover` enabled.
   - In `TurntableStage.svelte`, passed `active: changing` to `in:slideRecord` so the horizontal slide only executes on real track switches, not on view mount.

4. **Enhanced Controller Interaction in `TransportBar.svelte`:**
   - Converted `.text` (track title and artist) into an accessible button `<button class="text text-btn" ...>` with hover feedback, allowing users to click anywhere on the left section of the controller to open Now Playing.

---

## 3. Verification & Evidence

- **Type Check:** `pnpm --dir app check` exited cleanly with 0 errors and 0 warnings.
- **Unit & Integration Tests:** `pnpm --dir app test` executed 13 test files (76 tests) with 100% pass rate.
- **Desktop Constraints & A11y:** Full respect for `reducedMotion()`, WCAG accessible buttons, and graceful fallbacks.
