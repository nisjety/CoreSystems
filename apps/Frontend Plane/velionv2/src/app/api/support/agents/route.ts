import { supportRouteError } from "@/app/api/support/_lib/errors";
import { listSupportAgents } from "@/lib/integrations/conversation-core";
import { RequestActorError, requireRequestActor } from "@/lib/integrations/request-actor";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

export async function GET() {
  try {
    const actor = await requireRequestActor();
    return Response.json(listSupportAgents(actor));
  } catch (error) {
    if (!(error instanceof RequestActorError)) {
      return Response.json([]);
    }
    return supportRouteError(error, "agents_fetch_failed", "Support agents could not be loaded.");
  }
}
