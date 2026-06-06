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
    const latestCustomerMessage = articles.filter((article) => !article.internal).at(-1)?.body ?? "your message";

    return Response.json({
      options: [
        `Thanks for the context. I am checking ${stripHtml(latestCustomerMessage).slice(0, 80) || "this"} and will come back with the next step.`,
        "I can help with that. I will verify the account and order details before making any changes.",
        "Thanks for flagging this. I will keep this thread updated as soon as I have a confirmed answer.",
      ],
    });
  } catch (error) {
    return supportRouteError(error, "quick_replies_failed", "Quick replies could not be generated.");
  }
}

function stripHtml(value: string) {
  return value.replace(/<[^>]*>/g, " ").replace(/\s+/g, " ").trim();
}
