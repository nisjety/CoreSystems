import { describe, expect, it } from "vitest";
import { getNextOwnedProductIndex } from "./product-selection";

describe("getNextOwnedProductIndex", () => {
  it.each([
    ["ArrowRight", 0, 2, 1],
    ["ArrowRight", 1, 2, 0],
    ["ArrowLeft", 0, 2, 1],
    ["ArrowLeft", 1, 2, 0],
    ["Home", 1, 2, 0],
    ["End", 0, 2, 1],
  ] as const)(
    "moves from product $currentIndex with $key",
    (key, currentIndex, productCount, expectedIndex) => {
      expect(getNextOwnedProductIndex(key, currentIndex, productCount)).toBe(
        expectedIndex,
      );
    },
  );

  it("ignores unsupported keys and empty collections", () => {
    expect(getNextOwnedProductIndex("Enter", 0, 2)).toBeNull();
    expect(getNextOwnedProductIndex("ArrowRight", 0, 0)).toBeNull();
  });
});
