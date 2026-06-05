/**
 * BFF voice-session route — POST /api/voice/session
 *
 * Mints an ephemeral realtime voice session (chat-parity Phase 3 voice). Proxies
 * to Model Plane POST /v1/ai/realtime, which returns
 * { session_id, client_secret, websocket_url, expires_at, model_used, voice }.
 * The browser then opens the realtime media connection directly using the
 * minted ephemeral credential (the standard realtime pattern — the server's
 * role is to mint, not to relay media). Auth + JWT mint reuse the stream/cancel
 * path; the ephemeral client_secret is short-lived and org-scoped server-side.
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

type VoiceSessionBody = {
  model?: string;
  voice?: string;
  instructions?: string;
};

export async function POST(request: NextRequest): Promise<Response> {
  try {
    await requireRequestActor();
  } catch (err) {
    if (err instanceof RequestActorError) {
      return NextResponse.json({ error: "Unauthenticated" }, { status: 401 });
    }
    return NextResponse.json({ error: "Auth error" }, { status: 500 });
  }

  let body: VoiceSessionBody = {};
  try {
    body = (await request.json().catch(() => ({}))) as VoiceSessionBody;
  } catch {
    body = {};
  }

  const bearerToken = await mintAudienceToken(request, getModelPlaneAudience());
  const authHeader = bearerToken
    ? `Bearer ${bearerToken}`
    : `Bearer ${process.env.MODEL_GATEWAY_BEARER ?? process.env.INTERNAL_API_KEY ?? process.env.INTERNAL_SERVICE_SECRET ?? "dev-bypass"}`;

  try {
    const res = await fetch(`${modelPlaneBase()}/v1/ai/realtime`, {
      method: "POST",
      headers: { Authorization: authHeader, "Content-Type": "application/json" },
      body: JSON.stringify({
        model: typeof body.model === "string" ? body.model : undefined,
        voice: typeof body.voice === "string" ? body.voice : undefined,
        instructions: typeof body.instructions === "string" ? body.instructions : undefined,
      }),
      signal: AbortSignal.timeout(10_000),
    });

    if (!res.ok) {
      return NextResponse.json(
        { error: "voice session mint failed" },
        { status: res.status === 404 ? 404 : 502 },
      );
    }
    // Pass the ephemeral session through verbatim for the browser media client.
    const session = (await res.json()) as Record<string, unknown>;
    return NextResponse.json(session);
  } catch {
    return NextResponse.json({ error: "voice session forward failed" }, { status: 502 });
  }
}
