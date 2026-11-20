# Sonder Design System & Architecture

A local lossless music player: Rust core, Tauri 2 UI (Svelte 5), bit-perfect PCM and DSD playback with an atmospheric cosmic glass stage.

---

## 1. Brand Essence & Art Direction

- **Essence:** Bit-perfect audiophile fidelity delivered through an intimate, distraction-free listening canvas.
- **Atmospheric Cosmic Glass:** Deep pitch-black canvas (`#000000`) layered with dynamic ambient nebula color washes (`AmbientBackdrop`) extracted from the currently playing album art.
- **Tactile Analog Nostalgia:** Functional, authentic representation of physical music media—spinning vinyl records, micro-groove optical refraction, hydraulic tonearm cueing physics, and warm mechanical motion.
- **Signature Typography:** Contrast between hyper-clean Swiss modernist UI type (`Momo Trust Sans`) and warm, editorial script (`Momo Signature`) for greetings and wordmarks.

---

## 2. Design Tokens

### Canvas & Ink Palette
Contrast strictly adheres to WCAG 2.1 AA (all text on glass $\ge 5.3:1$, on canvas $\ge 10.6:1$).

| Token | Value | Role |
|---|---|---|
| `--canvas` | `#000000` | Deep pitch-black root background |
| `--ink` | `#f4f4f6` | High-contrast primary text, active icons, play button fill |
| `--ink-2` | `#b8b8c0` | Secondary metadata (artists, times, inactive nav icons) |
| `--ink-3` | `#8f8f99` | Tertiary labels, subtle borders, inactive track numbers |
| `--ok` | `#6fcf97` | Bit-perfect status dot, active repeat indicator, lossless check mark |
| `--warn` | `#f2c94c` | Software volume active, non-bit-perfect DSP notice |
| `--error` | `#ff8a80` | Corrupted files, underruns, scan failures |

### Surfaces & Glass Chrome
- `--soft`: `rgb(255 255 255 / 0.06)` (cards, row backgrounds)
- `--soft-hover`: `rgb(255 255 255 / 0.10)`
- `--soft-active`: `rgb(255 255 255 / 0.16)`
- `--hairline`: `rgb(255 255 255 / 0.08)` (subtle borders and divider lines)
- `--glass-tint`: `rgb(16 16 19 / 0.8)`
- `--glass-blur`: `28px`
- `--glass-saturate`: `1.7`
- `--glass-shadow`: `0 18px 44px rgb(0 0 0 / 0.5), 0 2px 8px rgb(0 0 0 / 0.35)`

### Typography Scale
- **Primary UI:** `"Momo Trust Sans Variable", "Segoe UI Variable", system-ui, sans-serif`
- **Tabular Figures:** `"Segoe UI Variable", system-ui, sans-serif` (prevents jitter in clocks, seekbars, and bit rates)
- **Signature Script:** `"Momo Signature"` (wordmark and "Late night listening" header)
- **Scale:**
  - Display: `clamp(34px, 3.4vw + 8px, 52px)`
  - Headline: `30px`
  - Title: `18px`
  - Body: `14px`
  - Small: `12.5px`
  - Label: `11.5px`

---

## 3. Desktop Layout & Responsive Breakpoints

Sonder is a dedicated desktop application with support for dynamic window tiling and snapping (e.g. Windows 11 Snap Assist, FancyZones, macOS Split View).

| Viewport / Container | State | Layout Behavior |
|---|---|---|
| **$\ge 1100\text{px}$** | Full Desktop | Full expanded sidebar (`232px`), inline seekbar, all signal-path chips visible. |
| **$861\text{px}$ – $1099\text{px}$** | Tiled Desktop | Sidebar automatically folds to compact icon rail (`72px`). Full transport dock. |
| **$641\text{px}$ – $860\text{px}$** | Split Screen | Sidebar rail (`72px`). Seekbar collapses to inline bottom edge line inside `.pill`. |
| **$\le 640\text{px}$** | Narrow Dock / 1/3 Snap | Compact pill (`@container`): disc scales to 48px, status chip hides, control buttons compact to 34px. Zero overflow down to 480px. |
| **Window Minimum** | Native Window Constraint | `minWidth: 640px`, `minHeight: 540px` configured in `tauri.conf.json`. |

### Cross-Platform Title Bar (`TitleBar.svelte`)
- **Height:** `--titlebar-h: 40px`, rendered as a transparent atmospheric layer over `AmbientBackdrop` / `FogBackdrop` (`z-index: 200`) with a subtle top dark scrim (`rgb(0 0 0 / 0.36)`) to preserve WCAG AA contrast over bright album covers, keeping the brand mark, sidebar toggle, back navigation, quick search pill, and always-on-top pin aligned in a balanced `1fr auto 1fr` grid (collapsing to `auto minmax(0, 1fr) auto` below `760px`).
- **OS Detection & Theme:** Rust injects `window.__SONDER_OS__` and forces `Theme::Dark` on the main window so native borders/menus never flash light-mode chrome against `#000000`.
- **macOS:** Uses `TitleBarStyle::Overlay` with `hiddenTitle: true`, `.traffic_light_position(14, 18)` (centered in `40px`), and `78px` left padding (collapsing to `14px` in native fullscreen) with macOS shortcuts (`⌘F`, `⌘B`, `⌘[`).
- **Windows:** Uses frameless DWM shadow + rounded corners (`decorations: false`, `shadow: true`) paired with flush `46px × 100%` Fluent 11 caption controls on the top-right (Fitts's Law) and 8-way resize handles stopping clear of the close button (`right: 142px`).
- **Linux:** Uses circular frosted GNOME/Adwaita-inspired caption controls, an inset 1px window hairline when unmaximized, and 8-way invisible edge/corner resize handles via `startResizeDragging`.

---

## 4. Motion & Animation Principles

- **Critically Damped Springs:** Spring dynamics use damping ratio $\zeta = 0.58$ and natural frequency $\omega = 10\text{ rad/s}$ with smootherstep settling (`app/src/lib/stageMotion.ts`).
- **Ballistic Flight Arc:** `DiscFlight.svelte` uses parabolic elevation arcs with z-index isolation and landing handoff between Now Playing stage and bottom player dock.
- **SplitMix32 PRNG:** Deterministic 64-bit pseudo-random kinetic typography for Folia running lyrics (`stageMotion.ts`). Seeking forward or backward produces the exact same frame.
- **Accessibility:** Any animation must respect user preference via `reducedMotion()` (`app/src/lib/motion.ts`), degrading to instant cut or linear opacity transition without transform scaling.
