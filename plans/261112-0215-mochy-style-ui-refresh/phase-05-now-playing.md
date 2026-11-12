---
phase: 5
title: "Phase 5: Now Playing"
status: in-progress
priority: P1
effort: "1d"
dependencies: [2]
---

# Phase 5: Now Playing

## Requirements

- [x] **Backdrop**: the ambient pools at full strength over black (replaces the blurred
      `<img>`, which re-blurs every frame under the spinning label); a touch darker toward
      the bottom.
- [x] **Layout** by container queries (`cqw` / `cqh`), two columns down to a 640 px pane;
      small centred "Now Playing · Album" header; title 26–40 px, artist line, album link.
- [x] **Hero controls** in Mochy's style: codec chip beside the title, 8 px seek bar (white
      on translucent white) with elapsed and total, **bare white prev / PLAY / next glyphs**
      (large, with a round glass press state), a row of round glass buttons (queue, album,
      bit-perfect / volume, cover style), source → device → Bit-perfect badges, device path.
- [x] **Frost band** between stage and controls when stacked: the record melts into the
      controls through `.frost-fade`.
- [x] **Record realism**: bands follow the arm (lead-in, programme area, run-out at
      `END_FRACTION`, dead wax); grooves masked to the light lobes; no blend mode on the
      rotating layer; paper label edge; chrome spindle; platter with strobe dots; fix the
      shadow clipped by `contain: paint`.
- [x] **Tonearm drawing**: thicker tube with a specular line, tapered headshell with
      cartridge, cylindrical counterweight, round bearing base with gimbal ring, U-shaped
      rest; shadow blurred once inside the SVG. No change to `turntable.ts`.
- [x] **Tonearm fixes**: no timer or frame loop after unmount; Escape cancels a mouse drag
      (focus on pointerdown); lost pointer capture cancels; repeated keys accumulate; value
      text does not chatter while focused.
- [x] **Motion retune** per report 5: arm springs, record-change order (arm lifts, old record
      leaves, new one lands, arm drops), longer spin-down, shorter colour cross-fade.

## Success Criteria

- No clipped badges at 900×600; nothing overflows at 1280×820.
- With music playing, the DevTools trace shows no per-frame blur work (only the label and
  arm transforms and the stepped ambient canvas).
- `pnpm test` geometry tests still pass unchanged.
