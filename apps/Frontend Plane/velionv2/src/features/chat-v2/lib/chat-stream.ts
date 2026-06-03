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
  | { type: "done"; modelUsed: string; inputTokens: number; outputTokens: number; timing?: ChatTiming }
  | { type: "error"; message: string };

/**
 * Async-generator that yields SSE chunks from the BFF chat stream route.
 * Throws on HTTP errors so callers can catch and surface a UI error state.
 */
export async function* streamChat(
  opts: ChatStreamOptions,
): AsyncGenerator<ChatStreamChunk, void, void> {
  const { content, model, sessionId, browseWeb, tools, url, signal } = opts;

  const response = await fetch("/api/chat/stream", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    credentials: "include",
    body: JSON.stringify({ content, model, sessionId, browseWeb, tools, url }),
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

        if (typeof data["delta"] === "string") {
          yield {
            type: "delta",
            delta: data["delta"],
            requestId: typeof data["requestId"] === "string" ? data["requestId"] : undefined,
          };
        }
      }
    }
  } finally {
    reader.releaseLock();
  }
}

function readErrorMessage(data: Record<string, unknown>) {
  const message = data["message"] ?? data["error"];
  return typeof message === "string" && message.trim()
    ? message.trim()
    : "Model Plane stream failed.";
}
