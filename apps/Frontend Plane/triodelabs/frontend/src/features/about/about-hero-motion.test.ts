import { describe, expect, it } from "vitest";
import {
  ABOUT_HERO_COMPACT_HEIGHT_VH,
  ABOUT_HERO_COMPACT_WIDTH_VW,
  ABOUT_HERO_SCROLL_DISTANCE_VH,
  ABOUT_HERO_START_HEIGHT_VH,
} from "./about-hero-motion";

describe("about hero motion", () => {
  it("opens in an inset 80vh frame before compacting into an editorial frame", () => {
    expect(ABOUT_HERO_SCROLL_DISTANCE_VH).toBe(138);
    expect(ABOUT_HERO_START_HEIGHT_VH).toBe(80);
    expect(ABOUT_HERO_COMPACT_WIDTH_VW).toBe(82);
    expect(ABOUT_HERO_COMPACT_HEIGHT_VH).toBe(52);
  });
});
