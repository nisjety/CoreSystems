/**
 * chat-stream.ts — browser-side SSE client for /api/chat/stream
 *
 * Usage:
 *   for await (const chunk of streamChat({ content: "Hello" })) {
 *     if (chunk.type === "delta") appendText(chunk.delta);
 *     if (chunk.type === "done")  markFinished(chunk.modelUsed);
 *   }
 */

export type ChatStreamOptions = {
  content: string;
  model?: string;
  sessionId?: string;
  browseWeb?: boolean;
  tools?: string[];
  url?: string;
  /** Opt-in rich SSE event families (chat-parity §2), e.g. ["usage","citations"]. */
  features?: string[];
  signal?: AbortSignal;
};

export type ChatTiming = {
  authMs?: number;
  mintMs?: number;
  scrapeMs?: number;
  upstreamConnectMs?: number;
  ttftMs?: number;
  totalMs?: number;
};

export type ChatStreamChunk =
  | { type: "connected" }
  | { type: "delta"; delta: string; requestId?: string }
  | { type: "reasoning_delta"; delta: string }
  | { type: "citation"; id: string; title: string; url: string; snippet: string }
  | {
      type: "usage";
      inputTokens: number;
      outputTokens: number;
      costUsd?: number;
      latencyMs?: number;
      confidence?: number;
    }
  | { type: "done"; modelUsed: string; inputTokens: number; outputTokens: number; timing?: ChatTiming }
  | { type: "error"; message: string };

/**
 * Async-generator that yields SSE chunks from the BFF chat stream route.
 * Throws on HTTP errors so callers can catch and surface a UI error state.
 */
export async function* streamChat(
  opts: ChatStreamOptions,
): AsyncGenerator<ChatStreamChunk, void, void> {
  const { content, model, sessionId, browseWeb, tools, url, features, signal } = opts;

  const response = await fetch("/api/chat/stream", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    credentials: "include",
    body: JSON.stringify({ content, model, sessionId, browseWeb, tools, url, features }),
    signal,
  });

  if (!response.ok || !response.body) {
    const text = await response.text().catch(() => "");
    throw new Error(`stream ${response.status}: ${text.slice(0, 200)}`);
  }

  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  let buffer = "";

  try {
    while (true) {
      const { value, done } = await reader.read();
      if (done) break;
      if (value) buffer += decoder.decode(value, { stream: true });

      let sep = buffer.indexOf("\n\n");
      while (sep !== -1) {
        const rawEvent = buffer.slice(0, sep);
        buffer = buffer.slice(sep + 2);
        sep = buffer.indexOf("\n\n");

        let eventName = "message";
        const dataLines: string[] = [];
        for (const line of rawEvent.split("\n")) {
          if (line.startsWith("event:")) {
            eventName = line.slice(6).trim();
          } else if (line.startsWith("data:")) {
            dataLines.push(line.slice(5).trim());
          }
        }
        if (dataLines.length === 0) continue;

        let data: Record<string, unknown>;
        try {
          data = JSON.parse(dataLines.join("\n")) as Record<string, unknown>;
        } catch {
          continue;
        }

        if (eventName === "connected") {
          yield { type: "connected" };
          continue;
        }

        if (eventName === "error") {
          yield {
            type: "error",
            message: readErrorMessage(data),
          };
          return;
        }

        if (eventName === "done" || data["done"] === true) {
          yield {
            type: "done",
            modelUsed: typeof data["modelUsed"] === "string" ? data["modelUsed"] : "",
            inputTokens:
              typeof data["inputTokens"] === "number" ? data["inputTokens"] : 0,
            outputTokens:
              typeof data["outputTokens"] === "number" ? data["outputTokens"] : 0,
            timing:
              data["timing"] && typeof data["timing"] === "object"
                ? (data["timing"] as ChatTiming)
                : undefined,
          };
          return;
        }

        // Rich opt-in events (chat-parity §2), dispatched by event NAME so a
        // `reasoning_delta` (which also carries a `delta` field) is never
        // mistaken for answer text.
        if (eventName === "reasoning_delta") {
          if (typeof data["delta"] === "string") {
            yield { type: "reasoning_delta", delta: data["delta"] };
          }
          continue;
        }

        if (eventName === "citation") {
          yield {
            type: "citation",
            id: asString(data["id"]),
            title: asString(data["title"]),
            url: asString(data["url"]),
            snippet: asString(data["snippet"]),
          };
          continue;
        }

        if (eventName === "usage") {
          yield {
            type: "usage",
            inputTokens: asNumber(data["input_tokens"]),
            outputTokens: asNumber(data["output_tokens"]),
            costUsd: typeof data["cost_usd"] === "number" ? data["cost_usd"] : undefined,
            latencyMs: typeof data["latency_ms"] === "number" ? data["latency_ms"] : undefined,
            confidence: typeof data["confidence"] === "number" ? data["confidence"] : undefined,
          };
          continue;
        }

        // Text answer delta — ONLY the "message" channel.
        if (eventName === "message" && typeof data["delta"] === "string") {
          yield {
            type: "delta",
            delta: data["delta"],
            requestId: typeof data["requestId"] === "string" ? data["requestId"] : undefined,
          };
        }
        // Any other event family (tool_call, artifact, step_update, …) is
        // ignored gracefully until its consumer lands — forward-compatible.
      }
    }
  } finally {
    reader.releaseLock();
  }
}

function asString(value: unknown): string {
  return typeof value === "string" ? value : "";
}

function asNumber(value: unknown): number {
  return typeof value === "number" ? value : 0;
}

function readErrorMessage(data: Record<string, unknown>) {
  const message = data["message"] ?? data["error"];
  return typeof message === "string" && message.trim()
    ? message.trim()
    : "Model Plane stream failed.";
}
