import { describe, expect, it } from "vitest";
import { numberField, splitNames } from "./tagEdit";

describe("splitNames", () => {
  it("splits on semicolons and trims each name", () => {
    expect(splitNames("Miles Davis; John Coltrane")).toEqual(["Miles Davis", "John Coltrane"]);
    expect(splitNames("  Miles Davis ;;John Coltrane; ")).toEqual(["Miles Davis", "John Coltrane"]);
  });

  it("keeps a single name with slashes or commas whole", () => {
    expect(splitNames("AC/DC")).toEqual(["AC/DC"]);
    expect(splitNames("Earth, Wind & Fire")).toEqual(["Earth, Wind & Fire"]);
  });

  it("gives an empty list for an empty field", () => {
    expect(splitNames("")).toEqual([]);
    expect(splitNames("   ;  ")).toEqual([]);
  });
});

describe("numberField", () => {
  it("reads a number", () => {
    expect(numberField("3")).toBe(3);
    expect(numberField(" 12 ")).toBe(12);
  });

  it("treats an empty or unusable box as no value", () => {
    expect(numberField("")).toBeNull();
    expect(numberField("   ")).toBeNull();
    expect(numberField("0")).toBeNull();
    expect(numberField("-2")).toBeNull();
    expect(numberField("jazz")).toBeNull();
  });

  it("keeps a number inside what the tags hold", () => {
    expect(numberField("99999999")).toBe(9999);
  });
});
