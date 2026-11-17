# UX/AX review: Sonder — 2026-11-17

## Verdict
Sonder presents an exceptionally polished desktop audiophile UI with distinctive cosmic glass aesthetics, mechanical turntable authenticity, and fluid kinetic running lyrics. However, on mobile viewports (375×812) the player bar overflows horizontally and the persistent icon rail squishes content, while public discovery surfaces (robots.txt, sitemap.xml, llms.txt, JSON-LD, Open Graph) are completely missing for web and answer-engine crawlers. Addressing mobile navigation/player responsiveness and deploying standardized machine-readable discovery assets represent the highest-impact gains. Current status: Target defined, report DONE.

---

## Scope and environment
- **Mode:** default (Review → analyze → propose → define DONE; report only)
- **Product type:** Desktop local lossless & DSD music player (Tauri 2 + Svelte 5 + Rust)
- **Active commit:** `6f87ad6` (✨ feat(ui): optimize motion transitions and ui/ux across all views)
- **Development server:** Running on `http://localhost:1420` (Vite 8.3.8, Svelte 5.57.3, Node v22.23.4)
- **Tested viewports:**
  - Desktop: 1440 × 900
  - Tablet: 768 × 1024
  - Mobile: 375 × 812
- **Pages reviewed:** Home (Empty State), Now Playing (Turntable & Lyrics Stage), Settings (Bit-perfect DSP), Quality Check (Spectrum analyzer)
- **Checks that could not run:** Native DAC exclusive-mode output hardware testing (not applicable to UX/AX interface audit).

---

## Baseline evidence

### Screenshots per page × viewport
All baseline screenshots are preserved in `plans/reports/enhance-ux-ax-261117-0425-sonder-lossless-player/round-0/`:

