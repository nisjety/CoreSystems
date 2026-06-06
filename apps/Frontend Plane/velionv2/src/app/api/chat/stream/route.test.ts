import { NextRequest } from "next/server";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const requireRequestActor = vi.fn();
const mintAudienceToken = vi.fn();
const requireSession = vi.hoisted(() => vi.fn());
const resolveActiveOrgId = vi.hoisted(() => vi.fn());

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
  buildServiceHeaders: () => ({ "X-Internal-Api-Key": "test-key", "X-User-Id": "user-1", "X-Org-ID": "org-1" }),
  getGraphIndexUrl: () => "http://graph-index:9203",
  getKnowledgeRetrievalUrl: () => "http://retrieval-engine:8004",
  getModelPlaneAudience: () => "model-plane",
  getQuarryAudience: () => "quarry",
  mintAudienceToken,
  resolveActiveOrgId,
}));

vi.mock("@/app/api/_lib/control-plane-auth", () => ({
  requireSession,
}));

function makeSseResponse(body = 'event: done\ndata: {"done":true}\n\n'): Response {
  return new Response(body, {
    status: 200,
    headers: { "Content-Type": "text/event-stream" },
  });
}

describe("POST /api/chat/stream", () => {
  beforeEach(() => {
    requireRequestActor.mockResolvedValue({ userId: "user-1" });
    mintAudienceToken.mockResolvedValue("model-token");
    requireSession.mockResolvedValue({ user: { id: "user-1" } });
    resolveActiveOrgId.mockResolvedValue(null);
    vi.stubGlobal("fetch", vi.fn());
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.resetAllMocks();
  });

  it("injects Quarry web tools when browse-web search is enabled", async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValueOnce(makeSseResponse());

    const { POST } = await import("@/app/api/chat/stream/route");

    const request = new NextRequest("http://localhost/api/chat/stream", {
      method: "POST",
      headers: {
        origin: "http://localhost",
        host: "localhost",
        "content-type": "application/json",
      },
      body: JSON.stringify({
        content: "Find the latest OpenAI news",
        browseWeb: true,
        tools: ["search"],
        features: ["citations"],
      }),
    });

    const response = await POST(request);

    expect(response.status).toBe(200);
    expect(fetchMock).toHaveBeenCalledTimes(1);

    const [, init] = fetchMock.mock.calls[0] ?? [];
    const payload = JSON.parse(String(init?.body)) as {
      features?: string[];
      tools?: Array<{ name: string; description: string; parameters_json: string }>;
    };

    expect(payload.features).toEqual(expect.arrayContaining(["citations", "tools"]));
    expect(payload.tools?.map((tool) => tool.name)).toEqual([
      "web_search",
      "fetch_url",
    ]);
    expect(payload.tools?.[0]?.parameters_json).toContain("\"query\"");
    expect(payload.tools?.[1]?.parameters_json).toContain("\"url\"");
  });

  it("forwards upstream grounding events without prefetching Data Plane context in the BFF", async () => {
    const fetchMock = vi.mocked(fetch);
    resolveActiveOrgId.mockResolvedValue("org-1");
    fetchMock.mockResolvedValueOnce(
      makeSseResponse(
        [
          "event: grounding",
          'data: {"mode":"hybrid","query":"refund policy","traceId":"trace-1","lowConfidence":false,"factCount":1,"sourceCount":1,"facts":[{"knowledgeId":"kid-1","documentId":"doc-1","text":"Refunds are accepted within 30 days.","score":0.93,"sourceTitle":"Refund policy","sourceType":"policy","provider":"Notion","chunkIndex":0}],"sources":[{"id":"doc-1","kind":"knowledge","title":"Refund policy","snippet":"Refunds are accepted within 30 days.","provider":"Notion","sourceType":"policy","documentId":"doc-1","href":"/knowledge","score":0.93}],"graph":{"traceId":"graph-1","communitySummaries":["Refund policy connects with return workflow."],"edgeCount":0,"nodes":[{"id":"node-1","label":"Refund policy","kind":"policy"}]}}',
          "",
          "event: done",
          'data: {"done":true,"model_used":"gpt-4o","input_tokens":12,"output_tokens":34}',
          "",
        ].join("\n"),
      ),
    );

    const { POST } = await import("@/app/api/chat/stream/route");

    const request = new NextRequest("http://localhost/api/chat/stream", {
      method: "POST",
      headers: {
        origin: "http://localhost",
        host: "localhost",
        "content-type": "application/json",
      },
      body: JSON.stringify({
        content: "What is the refund policy?",
        features: ["citations"],
      }),
    });

    const response = await POST(request);
    const body = await response.text();

    expect(response.status).toBe(200);
    expect(fetchMock).toHaveBeenCalledTimes(1);

    const [, init] = fetchMock.mock.calls[0] ?? [];
    const payload = JSON.parse(String(init?.body)) as {
      content?: string;
    };

    expect(payload.content).not.toContain("[Knowledge grounding]");
    expect(body).toContain("event: grounding");
    expect(body).toContain('"mode":"hybrid"');
  });

  it("keeps plain chat requests tool-free when search is disabled", async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValueOnce(makeSseResponse());

    const { POST } = await import("@/app/api/chat/stream/route");

    const request = new NextRequest("http://localhost/api/chat/stream", {
      method: "POST",
      headers: {
        origin: "http://localhost",
        host: "localhost",
        "content-type": "application/json",
      },
      body: JSON.stringify({
        content: "Summarize this conversation",
        features: ["usage"],
      }),
    });

    const response = await POST(request);

    expect(response.status).toBe(200);

    const [, init] = fetchMock.mock.calls[0] ?? [];
    const payload = JSON.parse(String(init?.body)) as {
      features?: string[];
      tools?: Array<{ name: string }>;
    };

    expect(payload.features).toEqual(["usage"]);
    expect(payload.tools).toBeUndefined();
  });
});
