import { afterEach, describe, expect, it, vi } from "vitest";
import { configuredEnvironment } from "./test-environment";
import { getVitalsConfig } from "./config";
import { handleVitalsPost, handleVitalsExport } from "./handler";

const sample = {
  sampleId: "12345678-1234-4234-8234-123456789012",
  sequence: 1,
  metric: "LCP",
  value: 1324.5,
  viewport: "wide",
  path: "/",
} as const;
const now = new Date("2026-10-23T12:00:00Z");
const request = (body = JSON.stringify(sample), headers = {}) =>
  new Request("http://localhost:3000/api/web-vitals", {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      Origin: "http://localhost:3000",
      "Sec-Fetch-Site": "same-origin",
      ...headers,
    },
    body,
  });
afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllEnvs();
});
describe("first-party collection boundary", () => {
  it.each(["DNT", "Sec-GPC"])(
    "never records after the %s privacy signal",
    async (header) => {
      configuredEnvironment();
      const record = vi.fn();
      expect(
        (
          await handleVitalsPost(request(undefined, { [header]: "1" }), {
            record,
          })
        ).status,
      ).toBe(204);
      expect(record).not.toHaveBeenCalled();
    },
  );
  it("stores six validated fields with server timestamp only", async () => {
    configuredEnvironment();
    const config = getVitalsConfig()!;
    const record = vi.fn().mockResolvedValue("stored");
    const response = await handleVitalsPost(request(), {
      config: () => config,
      record,
      now: () => now,
    });
    expect(response.status).toBe(204);
    expect(response.headers.get("Cache-Control")).toBe("no-store");
    expect(record).toHaveBeenCalledExactlyOnceWith(config, sample, now);
    expect(response.headers.get("set-cookie")).toBeNull();
  });
  it.each([
    [
      "origin",
      () => request(undefined, { Origin: "https://evil.example" }),
      403,
    ],
    [
      "fetch-site",
      () => request(undefined, { "Sec-Fetch-Site": "cross-site" }),
      403,
    ],
    ["media", () => request(undefined, { "Content-Type": "text/plain" }), 415],
    [
      "query",
      () =>
        new Request("http://localhost:3000/api/web-vitals?site=evil", {
          method: "POST",
          body: JSON.stringify(sample),
        }),
      400,
    ],
    ["oversize", () => request(" ".repeat(2049)), 413],
    [
      "duplicate",
      () =>
        request(
          JSON.stringify(sample).replace(
            '"sequence":1',
            '"sequence":1,"sequence":2',
          ),
        ),
      400,
    ],
    [
      "unknown",
      () => request(JSON.stringify({ ...sample, email: "private" })),
      400,
    ],
    [
      "sensitive",
      () => request(JSON.stringify({ ...sample, path: "/kontakt" })),
      400,
    ],
  ])("rejects %s before storage", async (_name, create, status) => {
    configuredEnvironment();
    const record = vi.fn();
    const response = await handleVitalsPost(create(), { record });
    expect(response.status).toBe(status);
    expect(record).not.toHaveBeenCalled();
  });
  it.each([
    ["duplicate", 204],
    ["conflict", 409],
    ["rate_limited", 429],
    ["capacity", 429],
  ])("handles %s receipts", async (result, status) => {
    configuredEnvironment();
    const response = await handleVitalsPost(request(), {
      record: vi.fn().mockResolvedValue(result),
    });
    expect(response.status).toBe(status);
    if (status === 429) expect(response.headers.get("Retry-After")).toBe("60");
  });
  it("fails closed without durable storage or configuration", async () => {
    configuredEnvironment();
    const failure = await handleVitalsPost(request(), {
      record: vi.fn().mockRejectedValue(new Error("redis://password@private")),
    });
    expect(failure.status).toBe(503);
    expect(await failure.text()).not.toContain("password");
    vi.stubEnv("WEB_VITALS_ENABLED", "false");
    expect((await handleVitalsPost(request())).status).toBe(503);
  });
});
describe("protected aggregate export", () => {
  const exportRequest = (
    authorization = `Bearer ${"t".repeat(43)}`,
    query = "",
  ) =>
    new Request(`http://localhost:3000/api/web-vitals/export${query}`, {
      headers: { Authorization: authorization },
    });
  it("requires dedicated token before reading Redis", async () => {
    configuredEnvironment();
    const read = vi.fn();
    expect(
      (await handleVitalsExport(exportRequest("Bearer wrong"), { read }))
        .status,
    ).toBe(401);
    expect(read).not.toHaveBeenCalled();
    expect(
      (await handleVitalsExport(exportRequest(undefined, "?token=x"), { read }))
        .status,
    ).toBe(400);
  });
  it("returns exact scoped report and honest empty array", async () => {
    configuredEnvironment();
    const response = await handleVitalsExport(exportRequest(), {
      read: vi.fn().mockResolvedValue([]),
      now: () => now,
    });
    expect(response.status).toBe(200);
    expect(response.headers.get("Cache-Control")).toBe("no-store");
    expect(await response.json()).toEqual({
      success: true,
      data: {
        websiteId: "web_12345678901234567890123456789012",
        websiteRevision: 7,
        site: "https://triodelab.no/",
        environment: "development",
        reportingOrigin: "http://localhost:3000",
        status: "connected",
        samples: [],
        observedAt: now.toISOString(),
      },
    });
  });
  it("never turns failed reads into empty success", async () => {
    configuredEnvironment();
    const response = await handleVitalsExport(exportRequest(), {
      read: vi.fn().mockRejectedValue(new Error("secret")),
    });
    expect(response.status).toBe(503);
    expect(await response.text()).not.toContain("secret");
  });
});
