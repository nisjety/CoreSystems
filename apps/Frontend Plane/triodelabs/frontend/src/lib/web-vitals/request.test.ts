import { describe, expect, it } from "vitest";
import { parseSampleJSON, readSample } from "./request";

const sample = {
  sampleId: "12345678-1234-4234-8234-123456789012",
  sequence: 1,
  metric: "LCP",
  value: 0,
  viewport: "wide",
  path: "/",
};
describe("Web Vitals flat JSON request", () => {
  it("accepts precise primitive fields and whitespace", () => {
    expect(parseSampleJSON(` \n${JSON.stringify(sample)} \t`)).toEqual(sample);
  });
  it.each([
    "",
    "null",
    "[]",
    "{}",
    '{"x":{}}',
    '{"x":true}',
    JSON.stringify(sample) + "{}",
    JSON.stringify(sample).replace('"sequence":1', '"sequence":1,'),
    JSON.stringify(sample).replace(
      '"path":"/"',
      '"path":"/","pa\\u0074h":"/innsikt"',
    ),
    JSON.stringify(sample).replace('"path":"/"', '"path":"/","__proto__":"x"'),
    JSON.stringify(sample).replace('"value":0', '"value":1e999'),
  ])("rejects malformed, unknown or duplicate fields: %s", (raw) => {
    expect(() => parseSampleJSON(raw)).toThrow();
  });
  it("rejects large streamed bodies without trusting content length", async () => {
    const req = new Request("http://localhost:3000/api/web-vitals", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: " ".repeat(2049),
    });
    await expect(readSample(req)).rejects.toMatchObject({ status: 413 });
  });
});
