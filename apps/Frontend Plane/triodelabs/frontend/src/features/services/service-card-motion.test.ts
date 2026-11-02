import { describe, expect, it } from "vitest";
import {
  getServiceCardMotionProfile,
  getServiceCardPointerTransform,
} from "./service-card-motion";

describe("service card pointer motion", () => {
  it("gives each card its own depth and pointer-response profile", () => {
    const firstCard = getServiceCardMotionProfile(0);
    const secondCard = getServiceCardMotionProfile(1);

    expect(firstCard).not.toEqual(secondCard);
    expect(firstCard.scale).toBeLessThan(secondCard.scale);
  });

  it("moves a card across both axes and adds a subtle rotation", () => {
    expect(getServiceCardPointerTransform(0, { x: 1, y: -1 }, false)).toEqual({
      x: 16,
      y: -10,
      rotate: 0.7,
      scale: 1.004,
    });
  });

  it("keeps cards still when reduced motion is preferred", () => {
    expect(getServiceCardPointerTransform(2, { x: 1, y: -1 }, true)).toEqual({
      x: 0,
      y: 0,
      rotate: 0,
      scale: 1,
    });
  });
});
