import { afterEach, describe, expect, it } from "vitest";
import { durationShort, plural } from "./format";
import { i18n, resolveLocale, systemLocale, t } from "./i18n.svelte";
import { en } from "./locales/en";
import { vi } from "./locales/vi";

const initial = i18n.locale;
afterEach(() => {
  i18n.locale = initial;
});

/** The `{name}` placeholders a message uses, across its plural forms. */
function placeholders(message: string | { one: string; other: string }): string[] {
  const text = typeof message === "string" ? message : `${message.one} ${message.other}`;
  return [...new Set(text.match(/\{\w+\}/g) ?? [])].sort();
}

describe("catalogs", () => {
  it("translate every English message with the same placeholders", () => {
    expect(Object.keys(vi).sort()).toEqual(Object.keys(en).sort());
    for (const key of Object.keys(en) as (keyof typeof en)[]) {
      const message = vi[key];
      expect(placeholders(message), key).toEqual(placeholders(en[key]));
      expect((typeof message === "string" ? message : message.other).trim(), key).not.toBe("");
    }
  });
});

describe("t", () => {
  it("fills placeholders and picks English plural forms", () => {
    i18n.locale = "en";
    expect(t("playlist.added", { count: 1, name: "Jazz" })).toBe("Added to Jazz");
    expect(t("playlist.added", { count: 3, name: "Jazz" })).toBe("3 tracks added to Jazz");
    expect(plural(1, "track")).toBe("1 track");
    expect(plural(2, "album")).toBe("2 albums");
  });

  it("formats numbers in the locale's style and leaves strings as given", () => {
    i18n.locale = "en";
    expect(plural(1234, "album")).toBe("1,234 albums");
    i18n.locale = "vi";
    expect(plural(1234, "album")).toBe("1.234 album");
    expect(t("badges.volume", { db: "-6.5" })).toBe("Âm lượng -6.5 dB");
  });

  it("follows the active locale", () => {
    i18n.locale = "vi";
    expect(t("common.play")).toBe("Phát");
    expect(durationShort(72 * 60_000)).toBe("1 giờ 12 phút");
    i18n.locale = "en";
    expect(durationShort(72 * 60_000)).toBe("1 h 12 min");
    expect(durationShort(60 * 60_000)).toBe("1 h");
  });
});

describe("locale resolution", () => {
  it("uses the first system language Sonder has, else English", () => {
    expect(systemLocale(["vi-VN", "en-US"])).toBe("vi");
    expect(systemLocale(["fr-FR", "vi"])).toBe("vi");
    expect(systemLocale(["en-GB", "vi-VN"])).toBe("en");
    expect(systemLocale(["ja-JP"])).toBe("en");
    expect(systemLocale([])).toBe("en");
  });

  it("lets an explicit choice override the system", () => {
    expect(resolveLocale("en", ["vi-VN"])).toBe("en");
    expect(resolveLocale("vi", ["en-US"])).toBe("vi");
    expect(resolveLocale("system", ["vi-VN"])).toBe("vi");
    expect(resolveLocale(undefined, ["vi-VN"])).toBe("vi");
  });
});
