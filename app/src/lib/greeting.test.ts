import { afterEach, describe, expect, it } from "vitest";
import { greeting } from "./greeting";
import { i18n } from "./i18n.svelte";

describe("greeting", () => {
  const initial = i18n.locale;
  afterEach(() => {
    i18n.locale = initial;
  });

  it("changes exactly at each boundary hour", () => {
    i18n.locale = "en";
    expect(greeting(4)).toBe("Late night listening");
    expect(greeting(5)).toBe("Good morning");
    expect(greeting(11)).toBe("Good morning");
    expect(greeting(12)).toBe("Good afternoon");
    expect(greeting(16)).toBe("Good afternoon");
    expect(greeting(17)).toBe("Good evening");
    expect(greeting(20)).toBe("Good evening");
    expect(greeting(21)).toBe("Late night listening");
    expect(greeting(0)).toBe("Late night listening");
  });

  it("speaks the interface language", () => {
    i18n.locale = "vi";
    expect(greeting(8)).toBe("Chào buổi sáng");
    expect(greeting(23)).toBe("Nghe nhạc đêm khuya");
  });
});
