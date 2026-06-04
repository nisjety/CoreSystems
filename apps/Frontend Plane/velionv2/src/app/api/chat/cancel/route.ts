/**
 * BFF cancel route — POST /api/chat/cancel  { requestId }
 *
 * Forwards a cooperative stop to Model Plane /v1/invoke/{requestId}/cancel
 * (chat-parity §4). The browser's AbortController halts the local SSE read; this
 * tells the gateway to stop the upstream generation too (and emit `stopped`).
 * Auth + JWT mint reuse the same path as the stream route.
 */

import { type NextRequest, NextResponse } from "next/server";
import { getModelPlaneAudience, mintAudienceToken } from "@/app/api/onboarding/_lib/onboarding-proxy";
import { requireRequestActor, RequestActorError } from "@/lib/integrations/request-actor";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

function modelPlaneBase(): string {
  return (
    process.env.MODEL_PLANE_URL ||
    process.env.MODEL_PLANE_AI_URL ||
    "http://model-gateway:8080"
  );
}

export async function POST(request: NextRequest): Promise<Response> {
  try {
    await requireRequestActor();
  } catch (err) {
    if (err instanceof RequestActorError) {
      return NextResponse.json({ error: "Unauthenticated" }, { status: 401 });
    }
    return NextResponse.json({ error: "Auth error" }, { status: 500 });
  }

  let body: { requestId?: string };
  try {
    body = (await request.json()) as { requestId?: string };
  } catch {
    return NextResponse.json({ error: "Invalid JSON" }, { status: 400 });
  }

  const requestId = typeof body.requestId === "string" ? body.requestId.trim() : "";
  if (!requestId || requestId.length > 160) {
    return NextResponse.json({ error: "requestId is required" }, { status: 400 });
  }

  const bearerToken = await mintAudienceToken(request, getModelPlaneAudience());
  const authHeader = bearerToken
    ? `Bearer ${bearerToken}`
    : `Bearer ${process.env.MODEL_GATEWAY_BEARER ?? process.env.INTERNAL_API_KEY ?? process.env.INTERNAL_SERVICE_SECRET ?? "dev-bypass"}`;

  try {
    const res = await fetch(
      `${modelPlaneBase()}/v1/invoke/${encodeURIComponent(requestId)}/cancel`,
      {
        method: "POST",
        headers: { Authorization: authHeader },
        signal: AbortSignal.timeout(5_000),
      },
    );
    // 202 when the gateway accepted the cancel; pass through a miss as 404.
    return NextResponse.json(
      { cancelled: res.ok, requestId },
      { status: res.ok ? 202 : res.status === 404 ? 404 : 502 },
    );
  } catch {
    return NextResponse.json({ cancelled: false, error: "cancel forward failed" }, { status: 502 });
  }
}
