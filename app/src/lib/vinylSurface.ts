// Paints a vinyl record's still surface into a canvas at the screen's own resolution, once.
//
// What makes a record read as real, and how each is drawn here:
// - Grooves are concentric and far finer than a pixel, so they are never drawn one by one at
//   that scale. Instead the programme area is built from narrow bands whose darkness varies
//   the way loud and quiet passages cut differently, plus fine rings about a pixel apart.
// - A single fixed light makes a soft anisotropic sheen: brightest along one axis through the
//   centre and falling away smoothly with the angle (a power of the cosine), never a hard
//   streak. Smooth areas (lead-in, gaps between tracks, run-out, the lip) take the same light
//   as narrow bright lines.
// - Gaps between tracks are glossy rings; their places come from the album so every record
//   looks a little different, the same way every time.
//
// The surface is still: only the label above it rotates. Redraw only on resize or a new album.

/** Zone edges as fractions of the record radius; these match the tonearm (turntable.ts). */
export const ZONES = {
  label: 0.44,
  runOut: 0.51,
  programmeIn: 0.52,
  programmeOut: 0.915,
  leadInOut: 0.955,
  lipIn: 0.962,
};

/** The light's axis, radians from the +x axis (clockwise on screen): from the upper left. */
const LIGHT_AXIS = -0.42;

type Ctx = CanvasRenderingContext2D;

/** A deterministic 0–1 number for `i` under `seed` (no state, same answer every time). */
function hash(i: number, seed: number): number {
  const s = Math.sin(i * 127.1 + seed * 311.7) * 43758.5453;
  return s - Math.floor(s);
}

/** A stable seed from text (FNV-1a), so an album always gets the same record. */
export function seedOf(text: string): number {
  let h = 2166136261;
  for (let i = 0; i < text.length; i++) h = Math.imul(h ^ text.charCodeAt(i), 16777619);
  return (h >>> 0) / 4294967296;
}

/** Where the gaps between tracks fall, as fractions of the radius, outermost first. */
export function trackGaps(seed: number): number[] {
  const count = 3 + Math.floor(hash(1, seed) * 4);
  const span = ZONES.programmeOut - ZONES.programmeIn;
  // Uneven track lengths: random weights, then cumulative positions inside the programme.
  const weights = Array.from({ length: count + 1 }, (_, i) => 0.6 + hash(i + 10, seed));
  const total = weights.reduce((a, b) => a + b, 0);
  const gaps: number[] = [];
  let acc = 0;
  for (let i = 0; i < count; i++) {
    acc += weights[i];
    gaps.push(ZONES.programmeOut - span * (acc / total));
  }
  return gaps;
}

function ring(ctx: Ctx, c: number, r: number, width: number, style: string | CanvasGradient) {
  ctx.beginPath();
  ctx.arc(c, c, r, 0, Math.PI * 2);
  ctx.lineWidth = width;
  ctx.strokeStyle = style;
  ctx.stroke();
}

function annulus(ctx: Ctx, c: number, inner: number, outer: number) {
  ctx.beginPath();
  ctx.arc(c, c, outer, 0, Math.PI * 2);
  ctx.arc(c, c, inner, 0, Math.PI * 2, true);
}

/**
 * White light around the record: `peak` where the grooves face the light square on, falling
 * away as |cos|^`power`, never below `floor`. Higher powers make tighter highlights (smooth
 * vinyl); low powers a broad glow (grooved vinyl scatters the light).
 */
function sheen(ctx: Ctx, c: number, peak: number, power: number, floor = 0): CanvasGradient | string {
  if (typeof ctx.createConicGradient !== "function") return `rgb(255 255 255 / ${floor + (peak - floor) * 0.15})`;
  const g = ctx.createConicGradient(LIGHT_AXIS, c, c);
  const steps = 120;
  for (let k = 0; k <= steps; k++) {
    const f = k / steps;
    const v = Math.abs(Math.cos(f * Math.PI * 2)) ** power;
    g.addColorStop(f, `rgb(255 255 255 / ${(floor + (peak - floor) * v).toFixed(4)})`);
  }
  return g;
}

