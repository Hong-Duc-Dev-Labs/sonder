import { describe, expect, it } from "vitest";
import type { MetadataCandidate } from "./bindings";
import {
  applyCandidate,
  coverDataUrl,
  coverFromDataUrl,
  formFromTags,
  numberField,
  splitNames,
  type TagForm,
} from "./tagEdit";

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

const form: TagForm = {
  title: "So What",
  artists: "Miles Davis",
  album: "",
  albumArtists: "",
  trackNo: "",
  discNo: "",
  year: "",
  genre: "Modal jazz",
};

const candidate: MetadataCandidate = {
  source: "Deezer",
  title: "So What (Live)",
  artists: ["Miles Davis", "John Coltrane"],
  album: "Kind of Blue",
  albumArtists: ["Miles Davis"],
  trackNo: 1,
  discNo: null,
  year: 1959,
  genre: "",
  durationMs: 562000,
  lengthMatches: true,
  coverUrl: null,
  thumbUrl: null,
};

describe("formFromTags", () => {
  it("joins names and turns numbers into text", () => {
    expect(
      formFromTags({
        title: "So What",
        artists: ["Miles Davis", "John Coltrane"],
        album: "Kind of Blue",
        albumArtists: [],
        trackNo: 1,
        discNo: null,
        year: 1959,
        genre: "",
        lyrics: null,
        cover: { kind: "keep" },
      }),
    ).toEqual({
      title: "So What",
      artists: "Miles Davis; John Coltrane",
      album: "Kind of Blue",
      albumArtists: "",
      trackNo: "1",
      discNo: "",
      year: "1959",
      genre: "",
    });
  });
});

describe("applyCandidate", () => {
  it("takes every field the catalogue knows", () => {
    const filled = applyCandidate(form, candidate);
    expect(filled.title).toBe("So What (Live)");
    expect(filled.artists).toBe("Miles Davis; John Coltrane");
    expect(filled.album).toBe("Kind of Blue");
    expect(filled.albumArtists).toBe("Miles Davis");
    expect(filled.trackNo).toBe("1");
    expect(filled.year).toBe("1959");
  });

  it("keeps what the user has where the catalogue knows nothing", () => {
    const filled = applyCandidate({ ...form, discNo: "2" }, candidate);
    expect(filled.genre).toBe("Modal jazz");
    expect(filled.discNo).toBe("2");
    expect(applyCandidate(form, { ...candidate, title: "  ", artists: [] })).toMatchObject({
      title: "So What",
      artists: "Miles Davis",
    });
  });
});

describe("cover data URLs", () => {
  it("reads JPEG and PNG and refuses other images", () => {
    expect(coverFromDataUrl("data:image/png;base64,iVBORw0KGgo=")).toEqual({
      mime: "image/png",
      data: "iVBORw0KGgo=",
    });
    expect(coverFromDataUrl("data:image/jpeg;base64,/9j/4AAQ")).toEqual({
      mime: "image/jpeg",
      data: "/9j/4AAQ",
    });
    expect(coverFromDataUrl("data:image/gif;base64,R0lGOD")).toBeNull();
    expect(coverFromDataUrl("https://example.com/a.jpg")).toBeNull();
  });

  it("round-trips through coverDataUrl", () => {
    const image = { mime: "image/jpeg", data: "/9j/4AAQ" };
    expect(coverFromDataUrl(coverDataUrl(image))).toEqual(image);
  });
});
