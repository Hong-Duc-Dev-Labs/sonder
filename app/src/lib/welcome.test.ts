import { afterEach, describe, expect, it, vi } from "vitest";
import { AppState } from "./app.svelte";
import { commands, type Settings } from "./bindings";
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

afterEach(() => {
  vi.restoreAllMocks();
});

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

  it("preserves device selection across subsequent mode/lyrics/completion saves", async () => {
    const state = new AppState();
    state.settings = {
      outputDevice: null,
      outputMode: "exclusive",
      lyricsOnline: true,
      onboardingCompleted: false,
    };
    state.welcomeOpen = true;

    const savedPayloads: Settings[] = [];
    vi.spyOn(commands, "deviceSelect").mockResolvedValue(null);
    vi.spyOn(commands, "settingsSet").mockImplementation(async (next) => {
      savedPayloads.push({ ...next });
      return { ...next };
    });

    const selected = await state.selectDevice("usb-dac-1");
    expect(selected).toBe(true);
    expect(state.settings.outputDevice).toBe("usb-dac-1");

    await state.patchSettings({ outputMode: "shared" });
    await state.completeWelcomeSetup();

    expect(state.welcomeOpen).toBe(false);
    expect(savedPayloads).toHaveLength(2);
    expect(savedPayloads[0]).toMatchObject({
      outputDevice: "usb-dac-1",
      outputMode: "shared",
      onboardingCompleted: false,
    });
    expect(savedPayloads[1]).toMatchObject({
      outputDevice: "usb-dac-1",
      outputMode: "shared",
      onboardingCompleted: true,
    });
  });

  it("serializes concurrent patchSettings and completeWelcomeSetup without clobbering fields", async () => {
    const state = new AppState();
    state.settings = {
      lyricsOnline: true,
      onboardingCompleted: false,
    };
    state.welcomeOpen = true;

    let releaseFirst!: () => void;
    const firstGate = new Promise<void>((r) => (releaseFirst = r));
    const savedPayloads: Settings[] = [];

    vi.spyOn(commands, "settingsSet").mockImplementation(async (next) => {
      const snapshot = { ...next };
      if (savedPayloads.length === 0) await firstGate;
      savedPayloads.push(snapshot);
      return snapshot;
    });

    const p1 = state.patchSettings({ lyricsOnline: false });
    const p2 = state.completeWelcomeSetup();
    releaseFirst();
    await Promise.all([p1, p2]);

    expect(state.settings.lyricsOnline).toBe(false);
    expect(state.settings.onboardingCompleted).toBe(true);
    expect(savedPayloads.at(-1)).toMatchObject({
      lyricsOnline: false,
      onboardingCompleted: true,
    });
  });
});
