import { supportRouteError } from "@/app/api/support/_lib/errors";
import { listSupportMacros } from "@/lib/integrations/conversation-core";
import { RequestActorError, requireRequestActor } from "@/lib/integrations/request-actor";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

export async function GET() {
  try {
    await requireRequestActor();
    return Response.json(listSupportMacros());
  } catch (error) {
    if (!(error instanceof RequestActorError)) {
      return Response.json([]);
    }
    return supportRouteError(error, "macros_fetch_failed", "Support macros could not be loaded.");
  }
}
