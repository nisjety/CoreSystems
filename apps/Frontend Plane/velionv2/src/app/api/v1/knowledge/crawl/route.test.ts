import { NextRequest } from "next/server";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const startKnowledgeWebsiteCrawl = vi.fn();

vi.mock("@/lib/knowledge/knowledge-workspace", () => ({
  startKnowledgeWebsiteCrawl,
}));

describe("POST /api/v1/knowledge/crawl", () => {
  beforeEach(() => {
    vi.resetAllMocks();
  });

  afterEach(() => {
    vi.resetAllMocks();
  });

  it("validates the website URL before starting a crawl", async () => {
    const { POST } = await import("@/app/api/v1/knowledge/crawl/route");

    const request = new NextRequest("http://localhost/api/v1/knowledge/crawl", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ url: "not-a-url" }),
    });

    const response = await POST(request);
    const payload = await response.json();

    expect(response.status).toBe(400);
    expect(payload.error).toEqual({
      code: "invalid_crawl_request",
      message: "A valid website URL is required.",
    });
    expect(startKnowledgeWebsiteCrawl).not.toHaveBeenCalled();
  });

  it("returns the started crawl payload on success", async () => {
    startKnowledgeWebsiteCrawl.mockResolvedValue({
      id: "crawl-1",
      status: "queued",
      target: "https://docs.velion.ai",
      createdAt: "2026-06-06T12:05:00.000Z",
    });

    const { POST } = await import("@/app/api/v1/knowledge/crawl/route");

    const request = new NextRequest("http://localhost/api/v1/knowledge/crawl", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ url: "https://docs.velion.ai", maxPages: 16 }),
    });

    const response = await POST(request);
    const payload = await response.json();

    expect(response.status).toBe(202);
    expect(payload.data).toEqual({
      id: "crawl-1",
      status: "queued",
      target: "https://docs.velion.ai",
      createdAt: "2026-06-06T12:05:00.000Z",
    });
    expect(startKnowledgeWebsiteCrawl).toHaveBeenCalledWith(
      expect.any(NextRequest),
      { url: "https://docs.velion.ai", maxPages: 16 },
    );
  });
});
