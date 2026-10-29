import { afterEach, describe, expect, it, vi } from "vitest";
import { mkdtempSync, writeFileSync, chmodSync, symlinkSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { getVitalsConfig } from "./config";
import { configuredEnvironment } from "./test-environment";
afterEach(() => vi.unstubAllEnvs());
describe("Web Vitals server configuration", () => {
  it("requires explicit enabled and separates production/local origin", () => {
    configuredEnvironment();
    expect(getVitalsConfig()?.environment).toBe("development");
    vi.stubEnv("WEB_VITALS_ENABLED", "false");
    expect(getVitalsConfig()).toBeNull();
    vi.stubEnv("WEB_VITALS_ENABLED", "true");
    vi.stubEnv("NODE_ENV", "production");
    expect(getVitalsConfig()).toBeNull();
    vi.stubEnv("WEB_VITALS_ENVIRONMENT", "production");
    vi.stubEnv("WEB_VITALS_REPORTING_ORIGIN", "https://www.triodelab.no");
    expect(getVitalsConfig()).toBeNull();
    vi.stubEnv(
      "WEB_VITALS_REDIS_URL",
      "rediss://default:secret@redis.example.com:6379",
    );
    expect(getVitalsConfig()?.environment).toBe("production");
    vi.stubEnv("WEB_VITALS_REPORTING_ORIGIN", "https://evil.example");
    expect(getVitalsConfig()).toBeNull();
  });
  it("accepts only the two explicit local ports in development", () => {
    configuredEnvironment();
    vi.stubEnv("WEB_VITALS_REPORTING_ORIGIN", "http://localhost:3001");
    expect(getVitalsConfig()?.reportingOrigin).toBe("http://localhost:3001");
    vi.stubEnv("WEB_VITALS_REPORTING_ORIGIN", "http://localhost:3002");
    expect(getVitalsConfig()).toBeNull();
    vi.stubEnv("WEB_VITALS_REPORTING_ORIGIN", "http://localhost:3001");
    vi.stubEnv("NODE_ENV", "production");
    expect(getVitalsConfig()).toBeNull();
  });
  it("reads only protected regular file secrets and rejects ambiguity", () => {
    configuredEnvironment();
    const dir = mkdtempSync(join(tmpdir(), "vitals-config-test-"));
    const file = join(dir, "token");
    writeFileSync(file, "t".repeat(43), { mode: 0o600 });
    vi.stubEnv("WEB_VITALS_EXPORT_TOKEN_FILE", file);
    expect(getVitalsConfig()).toBeNull();
    vi.stubEnv("WEB_VITALS_EXPORT_TOKEN", "");
    expect(getVitalsConfig()?.exportToken).toHaveLength(43);
    chmodSync(file, 0o644);
    expect(getVitalsConfig()).toBeNull();
    chmodSync(file, 0o600);
    const link = join(dir, "link");
    symlinkSync(file, link);
    vi.stubEnv("WEB_VITALS_EXPORT_TOKEN_FILE", link);
    expect(getVitalsConfig()).toBeNull();
  });
  it.each([
    ["WEB_VITALS_WEBSITE_ID", "other"],
    ["WEB_VITALS_WEBSITE_REVISION", "0"],
    ["WEB_VITALS_WEBSITE_REVISION", "07"],
    ["WEB_VITALS_SITE", "https://other.example/"],
    ["WEB_VITALS_ENVIRONMENT", "staging"],
    ["WEB_VITALS_EXPORT_TOKEN", "short"],
    ["WEB_VITALS_REDIS_URL", "http://localhost:6379"],
    ["WEB_VITALS_REDIS_URL", "redis://evil.example:6379"],
  ])("rejects invalid %s", (name, value) => {
    configuredEnvironment();
    vi.stubEnv(name, value);
    expect(getVitalsConfig()).toBeNull();
  });
});
