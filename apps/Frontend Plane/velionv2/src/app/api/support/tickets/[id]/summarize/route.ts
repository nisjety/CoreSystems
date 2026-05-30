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

  const publicArticles = (articles ?? []).filter((article) => !article.internal);
  if (publicArticles.length === 0) {
    return Response.json({ summary: "No public messages in this conversation." });
  }

  const first = stripHtml(publicArticles[0]?.body ?? "");
  const latest = stripHtml(publicArticles.at(-1)?.body ?? "");

  return Response.json({
    summary: [
      `Customer opened with: ${first || "No readable opening message."}`,
      `Latest public update: ${latest || "No readable latest message."}`,
      "Review ticket fields and respond with the next concrete support step.",
    ].join("\n"),
  });
}

function stripHtml(value: string) {
  return value.replace(/<[^>]*>/g, " ").replace(/\s+/g, " ").trim();
}
