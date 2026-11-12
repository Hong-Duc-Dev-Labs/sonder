// Turntable geometry, in the stage's 1000-unit square. Angles are screen degrees: 0 points
// right, 90 down. The tonearm turns about its pivot; its stylus (the tube's tip) sits `REACH`
// from the pivot, so the record radius under the stylus fixes the arm's angle by the law of
// cosines. Progress maps linearly from the lead-in near the rim to just outside the label.

export type Point = { x: number; y: number };

export const STAGE = 1000;
export const RECORD_CENTER: Point = { x: 450, y: 500 };
export const RECORD_RADIUS = 430;
/** The label's radius as a fraction of the record's (VinylDisc insets it 28 %). */
export const LABEL_FRACTION = 0.44;
export const PIVOT: Point = { x: 900, y: 140 };
/** Pivot to stylus. */
export const REACH = 520;
/** Where the stylus starts (song start) and ends (song end), as fractions of the record radius. */
export const START_FRACTION = 0.93;
export const END_FRACTION = LABEL_FRACTION + 0.07;
/** The arm's angle on its rest, clear of the record. */
export const REST_ANGLE = 84;

const toDeg = (r: number) => (r * 180) / Math.PI;
const toRad = (d: number) => (d * Math.PI) / 180;

const dx = RECORD_CENTER.x - PIVOT.x;
const dy = RECORD_CENTER.y - PIVOT.y;
/** Pivot to record centre. */
const D = Math.hypot(dx, dy);
/** Direction from the pivot to the record centre. */
const TOWARD_CENTER = toDeg(Math.atan2(dy, dx));

/** The arm angle that puts the stylus `radius` from the record centre. */
export function angleForRadius(radius: number): number {
  const cos = (REACH * REACH + D * D - radius * radius) / (2 * REACH * D);
  return TOWARD_CENTER - toDeg(Math.acos(Math.min(1, Math.max(-1, cos))));
}

/** The stylus's distance from the record centre with the arm at `angle`. */
export function radiusForAngle(angle: number): number {
  const turn = toRad(TOWARD_CENTER - angle);
  return Math.sqrt(Math.max(0, REACH * REACH + D * D - 2 * REACH * D * Math.cos(turn)));
}

export const START_ANGLE = angleForRadius(START_FRACTION * RECORD_RADIUS);
export const END_ANGLE = angleForRadius(END_FRACTION * RECORD_RADIUS);

/** The arm angle for `fraction` (0–1) of the way through the song. */
export function armAngleFor(fraction: number): number {
  const f = Math.min(1, Math.max(0, fraction));
  const r = (START_FRACTION + (END_FRACTION - START_FRACTION) * f) * RECORD_RADIUS;
  return angleForRadius(r);
}

/** How far through the song (0–1) the arm at `angle` stands for; clamped to the record. */
export function fractionForAngle(angle: number): number {
  if (angle <= START_ANGLE) return 0;
  if (angle >= END_ANGLE) return 1;
  const f = (radiusForAngle(angle) / RECORD_RADIUS - START_FRACTION) / (END_FRACTION - START_FRACTION);
  return Math.min(1, Math.max(0, f));
}

/** Where the stylus is with the arm at `angle`. */
export function stylusAt(angle: number): Point {
  const a = toRad(angle);
  return { x: PIVOT.x + REACH * Math.cos(a), y: PIVOT.y + REACH * Math.sin(a) };
}

/** The angle of `p` around the pivot, in degrees. */
export function angleAroundPivot(p: Point): number {
  return toDeg(Math.atan2(p.y - PIVOT.y, p.x - PIVOT.x));
}

/** SVG path of the stylus's arc from the song start to `angle` (for the scrub time arc). */
export function stylusArcPath(toAngle: number, radiusOut = REACH + 26): string {
  const a0 = toRad(START_ANGLE);
  const a1 = toRad(Math.max(START_ANGLE, toAngle));
  const p0 = { x: PIVOT.x + radiusOut * Math.cos(a0), y: PIVOT.y + radiusOut * Math.sin(a0) };
  const p1 = { x: PIVOT.x + radiusOut * Math.cos(a1), y: PIVOT.y + radiusOut * Math.sin(a1) };
  return `M ${p0.x.toFixed(1)} ${p0.y.toFixed(1)} A ${radiusOut} ${radiusOut} 0 0 1 ${p1.x.toFixed(1)} ${p1.y.toFixed(1)}`;
}
