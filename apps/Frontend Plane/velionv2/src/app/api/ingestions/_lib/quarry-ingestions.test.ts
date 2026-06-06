import { NextRequest } from "next/server";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const requireSession = vi.hoisted(() => vi.fn());
const mintAudienceToken = vi.hoisted(() => vi.fn());
const resolveActiveOrgId = vi.hoisted(() => vi.fn());

vi.mock("@/app/api/_lib/control-plane-auth", async () => {
  const actual = await vi.importActual<typeof import("@/app/api/_lib/control-plane-auth")>("@/app/api/_lib/control-plane-auth");
  return {
    ...actual,
    requireSession,
  };
});

vi.mock("@/app/api/onboarding/_lib/onboarding-proxy", () => ({
  getQuarryAudience: () => "quarry",
  getQuarryEdgeUrl: () => "http://quarry-edge:8082",
  mintAudienceToken,
  resolveActiveOrgId,
}));

describe("fetchQuarry", () => {
  beforeEach(() => {
    requireSession.mockResolvedValue({ user: { id: "user-1" } });
    mintAudienceToken.mockResolvedValue("quarry-token");
    resolveActiveOrgId.mockResolvedValue("org-1");
    vi.stubGlobal("fetch", vi.fn());
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.resetAllMocks();
  });

  it("maps abort timeouts to a ControlPlaneAuthError", async () => {
    vi.mocked(fetch).mockImplementationOnce((_, init) => new Promise((_, reject) => {
      const signal = init?.signal;
      if (signal instanceof AbortSignal) {
        signal.addEventListener("abort", () => reject(new DOMException("The operation was aborted.", "AbortError")), { once: true });
      }
    }));

    const { fetchQuarry } = await import("@/app/api/ingestions/_lib/quarry-ingestions");

    await expect(fetchQuarry(
      new NextRequest("http://localhost/api/ingestions/sources"),
      "/v1/sources",
      { timeoutMs: 10 },
    )).rejects.toMatchObject({
      code: "quarry_request_timeout",
      status: 504,
    });
  });
});