/** Paints the surface to fill `canvas` (sized to its CSS box × device pixel ratio). */
export function drawVinylSurface(canvas: HTMLCanvasElement, seedText: string) {
  const cssSize = canvas.clientWidth;
  if (!cssSize) return;
  const dpr = Math.min(window.devicePixelRatio || 1, 3);
  const size = Math.round(cssSize * dpr);
  if (canvas.width !== size) {
    canvas.width = size;
    canvas.height = size;
  }
  const ctx = canvas.getContext("2d");
  if (!ctx) return;
  const c = size / 2;
  const R = c;
  const px = dpr;
  const seed = seedOf(seedText || "sonder");
  ctx.clearRect(0, 0, size, size);

  // The black body.
  ctx.beginPath();
  ctx.arc(c, c, R, 0, Math.PI * 2);
  ctx.fillStyle = "#0b0b0d";
  ctx.fill();

  const pIn = ZONES.programmeIn * R;
  const pOut = ZONES.programmeOut * R;

  // Bands of louder and quieter passages, each a couple of pixels wide.
  const band = 2.2 * px;
  for (let i = 0, r = pIn; r < pOut; r += band, i++) {
    const slow = 0.5 + 0.5 * Math.sin(i * 0.17 + seed * 40 + 1.4 * Math.sin(i * 0.031 + seed * 9));
    const fine = hash(i, seed);
    const dark = Math.min(0.5, 0.04 + 0.22 * fine * fine + 0.22 * slow);
    ring(ctx, c, r + band / 2, band, `rgb(0 0 0 / ${dark.toFixed(3)})`);
  }
  // The grooves themselves: fine rings about a pixel apart.
  for (let r = pIn; r < pOut; r += 1.15 * px) ring(ctx, c, r, 0.45 * px, "rgb(255 255 255 / 0.045)");

  // The broad sheen across the grooved area.
  ctx.save();
  annulus(ctx, c, pIn, pOut);
  ctx.clip();
  ctx.fillStyle = sheen(ctx, c, 0.26, 3.5, 0.02);
  ctx.fillRect(0, 0, size, size);
  ctx.restore();

  // Gaps between tracks: smooth vinyl, dark with one bright line where the light hits square.
  const gapWidth = Math.max(1.6 * px, R * 0.0055);
  for (const g of trackGaps(seed)) {
    ring(ctx, c, g * R, gapWidth, "rgb(0 0 0 / 0.55)");
    ring(ctx, c, g * R, gapWidth * 0.5, sheen(ctx, c, 0.32, 90));
  }

  // Lead-in by the rim and the run-out around the label: smooth, faintly lit.
  ctx.save();
  annulus(ctx, c, pOut, ZONES.leadInOut * R);
  ctx.clip();
  ctx.fillStyle = "#0d0d10";
  ctx.fillRect(0, 0, size, size);
  ctx.fillStyle = sheen(ctx, c, 0.09, 40, 0.008);
  ctx.fillRect(0, 0, size, size);
  ctx.restore();

  ctx.save();
  annulus(ctx, c, ZONES.label * R, ZONES.runOut * R);
  ctx.clip();
  ctx.fillStyle = "#0e0e11";
  ctx.fillRect(0, 0, size, size);
  ctx.fillStyle = sheen(ctx, c, 0.06, 60, 0.006);
  ctx.fillRect(0, 0, size, size);
  ctx.restore();

  // The run-out groove where the stylus ends: a sharp ring with a glint.
  ring(ctx, c, ZONES.runOut * R, Math.max(1.2 * px, R * 0.004), "rgb(0 0 0 / 0.6)");
  ring(ctx, c, ZONES.runOut * R + px, 0.6 * px, sheen(ctx, c, 0.28, 70));

  // The lip: a raised, duller rim whose rounded edge catches a thin line of light.
  ctx.save();
  annulus(ctx, c, ZONES.lipIn * R, R);
  ctx.clip();
  ctx.fillStyle = "#0f0f12";
  ctx.fillRect(0, 0, size, size);
  ctx.fillStyle = sheen(ctx, c, 0.08, 4);
  ctx.fillRect(0, 0, size, size);
  ctx.restore();
  ring(ctx, c, R * 0.99, Math.max(0.8 * px, R * 0.003), sheen(ctx, c, 0.38, 10, 0.05));
  ring(ctx, c, R - 0.6 * px, 1.2 * px, "rgb(0 0 0 / 0.6)");
}
