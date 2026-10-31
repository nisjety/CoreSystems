import { randomBytes, randomUUID } from "node:crypto";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { createClient } from "redis";
import { recordVital, readVitals, storagePrefix } from "./storage";
import type { VitalsConfig } from "./config";

const url = process.env.WEB_VITALS_TEST_REDIS_URL;
describe.skipIf(!url)("Web Vitals actual Redis atomic storage", () => {
  const config: VitalsConfig = {
    websiteId: `web_${randomBytes(16).toString("hex")}`,
    websiteRevision: 1,
    site: "https://triodelab.no/",
    environment: "development",
    reportingOrigin: "http://localhost:3000",
    redisUrl: url ?? "redis://localhost:6379/15",
    exportToken: "t".repeat(43),
  };
  const client = createClient({
    url: config.redisUrl,
    socket: { reconnectStrategy: false, connectTimeout: 3000 },
  });
  client.on("error", () => undefined);
  const prefix = storagePrefix(config);
  beforeAll(async () => {
    await client.connect();
  });
  afterAll(async () => {
    if (client.isReady) {
      const keys = await client.keys(`${prefix}*`);
      if (keys.length) await client.unlink(keys);
    }
    if (client.isOpen) client.destroy();
  });
  it("upserts latest sequence, keeps first received time and page context", async () => {
    const now = new Date(),
      sample = {
        sampleId: randomUUID(),
        sequence: 1,
        metric: "LCP",
        value: 1234,
        viewport: "wide",
        path: "/",
      } as const;
    expect(await recordVital(config, sample, now)).toBe("stored");
    expect(await recordVital(config, sample, new Date(now.getTime() + 1))).toBe(
      "duplicate",
    );
    expect(await recordVital(config, { ...sample, value: 4321 }, now)).toBe(
      "conflict",
    );
    expect(
      await recordVital(
        config,
        { ...sample, sequence: 2, value: 1500 },
        new Date(now.getTime() + 2),
      ),
    ).toBe("stored");
    expect(
      await recordVital(config, { ...sample, metric: "INP", value: 0 }, now),
    ).toBe("stored");
    expect(
      await recordVital(
        config,
        { ...sample, metric: "CLS", value: 0, path: "/innsikt" },
        now,
      ),
    ).toBe("conflict");
    const rows = (await readVitals(config, new Date())).filter(
      (s) => s.sampleId === sample.sampleId,
    );
    expect(rows).toHaveLength(2);
    expect(rows.find((s) => s.metric === "LCP")).toMatchObject({
      value: 1500,
      sequence: 2,
      receivedAt: now.toISOString(),
    });
  });
  it("never extends metric, page or day retention on later updates", async () => {
    const now = new Date(),
      first = new Date(
        Date.UTC(
          now.getUTCFullYear(),
          now.getUTCMonth(),
          now.getUTCDate() - 6,
          1,
        ),
      );
    const sample = {
      sampleId: randomUUID(),
      sequence: 1,
      metric: "CLS",
      value: 0,
      viewport: "narrow",
      path: "/innsikt",
    } as const;
    expect(await recordVital(config, sample, first)).toBe("stored");
    const metric = `${prefix}sample:${sample.sampleId}:CLS`,
      page = `${prefix}page:${sample.sampleId}`,
      index = `${prefix}day:${first.toISOString().slice(0, 10)}`;
    const expiry = Date.UTC(
      first.getUTCFullYear(),
      first.getUTCMonth(),
      first.getUTCDate() + 7,
    );
    // Dragonfly's compatibility timestamp is derived from a relative timer;
    // tolerate sub-second early rounding, but never a later expiry.
    const checkExpiry = async (key: string) => {
      const actual = Number(await client.pExpireTime(key));
      expect(actual).toBeLessThanOrEqual(expiry);
      expect(actual).toBeGreaterThanOrEqual(expiry - 1000);
    };
    await checkExpiry(metric);
    expect(
      await recordVital(config, { ...sample, sequence: 2, value: 0.04 }, now),
    ).toBe("stored");
    await checkExpiry(metric);
    await checkExpiry(page);
    await checkExpiry(index);
    expect(
      (await readVitals(config, now)).find(
        (s) => s.sampleId === sample.sampleId,
      ),
    ).toMatchObject({ receivedAt: first.toISOString(), value: 0.04 });
  });
  it("takes an explicit snapshot cutoff while concurrent later receipts wait for the next export", async () => {
    const cutoff = new Date(),
      receivedLater = new Date(cutoff.getTime() + 1000);
    const sample = {
      sampleId: randomUUID(),
      sequence: 1,
      metric: "LCP",
      value: 1200,
      viewport: "wide",
      path: "/",
    } as const;
    expect(await recordVital(config, sample, receivedLater)).toBe("stored");
    expect(
      (await readVitals(config, cutoff)).some(
        (s) => s.sampleId === sample.sampleId,
      ),
    ).toBe(false);
    expect(
      (await readVitals(config, receivedLater)).some(
        (s) => s.sampleId === sample.sampleId,
      ),
    ).toBe(true);
  });
  it("rejects more than120 requests in a site minute atomically", async () => {
    const now = new Date(),
      rate = `${prefix}rate:${Math.floor(now.getTime() / 60000)}`;
    await client.set(rate, "120", {
      PXAT: (Math.floor(now.getTime() / 60000) + 1) * 60000,
    });
    expect(
      await recordVital(
        config,
        {
          sampleId: randomUUID(),
          sequence: 1,
          metric: "LCP",
          value: 100,
          viewport: "wide",
          path: "/",
        },
        now,
      ),
    ).toBe("rate_limited");
    await client.del(rate);
  });
  it("enforces the whole seven-day sample cap, not only today's count", async () => {
    const now = new Date(),
      yesterday = new Date(now.getTime() - 86400000).toISOString().slice(0, 10);
    const index = `${prefix}day:${yesterday}`;
    await client.sAdd(
      index,
      Array.from({ length: 5000 }, (_, i) => `test-capacity-${i}`),
    );
    expect(
      await recordVital(
        config,
        {
          sampleId: randomUUID(),
          sequence: 1,
          metric: "INP",
          value: 0,
          viewport: "wide",
          path: "/",
        },
        now,
      ),
    ).toBe("capacity");
    await client.del(index);
  });
  it("fails closed on malformed stored timestamps even when they sort after the cutoff", async () => {
    const now = new Date(),
      sampleId = randomUUID(),
      index = `${prefix}day:${now.toISOString().slice(0, 10)}`,
      member = `${sampleId}:LCP`;
    await client.set(
      `${prefix}sample:${member}`,
      JSON.stringify({
        sampleId,
        sequence: 1,
        metric: "LCP",
        value: 1000,
        viewport: "wide",
        path: "/",
        receivedAt: "zzz",
      }),
    );
    await client.sAdd(index, member);
    await expect(readVitals(config, now)).rejects.toThrow(
      "web_vitals_storage_unavailable",
    );
    await client.del(`${prefix}sample:${member}`);
    await client.sRem(index, member);
  });
});
