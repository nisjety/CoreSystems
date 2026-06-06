import { ConversationCoreError } from "@/lib/integrations/conversation-core";
import { RequestActorError } from "@/lib/integrations/request-actor";

export function supportRouteError(error: unknown, code: string, message: string) {
  if (error instanceof ConversationCoreError || error instanceof RequestActorError) {
    return Response.json({ error: { code: error.code, message: error.message } }, { status: error.status });
  }

  return Response.json({ error: { code, message } }, { status: 500 });
}
