import { afterEach, describe, expect, it, vi } from "vitest";

import { getPublicConvexUrl } from "@/lib/convex/convex-env";

const ORIGINAL_ENV = { ...process.env };

afterEach(() => {
  vi.unstubAllEnvs();
  process.env = { ...ORIGINAL_ENV };
});

describe("getPublicConvexUrl", () => {
  it("prefers NEXT_PUBLIC_CONVEX_URL", () => {
    vi.stubEnv("NEXT_PUBLIC_CONVEX_URL", "http://localhost:3210/");
    vi.stubEnv("NEXT_PUBLIC_CONVEX_HTTP", "http://localhost:3211");

    expect(getPublicConvexUrl()).toBe("http://localhost:3210");
  });

  it("falls back to NEXT_PUBLIC_CONVEX_HTTP", () => {
    vi.stubEnv("NEXT_PUBLIC_CONVEX_URL", "");
    vi.stubEnv("NEXT_PUBLIC_CONVEX_HTTP", "http://localhost:3210/");

    expect(getPublicConvexUrl()).toBe("http://localhost:3210");
  });

  it("returns null when no public Convex URL is configured", () => {
    vi.stubEnv("NEXT_PUBLIC_CONVEX_URL", "");
    vi.stubEnv("NEXT_PUBLIC_CONVEX_HTTP", "");

    expect(getPublicConvexUrl()).toBeNull();
  });
});
