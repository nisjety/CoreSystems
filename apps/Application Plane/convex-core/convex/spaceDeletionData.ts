/** Data Plane binding-revocation adapter. It carries only the Control-minted
 * `aud=data-plane` deletion credential; a failed delivery remains retriable
 * and no response can update a different owner/request receipt. A typed legal-hold
 * refusal parks the delivery and blocks its receipt, as the Model adapter does; with
 * SPACE_DELETION_PERMANENT_REFUSAL_PARK on, a permanent refusal (400, 401, 403, 412)
 * parks it as `failed_permanent`, and with it off that refusal retries as before; any
 * throw after the claim releases it for retry, and parks it as `failed_permanent` once its
 * claim count reaches the bound in `spaces.ts`. */
import { v } from "convex/values";
import { internalAction } from "./_generated/server";
import { internal } from "./_generated/api";
import { isOwnerLegalHoldRefusal, isPermanentOwnerRefusal, permanentRefusalParkEnabled } from "./spaceDeletionReceipts";

const DATA_SPACE_DELETION_URL = process.env.DATA_SPACE_DELETION_URL || "";
const CONTROL_SPACE_DELETION_DATA_BEARER = process.env.CONTROL_SPACE_DELETION_DATA_BEARER || "";
type DataOutcome = "succeeded" | "partial" | "failed" | "unknown";

export function dataDeletionReceipt(receipt: unknown, requestId: string): { status: DataOutcome; receiptRef: string; detail: string } | null {
  if (!receipt || typeof receipt !== "object") return null;
  const body = receipt as { request_id?: unknown; owner_plane?: unknown; owner_outcome?: unknown; remaining_work?: unknown };
  if (body.request_id !== requestId || body.owner_plane !== "data") return null;
  if (body.owner_outcome !== "succeeded" && body.owner_outcome !== "partial" && body.owner_outcome !== "failed" && body.owner_outcome !== "unknown") return null;
  const remaining = typeof body.remaining_work === "string" ? body.remaining_work.slice(0, 180) : "";
  return { status: body.owner_outcome, receiptRef: `data:${requestId}`, detail: remaining ? `Data owner receipt: ${remaining}` : "Data owner receipt accepted" };
}

async function isLegalHoldRefusal(response: Response): Promise<boolean> {
  try {
    return isOwnerLegalHoldRefusal(await response.json());
  } catch {
    return false;
  }
}

function configurationError(): string | null {
  if (!DATA_SPACE_DELETION_URL.trim()) return "DATA_SPACE_DELETION_URL is not configured";
  if (!CONTROL_SPACE_DELETION_DATA_BEARER.trim()) return "CONTROL_SPACE_DELETION_DATA_BEARER is not configured";
  return null;
}
function nextWorker(eventId: string): string { return `space-deletion-data:${eventId}:${crypto.randomUUID()}`; }

export const deliverOne = internalAction({
  args: { workerId: v.string() },
  handler: async (ctx, args) => {
    const claimed = await ctx.runMutation(internal.spaces.claimNextDataDeletionOwnerDelivery, { now: Date.now(), workerId: args.workerId });
    if (!claimed) return { status: "idle" as const };
    // `afterThrow` marks a release after a throw. The mutation bounds those: past the bound
    // the row parks as failed_permanent and nothing is scheduled.
    const release = async (reason: string, afterThrow = false) => {
      const retryAt = await ctx.runMutation(internal.spaces.releaseDataDeletionOwnerDelivery, { eventId: claimed.eventId, workerId: args.workerId, now: Date.now(), error: reason, afterThrow });
      if (retryAt === null) {
        console.error("[Space deletion] Data delivery parked after repeated failures after the claim");
        return { eventId: claimed.eventId, status: "failed_permanent" as const };
      }
      await ctx.scheduler.runAfter(Math.max(0, retryAt - Date.now()), internal.spaceDeletionData.deliverOne, { workerId: nextWorker(claimed.eventId) });
      return { status: "retry_scheduled" as const, retryAt };
    };
    const blockByLegalHold = async () => {
      await ctx.runMutation(internal.spaces.blockDataDeletionOwnerDeliveryForLegalHold, { eventId: claimed.eventId, workerId: args.workerId, now: Date.now() });
      return { eventId: claimed.eventId, status: "blocked_legal_hold" as const };
    };
    const failPermanently = async (reason: string) => {
      await ctx.runMutation(internal.spaces.failDataDeletionOwnerDeliveryPermanently, { eventId: claimed.eventId, workerId: args.workerId, now: Date.now(), reason });
      return { eventId: claimed.eventId, status: "failed_permanent" as const };
    };
    try {
      const misconfigured = configurationError();
      if (misconfigured) return await release(misconfigured);
      let response: Response;
      try {
        response = await fetch(DATA_SPACE_DELETION_URL, {
          method: "POST",
          headers: { "Authorization": `Bearer ${CONTROL_SPACE_DELETION_DATA_BEARER}`, "Content-Type": "application/json" },
          body: JSON.stringify({
            deletion_request_id: claimed.request.requestId,
            space_ref: claimed.request.spaceRef,
            reason: "Control-authorized Space deletion binding revocation",
            idempotency_key: `space-delete:${claimed.request.requestId}`,
          }),
        });
      } catch { return await release("Data Space deletion request failed"); }
      if (response.status !== 200) {
        if (await isLegalHoldRefusal(response)) return await blockByLegalHold();
        if (permanentRefusalParkEnabled() && isPermanentOwnerRefusal(response.status)) {
          return await failPermanently(`permanent_refusal_http_${response.status}`);
        }
        return await release(`Data Space deletion returned HTTP ${response.status}`);
      }
      let receipt: unknown;
      try { receipt = await response.json(); } catch { return await release("Data Space deletion returned an invalid receipt"); }
      const outcome = dataDeletionReceipt(receipt, claimed.request.requestId);
      if (!outcome) return await release("Data Space deletion receipt did not match the claimed request");
      await ctx.runMutation(internal.spaces.acknowledgeDataDeletionOwnerDelivery, { eventId: claimed.eventId, workerId: args.workerId, now: Date.now(), ...outcome });
      return { eventId: claimed.eventId, status: outcome.status };
    } catch {
      // Only a release schedules the next attempt; without it an abandoned claim
      // stays claimed, and the release parks it once the claim count passes its bound.
      // The logged text is fixed, so no upstream body is recorded.
      console.error("[Space deletion] Data delivery failed after the claim; releasing it for retry");
      return await release("Data Space deletion delivery failed before an owner receipt was recorded", true);
    }
  },
});
