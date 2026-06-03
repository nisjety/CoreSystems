import { type NextRequest, NextResponse } from "next/server";
import { fail } from "@/lib/api/envelope";
import { RequestActorError, requireRequestActor } from "@/lib/integrations/request-actor";
import {
  getQuarryEdgeUrl,
  getQuarryAudience,
  mintAudienceToken,
} from "@/app/api/onboarding/_lib/onboarding-proxy";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

// BFF SSE proxy — POST /api/v1/search/answer/stream
//
// Authenticates the caller, mints a Quarry audience token, opens an SSE stream
// to quarry-edge `POST /v1/answer/stream`, and re-streams the frames to the
// browser verbatim. The edge runs the full high-quality pipeline (hybrid search
// → full-page fetch → grounded context) and streams synthesis token-by-token,
// emitting `event: citations` / `event: delta` / `event: done` (+ `error`).

function isSameOriginRequest(request: Request): boolean {
  const origin = request.headers.get("origin");
  const host = request.headers.get("host");
  if (!origin || !host) return true;
  try {
    return new URL(origin).host === host;
  } catch {
    return false;
  }
}

export async function POST(request: NextRequest) {
  if (!isSameOriginRequest(request)) {
    return NextResponse.json(
      fail({ code: "invalid_origin", message: "Request origin is not allowed." }),
      { status: 403 },
    );
  }

  try {
    await requireRequestActor();

    const body = (await request.json().catch(() => null)) as { query?: string } | null;
    const query = typeof body?.query === "string" ? body.query.trim() : "";
    if (!query) {
      return NextResponse.json(
        fail({ code: "invalid_query", message: "A non-empty query is required." }),
        { status: 400 },
      );
    }

    const token = await mintAudienceToken(request, getQuarryAudience());
    const headers: Record<string, string> = {
      "Content-Type": "application/json",
      Accept: "text/event-stream",
    };
    if (token) headers["Authorization"] = `Bearer ${token}`;

    // No request timeout — this is a long-lived SSE stream.
    const upstream = await fetch(`${getQuarryEdgeUrl()}/v1/answer/stream`, {
      method: "POST",
      headers,
      body: JSON.stringify({ query }),
    });

    if (!upstream.ok || !upstream.body) {
      const status = upstream.status;
      if (status === 501) {
        return NextResponse.json(
          fail({
            code: "answer_unconfigured",
            message: "AI answers are unavailable — no answer pipeline is configured.",
          }),
          { status: 501 },
        );
      }
      return NextResponse.json(
        fail({
          code: "answer_stream_unavailable",
          message: `Answer streaming could not be started (upstream ${status}).`,
        }),
        { status: 502 },
      );
    }

    // Re-stream the edge's SSE body to the browser unchanged.
    return new Response(upstream.body, {
      status: 200,
      headers: {
        "Content-Type": "text/event-stream; charset=utf-8",
        "Cache-Control": "no-cache, no-transform",
        Connection: "keep-alive",
        // Disable proxy buffering so deltas flush immediately.
        "X-Accel-Buffering": "no",
      },
    });
  } catch (error) {
    if (error instanceof RequestActorError) {
      return NextResponse.json(
        fail({ code: error.code, message: error.message }),
        { status: error.status },
      );
    }
    return NextResponse.json(
      fail({ code: "answer_stream_failed", message: "Answer streaming could not be started." }),
      { status: 500 },
    );
  }
}
