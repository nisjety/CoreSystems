import { describe, expect, it } from "vitest";
import { getNextServiceIndex } from "./service-selection";

describe("getNextServiceIndex", () => {
  it.each([
    ["ArrowRight", 0, 4, 1],
    ["ArrowLeft", 0, 4, 3],
    ["ArrowLeft", 0, 5, 4],
    ["Home", 3, 4, 0],
    ["End", 0, 4, 3],
    ["End", 0, 5, 4],
  ] as const)(
    "moves from service $currentIndex with $key",
    (key, currentIndex, serviceCount, expectedIndex) => {
      expect(getNextServiceIndex(key, currentIndex, serviceCount)).toBe(
        expectedIndex,
      );
    },
  );

  it("ignores unsupported keys", () => {
    expect(getNextServiceIndex("Enter", 1, 4)).toBeNull();
  });

  it("does not navigate an empty service collection", () => {
    expect(getNextServiceIndex("ArrowRight", 0, 0)).toBeNull();
  });
});
