import { afterEach, describe, expect, it, vi } from "vitest";
import { loadModels, loadThreadHistory, streamChat, uploadDocument } from "./chat-stream";

describe("streamChat", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("posts chat context to the BFF stream route and yields SSE chunks", async () => {
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response([
        "event: connected",
        "data: {\"ok\":true}",
        "",
        "event: message",
        "data: {\"delta\":\"Hello\",\"requestId\":\"req-1\"}",
        "",
        "event: done",
        "data: {\"done\":true,\"modelUsed\":\"gpt-4o-mini\",\"inputTokens\":3,\"outputTokens\":2}",
        "",
        "",
      ].join("\n")),
    );

    const chunks = [];
    for await (const chunk of streamChat({
      browseWeb: true,
      content: "Hello",
      model: "gpt-4o-mini",
      sessionId: "session-1",
      tools: ["search"],
    })) {
      chunks.push(chunk);
    }

    const [, init] = fetchMock.mock.calls[0];
    expect(init).toMatchObject({
      credentials: "include",
      method: "POST",
    });
    expect(JSON.parse(String(init?.body))).toEqual({
      browseWeb: true,
      content: "Hello",
      model: "gpt-4o-mini",
      sessionId: "session-1",
      tools: ["search"],
    });
    expect(chunks).toEqual([
      { type: "connected" },
      { type: "delta", delta: "Hello", requestId: "req-1" },
      {
        type: "done",
        inputTokens: 3,
        modelUsed: "gpt-4o-mini",
        outputTokens: 2,
      },
    ]);
  });

  it("yields an error chunk for upstream Model Plane error events", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response([
        "event: error",
        "data: {\"message\":\"gateway unavailable\"}",
        "",
        "",
      ].join("\n")),
    );

    const chunks = [];
    for await (const chunk of streamChat({ content: "Hello" })) {
      chunks.push(chunk);
    }

    expect(chunks).toEqual([
      { type: "error", message: "gateway unavailable" },
    ]);
  });

  it("forwards opt-in features in the request body", async () => {
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response(["event: done", 'data: {"done":true}', "", ""].join("\n")),
    );
    for await (const _chunk of streamChat({ content: "Hi", features: ["usage", "citations"] })) {
      void _chunk;
    }
    const [, init] = fetchMock.mock.calls[0];
    expect(JSON.parse(String(init?.body)).features).toEqual(["usage", "citations"]);
  });

  it("parses rich events by NAME — reasoning_delta is never mistaken for answer text", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response([
        "event: reasoning_delta",
        'data: {"delta":"thinking...","request_id":"r1"}',
        "",
        "event: citation",
        'data: {"id":"src-1","title":"Doc","url":"https://x","snippet":"hello"}',
        "",
        "event: message",
        'data: {"delta":"Answer","requestId":"r1"}',
        "",
        "event: usage",
        'data: {"input_tokens":12,"output_tokens":34,"cost_usd":0.002,"latency_ms":880,"confidence":0.77}',
        "",
        "event: done",
        'data: {"done":true,"modelUsed":"gpt-4o","inputTokens":12,"outputTokens":34}',
        "",
        "",
      ].join("\n")),
    );

    const chunks = [];
    for await (const chunk of streamChat({
      content: "Hi",
      features: ["reasoning", "citations", "usage"],
    })) {
      chunks.push(chunk);
    }

    expect(chunks).toEqual([
      { type: "reasoning_delta", delta: "thinking..." },
      { type: "citation", id: "src-1", title: "Doc", url: "https://x", snippet: "hello" },
      { type: "delta", delta: "Answer", requestId: "r1" },
      {
        type: "usage",
        inputTokens: 12,
        outputTokens: 34,
        costUsd: 0.002,
        latencyMs: 880,
        confidence: 0.77,
      },
      { type: "done", inputTokens: 12, modelUsed: "gpt-4o", outputTokens: 34 },
    ]);
  });

  it("parses internal grounding events separately from citations and text deltas", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response([
        "event: grounding",
        'data: {"mode":"retrieve","query":"refund policy","traceId":"trace-1","lowConfidence":false,"factCount":2,"sourceCount":1,"facts":[{"knowledgeId":"kid-1","documentId":"doc-1","text":"Refunds are accepted within 30 days.","score":0.93,"sourceTitle":"Refund policy","sourceType":"policy","provider":"Notion","chunkIndex":0}],"sources":[{"id":"doc-1","kind":"knowledge","title":"Refund policy","snippet":"Refunds are accepted within 30 days.","provider":"Notion","sourceType":"policy","documentId":"doc-1","href":"/knowledge","score":0.93}],"graph":{"traceId":"graph-1","communitySummaries":["Refund policy connects with return workflow."],"edgeCount":2,"nodes":[{"id":"node-1","label":"Refund policy","kind":"policy"}]}}',
        "",
        "event: message",
        'data: {"delta":"Answer","requestId":"r1"}',
        "",
        "event: done",
        'data: {"done":true,"modelUsed":"gpt-4o","inputTokens":12,"outputTokens":34}',
        "",
        "",
      ].join("\n")),
    );

    const chunks = [];
    for await (const chunk of streamChat({ content: "Hi", features: ["citations"] })) {
      chunks.push(chunk);
    }

    expect(chunks).toEqual([
      {
        type: "grounding",
        grounding: {
          mode: "retrieve",
          query: "refund policy",
          traceId: "trace-1",
          lowConfidence: false,
          factCount: 2,
          sourceCount: 1,
          facts: [
            {
              knowledgeId: "kid-1",
              documentId: "doc-1",
              text: "Refunds are accepted within 30 days.",
              score: 0.93,
              sourceTitle: "Refund policy",
              sourceType: "policy",
              provider: "Notion",
              chunkIndex: 0,
            },
          ],
          sources: [
            {
              id: "doc-1",
              kind: "knowledge",
              title: "Refund policy",
              snippet: "Refunds are accepted within 30 days.",
              provider: "Notion",
              sourceType: "policy",
              documentId: "doc-1",
              href: "/knowledge",
              score: 0.93,
            },
          ],
          graph: {
            traceId: "graph-1",
            communitySummaries: ["Refund policy connects with return workflow."],
            edgeCount: 2,
            nodes: [{ id: "node-1", label: "Refund policy", kind: "policy" }],
          },
        },
      },
      { type: "delta", delta: "Answer", requestId: "r1" },
      { type: "done", inputTokens: 12, modelUsed: "gpt-4o", outputTokens: 34 },
    ]);
  });

  it("surfaces structured error code + retryable when present", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response([
        "event: error",
        'data: {"code":"model_plane_unavailable","message":"down","retryable":true}',
        "",
        "",
      ].join("\n")),
    );
    const chunks = [];
    for await (const chunk of streamChat({ content: "Hi" })) {
      chunks.push(chunk);
    }
    expect(chunks).toEqual([
      { type: "error", message: "down", code: "model_plane_unavailable", retryable: true },
    ]);
  });
});

