import { beforeEach, describe, expect, it, vi } from "vitest";

const STORAGE_KEY = "velion:v2:chat:sessions";
const streamChatMock = vi.hoisted(() => vi.fn());
const loadThreadHistoryMock = vi.hoisted(() => vi.fn());
const cancelChatMock = vi.hoisted(() => vi.fn());

vi.mock("./chat-stream", () => ({
  streamChat: streamChatMock,
  loadThreadHistory: loadThreadHistoryMock,
  cancelChat: cancelChatMock,
}));

describe("launchChatSessionFromComposer", () => {
  beforeEach(() => {
    localStorage.clear();
    sessionStorage.clear();
    streamChatMock.mockReset();
    loadThreadHistoryMock.mockReset();
    cancelChatMock.mockReset();
    vi.resetModules();
  });

  it("persists a dashboard composer session and streams the assistant response", async () => {
    streamChatMock.mockImplementation(async function* () {
      yield { type: "connected" };
      yield { type: "delta", delta: "Model Plane " };
      yield { type: "delta", delta: "response.", requestId: "req-1" };
      yield {
        type: "done",
        inputTokens: 12,
        modelUsed: "gpt-4o-mini",
        outputTokens: 8,
      };
    });

    const { launchChatSessionFromComposer } = await import("./chat-workspace");

    const session = launchChatSessionFromComposer({
      text: "Help me compare these support tickets",
      model: "gpt-4o-mini",
      tools: ["search"],
      attachments: [
        {
          id: "file-1",
          name: "tickets.csv",
          size: 1200,
          type: "text/csv",
        },
      ],
    });

    const stored = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "[]");

    expect(stored).toHaveLength(1);
    expect(stored[0]).toMatchObject({
      id: session.id,
      title: "Help me compare these support tickets",
      preview: "Help me compare these support tickets",
      branchCount: 0,
    });
    expect(stored[0].messages).toHaveLength(2);
    expect(stored[0].messages[0]).toMatchObject({
      role: "user",
      content: "Help me compare these support tickets",
      model: "gpt-4o-mini",
      tools: ["search"],
      attachments: [
        {
          id: "file-1",
          name: "tickets.csv",
          size: 1200,
          type: "text/csv",
        },
      ],
    });
    await vi.waitFor(() => {
      expect(streamChatMock).toHaveBeenCalledTimes(1);
      expect(streamChatMock.mock.calls[0]?.[0]).toEqual(expect.objectContaining({
        browseWeb: true,
        model: "gpt-4o-mini",
        sessionId: session.id,
        tools: ["search"],
        features: ["usage", "citations", "reasoning", "steps", "tools", "artifacts"],
      }));
      expect(String(streamChatMock.mock.calls[0]?.[0]?.content)).toContain("Help me compare these support tickets");
    });

    await vi.waitFor(() => {
      const nextStored = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "[]");
      expect(nextStored[0].messages[1]).toMatchObject({
        content: "Model Plane response.",
        inputTokens: 12,
        modelUsed: "gpt-4o-mini",
        outputTokens: 8,
        requestId: "req-1",
        role: "assistant",
        tools: ["search"],
      });
      expect(nextStored[0].messages[1].status).toBeUndefined();
      expect(nextStored[0].taskSteps.map((step: { title: string; status: string }) => [
        step.title,
        step.status,
      ])).toEqual([
        ["Prompt received", "done"],
        ["Context route prepared", "done"],
        ["Model gateway", "done"],
      ]);
    });
  });

  it("hydrates an empty session from server history (cross-device resume)", async () => {
    loadThreadHistoryMock.mockResolvedValue([
      { role: "user", content: "earlier question" },
      { role: "system", content: "ignored system row" },
      { role: "assistant", content: "earlier answer" },
    ]);

    const { hydrateSessionFromServer } = await import("./chat-workspace");
    await hydrateSessionFromServer("thread-xyz");

    expect(loadThreadHistoryMock).toHaveBeenCalledWith("thread-xyz");
    const stored = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "[]");
    expect(stored).toHaveLength(1);
    expect(stored[0].id).toBe("thread-xyz");
    // system/tool rows dropped; only user + assistant turns rendered, in order.
    expect(stored[0].messages.map((m: { role: string; content: string }) => [m.role, m.content])).toEqual([
      ["user", "earlier question"],
      ["assistant", "earlier answer"],
    ]);
  });

  it("does not clobber a session that already has local messages", async () => {
    streamChatMock.mockImplementation(async function* () {
      yield { type: "connected" };
      yield { type: "delta", delta: "local answer", requestId: "req-1" };
      yield { type: "done", inputTokens: 1, modelUsed: "m", outputTokens: 1 };
    });
    loadThreadHistoryMock.mockResolvedValue([{ role: "user", content: "server-only" }]);

    const { launchChatSessionFromComposer, hydrateSessionFromServer } = await import("./chat-workspace");
    const session = launchChatSessionFromComposer({ text: "local question", tools: [], attachments: [] });

    await hydrateSessionFromServer(session.id);

    // The server history must NOT have been applied — local turns win.
    expect(loadThreadHistoryMock).not.toHaveBeenCalled();
    const stored = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "[]");
    expect(stored[0].messages[0]).toMatchObject({ role: "user", content: "local question" });
  });

  it("consumes dashboard launch motion only for the matching fresh session", async () => {
    const { consumeChatLaunchMotion, writeChatLaunchMotion } = await import("./chat-workspace");

    writeChatLaunchMotion("session-1", "Hello");

    expect(consumeChatLaunchMotion("session-2")).toBe(false);

    writeChatLaunchMotion("session-1", "Hello");

    expect(consumeChatLaunchMotion("session-1")).toBe(true);
    expect(consumeChatLaunchMotion("session-1")).toBe(false);
  });

  it("marks the assistant response as failed when Model Plane returns an error event", async () => {
    streamChatMock.mockImplementation(async function* () {
      yield { type: "connected" };
      yield { type: "error", message: "upstream unavailable" };
    });

    const { launchChatSessionFromComposer } = await import("./chat-workspace");

    launchChatSessionFromComposer({
      text: "hello",
      tools: [],
      attachments: [],
    });

    await vi.waitFor(() => {
      const stored = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "[]");
      expect(stored[0].messages[1]).toMatchObject({
        content: "upstream unavailable",
        role: "assistant",
        status: "error",
      });
      expect(stored[0].taskSteps.at(-1)).toMatchObject({
        detail: "upstream unavailable",
        status: "error",
        title: "Model gateway",
      });
    });
  });

  it("stores internal grounding evidence on the assistant message", async () => {
    streamChatMock.mockImplementation(async function* () {
      yield { type: "connected" };
      yield {
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
      };
      yield { type: "delta", delta: "Policy answer", requestId: "req-2" };
      yield { type: "done", inputTokens: 4, modelUsed: "gpt-4o-mini", outputTokens: 6 };
    });

    const { launchChatSessionFromComposer } = await import("./chat-workspace");

    launchChatSessionFromComposer({
      text: "What is the refund policy?",
      tools: [],
      attachments: [],
    });

    await vi.waitFor(() => {
      const stored = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "[]");
      expect(stored[0].messages[1].grounding).toMatchObject({
        traceId: "trace-1",
        sourceCount: 1,
        factCount: 2,
        sources: [
          expect.objectContaining({
            kind: "knowledge",
            title: "Refund policy",
          }),
        ],
        graph: expect.objectContaining({
          edgeCount: 2,
        }),
      });
    });
  });
});
