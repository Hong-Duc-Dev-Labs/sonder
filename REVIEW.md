# Sonder Desktop Review Checklist

Use this rubric when reviewing pull requests, features, and UI modifications in Sonder.

---

## 1. Desktop UX & Layout Checklist

- [ ] **Window Resizing & Snapping:**
  - Test at full width (1920×1080 / 1440×900).
  - Test at standard laptop resolution (1280×800).
  - Test snapped to 1/2 or 1/3 screen width (640×800) using Windows Snap or macOS Split View.
  - Verify that `document.documentElement.scrollWidth <= clientWidth` (zero horizontal overflow or accidental scrollbars).
- [ ] **Transport Bar Integrity:**
  - Verify player dock (`.pill`) scales smoothly without clipping previous/play/next buttons or right-hand utility icons (sleep timer, lyrics, queue, volume).
  - Verify seekbar scrubbing remains responsive across all window widths.
- [ ] **Audiophile Guarantees:**
  - With software volume OFF and DSP disabled, verify output status displays "Bit-perfect" with green indicator.
  - Verify signal path inspector popover accurately lists file decoding format and DAC sample rate.
- [ ] **Typography & Contrast:**
  - Ensure contrast on glass meets WCAG 2.1 AA ($\ge 5.3:1$).
  - Clocks and counter labels must use `--font-figures` (`Segoe UI Variable` tabular nums) to prevent figure jitter.
- [ ] **Motion & Accessibility:**
  - Verify that enabling `prefers-reduced-motion` at OS level disables vinyl rotation, ballistic disc flights, and spring scale transforms.
  - Verify keyboard shortcuts: `Space` (play/pause), `Ctrl+F` (search), `Ctrl+B` (toggle sidebar rail), `Alt+Left` (back).

---

## 2. Automated Verification Commands

Run before committing any UI changes:

```sh
# Typecheck & Svelte diagnostics
pnpm --dir app check

# Unit and motion tests
pnpm --dir app test

# Build verification
pnpm --dir app build
```
