import { afterAll, beforeEach, describe, expect, it } from "vitest";
import { buildInformationCoreHeaders, getInformationCoreUrl } from "@/app/api/v1/information/_lib/upstream";

describe("information-core upstream config", () => {
  const originalEnv = process.env;

  beforeEach(() => {
    process.env = { ...originalEnv };
  });

  afterAll(() => {
    process.env = originalEnv;
  });

  it("prefers explicit information core url", () => {
    process.env.INFORMATION_CORE_URL = "http://example:3190/";
    expect(getInformationCoreUrl()).toBe("http://example:3190");
  });

  it("builds internal auth headers", () => {
    process.env.INTERNAL_API_KEY = "secret";
    expect(buildInformationCoreHeaders()).toEqual({
      "Content-Type": "application/json",
      "X-Internal-Api-Key": "secret",
    });
  });
});
