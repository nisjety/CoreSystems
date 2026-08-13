/** Data Plane binding-revocation adapter. It carries only the Control-minted
 * `aud=data-plane` deletion credential; a failed delivery remains retriable
 * and no response can update a different owner/request receipt. */
import { v } from "convex/values";
import { internalAction } from "./_generated/server";
import { internal } from "./_generated/api";

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
    const release = async (reason: string) => {
      const retryAt = await ctx.runMutation(internal.spaces.releaseDataDeletionOwnerDelivery, { eventId: claimed.eventId, workerId: args.workerId, now: Date.now(), error: reason });
      await ctx.scheduler.runAfter(Math.max(0, retryAt - Date.now()), internal.spaceDeletionData.deliverOne, { workerId: nextWorker(claimed.eventId) });
      return { status: "retry_scheduled" as const, retryAt };
    };
    const misconfigured = configurationError();
    if (misconfigured) return release(misconfigured);
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
    } catch { return release("Data Space deletion request failed"); }
    if (response.status !== 200) return release(`Data Space deletion returned HTTP ${response.status}`);
    let receipt: unknown;
    try { receipt = await response.json(); } catch { return release("Data Space deletion returned an invalid receipt"); }
    const outcome = dataDeletionReceipt(receipt, claimed.request.requestId);
    if (!outcome) return release("Data Space deletion receipt did not match the claimed request");
    await ctx.runMutation(internal.spaces.acknowledgeDataDeletionOwnerDelivery, { eventId: claimed.eventId, workerId: args.workerId, now: Date.now(), ...outcome });
    return { eventId: claimed.eventId, status: outcome.status };
  },
});
