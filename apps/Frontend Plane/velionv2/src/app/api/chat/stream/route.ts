/**
 * BFF SSE route — POST /api/chat/stream
 *
 * Authenticates the caller, optionally runs a Quarry web-scrape for
 * grounding, then opens an SSE stream to Model Plane /v1/invoke/stream
 * and re-streams the deltas to the browser verbatim.
 *
 * Ported from velion/src/app/api/chat/stream/route.ts (Wave 11 / v1 ref).
 * Deliberately minimal: no Convex persistence, no tool-loop — plain chat
 * streaming only. Deferred: resume, deep-research, agent tools.
 */

import { type NextRequest, NextResponse } from "next/server";
import {
  getModelPlaneAudience,
  getQuarryAudience,
  mintAudienceToken,
} from "@/app/api/onboarding/_lib/onboarding-proxy";
import { requireRequestActor } from "@/lib/integrations/request-actor";
import { RequestActorError } from "@/lib/integrations/request-actor";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

// ──────────────────────────────────────────────────────────────────────────────
// Config helpers
// ──────────────────────────────────────────────────────────────────────────────

function modelPlaneStreamUrl(): string {
  const base =
    process.env.MODEL_PLANE_URL ||
    process.env.MODEL_PLANE_AI_URL ||
    "http://model-gateway:8080";
  return `${base}/v1/invoke/stream`;
}

function quarryEdgeUrl(): string {
  return process.env.QUARRY_EDGE_URL || "http://quarry-edge:8082";
}

function quarryRuntimeToken(): string | undefined {
  return process.env.QUARRY_RUNTIME_AUTH_TOKEN;
}

// ──────────────────────────────────────────────────────────────────────────────
// Request schema (permissive — validated below)
// ──────────────────────────────────────────────────────────────────────────────

type ChatStreamRequest = {
  content: string;
  model?: string;
  sessionId?: string;
  browseWeb?: boolean;
  tools?: string[];
  url?: string; // explicit URL to scrape
  // Opt-in rich SSE event families (chat-parity §2). Forwarded to Model Plane
  // so the gateway emits the matching events; the BFF re-streams them verbatim.
  features?: string[];
  // Optional client idempotency key (chat-parity §1). Dedupes a concurrent
  // duplicate stream (double-click / retry) and enables cached-answer replay.
  idempotencyKey?: string;
  // Multimodal attachments (chat-parity §2). An image attachment routes the
  // turn through inference-core AnalyzeImage (vision). Forwarded verbatim.
  attachments?: Array<{
    kind?: string;
    url?: string;
    data_base64?: string;
    mime_type?: string;
  }>;
};

// ──────────────────────────────────────────────────────────────────────────────
// SSE encoding helpers (ported from v1 reasoning.ts)
// ──────────────────────────────────────────────────────────────────────────────

function encodeSse(event: string, data: unknown): Uint8Array {
  const enc = new TextEncoder();
  return enc.encode(`event: ${event}\ndata: ${JSON.stringify(data)}\n\n`);
}

function normalizeOptionalString(value: unknown, maxLength: number): string | undefined {
  if (typeof value !== "string") {
    return undefined;
  }

  const trimmed = value.trim();
  if (!trimmed || trimmed.length > maxLength) {
    return undefined;
  }

  return trimmed;
}

// Nudge the model to emit GitHub-flavoured Markdown tables for tabular data so
// the chat renderer (ChatMarkdown) can display them. Mirrors the existing
// `[Web context]` content-framing convention. Scoped to velionv2 chat only —
// no shared Model Plane prompt change required.
const RESPONSE_FORMAT_DIRECTIVE = [
  "[Response formatting]",
  "When the answer contains comparisons, specifications, metrics, schedules, or other tabular data, present it as a GitHub-flavoured Markdown table: a header row, a |---|---| separator row, then one row per record. Keep tables compact and do not wrap them in code fences.",
  "Use normal Markdown (headings, bold, lists, code) for everything else.",
].join("\n");

// ──────────────────────────────────────────────────────────────────────────────
// Quarry web-scrape (best-effort, degrades gracefully)
// ──────────────────────────────────────────────────────────────────────────────

async function scrapeUrl(
  request: NextRequest,
  url: string,
): Promise<string | null> {
  try {
    const token =
      quarryRuntimeToken() ?? (await mintAudienceToken(request, getQuarryAudience()));
    const headers: Record<string, string> = {
      "Content-Type": "application/json",
    };
    if (token) headers["Authorization"] = `Bearer ${token}`;

    const res = await fetch(`${quarryEdgeUrl()}/v1/scrape`, {
      method: "POST",
      headers,
      body: JSON.stringify({ url, maxPages: 1, formats: ["markdown"] }),
      signal: AbortSignal.timeout(12_000),
    });
    if (!res.ok) return null;
    const body = (await res.json()) as {
      data?: { markdown?: string };
    };
    return body?.data?.markdown ?? null;
  } catch {
    return null;
  }
}

