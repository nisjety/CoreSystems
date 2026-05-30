import { RequestActorError } from "@/lib/integrations/request-actor";
import { UserCoreError } from "@/lib/integrations/user-core";

export function isReadIntegrationUnavailable(error: unknown) {
  if (error instanceof RequestActorError) {
    return error.status === 401 || error.status === 503;
  }

  if (error instanceof UserCoreError) {
    return error.status === 401 || error.status === 502 || error.status === 503;
  }

  return false;
}
