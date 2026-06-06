import { describe, expect, it } from "vitest";
import {
  readCachedValue,
  writeCachedValue,
} from "@/features/search-v2/lib/search-query-cache";

describe("search-query-cache", () => {
  it("returns a cached value before it expires", () => {
    const cache = new Map();
    writeCachedValue(cache, "query", ["a", "b"], 1_000, 10);

    expect(readCachedValue(cache, "query", 500)).toEqual(["a", "b"]);
  });

  it("drops expired entries", () => {
    const cache = new Map();
    writeCachedValue(cache, "query", ["a"], 100, 10);

    expect(readCachedValue(cache, "query", 111)).toBeNull();
    expect(cache.has("query")).toBe(false);
  });
});
