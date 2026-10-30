import { describe, expect, it } from "vitest";
import { smoothScrollEasing } from "./scroll-easing";

describe("smoothScrollEasing", () => {
  it("uses the existing bounded exponential scroll easing", () => {
    expect(smoothScrollEasing(0)).toBeCloseTo(0.001);
    expect(smoothScrollEasing(0.5)).toBeCloseTo(0.96975);
    expect(smoothScrollEasing(1)).toBe(1);
  });
});
