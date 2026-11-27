// Pure state, step-navigation, and persistence helpers for the first-run Welcome Setup wizard.

import type { Settings } from "./bindings";

export const WELCOME_STEPS = ["welcome", "sources", "audio", "finish"] as const;
export type WelcomeStepId = (typeof WELCOME_STEPS)[number];

export const WELCOME_STEP_COUNT = WELCOME_STEPS.length;

/** Clamps a step index into the valid `[0, WELCOME_STEP_COUNT - 1]` range. */
export function clampStep(index: number): number {
  if (!Number.isFinite(index)) return 0;
  return Math.max(0, Math.min(WELCOME_STEP_COUNT - 1, Math.trunc(index)));
}

/** Advances one step forward, stopping at the final step. */
export function nextStep(current: number): number {
  return clampStep(current + 1);
}

/** Moves one step backward, stopping at the first step. */
export function prevStep(current: number): number {
  return clampStep(current - 1);
}

/**
 * Whether the user may jump directly to `target` from the progress indicator.
 * Any step already visited (up through `highestVisited`) is clickable; unvisited future
 * steps stay reachable via Next so the wizard guides linearly while letting users review.
 */
export function canJumpToStep(target: number, highestVisited: number): boolean {
  const clamped = clampStep(target);
  return clamped === target && clamped <= clampStep(highestVisited);
}

/** First launch (flag missing or false) opens the Welcome wizard automatically. */
export function shouldShowWelcomeOnLaunch(settings: Pick<Settings, "onboardingCompleted">): boolean {
  return !settings.onboardingCompleted;
}

/** Returns a new `Settings` object with `onboardingCompleted` marked `true`. */
export function markOnboardingCompleted(settings: Settings): Settings {
  return { ...settings, onboardingCompleted: true };
}