// ──────────────────────────────────────────────────────────────────────────────
// Main handler
// ──────────────────────────────────────────────────────────────────────────────

export async function POST(request: NextRequest): Promise<Response> {
  // Latency tracking — time each phase (auth → mint → upstream-connect → TTFT
  // → done) so a slow chat shows WHERE the time goes. Logged server-side and
  // returned to the client in the `done` event's `timing` field. A large gap
  // between `upstreamConnectMs` and `ttftMs` means the Model Plane stream is
  // waiting on generation (e.g. the non-streaming Infer fallback) — not the BFF.
  const t0 = Date.now();
  const sinceStart = () => Date.now() - t0;
  let authMs = 0;
  let mintMs = 0;
  let scrapeMs = 0;
  let upstreamConnectMs = 0;

  // 1. Auth
  let actor: Awaited<ReturnType<typeof requireRequestActor>>;
  try {
    actor = await requireRequestActor();
  } catch (err) {
    if (err instanceof RequestActorError) {
      return NextResponse.json({ error: "Unauthenticated" }, { status: 401 });
    }
    return NextResponse.json({ error: "Auth error" }, { status: 500 });
  }
  authMs = sinceStart();

  // 2. Parse body
  let parsed: ChatStreamRequest;
  try {
    parsed = (await request.json()) as ChatStreamRequest;
  } catch {
    return NextResponse.json({ error: "Invalid JSON" }, { status: 400 });
  }

  const { content, browseWeb, url: explicitUrl } = parsed;
  const model = normalizeOptionalString(parsed.model, 120);
  const sessionId = normalizeOptionalString(parsed.sessionId, 160);
  // Opt-in rich SSE event families (chat-parity §2). Forwarded to Model Plane;
  // the gateway gates rich events on this, the BFF re-streams them verbatim.
  const features = Array.isArray(parsed.features)
    ? parsed.features.filter((f): f is string => typeof f === "string").slice(0, 16)
    : [];
  const wantsCitations = features.includes("citations");
  const idempotencyKey = normalizeOptionalString(parsed.idempotencyKey, 160);
  // Multimodal attachments (chat-parity §2): forward up to 8, image kinds only
  // matter to the gateway's vision branch; others are ignored downstream.
  const attachments = Array.isArray(parsed.attachments)
    ? parsed.attachments.filter((a) => a && typeof a === "object").slice(0, 8)
    : [];

  if (!content || typeof content !== "string" || !content.trim()) {
    return NextResponse.json({ error: "content is required" }, { status: 400 });
  }
  if (content.length > 102_400) {
    return NextResponse.json({ error: "content exceeds maximum length" }, { status: 413 });
  }

  // 3. Optional Quarry scrape (browse_web toggle or explicit URL)
  let groundingMarkdown: string | null = null;
  if ((browseWeb || explicitUrl) && (explicitUrl || browseWeb)) {
    const scrapeTarget = explicitUrl ?? undefined;
    if (scrapeTarget) {
      const scrapeStart = Date.now();
      groundingMarkdown = await scrapeUrl(request, scrapeTarget);
      scrapeMs = Date.now() - scrapeStart;
    }
  }

  const finalContent = [
    RESPONSE_FORMAT_DIRECTIVE,
    groundingMarkdown ? `[Web context]\n${groundingMarkdown}` : null,
    `[User message]\n${content}`,
  ]
    .filter(Boolean)
    .join("\n\n");

  // 4. Mint Model Plane JWT
  const mintStart = Date.now();
  const bearerToken = await mintAudienceToken(request, getModelPlaneAudience());
  mintMs = Date.now() - mintStart;

  if (!bearerToken) {
    // Dev: fall back to env bypass token
    const devBypass =
      process.env.MODEL_GATEWAY_BEARER ??
      process.env.INTERNAL_API_KEY ??
      process.env.INTERNAL_SERVICE_SECRET;
    if (!devBypass) {
      return NextResponse.json(
        { error: "Unable to mint model-plane token" },
        { status: 502 },
      );
    }
  }

  const authHeader = bearerToken
    ? `Bearer ${bearerToken}`
    : `Bearer ${process.env.MODEL_GATEWAY_BEARER ?? process.env.INTERNAL_API_KEY ?? process.env.INTERNAL_SERVICE_SECRET ?? "dev-bypass"}`;

  // 5. Open upstream SSE connection
  let upstream: Response;
  try {
    upstream = await fetch(modelPlaneStreamUrl(), {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        Authorization: authHeader,
        Accept: "text/event-stream",
        // Forward correlation headers for tracing
        "x-user-id": actor.userId ?? "",
      },
      body: JSON.stringify({
        content: finalContent,
        model,
        session_key: sessionId,
        thread_id: sessionId,
        profile: "chat",
        features,
        ...(idempotencyKey ? { idempotency_key: idempotencyKey } : {}),
        ...(attachments.length > 0 ? { attachments } : {}),
      }),
      // Don't use the request's signal — keep the Model Plane call alive even
      // if the browser tab closes (ported from v1 rationale).
    });
  } catch {
    return NextResponse.json(
      { error: "Failed to connect to Model Plane" },
      { status: 502 },
    );
  }

  if (!upstream.ok || !upstream.body) {
    return NextResponse.json(
      { error: `Model Plane error ${upstream.status}` },
      { status: 502 },
    );
  }

  // Time-to-open the Model Plane stream (auth + mint + scrape + connect).
  // Everything after this is generation latency, not BFF overhead.
  upstreamConnectMs = sinceStart();

  // 6. Pipe upstream SSE → browser SSE via ReadableStream
  const upstreamReader = upstream.body.getReader();
  const decoder = new TextDecoder();

  const stream = new ReadableStream<Uint8Array>({
    async start(controller) {
      let buffer = "";
      let firstDeltaMs = 0;

      // Emit an initial "connected" event so the client knows the stream is live
      try {
        controller.enqueue(encodeSse("connected", { ok: true }));
        // chat-parity §7: surface the Quarry-scraped source as a citation when
        // the client opted into "citations". Reuses the grounding scrape — no
        // extra fetch.
        if (wantsCitations && groundingMarkdown && explicitUrl) {
          controller.enqueue(
            encodeSse("citation", {
              id: "src-1",
              title: explicitUrl,
              url: explicitUrl,
              snippet: groundingMarkdown.slice(0, 240),
            }),
          );
        }
      } catch {
        return;
      }

      try {
        while (true) {
          let value: Uint8Array | undefined;
          let done: boolean;
          try {
            ({ value, done } = await upstreamReader.read());
          } catch {
            // upstream read error
            break;
          }

          if (done) break;
          if (value) buffer += decoder.decode(value, { stream: true });

          // Parse complete SSE events (delimited by blank lines)
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

            let chunk: {
              request_id?: string;
              delta?: string;
              done?: boolean;
              model_used?: string;
              input_tokens?: number;
              output_tokens?: number;
              error?: string;
              message?: string;
            };
            try {
              chunk = JSON.parse(dataLines.join("\n")) as typeof chunk;
            } catch {
              continue;
            }

            if (eventName === "error") {
              try {
                controller.enqueue(
                  encodeSse("error", {
                    message: chunk.message ?? chunk.error ?? "Model Plane stream failed.",
                    requestId: chunk.request_id,
                  }),
                );
              } catch {
                // consumer gone
              }
              controller.close();
              return;
            }

            const isDone = eventName === "done" || chunk.done === true;

            if (isDone) {
              const timing = {
                authMs,
                mintMs,
                scrapeMs,
                upstreamConnectMs,
                ttftMs: firstDeltaMs,
                totalMs: sinceStart(),
              };
              // `upstreamConnectMs` ≈ BFF overhead; `ttftMs - upstreamConnectMs`
              // ≈ Model Plane time-to-first-token; if ttftMs ≈ totalMs the
              // gateway didn't stream (Infer fallback) — generation, not BFF.
              console.info("[chat-stream timing]", {
                sessionId,
                modelUsed: chunk.model_used ?? "",
                ...timing,
              });
              try {
                controller.enqueue(
                  encodeSse("done", {
                    done: true,
                    requestId: chunk.request_id,
                    modelUsed: chunk.model_used ?? "",
                    inputTokens: chunk.input_tokens ?? 0,
                    outputTokens: chunk.output_tokens ?? 0,
                    timing,
                  }),
                );
              } catch {
                // consumer gone
              }
              controller.close();
              return;
            }

            if (eventName === "chunk" && typeof chunk.delta === "string") {
              // Text answer delta → re-emit on the client's "message" channel.
              if (firstDeltaMs === 0) {
                firstDeltaMs = sinceStart();
              }
              try {
                controller.enqueue(
                  encodeSse("message", {
                    delta: chunk.delta,
                    requestId: chunk.request_id,
                  }),
                );
              } catch {
                // consumer gone — stop writing
                upstreamReader.cancel().catch(() => undefined);
                return;
              }
            } else if (eventName !== "connected" && eventName !== "message") {
              // chat-parity §2: forward rich/opt-in events VERBATIM under their
              // own name (usage, reasoning_delta, citation, tool_call,
              // tool_result, artifact, attachment, step_update, stopped). The
              // gateway only emits these when the client opted in via features[];
              // forwarding by name means new event types need NO BFF change.
              try {
                controller.enqueue(encodeSse(eventName, chunk));
              } catch {
                upstreamReader.cancel().catch(() => undefined);
                return;
              }
            }
          }
        }
      } finally {
        upstreamReader.releaseLock();
        try {
          controller.close();
        } catch {
          // already closed
        }
      }
    },
  });

  return new Response(stream, {
    headers: {
      "Content-Type": "text/event-stream",
      "Cache-Control": "no-cache, no-transform",
      Connection: "keep-alive",
      "X-Accel-Buffering": "no",
    },
  });
}
