import { describe, expect, it } from "vitest";
import {
  formatRelativeNorwegianTime,
  newsCategoryOptions,
  weatherGlyph,
} from "@/features/dashboard-v2/lib/information-core";

describe("information dashboard helpers", () => {
  it("maps weather conditions to glyphs", () => {
    expect(weatherGlyph("Partly cloudy")).toBe("⛅");
  });

  it("formats recent relative time in norwegian", () => {
    const value = new Date(Date.now() - 5 * 60 * 1000).toISOString();
    expect(formatRelativeNorwegianTime(value)).toMatch(/m siden/);
  });

  it("includes an all option for news filters", () => {
    expect(newsCategoryOptions[0]).toEqual({ value: "all", label: "Alle" });
  });
});
