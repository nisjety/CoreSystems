/**
 * BFF SSE route — POST /api/chat/stream
 *
 * Authenticates the caller, optionally runs a Quarry web-scrape for
 * web context, injects built-in Quarry search tools when web search is enabled,
 * then opens an SSE stream to Model Plane /v1/invoke/stream and re-streams
 * the deltas to the browser verbatim.
 *
 * Ported from verevon/src/app/api/chat/stream/route.ts (Wave 11 / v1 ref).
 * Deliberately minimal: no Convex persistence. Model Plane owns the tool-loop;
 * this BFF only chooses which tools to expose per request.
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
  // Explicit image-generation intent (chat-parity §2). Routes to GenerateImage
  // and emits an `artifact` event.
  generateImage?: boolean;
  // Function-calling tool definitions (chat-parity §2). Distinct from `tools`
  // (browse toggle names). Each is a name + description + JSON-schema string.
  toolDefs?: Array<{ name: string; description?: string; parameters_json?: string }>;
};

type ModelToolDefinition = {
  name: string;
  description: string;
  parameters_json: string;
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

function normalizeToolIds(value: unknown): string[] {
  if (!Array.isArray(value)) {
    return [];
  }

  return value
    .filter((tool): tool is string => typeof tool === "string")
    .map((tool) => tool.trim())
    .filter((tool) => tool.length > 0)
    .slice(0, 16);
}

// Nudge the model to emit GitHub-flavoured Markdown tables for tabular data so
// the chat renderer (ChatMarkdown) can display them. Mirrors the existing
// `[Web context]` content-framing convention. Scoped to verevonv2 chat only —
// no shared Model Plane prompt change required.
const RESPONSE_FORMAT_DIRECTIVE = [
  "[Response formatting]",
  "When the answer contains comparisons, specifications, metrics, schedules, or other tabular data, present it as a GitHub-flavoured Markdown table: a header row, a |---|---| separator row, then one row per record. Keep tables compact and do not wrap them in code fences.",
  "Use normal Markdown (headings, bold, lists, code) for everything else.",
].join("\n");

const WEB_TOOL_DIRECTIVE = [
  "[Web search tools]",
  "When current or external information matters, use `web_search` to discover relevant sources and `fetch_url` to inspect a promising page before relying on it.",
  "Prefer specific multi-term web queries that include the entity, the exact topic, and any useful timeframe instead of broad one-word navigational searches.",
  "Set `intent` on `web_search` when it helps: use `research` for broad multi-source discovery, `factual` for direct verification, and `navigational` when you are trying to find a specific site.",
  "Cite the strongest sources you actually used, and avoid claiming you verified something unless you searched or fetched it.",
].join("\n");

const BUILT_IN_WEB_TOOLS: ModelToolDefinition[] = [
  {
    name: "web_search",
    description:
      "Search the public web through Quarry v2. Use this first to discover relevant current sources before answering.",
    parameters_json: JSON.stringify({
      type: "object",
      properties: {
        query: {
          type: "string",
          description: "The web search query to run.",
        },
        limit: {
          type: "integer",
          minimum: 1,
          maximum: 10,
          description: "Maximum number of search results to return.",
        },
        intent: {
          type: "string",
          enum: ["factual", "research", "navigational"],
          description:
            "Optional Quarry router hint. Use research for broad discovery, factual for direct verification, navigational for finding a specific site.",
        },
      },
      required: ["query"],
      additionalProperties: false,
    }),
  },
  {
    name: "fetch_url",
    description:
      "Fetch and read a specific web page through Quarry v2 after a web_search call identifies a promising source.",
    parameters_json: JSON.stringify({
      type: "object",
      properties: {
        url: {
          type: "string",
          description: "The absolute URL to fetch and inspect.",
        },
      },
      required: ["url"],
      additionalProperties: false,
    }),
  },
];

function buildImplicitToolDefs({
  browseWeb,
  toolIds,
}: {
  browseWeb?: boolean;
  toolIds: string[];
}): ModelToolDefinition[] {
  const wantsWebTools =
    browseWeb === true ||
    toolIds.some((toolId) => toolId === "search" || toolId === "research");

  return wantsWebTools ? BUILT_IN_WEB_TOOLS : [];
}

function mergeToolDefs(
  implicit: ModelToolDefinition[],
  explicit: ModelToolDefinition[],
): ModelToolDefinition[] {
  if (implicit.length === 0 && explicit.length === 0) {
    return [];
  }

  const merged = new Map<string, ModelToolDefinition>();

  for (const tool of implicit) {
    merged.set(tool.name, tool);
  }

  for (const tool of explicit) {
    merged.set(tool.name, tool);
  }

  return Array.from(merged.values());
}

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
  const toolIds = normalizeToolIds(parsed.tools);
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
  const generateImage = parsed.generateImage === true;
  // Image generation emits an `artifact` event — ensure the family is enabled
  // so the gateway forwards it (the BFF re-streams artifacts verbatim).
  if (generateImage && !features.includes("artifacts")) {
    features.push("artifacts");
  }

  // Function-calling tool definitions (chat-parity §2). Distinct from the
  // `tools: string[]` browse toggle — these are name+description+schema specs
  // the model may call. Forward up to 16, name-required. Enabling them turns on
  // the `tools` family so the gateway runs the loop + streams tool events.
  const explicitToolDefs = Array.isArray(parsed.toolDefs)
    ? parsed.toolDefs
        .filter((t): t is { name: string; description?: string; parameters_json?: string } =>
          Boolean(t) && typeof t === "object" && typeof (t as { name?: unknown }).name === "string")
        .slice(0, 16)
        .map((t) => ({
          name: t.name,
          description: typeof t.description === "string" ? t.description : "",
          parameters_json: typeof t.parameters_json === "string" ? t.parameters_json : "",
        }))
    : [];
  const toolDefs = mergeToolDefs(
    buildImplicitToolDefs({ browseWeb, toolIds }),
    explicitToolDefs,
  );
  const hasWebTools = toolDefs.some(
    (tool) => tool.name === "web_search" || tool.name === "fetch_url",
  );
  if (toolDefs.length > 0 && !features.includes("tools")) {
    features.push("tools");
  }

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
    hasWebTools ? WEB_TOOL_DIRECTIVE : null,
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
        ...(generateImage ? { generate_image: true } : {}),
        ...(toolDefs.length > 0 ? { tools: toolDefs } : {}),
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