| Page / State | Desktop (1440 × 900) | Tablet (768 × 1024) | Mobile (375 × 812) |
|---|---|---|---|
| **Home (Library Empty)** | [`desktop-home-loaded.png`](file:///D:/Dev/Sonder/plans/reports/enhance-ux-ax-261117-0425-sonder-lossless-player/round-0/desktop-home-loaded.png) | [`tablet-home-loaded.png`](file:///D:/Dev/Sonder/plans/reports/enhance-ux-ax-261117-0425-sonder-lossless-player/round-0/tablet-home-loaded.png) | [`mobile-home-loaded.png`](file:///D:/Dev/Sonder/plans/reports/enhance-ux-ax-261117-0425-sonder-lossless-player/round-0/mobile-home-loaded.png) |
| **Settings (Bit-perfect DSP)** | [`desktop-settings-loaded.png`](file:///D:/Dev/Sonder/plans/reports/enhance-ux-ax-261117-0425-sonder-lossless-player/round-0/desktop-settings-loaded.png) | *(inherited rail layout)* | [`mobile-settings-loaded.png`](file:///D:/Dev/Sonder/plans/reports/enhance-ux-ax-261117-0425-sonder-lossless-player/round-0/mobile-settings-loaded.png) |
| **Now Playing (Turntable)** | [`desktop-nowplaying-loaded.png`](file:///D:/Dev/Sonder/plans/reports/enhance-ux-ax-261117-0425-sonder-lossless-player/round-0/desktop-nowplaying-loaded.png) | *(inherited rail layout)* | [`mobile-nowplaying-loaded.png`](file:///D:/Dev/Sonder/plans/reports/enhance-ux-ax-261117-0425-sonder-lossless-player/round-0/mobile-nowplaying-loaded.png) |
| **Quality Check** | [`desktop-quality-loaded.png`](file:///D:/Dev/Sonder/plans/reports/enhance-ux-ax-261117-0425-sonder-lossless-player/round-0/desktop-quality-loaded.png) | *(inherited rail layout)* | *(responsive stack)* |

### Discovery scan summary (`check-discovery-surfaces.mjs`)
Ran against `http://localhost:1420`:
```text
Discovery surfaces for http://localhost:1420: 0 URL(s) in sitemap, 1 page(s) checked

ERROR (4)
- [robots] http://localhost:1420/robots.txt: robots.txt not served as text (status 200)
- [sitemap] http://localhost:1420/sitemap.xml: sitemap missing or not XML (status 200)
- [meta-description] http://localhost:1420/: missing meta description
- [open-graph] http://localhost:1420/: missing og:image

WARN (10)
- [llms-txt] http://localhost:1420/llms.txt: llms.txt missing (status 200)
- [llms-full-txt] http://localhost:1420/llms-full.txt: llms-full.txt missing (status 200)
- [canonical] http://localhost:1420/: missing canonical link
- [open-graph] http://localhost:1420/: missing og:title
- [open-graph] http://localhost:1420/: missing og:description
- [open-graph] http://localhost:1420/: missing og:url
- [twitter-card] http://localhost:1420/: missing twitter:card
- [json-ld] http://localhost:1420/: no JSON-LD structured data
- [server-render] http://localhost:1420/: server HTML has under 200 characters of text
- [markdown-variant] http://localhost:1420/index.html.md: markdown twin returns HTML

INFO (2)
- [markdown-alternate] http://localhost:1420/: no <link rel="alternate" type="text/markdown">
- [content-negotiation] http://localhost:1420/: no markdown for Accept: text/markdown (optional)
```

---

## Scores

Scored 0–3 based on `references/ux-review-rubric.md` and `references/seo-geo-ax-checklist.md`:

| Area | Score (0–3) | Evidence |
|---|:---:|---|
| **First impression** | 2 | Atmospheric dark cosmic glass theme with "Late night listening" sets immediate tone. A stranger quickly grasps the musical mood, but the specific audiophile bit-perfect lossless capability requires exploring Settings. [`desktop-home-loaded.png`](file:///D:/Dev/Sonder/plans/reports/enhance-ux-ax-261117-0425-sonder-lossless-player/round-0/desktop-home-loaded.png) |
| **Brand recall** | 3 | High brand distinctiveness: Momo Signature cursive typography for title/headings, custom vinyl groove icon mark, turntable stage, and ambient nebulous dark palette. Highly recognizable even when cropped. |
| **Content punch** | 2 | Plainspoken, outcome-oriented copy: "Your library is empty. Add a music folder and Sonder will find your albums", "Check any song". Settings copy is clear, though mobile word wrapping compromises density. |
| **Clarity and hierarchy** | 2 | Navigation structure is logical (Top, Library, Playlists, Now Playing, Settings). Primary call-to-actions (white pill buttons) stand out cleanly. Hierarchy degrades on mobile where the left rail squeezes content. |
| **Storytelling** | 3 | Audiophile storytelling is masterfully conveyed: mechanical turntable cueing, signal path stage-by-stage inspector (decoder → EQ → SRC → DAC), and Folia kinetic running lyrics. |
| **Knowledge and trust** | 2 | Settings prominently showcases "Bit-perfect" status with green indicator; built-in quality checker detects lossy transcodes. However, no public web docs, changelog, or security/privacy policies exist in web discovery surfaces. |
| **Motion** | 3 | Exceptional motion implementation: SplitMix32 PRNG kinetic typography, critically-damped spring dynamics (`zeta = 0.58`), hydraulic tonearm cueing, 60fps GPU fog shader. Respects `reducedMotion()` everywhere (`app/src/lib/motion.ts`). |
| **Responsive** | 1 | Tablet (768px) adapts well via auto-collapsing rail. Mobile (375px) suffers critical bugs: player bar controls overflow off-screen (sleep timer clipped, lyrics/queue hidden), and the persistent 56px rail crushes content into 319px. [`mobile-home-loaded.png`](file:///D:/Dev/Sonder/plans/reports/enhance-ux-ax-261117-0425-sonder-lossless-player/round-0/mobile-home-loaded.png) |
| **Accessibility** | 2 | High contrast dark mode, polite ARIA live regions for notices, tablist role attributes. Keyboard navigation works on desktop (`Space`, `Ctrl+F`, `Ctrl+B`, `Alt+Left`). Mobile horizontal overflow impairs touch target accessibility. |
| **Performance feel** | 3 | Vitest test suite runs 13 files (74 tests) in 669ms. Svelte 5 runes provide instantaneous reactivity without virtual DOM overhead. Canvas fog shader uses polynomial hash optimization. |
| **AX / GEO / SEO Discovery** | 0 | Discovery scan found 4 Errors, 10 Warnings. No robots.txt, sitemap.xml, llms.txt, Open Graph, or JSON-LD. Server HTML provides zero text to non-JS crawlers (`<div id="app"></div>`). |

---

## Proposals

### P1 — Fix Mobile Responsive Layout & TransportBar Overflow (Must)
- **Evidence:** `plans/reports/enhance-ux-ax-261117-0425-sonder-lossless-player/round-0/mobile-home-loaded.png` and `mobile-settings-loaded.png`. At 375×812 viewport:
  1. `TransportBar` horizontally overflows the viewport (`scrollWidth > clientWidth`). The sleep timer button is clipped in half, and the lyrics and queue buttons are pushed completely off-screen.
  2. The permanent 56px left sidebar icon rail consumes 15% of the 375px width, leaving only ~319px for content. This causes "Late night listening" to wrap onto 3 cramped lines and Settings toggle labels to wrap onto 8 narrow lines.
- **Change:**
  1. In `app/src/components/TransportBar.svelte`: at `@media (max-width: 600px)`, wrap or compact secondary utility buttons (sleep timer, lyrics, queue) and reduce padding so that the full bar width fits within 345px with zero horizontal scrolling.
  2. In `app/src/App.svelte` and `app/src/app.css`: at `@media (max-width: 600px)`, transition from the fixed left icon rail to a bottom navigation bar or collapsible header drawer, granting the active view the full 375px width.
  3. In `app/src/components/SettingsView.svelte`: adjust `.toggle-row` to flex-wrap gracefully on narrow viewports to eliminate awkward vertical word stacking.
- **Files:**
  - `app/src/components/TransportBar.svelte`
  - `app/src/App.svelte`
  - `app/src/app.css`
  - `app/src/components/SettingsView.svelte`
- **Acceptance:**
  - At 375×812 viewport, `document.documentElement.scrollWidth <= document.documentElement.clientWidth` (0px horizontal overflow).
  - All transport controls (prev, play, next, repeat, time, seek, sleep timer, lyrics, queue) are fully visible and clickable without scrolling.
  - "Late night listening" fits cleanly without single-word awkward line breaks.

### P2 — Complete Discovery Surfaces: robots.txt, sitemap.xml, llms.txt & llms-full.txt (Must)
- **Evidence:** Discovery scan (`scripts/check-discovery-surfaces.mjs http://localhost:1420`) exits 1 with 4 errors and 10 warnings. SPA fallback serves HTML for `/robots.txt` and `/sitemap.xml`. AI search agents (PerplexityBot, Claude-SearchBot, OAI-SearchBot) cannot crawl or quote Sonder's architecture.
- **Change:**
  1. Create `app/public/robots.txt` allowing indexing, disallowing private endpoints, and referencing `Sitemap: https://sonder.audio/sitemap.xml`.
  2. Create `app/public/sitemap.xml` listing canonical URLs with accurate `lastmod`.
  3. Create `app/public/llms.txt` following llmstxt.org specification: `# Sonder`, `> Bit-perfect lossless & DSD music player`, linking to markdown twins for Audio Engine, DSP chain, and Verification.
  4. Create `app/public/llms-full.txt` providing a comprehensive, single-page markdown document explaining Sonder's core, exclusive-mode audio backends, 64-bit float DSP chain, AutoEQ search integration, and bit-perfect verification proofs.
  5. Create `app/public/index.html.md` as the markdown twin for the main route.
- **Files:**
  - `app/public/robots.txt`
  - `app/public/sitemap.xml`
  - `app/public/llms.txt`
  - `app/public/llms-full.txt`
  - `app/public/index.html.md`
- **Acceptance:**
  - `node C:\Users\hongducdev\.gemini\antigravity-cli\skills\ak-enhance-ux-ax\scripts\check-discovery-surfaces.mjs http://localhost:1420 --site-origin https://sonder.audio` exits 0 with 0 errors.
  - `curl -s http://localhost:1420/robots.txt` returns `text/plain` with status 200.
  - `curl -s http://localhost:1420/llms.txt` returns valid markdown with `# Sonder` header.

### P3 — Head Metadata, Open Graph, Twitter Cards & JSON-LD Structured Data (Must)
- **Evidence:** `app/index.html` has no description, no Open Graph tags, no Twitter cards, and no JSON-LD structured data. Non-JS crawlers see an empty `<div id="app"></div>` shell (`[server-render]` warning).
- **Change:**
  1. In `app/index.html`: add `<meta name="description" content="Sonder is a bit-perfect local lossless music player with a 64-bit float DSP chain, DSD playback, AutoEQ headphone correction, and folia kinetic running lyrics.">`.
  2. Add Open Graph tags: `og:title`, `og:description`, `og:image`, `og:url`, `og:type` (`website`), `og:site_name` (`Sonder`).
  3. Add Twitter card: `twitter:card` (`summary_large_image`).
  4. Add JSON-LD schema with `@type`: `SoftwareApplication` (category: `MultimediaApplication`, operatingSystem: `Windows, macOS, Linux`, applicationSubCategory: `Audio Player`, offers: `Free`).
  5. Add accessible noscript/pre-rendered fallback content inside `<div id="app">` containing an overview of Sonder, features, and system requirements (>200 characters) so crawlers reading static HTML receive meaningful data.
  6. Add `app/public/og-image.png` (1200×630 branded card featuring the Sonder wordmark, vinyl record, and cosmic nebula backdrop).
- **Files:**
  - `app/index.html`
  - `app/public/og-image.png`
- **Acceptance:**
  - Discovery scan reports 0 errors and 0 warnings for `meta-description`, `open-graph`, `twitter-card`, `json-ld`, and `server-render`.
  - Schema.org validator validates `SoftwareApplication` with 0 errors.

### P4 — Graceful Web Browser / Preview Mock Mode (Should)
- **Evidence:** `app/src/main.ts:13` invokes `getCurrentWebviewWindow().label === "mini"` without checking if the runtime is Tauri. When opened in a web browser or preview, the app crashes with an uncaught `TypeError: Cannot read properties of undefined (reading 'metadata')`, breaking browser previews, E2E browser tests, and documentation visualizers.
- **Change:**
  1. In `app/src/main.ts`: wrap `getCurrentWebviewWindow()` in an environment check (`try { ... } catch { false }`) or check `window.__TAURI_INTERNALS__`.
  2. In `app/src/lib/bindings.ts` or a lightweight dev mock utility: provide safe fallbacks for read-only commands (`settingsGet`, `playerSnapshot`, `queueItems`, `libraryPlaylists`) when running in browser mode, showing graceful empty/demo states instead of crashing.
- **Files:**
  - `app/src/main.ts`
  - `app/src/lib/bindings.ts`
- **Acceptance:**
  - Navigating to `http://localhost:1420` in a fresh headless or desktop browser renders the UI cleanly.
  - `npx agent-browser errors` returns 0 uncaught exceptions on load.

### P5 — Consolidate Design & Architecture Guidelines: DESIGN.md, REVIEW.md, and AGENTS.md (Should)
- **Evidence:** Design decisions, typography pairings (Momo Signature + Momo Trust Sans), motion spring parameters (`zeta = 0.58`), and UX review expectations are scattered across implementation plans but missing from repository root guidance files.
- **Change:**
  1. Create `DESIGN.md` in repository root: brand essence, art direction (cosmic glass, audiophile precision, mechanical turntable nostalgia), typography tokens, color palette, motion physics principles (`zeta = 0.58`, SplitMix32 kinetic typography, reduced motion fallbacks), and component guidelines.
  2. Create `REVIEW.md` in repository root: UX/AX review rubric, 3-viewport verification requirements, and discovery surface checks.
  3. Create `AGENTS.md` in repository root: instructions for AI collaborators to read `DESIGN.md`, test responsive layouts at 375/768/1440px, and maintain discovery surfaces when routes change.
- **Files:**
  - `DESIGN.md`
  - `REVIEW.md`
  - `AGENTS.md`
- **Acceptance:**
  - `DESIGN.md`, `REVIEW.md`, and `AGENTS.md` exist in root.
  - Design tokens in `DESIGN.md` accurately match `app/src/app.css` and `app/src/lib/stageMotion.ts`.

### P6 — Deduplicate Mobile Now Playing Status Text (Could)
- **Evidence:** In `plans/reports/enhance-ux-ax-261117-0425-sonder-lossless-player/round-0/mobile-nowplaying-loaded.png`, the string "Nothing playing." appears twice: once in the track title block and again at the bottom of the viewport below the controls.
- **Change:**
  In `app/src/components/NowPlayingView.svelte`: hide the secondary bottom status line on narrow viewports when the track header already states "Nothing playing".
- **Files:**
  - `app/src/components/NowPlayingView.svelte`
- **Acceptance:**
  - On 375×812 viewport, "Nothing playing" appears exactly once in Now Playing view.

---

## Project DONE contract

Implementation is DONE only when every line is true and evidenced in the report:

1. Every Must (P1, P2, P3) and Should (P4, P5) proposal meets its acceptance checks; each skipped item has a stated reason accepted in the report.
2. `check-discovery-surfaces.mjs` exits 0 against the running site (with `--site-origin https://sonder.audio` for dev/preview) and checked more than the home page whenever the sitemap lists other pages; every remaining warning is either fixed or listed with the reason it is accepted.
3. Screenshots at 1440×900, 768×1024 and 375×812 of every changed page show no horizontal overflow, clipped text, overlapping elements or broken images, and the vision review finds no High issue.
4. No rubric area regressed from baseline; each area that scored below 2 now scores at least 2, or its gap is recorded as an unresolved question.
5. Keyboard and reduced-motion checks pass on changed interactive elements.
6. The project's build, lint and tests pass, or failures are shown as pre-existing with evidence (`pnpm check` exits 0, `pnpm test` passes 13/13 test files).
7. `DESIGN.md`, `REVIEW.md` and `AGENTS.md` reflect the delivered direction and checks, with prior content preserved.
8. Dev servers and other processes started by the review are stopped.

*(Default mode ends at report DONE; it lists this contract as the target but does not claim it.)*

---

## Unresolved questions
1. **Target Production Domain for Sitemap & Discovery:**
   Is `https://sonder.audio` the intended canonical public domain for Sonder's website and docs, or will it be hosted on a GitHub Pages URL (e.g. `https://hong-duc-dev-labs.github.io/sonder/`)?
2. **Mobile Layout Strategy:**
   For mobile viewports (≤ 600px), does the project team prefer folding the sidebar into a standard bottom navigation tab bar (Home, Search, Library, Settings), or keeping a top-left hamburger drawer? (Bottom tab bar is recommended for one-handed mobile music navigation).
