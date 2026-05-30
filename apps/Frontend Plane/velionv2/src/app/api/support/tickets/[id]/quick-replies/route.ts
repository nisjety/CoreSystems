import { NextRequest } from "next/server";
import { jsonOrNull, notConfiguredResponse, ZAMMAD_URL, zammadConfigured, zammadHeaders } from "@/app/api/support/_lib/zammad";

type ZammadArticle = {
  internal?: boolean;
  body?: string;
  from?: string;
};

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

  const latestCustomerMessage = (articles ?? []).filter((article) => !article.internal).at(-1)?.body ?? "your message";

  return Response.json({
    options: [
      `Thanks for the context. I am checking ${stripHtml(latestCustomerMessage).slice(0, 80) || "this"} and will come back with the next step.`,
      "I can help with that. I will verify the account and order details before making any changes.",
      "Thanks for flagging this. I will keep this thread updated as soon as I have a confirmed answer.",
    ],
  });
}

function stripHtml(value: string) {
  return value.replace(/<[^>]*>/g, " ").replace(/\s+/g, " ").trim();
}
