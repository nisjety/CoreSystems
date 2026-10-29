import { describe, expect, it } from "vitest";
import { safeVitalsSample, vitalsPage } from "./policy";

describe("first-party performance privacy boundary", () => {
  it("allows only known public routes and strips no private URLs into telemetry", () => {
    expect(vitalsPage("https://www.triodelab.no/innsikt", false)).toBe(
      "/innsikt",
    );
    expect(vitalsPage("http://localhost:3000/", true)).toBe("/");
    for (const url of [
      "https://www.triodelab.no/?secret=x",
      "https://www.triodelab.no/#secret",
      "https://www.triodelab.no/kontakt",
      "https://www.triodelab.no/unknown",
      "https://other.no/",
      "http://localhost:3000/",
    ]) {
      expect(vitalsPage(url, false)).toBeNull();
    }
  });
  it("accepts bounded numeric metrics only, with no DOM attribution or browser timestamp", () => {
    const sample = {
      sampleId: "11111111-1111-4111-8111-111111111111",
      sequence: 1,
      metric: "CLS",
      value: 0,
      viewport: "wide",
      path: "/",
    };
    expect(safeVitalsSample(sample)).toEqual(sample);
    for (const change of [
      { value: -1 },
      { value: Infinity },
      { value: 101 },
      { sequence: 33 },
      { sampleId: "user@example.com" },
      { path: "/?token=x" },
      { path: "/kontakt" },
      { viewport: "phone" },
      { metric: "FID" },
      { entries: [] },
      { receivedAt: new Date().toISOString() },
    ]) {
      expect(safeVitalsSample({ ...sample, ...change })).toBeNull();
    }
  });
});
