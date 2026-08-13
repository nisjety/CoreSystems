/**
 * Application -> Control deletion authorization delivery. This worker only
 * turns a durable human request into a Control receipt. It never calls an
 * owner-plane purge endpoint and cannot claim erasure on a successful reply.
 */
import { v } from "convex/values";
import { internalAction } from "./_generated/server";
import { internal } from "./_generated/api";

const CONTROL_SPACE_DELETION_AUTHORIZATION_URL = process.env.CONTROL_SPACE_DELETION_AUTHORIZATION_URL || "";
const APPLICATION_SPACE_LIFECYCLE_TOKEN = process.env.APPLICATION_SPACE_LIFECYCLE_TOKEN || "";

type AuthorizationStatus = "authorized" | "blocked_legal_hold" | "rejected";

function configurationError(): string | null {
  if (!CONTROL_SPACE_DELETION_AUTHORIZATION_URL.trim()) return "CONTROL_SPACE_DELETION_AUTHORIZATION_URL is not configured";
  if (!APPLICATION_SPACE_LIFECYCLE_TOKEN.trim()) return "APPLICATION_SPACE_LIFECYCLE_TOKEN is not configured";
  return null;
}

function nextWorker(eventId: string): string {
  return `space-deletion-authorization:${eventId}:${crypto.randomUUID()}`;
}

export function receiptStatus(receipt: unknown, requestId: string): AuthorizationStatus | null {
  if (!receipt || typeof receipt !== "object") return null;
  const data = (receipt as { data?: unknown }).data;
  if (!data || typeof data !== "object") return null;
  const value = data as { request_id?: unknown; status?: unknown };
  if (value.request_id !== requestId) return null;
  if (value.status === "authorized" || value.status === "blocked_legal_hold" || value.status === "rejected") {
    return value.status;
  }
  return null;
}

export const deliverOne = internalAction({
  args: { workerId: v.string() },
  handler: async (ctx, args) => {
    const claimed = await ctx.runMutation(internal.spaces.claimNextDeletionAuthorization, {
      now: Date.now(), workerId: args.workerId,
    });
    if (!claimed) return { status: "idle" as const };
    const release = async (reason: string) => {
      const retryAt = await ctx.runMutation(internal.spaces.releaseDeletionAuthorizationClaim, {
        eventId: claimed.eventId, workerId: args.workerId, now: Date.now(), error: reason,
      });
      await ctx.scheduler.runAfter(Math.max(0, retryAt - Date.now()), internal.spaceDeletionAuthorization.deliverOne, {
        workerId: nextWorker(claimed.eventId),
      });
      return { status: "retry_scheduled" as const, retryAt };
    };
    const misconfigured = configurationError();
    if (misconfigured) return release(misconfigured);
    let response: Response;
    try {
      response = await fetch(CONTROL_SPACE_DELETION_AUTHORIZATION_URL, {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          "X-Service-Id": "application-space-lifecycle",
          "X-Service-Token": APPLICATION_SPACE_LIFECYCLE_TOKEN,
        },
        body: JSON.stringify({
          space_ref: claimed.request.spaceRef,
          org_id: claimed.request.externalOrgId,
          owner_principal_id: claimed.request.ownerExternalAuthId,
          request_id: claimed.request.requestId,
          idempotency_key: claimed.request.idempotencyKey,
        }),
      });
    } catch {
      return release("Control Space deletion authorization request failed");
    }
    if (response.status !== 200) return release(`Control Space deletion authorization returned HTTP ${response.status}`);
    let receipt: unknown;
    try {
      receipt = await response.json();
    } catch {
      return release("Control Space deletion authorization returned an invalid receipt");
    }
    const status = receiptStatus(receipt, claimed.request.requestId);
    if (!status) return release("Control Space deletion authorization receipt did not match the claimed request");
    await ctx.runMutation(internal.spaces.acknowledgeDeletionAuthorizationClaim, {
      eventId: claimed.eventId, workerId: args.workerId, now: Date.now(), status,
    });
    return { eventId: claimed.eventId, status };
  },
});
