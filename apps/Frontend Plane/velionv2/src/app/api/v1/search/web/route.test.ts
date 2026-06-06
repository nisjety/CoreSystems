import { NextRequest } from "next/server";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const requireRequestActor = vi.fn();
const mintAudienceToken = vi.fn();

vi.mock("@/lib/integrations/request-actor", () => ({
  RequestActorError: class RequestActorError extends Error {
    code: string;
    status: number;

    constructor(message: string, code = "request_actor_failed", status = 500) {
      super(message);
      this.code = code;
      this.status = status;
    }
  },
  requireRequestActor,
}));

vi.mock("@/app/api/onboarding/_lib/onboarding-proxy", () => ({
  getQuarryAudience: () => "quarry",
  getQuarryEdgeUrl: () => "http://quarry-edge:8082",
  mintAudienceToken,
}));

function makeJsonResponse(body: unknown, status = 200): Response {
  return {
    ok: status >= 200 && status < 300,
    status,
    json: () => Promise.resolve(body),
  } as Response;
}

describe("POST /api/v1/search/web", () => {
  beforeEach(() => {
    requireRequestActor.mockResolvedValue(undefined);
    mintAudienceToken.mockResolvedValue("dev-token");
    vi.stubGlobal("fetch", vi.fn());
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.resetAllMocks();
  });

  it("returns Quarry search payload directly when Quarry succeeds with empty results", async () => {
    const mockFetch = vi.mocked(fetch);
    mockFetch.mockResolvedValueOnce(
      makeJsonResponse({
        query: "OpenAI",
        provider: "hybrid",
        results: [],
        count: 0,
        answer: "",
        citations: [],
      }),
    );

    const { POST } = await import("@/app/api/v1/search/web/route");

    const request = new NextRequest("http://localhost/api/v1/search/web", {
      method: "POST",
      headers: {
        origin: "http://localhost",
        host: "localhost",
        "content-type": "application/json",
      },
      body: JSON.stringify({ query: "OpenAI", limit: 3 }),
    });

    const response = await POST(request);
    const payload = await response.json();

    expect(response.status).toBe(200);
    expect(payload.data).toEqual({
      mode: "search",
      results: [],
      answer: null,
      citations: [],
    });
    expect(mockFetch).toHaveBeenCalledTimes(1);
    expect(mockFetch.mock.calls[0]?.[0]).toBe("http://quarry-edge:8082/v1/search");
  });

  it("surfaces Quarry upstream failures instead of bypassing Quarry", async () => {
    const mockFetch = vi.mocked(fetch);
    mockFetch.mockResolvedValueOnce(makeJsonResponse({ error: "missing bearer" }, 401));

    const { POST } = await import("@/app/api/v1/search/web/route");

    const request = new NextRequest("http://localhost/api/v1/search/web", {
      method: "POST",
      headers: {
        origin: "http://localhost",
        host: "localhost",
        "content-type": "application/json",
      },
      body: JSON.stringify({ query: "OpenAI API", limit: 4 }),
    });

    const response = await POST(request);
    const payload = await response.json();

    expect(response.status).toBe(502);
    expect(payload.error).toEqual({
      code: "web_search_unavailable",
      message: "Web search could not be completed (upstream 401).",
    });
    expect(mockFetch).toHaveBeenCalledTimes(1);
  });

  it("preserves the explicit Quarry 501 unconfigured response", async () => {
    const mockFetch = vi.mocked(fetch);
    mockFetch.mockResolvedValueOnce(makeJsonResponse({ error: "not configured" }, 501));

    const { POST } = await import("@/app/api/v1/search/web/route");

    const request = new NextRequest("http://localhost/api/v1/search/web", {
      method: "POST",
      headers: {
        origin: "http://localhost",
        host: "localhost",
        "content-type": "application/json",
      },
      body: JSON.stringify({ query: "OpenAI API", limit: 4 }),
    });

    const response = await POST(request);
    const payload = await response.json();

    expect(response.status).toBe(501);
    expect(payload.error).toEqual({
      code: "search_provider_unconfigured",
      message: "Web search is unavailable — no search provider is configured.",
    });
    expect(mockFetch).toHaveBeenCalledTimes(1);
  });
});
