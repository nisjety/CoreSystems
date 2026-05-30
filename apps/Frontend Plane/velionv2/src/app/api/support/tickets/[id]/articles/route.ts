import { NextRequest } from "next/server";
import { z } from "zod";
import { jsonOrNull, notConfiguredResponse, ZAMMAD_URL, zammadConfigured, zammadHeaders } from "@/app/api/support/_lib/zammad";

const addArticleSchema = z.object({
  body: z.string().min(1),
  internal: z.boolean(),
  type: z.string().optional(),
});

export async function GET(
  _request: NextRequest,
  { params }: { params: Promise<{ id: string }> },
) {
  if (!zammadConfigured()) return notConfiguredResponse();
  const { id } = await params;

  const response = await fetch(`${ZAMMAD_URL}/api/v1/ticket_articles/by_ticket/${encodeURIComponent(id)}`, {
    headers: zammadHeaders(),
    cache: "no-store",
  });
  const payload = await jsonOrNull<unknown[]>(response);

  if (!response.ok) {
    return Response.json(payload ?? { error: "articles_fetch_failed" }, { status: response.status });
  }

  return Response.json({ articles: Array.isArray(payload) ? payload : [] });
}

export async function POST(
  request: NextRequest,
  { params }: { params: Promise<{ id: string }> },
) {
  if (!zammadConfigured()) return notConfiguredResponse();
  const [{ id }, raw] = await Promise.all([
    params,
    request.json().catch(() => null),
  ]);
  const parsed = addArticleSchema.safeParse(raw);

  if (!parsed.success) {
    return Response.json({ error: parsed.error.flatten() }, { status: 422 });
  }

  const response = await fetch(`${ZAMMAD_URL}/api/v1/ticket_articles`, {
    method: "POST",
    headers: zammadHeaders(),
    cache: "no-store",
    body: JSON.stringify({
      ticket_id: Number(id),
      body: parsed.data.body,
      internal: parsed.data.internal,
      type: parsed.data.type || "note",
      content_type: "text/html",
    }),
  });
  const payload = await jsonOrNull<unknown>(response);

  return Response.json({ article: payload ?? null }, { status: response.status });
}
