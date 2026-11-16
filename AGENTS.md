# Agent Instructions for Sonder

Sonder is a dedicated local lossless & DSD music player built with a Rust core, Tauri 2, and Svelte 5 (Runes).

---

## 1. Operating Rules & Precedence

1. **Desktop-First Focus:** Sonder is an offline, native desktop application for Windows, macOS, and Linux. Do not add mobile-web frameworks, sitemaps, or mobile navigation bars to the desktop bundle.
2. **Read `DESIGN.md` Before UI Changes:** Strictly adhere to the design tokens, typography rules (`Momo Trust Sans` + `Momo Signature`), and spring motion parameters ($\zeta = 0.58$) documented in [`DESIGN.md`](file:///D:/Dev/Sonder/DESIGN.md).
3. **Preserve Bit-Perfect Guarantees:** Never insert lossy conversions, automatic resampling, or software volume in the default audio path.
4. **Motion & Reduced Motion:** Every visual animation must check `reducedMotion()` (`app/src/lib/motion.ts`) and gracefully fall back to linear or instant transitions.
5. **Git Commit Format:** All git commits must strictly follow **Gitmoji + Conventional Commits** standard in English.
   - Format: `<emoji> <type>(<scope>): <subject>` (e.g. `✨ feat(ui): improve transport bar responsiveness for desktop snapping`)

---

## 2. Desktop Window Verification

When modifying UI components, verify that layouts adapt seamlessly to desktop window tiling:
- **Full screen:** $\ge 1280\text{px}$ (expanded sidebar).
- **Narrow tiled:** $900\text{px} - 1099\text{px}$ (sidebar folds to 72px rail).
- **Split snap:** $640\text{px}$ (compact dock, inline seek bar, zero horizontal overflow).

---

## 3. Pre-Commit Verification

Always run and ensure clean exit:

```sh
pnpm --dir app check
pnpm --dir app test
```
