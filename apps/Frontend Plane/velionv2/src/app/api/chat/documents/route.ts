/**
 * BFF document-upload route — POST /api/chat/documents  { title, content, source?, type? }
 *
 * Uploads a document into Data Plane v2 (chat-parity §2 file upload) so it
 * becomes retrievable by the RAG path. Proxies Model Plane POST /v1/chat/documents,
 * which calls Data Plane's DocumentService (the ingest owner). Auth + JWT mint
 * reuse the stream/history routes; org scope is enforced server-side.
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

  let body: { title?: string; content?: string; source?: string; type?: string };
  try {
    body = (await request.json()) as typeof body;
  } catch {
    return NextResponse.json({ error: "Invalid JSON" }, { status: 400 });
  }

  const title = typeof body.title === "string" ? body.title.slice(0, 512) : "";
  const content = typeof body.content === "string" ? body.content : "";
  if (!content.trim()) {
    return NextResponse.json({ error: "content is required" }, { status: 400 });
  }
  if (content.length > 5_000_000) {
    return NextResponse.json({ error: "content exceeds maximum size" }, { status: 413 });
  }

  const bearerToken = await mintAudienceToken(request, getModelPlaneAudience());
  const authHeader = bearerToken
    ? `Bearer ${bearerToken}`
    : `Bearer ${process.env.MODEL_GATEWAY_BEARER ?? process.env.INTERNAL_API_KEY ?? process.env.INTERNAL_SERVICE_SECRET ?? "dev-bypass"}`;

  try {
    const res = await fetch(`${modelPlaneBase()}/v1/chat/documents`, {
      method: "POST",
      headers: { Authorization: authHeader, "Content-Type": "application/json" },
      body: JSON.stringify({
        title,
        content,
        ...(typeof body.source === "string" ? { source: body.source } : {}),
        ...(typeof body.type === "string" ? { type: body.type } : {}),
      }),
      signal: AbortSignal.timeout(30_000),
    });
    if (!res.ok) {
      return NextResponse.json({ error: "upload failed" }, { status: 502 });
    }
    const data = (await res.json()) as { document_id?: string; status?: string };
    return NextResponse.json({
      documentId: typeof data.document_id === "string" ? data.document_id : "",
      status: typeof data.status === "string" ? data.status : "",
    });
  } catch {
    return NextResponse.json({ error: "upload forward failed" }, { status: 502 });
  }
}
