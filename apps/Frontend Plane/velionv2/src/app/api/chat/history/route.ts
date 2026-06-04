/**
 * BFF chat-history route — GET /api/chat/history?threadId=...
 *
 * Reloads a thread's conversation for cross-device resume (chat-parity §1).
 * Forwards to Model Plane GET /v1/threads/{threadId}/messages, which reads
 * session-core's canonical ListConversation. Auth + JWT mint reuse the same
 * path as the stream/cancel routes; org scope is enforced server-side from the
 * minted token's claims, so a caller cannot read another org's thread.
 */

import { type NextRequest, NextResponse } from "next/server";
import { getModelPlaneAudience, mintAudienceToken } from "@/app/api/onboarding/_lib/onboarding-proxy";
import { requireRequestActor, RequestActorError } from "@/lib/integrations/request-actor";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

type SessionMessage = { role: string; content: string };

function modelPlaneBase(): string {
  return (
    process.env.MODEL_PLANE_URL ||
    process.env.MODEL_PLANE_AI_URL ||
    "http://model-gateway:8080"
  );
}

export async function GET(request: NextRequest): Promise<Response> {
  try {
    await requireRequestActor();
  } catch (err) {
    if (err instanceof RequestActorError) {
      return NextResponse.json({ error: "Unauthenticated" }, { status: 401 });
    }
    return NextResponse.json({ error: "Auth error" }, { status: 500 });
  }

  const threadId = (request.nextUrl.searchParams.get("threadId") ?? "").trim();
  if (!threadId || threadId.length > 160) {
    return NextResponse.json({ error: "threadId is required" }, { status: 400 });
  }

  const bearerToken = await mintAudienceToken(request, getModelPlaneAudience());
  const authHeader = bearerToken
    ? `Bearer ${bearerToken}`
    : `Bearer ${process.env.MODEL_GATEWAY_BEARER ?? process.env.INTERNAL_API_KEY ?? process.env.INTERNAL_SERVICE_SECRET ?? "dev-bypass"}`;

  try {
    const res = await fetch(
      `${modelPlaneBase()}/v1/threads/${encodeURIComponent(threadId)}/messages`,
      {
        method: "GET",
        headers: { Authorization: authHeader },
        signal: AbortSignal.timeout(10_000),
      },
    );

    if (!res.ok) {
      return NextResponse.json(
        { error: "history fetch failed", threadId },
        { status: res.status === 404 ? 404 : 502 },
      );
    }

    const data = (await res.json()) as { messages?: SessionMessage[] };
    const messages = Array.isArray(data.messages) ? data.messages : [];
    return NextResponse.json({ threadId, messages });
  } catch {
    return NextResponse.json({ error: "history forward failed", threadId }, { status: 502 });
  }
}
