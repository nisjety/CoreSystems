// These checks use only an in-process fake upstream. No mail or provider requests.
import { afterEach, describe, expect, it, vi } from "vitest";
import { chmodSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { handleContactRequest } from "./handler";
import { getWebsiteIntakeConfig } from "./website-intake";

const directory = mkdtempSync(join(tmpdir(), "triodelab-intake-"));
const secret = join(directory, "token");
writeFileSync(secret, "t".repeat(48), { mode: 0o600 });
const fields = {
  submissionId: "95572209-f995-45a3-93b8-b6e8f115b156",
  name: "Development test",
  email: "test@example.com",
  company: "Test",
  projectType: "ki",
  message: "Controlled local inquiry test.",
  source: "ki-og-automatisering",
  website: "",
};
function request(payload: unknown = fields) {
  return new Request("http://localhost:3000/api/contact", {
    method: "POST",
    headers: {
      origin: "http://localhost:3000",
      "content-type": "application/json",
    },
    body: JSON.stringify(payload),
  });
}
function configured() {
  vi.stubEnv("NODE_ENV", "development");
  vi.stubEnv("CONTACT_DELIVERY_MODE", "leads");
  vi.stubEnv(
    "CONTACT_WEBSITE_INTAKE_URL",
    "http://127.0.0.1:3164/api/v1/website-inquiries",
  );
  vi.stubEnv("CONTACT_WEBSITE_INTAKE_TOKEN_FILE", secret);
}
afterEach(() => {
  chmodSync(secret, 0o600);
  vi.unstubAllEnvs();
  vi.unstubAllGlobals();
});
import { afterAll } from "vitest";
afterAll(() => rmSync(directory, { recursive: true, force: true }));
describe("confirmed first-party inquiry delivery", () => {
  it("confirms only a durable Leads receipt and sends no Resend email or browser org scope", async () => {
    configured();
    const fetcher = vi
      .fn()
      .mockResolvedValue(
        Response.json({
          data: {
            receiptId: `plead_${"a".repeat(24)}`,
            acceptedAt: "2026-10-28T12:00:00Z",
          },
        }),
      );
    vi.stubGlobal("fetch", fetcher);
    const response = await handleContactRequest(request());
    expect(response.status).toBe(200);
    expect((await response.json()).success).toBe(true);
    expect(fetcher).toHaveBeenCalledTimes(1);
    expect(fetcher.mock.calls[0][0]).toBe(
      "http://127.0.0.1:3164/api/v1/website-inquiries",
    );
    expect(JSON.parse(fetcher.mock.calls[0][1].body)).not.toHaveProperty(
      "website",
    );
    expect(JSON.parse(fetcher.mock.calls[0][1].body)).not.toHaveProperty(
      "organizationId",
    );
  });
  it("does not claim conversion on upstream failure, invalid receipt or changed idempotent content", async () => {
    configured();
    const fetcher = vi.fn();
    vi.stubGlobal("fetch", fetcher);
    for (const status of [503, 409]) {
      fetcher.mockResolvedValue(new Response("private", { status }));
      const result = await handleContactRequest(request());
      expect(result.status).toBe(status === 409 ? 409 : 502);
      expect(await result.text()).not.toContain("private");
    }
    fetcher.mockResolvedValue(
      Response.json({ data: { receiptId: "invalid" } }),
    );
    expect((await handleContactRequest(request())).status).toBe(502);
  });
  it("rejects browser-supplied scope, unsafe secrets and production HTTP before delivery", async () => {
    configured();
    const fetcher = vi.fn();
    vi.stubGlobal("fetch", fetcher);
    expect(
      (
        await handleContactRequest(
          request({ ...fields, organizationId: "other" }),
        )
      ).status,
    ).toBe(400);
    chmodSync(secret, 0o644);
    expect((await handleContactRequest(request())).status).toBe(503);
    expect(fetcher).not.toHaveBeenCalled();
    chmodSync(secret, 0o600);
    vi.stubEnv("NODE_ENV", "production");
    expect(getWebsiteIntakeConfig()).toBeNull();
    expect(fetcher).not.toHaveBeenCalled();
  });
});
