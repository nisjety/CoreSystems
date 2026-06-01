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
  browseWeb?: boolean;
  url?: string; // explicit URL to scrape
};

// ──────────────────────────────────────────────────────────────────────────────
// SSE encoding helpers (ported from v1 reasoning.ts)
// ──────────────────────────────────────────────────────────────────────────────

function encodeSse(event: string, data: unknown): Uint8Array {
  const enc = new TextEncoder();
  return enc.encode(`event: ${event}\ndata: ${JSON.stringify(data)}\n\n`);
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

  // 2. Parse body
  let parsed: ChatStreamRequest;
  try {
    parsed = (await request.json()) as ChatStreamRequest;
  } catch {
    return NextResponse.json({ error: "Invalid JSON" }, { status: 400 });
  }

  const { content, model, browseWeb, url: explicitUrl } = parsed;

  if (!content || typeof content !== "string" || !content.trim()) {
    return NextResponse.json({ error: "content is required" }, { status: 400 });
  }

  // 3. Optional Quarry scrape (browse_web toggle or explicit URL)
  let groundingMarkdown: string | null = null;
  if ((browseWeb || explicitUrl) && (explicitUrl || browseWeb)) {
    const scrapeTarget = explicitUrl ?? undefined;
    if (scrapeTarget) {
      groundingMarkdown = await scrapeUrl(request, scrapeTarget);
    }
  }

  const finalContent = groundingMarkdown
    ? `[Web context]\n${groundingMarkdown}\n\n[User message]\n${content}`
    : content;

  // 4. Mint Model Plane JWT
  const bearerToken = await mintAudienceToken(request, getModelPlaneAudience());

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
        model: model ?? undefined,
        profile: "chat",
        browse_web: browseWeb ?? false,
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

  // 6. Pipe upstream SSE → browser SSE via ReadableStream
  const upstreamReader = upstream.body.getReader();
  const decoder = new TextDecoder();

  const stream = new ReadableStream<Uint8Array>({
    async start(controller) {
      let buffer = "";

      // Emit an initial "connected" event so the client knows the stream is live
      try {
        controller.enqueue(encodeSse("connected", { ok: true }));
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
            };
            try {
              chunk = JSON.parse(dataLines.join("\n")) as typeof chunk;
            } catch {
              continue;
            }

            const isDone = eventName === "done" || chunk.done === true;

            if (isDone) {
              try {
                controller.enqueue(
                  encodeSse("done", {
                    done: true,
                    modelUsed: chunk.model_used ?? "",
                    inputTokens: chunk.input_tokens ?? 0,
                    outputTokens: chunk.output_tokens ?? 0,
                  }),
                );
              } catch {
                // consumer gone
              }
              controller.close();
              return;
            }

            if (chunk.delta) {
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
