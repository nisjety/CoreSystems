import "server-only";

import type { RequestActor } from "@/lib/integrations/request-actor";

const getNotificationCoreUrl = () =>
  (
    process.env.NOTIFICATION_CORE_URL ||
    process.env.NOTIFICATION_SERVICE_URL ||
    "http://notification-core:3140"
  ).replace(/\/+$/, "");

const getInternalApiKey = () =>
  (process.env.INTERNAL_API_KEY || process.env.INTERNAL_SERVICE_SECRET || "").trim();

export async function sendOnboardingCompletedNotification(
  actor: RequestActor,
  input: { orgId?: string; plan?: string; source?: string; metadata?: unknown },
): Promise<void> {
  const internalKey = getInternalApiKey();
  if (!internalKey) return;

  await fetch(`${getNotificationCoreUrl()}/api/v1/notification-requests`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      "x-internal-api-key": internalKey,
    },
    cache: "no-store",
    signal: AbortSignal.timeout(3_000),
    body: JSON.stringify({
      idempotency_key: `onboarding.completed:${actor.userId}:${input.orgId ?? "no-org"}`,
      recipient_id: actor.userId,
      type: "onboarding.completed",
      source: input.source ?? "velionv2-onboarding",
      payload: {
        org_id: input.orgId,
        plan: input.plan,
        onboarding_metadata: input.metadata,
      },
    }),
  }).then(() => undefined);
}
