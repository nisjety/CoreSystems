import { NextRequest } from "next/server";
import { jsonOrNull, notConfiguredResponse, ZAMMAD_URL, zammadConfigured, zammadHeaders } from "@/app/api/support/_lib/zammad";

type ZammadArticle = {
  internal?: boolean;
  sender?: string;
  body?: string;
};

const frustratedWords = ["angry", "frustrated", "missing", "late", "broken", "refund", "urgent", "bad"];
const positiveWords = ["thanks", "great", "perfect", "helpful", "appreciate"];

export async function POST(
  _request: NextRequest,
  { params }: { params: Promise<{ id: string }> },
) {
  if (!zammadConfigured()) return notConfiguredResponse();
  const { id } = await params;

  const response = await fetch(`${ZAMMAD_URL}/api/v1/ticket_articles/by_ticket/${encodeURIComponent(id)}`, {
    headers: zammadHeaders(),
    cache: "no-store",
  });
  const articles = await jsonOrNull<ZammadArticle[]>(response);

  if (!response.ok) {
    return Response.json(articles ?? { error: "articles_fetch_failed" }, { status: response.status });
  }

  const message = stripHtml(
    (articles ?? [])
      .filter((article) => !article.internal && article.sender?.toLowerCase() !== "agent")
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
}

function stripHtml(value: string) {
  return value.replace(/<[^>]*>/g, " ").replace(/\s+/g, " ").trim();
}
