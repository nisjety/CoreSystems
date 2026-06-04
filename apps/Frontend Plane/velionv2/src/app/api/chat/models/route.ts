/**
 * BFF model-catalog route — GET /api/chat/models
 *
 * Lists available models + their per-model feature families (chat-parity §2)
 * so the composer can gate the opt-in `features[]` (reasoning/tools/vision/…)
 * per selected model. Proxies Model Plane GET /v1/models, which proxies
 * inference-core ListModels (the capability owner). Auth + JWT mint reuse the
 * same path as the stream/cancel/history routes.
 */

import { type NextRequest, NextResponse } from "next/server";
import { getModelPlaneAudience, mintAudienceToken } from "@/app/api/onboarding/_lib/onboarding-proxy";
import { requireRequestActor, RequestActorError } from "@/lib/integrations/request-actor";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

type ModelDescriptor = {
  id: string;
  provider: string;
  modality: string;
  streaming: boolean;
  features: string[];
};

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

  const bearerToken = await mintAudienceToken(request, getModelPlaneAudience());
  const authHeader = bearerToken
    ? `Bearer ${bearerToken}`
    : `Bearer ${process.env.MODEL_GATEWAY_BEARER ?? process.env.INTERNAL_API_KEY ?? process.env.INTERNAL_SERVICE_SECRET ?? "dev-bypass"}`;

  try {
    const res = await fetch(`${modelPlaneBase()}/v1/models`, {
      method: "GET",
      headers: { Authorization: authHeader },
      signal: AbortSignal.timeout(10_000),
    });
    if (!res.ok) {
      return NextResponse.json({ error: "models fetch failed", models: [] }, { status: 502 });
    }
    const data = (await res.json()) as { models?: ModelDescriptor[] };
    const models = Array.isArray(data.models) ? data.models : [];
    return NextResponse.json({ models });
  } catch {
    return NextResponse.json({ error: "models forward failed", models: [] }, { status: 502 });
  }
}
