import { NextRequest } from "next/server";
import { z } from "zod";

import { supportRouteError } from "@/app/api/support/_lib/errors";
import { addSupportArticle, listSupportArticles } from "@/lib/integrations/conversation-core";
import { requireRequestActor } from "@/lib/integrations/request-actor";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

const addArticleSchema = z.object({
  body: z.string().min(1),
  internal: z.boolean(),
  type: z.string().optional(),
});

export async function GET(
  _request: NextRequest,
  { params }: { params: Promise<{ id: string }> },
) {
  const { id } = await params;

  try {
    const actor = await requireRequestActor();
    const articles = await listSupportArticles(actor, id);
    return Response.json({ articles });
  } catch (error) {
    return supportRouteError(error, "articles_fetch_failed", "Support articles could not be loaded.");
  }
}

export async function POST(
  request: NextRequest,
  { params }: { params: Promise<{ id: string }> },
) {
  const [{ id }, raw] = await Promise.all([
    params,
    request.json().catch(() => null),
  ]);
  const parsed = addArticleSchema.safeParse(raw);

  if (!parsed.success) {
    return Response.json({ error: parsed.error.flatten() }, { status: 422 });
  }

  try {
    const actor = await requireRequestActor();
    const article = await addSupportArticle(actor, id, parsed.data);
    return Response.json({ article }, { status: 201 });
  } catch (error) {
    return supportRouteError(error, "article_create_failed", "Support article could not be created.");
  }
}
