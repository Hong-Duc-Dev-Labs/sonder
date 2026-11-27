import { describe, expect, it } from "vitest";
import {
  WELCOME_STEP_COUNT,
  WELCOME_STEPS,
  canJumpToStep,
  clampStep,
  markOnboardingCompleted,
  nextStep,
  prevStep,
  shouldShowWelcomeOnLaunch,
} from "./welcome";

describe("welcome wizard navigation", () => {
  it("defines the 4 ordered onboarding steps", () => {
    expect(WELCOME_STEPS).toEqual(["welcome", "sources", "audio", "finish"]);
    expect(WELCOME_STEP_COUNT).toBe(4);
  });

  it("clamps step indices and steps forward/backward within bounds", () => {
    expect(clampStep(-3)).toBe(0);
    expect(clampStep(99)).toBe(3);
    expect(clampStep(Number.NaN)).toBe(0);

    expect(nextStep(0)).toBe(1);
    expect(nextStep(2)).toBe(3);
    expect(nextStep(3)).toBe(3);

    expect(prevStep(3)).toBe(2);
    expect(prevStep(1)).toBe(0);
    expect(prevStep(0)).toBe(0);
  });

  it("allows jumping only to already-visited steps", () => {
    expect(canJumpToStep(0, 0)).toBe(true);
    expect(canJumpToStep(1, 0)).toBe(false);
    expect(canJumpToStep(1, 2)).toBe(true);
    expect(canJumpToStep(2, 2)).toBe(true);
    expect(canJumpToStep(3, 2)).toBe(false);
    expect(canJumpToStep(-1, 3)).toBe(false);
    expect(canJumpToStep(4, 3)).toBe(false);
  });
});

describe("welcome wizard persistence", () => {
  it("shows on first launch only when onboardingCompleted is missing or false", () => {
    expect(shouldShowWelcomeOnLaunch({})).toBe(true);
    expect(shouldShowWelcomeOnLaunch({ onboardingCompleted: false })).toBe(true);
    expect(shouldShowWelcomeOnLaunch({ onboardingCompleted: true })).toBe(false);
  });

  it("marks onboardingCompleted true without mutating other settings", () => {
    const initial = { language: "vi" as const, lyricsOnline: false, onboardingCompleted: false };
    const updated = markOnboardingCompleted(initial);
    expect(updated).toEqual({ language: "vi", lyricsOnline: false, onboardingCompleted: true });
    expect(initial.onboardingCompleted).toBe(false);
  });
});
