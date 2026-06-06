import { NextRequest } from "next/server";

import { supportRouteError } from "@/app/api/support/_lib/errors";
import { listSupportArticles } from "@/lib/integrations/conversation-core";
import { requireRequestActor } from "@/lib/integrations/request-actor";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

const frustratedWords = ["angry", "frustrated", "missing", "late", "broken", "refund", "urgent", "bad"];
const positiveWords = ["thanks", "great", "perfect", "helpful", "appreciate"];

export async function POST(
  _request: NextRequest,
  { params }: { params: Promise<{ id: string }> },
) {
  const { id } = await params;

  try {
    const actor = await requireRequestActor();
    const articles = await listSupportArticles(actor, id);
    const message = stripHtml(
      articles
        .filter((article) => !article.internal && article.sender.toLowerCase() !== "agent")
        .at(-1)?.body ?? "",
    ).toLowerCase();

    if (!message) {
      return Response.json({ sentiment: "neutral", score: 50 });
    }

    if (frustratedWords.some((word) => message.includes(word))) {
      return Response.json({ sentiment: "frustrated", score: 86 });
    }

    if (positiveWords.some((word) => message.includes(word))) {
      return Response.json({ sentiment: "positive", score: 74 });
    }

    return Response.json({ sentiment: "neutral", score: 58 });
  } catch (error) {
    return supportRouteError(error, "sentiment_failed", "Conversation sentiment could not be generated.");
  }
}

function stripHtml(value: string) {
  return value.replace(/<[^>]*>/g, " ").replace(/\s+/g, " ").trim();
}
