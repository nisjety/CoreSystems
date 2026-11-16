/** Data Plane deletion adapter: binding revocation, then the room canvas purge (WS19-S8). It carries only the Control-minted
 * `aud=data-plane` deletion credential; a failed delivery remains retriable
 * and no response can update a different owner/request receipt. */
import { v } from "convex/values";
import { internalAction } from "./_generated/server";
import { internal } from "./_generated/api";

const DATA_SPACE_DELETION_URL = process.env.DATA_SPACE_DELETION_URL || "";
// The room canvas purge is the second Data leg (WS19-S8). Its receipt is required before the owner receipt is acknowledged.
const DATA_SPACE_CANVAS_DELETION_URL = process.env.DATA_SPACE_CANVAS_DELETION_URL || "";
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

// The canvas purge answers the same top-level receipt shape and must also name the Space it purged.
export function dataCanvasPurgeReceipt(receipt: unknown, requestId: string, spaceRef: string): { status: DataOutcome; receiptRef: string; detail: string } | null {
  if (!receipt || typeof receipt !== "object") return null;
  if ((receipt as { space_ref?: unknown }).space_ref !== spaceRef) return null;
  const outcome = dataDeletionReceipt(receipt, requestId);
  if (!outcome) return null;
  return { status: outcome.status, receiptRef: outcome.receiptRef, detail: `Data canvas purge receipt: ${outcome.status}` };
}

// One owner receipt covers both Data legs, so it keeps the weaker outcome: failed, then unknown, then partial, then
// succeeded. The detail names both legs.
const DATA_OUTCOME_RANK: Record<DataOutcome, number> = { succeeded: 0, partial: 1, unknown: 2, failed: 3 };
export function combineDataOutcomes(
  first: { status: DataOutcome; receiptRef: string; detail: string },
  second: { status: DataOutcome; receiptRef: string; detail: string },
): { status: DataOutcome; receiptRef: string; detail: string } {
  const status = DATA_OUTCOME_RANK[second.status] > DATA_OUTCOME_RANK[first.status] ? second.status : first.status;
  return { status, receiptRef: first.receiptRef, detail: `${first.detail}; ${second.detail}`.slice(0, 256) };
}

function configurationError(): string | null {
  if (!DATA_SPACE_DELETION_URL.trim()) return "DATA_SPACE_DELETION_URL is not configured";
  if (!DATA_SPACE_CANVAS_DELETION_URL.trim()) return "DATA_SPACE_CANVAS_DELETION_URL is not configured";
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
    // Second leg (WS19-S8): purge the room canvas. Both receipts are needed before the owner receipt is acknowledged. A retry
    // repeats both legs, which is safe because the binding revocation and the canvas purge are idempotent.
    let canvasResponse: Response;
    try {
      canvasResponse = await fetch(DATA_SPACE_CANVAS_DELETION_URL, {
        method: "POST",
        headers: { "Authorization": `Bearer ${CONTROL_SPACE_DELETION_DATA_BEARER}`, "Content-Type": "application/json" },
        body: JSON.stringify({
          deletion_request_id: claimed.request.requestId,
          space_ref: claimed.request.spaceRef,
          idempotency_key: `space-delete:${claimed.request.requestId}`,
        }),
      });
    } catch { return release("Data canvas purge request failed"); }
    if (canvasResponse.status !== 200) return release(`Data canvas purge returned HTTP ${canvasResponse.status}`);
    let canvasReceipt: unknown;
    try { canvasReceipt = await canvasResponse.json(); } catch { return release("Data canvas purge returned an invalid receipt"); }
    const canvasOutcome = dataCanvasPurgeReceipt(canvasReceipt, claimed.request.requestId, claimed.request.spaceRef);
    if (!canvasOutcome) return release("Data canvas purge receipt did not match the claimed request");
    const combined = combineDataOutcomes(outcome, canvasOutcome);
    await ctx.runMutation(internal.spaces.acknowledgeDataDeletionOwnerDelivery, { eventId: claimed.eventId, workerId: args.workerId, now: Date.now(), ...combined });
    return { eventId: claimed.eventId, status: combined.status };
  },
});