describe("loadThreadHistory", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("GETs the BFF history route and returns parsed messages", async () => {
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response(
        JSON.stringify({
          threadId: "t-1",
          messages: [
            { role: "user", content: "hi" },
            { role: "assistant", content: "hello" },
          ],
        }),
        { status: 200 },
      ),
    );

    const messages = await loadThreadHistory("t-1");

    const [url, init] = fetchMock.mock.calls[0];
    expect(String(url)).toBe("/api/chat/history?threadId=t-1");
    expect(init).toMatchObject({ method: "GET", credentials: "include" });
    expect(messages).toEqual([
      { role: "user", content: "hi" },
      { role: "assistant", content: "hello" },
    ]);
  });

  it("returns [] without fetching for an empty threadId", async () => {
    const fetchMock = vi.spyOn(globalThis, "fetch");
    expect(await loadThreadHistory("")).toEqual([]);
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("returns [] on a non-ok response", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response("nope", { status: 404 }),
    );
    expect(await loadThreadHistory("t-2")).toEqual([]);
  });

  it("filters malformed messages from the payload", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response(
        JSON.stringify({
          messages: [
            { role: "user", content: "ok" },
            { role: 42, content: "bad-role" },
            { role: "assistant" },
            "garbage",
          ],
        }),
        { status: 200 },
      ),
    );
    expect(await loadThreadHistory("t-3")).toEqual([{ role: "user", content: "ok" }]);
  });
});

describe("loadModels", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("returns models with their feature families", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response(
        JSON.stringify({
          models: [
            { id: "gpt-4o", provider: "openai", modality: "chat", streaming: true, features: ["usage", "vision", "tools"] },
            { id: "text-embedding-3", provider: "openai", modality: "embedding", streaming: false, features: [] },
          ],
        }),
        { status: 200 },
      ),
    );
    const models = await loadModels();
    expect(models).toHaveLength(2);
    expect(models[0]).toMatchObject({ id: "gpt-4o", features: ["usage", "vision", "tools"] });
  });

  it("returns [] on non-ok response and filters malformed rows", async () => {
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValueOnce(
      new Response("nope", { status: 502 }),
    );
    expect(await loadModels()).toEqual([]);

    fetchMock.mockResolvedValueOnce(
      new Response(
        JSON.stringify({ models: [{ id: "ok", features: [] }, { id: 1, features: [] }, { id: "no-features" }] }),
        { status: 200 },
      ),
    );
    expect(await loadModels()).toEqual([{ id: "ok", features: [] }]);
  });
});

describe("uploadDocument", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("POSTs the document and returns its id + status", async () => {
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response(JSON.stringify({ documentId: "doc-9", status: "indexed" }), { status: 200 }),
    );
    const out = await uploadDocument({ title: "Notes", content: "hello world" });
    const [url, init] = fetchMock.mock.calls[0];
    expect(String(url)).toBe("/api/chat/documents");
    expect(init).toMatchObject({ method: "POST", credentials: "include" });
    expect(out).toEqual({ documentId: "doc-9", status: "indexed" });
  });

  it("returns null for empty content without fetching, and on failure", async () => {
    const fetchMock = vi.spyOn(globalThis, "fetch");
    expect(await uploadDocument({ title: "x", content: "   " })).toBeNull();
    expect(fetchMock).not.toHaveBeenCalled();

    fetchMock.mockResolvedValueOnce(new Response("err", { status: 502 }));
    expect(await uploadDocument({ title: "x", content: "real" })).toBeNull();
  });
});
