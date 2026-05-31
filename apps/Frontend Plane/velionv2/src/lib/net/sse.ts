/**
 * Minimal `fetch`-based Server-Sent-Events reader.
 *
 * We use `fetch` + manual stream parsing instead of the native `EventSource`
 * because EventSource cannot send credentials/headers on cross-fetch and only
 * supports GET. This reader supports POST bodies, cookie/credential forwarding,
 * custom headers, and abort — which the onboarding crawl-preview and
 * single-page scrape streams all require.
 *
 * Frame layout (per the SSE spec):
 *   event: <name>\n
 *   data: <json>\n
 *   \n            <- blank line terminates the frame
 */

export interface SseEvent {
  /** The `event:` name, or `"message"` when none was supplied. */
  event: string;
  /** Raw concatenated `data:` payload. */
  data: string;
}

export interface ConsumeSseOptions {
  method?: "GET" | "POST";
  body?: BodyInit | null;
  headers?: Record<string, string>;
  signal?: AbortSignal;
  /** Called for every parsed frame. */
  onEvent: (event: SseEvent) => void;
}

/**
 * Open `url`, parse the SSE stream, and invoke `onEvent` per frame until the
 * stream ends or `signal` aborts. Resolves when the stream closes; rejects on
 * network/HTTP error (callers typically catch and degrade).
 */
export async function consumeSse(url: string, options: ConsumeSseOptions): Promise<void> {
  const response = await fetch(url, {
    method: options.method ?? "POST",
    headers: {
      Accept: "text/event-stream",
      ...(options.body ? { "Content-Type": "application/json" } : {}),
      ...options.headers,
    },
    body: options.body ?? undefined,
    credentials: "include",
    cache: "no-store",
    signal: options.signal,
  });

  if (!response.ok || !response.body) {
    throw new Error(`SSE request failed (${response.status})`);
  }

  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  let buffer = "";

  try {
    for (;;) {
      const { value, done } = await reader.read();
      if (done) break;
      buffer += decoder.decode(value, { stream: true });

      let separator = buffer.indexOf("\n\n");
      while (separator !== -1) {
        const rawFrame = buffer.slice(0, separator);
        buffer = buffer.slice(separator + 2);
        separator = buffer.indexOf("\n\n");
        const parsed = parseFrame(rawFrame);
        if (parsed) options.onEvent(parsed);
      }
    }
  } finally {
    reader.releaseLock?.();
  }
}

/** Parse a single raw SSE frame into `{ event, data }`, or null if dataless. */
export function parseFrame(rawFrame: string): SseEvent | null {
  let event = "message";
  let data = "";
  for (const line of rawFrame.split("\n")) {
    if (line.startsWith("event:")) {
      event = line.slice(6).trim();
    } else if (line.startsWith("data:")) {
      data += line.slice(5).replace(/^\s/, "");
    }
  }
  return data ? { event, data } : null;
}

/** Convenience: parse the `data` payload of an event as JSON, or null. */
export function parseEventJson<T>(event: SseEvent): T | null {
  try {
    return JSON.parse(event.data) as T;
  } catch {
    return null;
  }
}
