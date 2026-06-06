import { NextRequest } from "next/server";

import { supportRouteError } from "@/app/api/support/_lib/errors";
import { listSupportArticles } from "@/lib/integrations/conversation-core";
import { requireRequestActor } from "@/lib/integrations/request-actor";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

export async function POST(
  _request: NextRequest,
  { params }: { params: Promise<{ id: string }> },
) {
  const { id } = await params;

  try {
    const actor = await requireRequestActor();
    const articles = await listSupportArticles(actor, id);
    const publicArticles = articles.filter((article) => !article.internal);
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
  } catch (error) {
    return supportRouteError(error, "summary_failed", "Conversation summary could not be generated.");
  }
}

function stripHtml(value: string) {
  return value.replace(/<[^>]*>/g, " ").replace(/\s+/g, " ").trim();
}
