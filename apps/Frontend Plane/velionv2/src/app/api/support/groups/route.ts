import { supportRouteError } from "@/app/api/support/_lib/errors";
import { listSupportGroups } from "@/lib/integrations/conversation-core";
import { RequestActorError, requireRequestActor } from "@/lib/integrations/request-actor";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

export async function GET() {
  try {
    const actor = await requireRequestActor();
    return Response.json(await listSupportGroups(actor));
  } catch (error) {
    if (!(error instanceof RequestActorError)) {
      return Response.json([]);
    }
    return supportRouteError(error, "groups_fetch_failed", "Support groups could not be loaded.");
  }
}
