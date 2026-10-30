import { openSync, fstatSync, lstatSync, readSync, closeSync } from "node:fs";
import { isAbsolute, normalize } from "node:path";

export type VitalsConfig = Readonly<{
  websiteId: string;
  websiteRevision: number;
  site: string;
  environment: "production" | "development";
  reportingOrigin: string;
  redisUrl: string;
  exportToken: string;
}>;

function secret(name: string): string | null {
  const inline = process.env[name]?.trim();
  const file = process.env[`${name}_FILE`]?.trim();
  if (inline && file) return null;
  if (!file) return inline || null;
  if (!isAbsolute(file) || normalize(file) !== file) return null;
  let fd: number | undefined;
  try {
    const before = lstatSync(file);
    if (
      !before.isFile() ||
      (before.mode & 0o777) !== 0o600 ||
      before.size < 1 ||
      before.size > 4096
    )
      return null;
    fd = openSync(file, "r");
    const after = fstatSync(fd);
    if (
      before.dev !== after.dev ||
      before.ino !== after.ino ||
      !after.isFile() ||
      (after.mode & 0o777) !== 0o600 ||
      after.size > 4096
    )
      return null;
    const buffer = Buffer.alloc(4097);
    const length = readSync(fd, buffer, 0, buffer.length, 0);
    return length <= 4096
      ? new TextDecoder("utf-8", { fatal: true })
          .decode(buffer.subarray(0, length))
          .trim()
      : null;
  } catch {
    return null;
  } finally {
    if (fd !== undefined) closeSync(fd);
  }
}

export function getVitalsConfig(): VitalsConfig | null {
  if (process.env.WEB_VITALS_ENABLED !== "true") return null;
  const websiteId = process.env.WEB_VITALS_WEBSITE_ID ?? "";
  const revision = process.env.WEB_VITALS_WEBSITE_REVISION ?? "";
  const site = process.env.WEB_VITALS_SITE ?? "";
  const environment = process.env.WEB_VITALS_ENVIRONMENT;
  const reportingOrigin = process.env.WEB_VITALS_REPORTING_ORIGIN ?? "";
  const redisUrl = secret("WEB_VITALS_REDIS_URL");
  const exportToken = secret("WEB_VITALS_EXPORT_TOKEN");
  if (
    !/^web_[a-f0-9]{32}$/.test(websiteId) ||
    !/^[1-9][0-9]{0,15}$/.test(revision) ||
    !Number.isSafeInteger(Number(revision)) ||
    site !== "https://triodelab.no/" ||
    !redisUrl ||
    !exportToken ||
    !/^[A-Za-z0-9_-]{32,256}$/.test(exportToken)
  )
    return null;
  const development =
    environment === "development" &&
    process.env.NODE_ENV === "development" &&
    ["http://localhost:3000", "http://localhost:3001"].includes(reportingOrigin);
  const production =
    environment === "production" &&
    ["https://www.triodelab.no", "https://triodelab.no"].includes(
      reportingOrigin,
    );
  if (!development && !production) return null;
  try {
    const redis = new URL(redisUrl);
    if (
      !redis.hostname ||
      redis.search ||
      redis.hash ||
      !/^(?:\/(?:[0-9]|1[0-5]))?$/.test(redis.pathname) ||
      (production && redis.protocol !== "rediss:") ||
      (development &&
        (redis.protocol !== "redis:" ||
          !["localhost", "127.0.0.1"].includes(redis.hostname)))
    )
      return null;
  } catch {
    return null;
  }
  return {
    websiteId,
    websiteRevision: Number(revision),
    site,
    environment: environment as "production" | "development",
    reportingOrigin,
    redisUrl,
    exportToken,
  };
}
