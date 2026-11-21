/**
 * Application -> Control deletion authorization delivery. This worker only
 * turns a durable human request into a Control receipt. It never calls an
 * owner-plane purge endpoint and cannot claim erasure on a successful reply.
 *
 * A claim that dies between its claim and its acknowledgement (a crash, a deploy
 * restart, a lost lease) is re-driven by `sweepAuthorizationEvents` on the cron in
 * crons.ts, when SPACE_DELETION_AUTH_SWEEP is the literal true. Without the sweep,
 * such a claim waits for another request's run to reclaim it.
 */
import { v } from "convex/values";
import { internalAction } from "./_generated/server";
import { internal } from "./_generated/api";

const CONTROL_SPACE_DELETION_AUTHORIZATION_URL = process.env.CONTROL_SPACE_DELETION_AUTHORIZATION_URL || "";
const APPLICATION_SPACE_LIFECYCLE_TOKEN = process.env.APPLICATION_SPACE_LIFECYCLE_TOKEN || "";

// The most expired claims one sweep re-drives. The sweep runs every five minutes, so a larger backlog drains over several runs.
const SWEEP_LIMIT = 20;

// SPACE_DELETION_AUTH_SWEEP turns on both the sweep and the release of a claim that threw after it was taken. It is read at
// call time, as the literal true only. Off, the authorization path behaves exactly as it did before.
function recoveryEnabled(): boolean {
  return process.env.SPACE_DELETION_AUTH_SWEEP === "true";
}

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
    try {
      const misconfigured = configurationError();
      if (misconfigured) return await release(misconfigured);
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
        return await release("Control Space deletion authorization request failed");
      }
      if (response.status !== 200) return await release(`Control Space deletion authorization returned HTTP ${response.status}`);
      let receipt: unknown;
      try {
        receipt = await response.json();
      } catch {
        return await release("Control Space deletion authorization returned an invalid receipt");
      }
      const status = receiptStatus(receipt, claimed.request.requestId);
      if (!status) return await release("Control Space deletion authorization receipt did not match the claimed request");
      // Application may refuse an authorized request whose Space it cannot fence. Report the
      // outcome it recorded; for every other answer that is Control's own answer.
      const acknowledged = await ctx.runMutation(internal.spaces.acknowledgeDeletionAuthorizationClaim, {
        eventId: claimed.eventId, workerId: args.workerId, now: Date.now(), status,
      });
      return { eventId: claimed.eventId, status: acknowledged.state };
    } catch (error) {
      // Off, the throw propagates as it always has. On, a throw after the claim (a failed mutation, or an acknowledgement the
      // event's state does not accept) must not leave the event claimed: only a release schedules the next attempt, and a
      // claim is otherwise re-driven only by a later run or by the sweep. The warning carries ids only, never the error text.
      if (!recoveryEnabled()) throw error;
      console.warn("[Space deletion] authorization delivery failed after the claim; releasing it for retry", {
        eventId: claimed.eventId, requestId: claimed.request.requestId,
      });
      return await release("Control Space deletion authorization delivery failed before a receipt was recorded");
    }
  },
});

// Re-drives the authorization claims whose lease has run out, at most SWEEP_LIMIT per run. Each one gets its own run of
// deliverOne: a run claims the earliest due authorization, and an expired claim is due, so the expired claim is among those
// the runs take. The switch is read at call time as the literal true only; with it off the sweep reads and schedules nothing.
export const sweepAuthorizationEvents = internalAction({
  args: {},
  handler: async (ctx) => {
    if (!recoveryEnabled()) return { status: "disabled" as const, scheduled: 0 };
    const eventIds: string[] = await ctx.runQuery(internal.spaces.expiredDeletionAuthorizationLeases, {
      now: Date.now(), limit: SWEEP_LIMIT,
    });
    for (const eventId of eventIds) {
      await ctx.scheduler.runAfter(0, internal.spaceDeletionAuthorization.deliverOne, {
        workerId: nextWorker(eventId),
      });
    }
    if (eventIds.length > 0) {
      console.warn("[Space deletion] authorization sweep re-drove claims whose lease had expired", { scheduled: eventIds.length });
    }
    return { status: "swept" as const, scheduled: eventIds.length };
  },
});
