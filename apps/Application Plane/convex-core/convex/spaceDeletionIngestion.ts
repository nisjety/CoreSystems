/** Ingestion bounded-purge adapter: only the Control-issued ingestion bearer
 * can cross the boundary; delivery stays leased until an exact owner receipt. */
import { v } from "convex/values";
import { internalAction } from "./_generated/server";
import { internal } from "./_generated/api";

const INGESTION_SPACE_DELETION_URL = process.env.INGESTION_SPACE_DELETION_URL || "";
const CONTROL_SPACE_DELETION_INGESTION_BEARER = process.env.CONTROL_SPACE_DELETION_INGESTION_BEARER || "";
type IngestionOutcome = "succeeded" | "partial" | "failed" | "unknown";

export function ingestionDeletionReceipt(receipt: unknown, requestId: string): { status: IngestionOutcome; receiptRef: string; detail: string } | null {
  if (!receipt || typeof receipt !== "object") return null;
  const body = receipt as { request_id?: unknown; owner_plane?: unknown; owner_outcome?: unknown; remaining_work?: unknown };
  if (body.request_id !== requestId || body.owner_plane !== "ingestion") return null;
  if (body.owner_outcome !== "succeeded" && body.owner_outcome !== "partial" && body.owner_outcome !== "failed" && body.owner_outcome !== "unknown") return null;
  const remaining = typeof body.remaining_work === "string" ? body.remaining_work.slice(0, 180) : "";
  return { status: body.owner_outcome, receiptRef: `ingestion:${requestId}`, detail: remaining ? `Ingestion owner receipt: ${remaining}` : "Ingestion owner receipt accepted" };
}
function nextWorker(eventId: string): string { return `space-deletion-ingestion:${eventId}:${crypto.randomUUID()}`; }

export const deliverOne = internalAction({
  args: { workerId: v.string() },
  handler: async (ctx, args) => {
    const claimed = await ctx.runMutation(internal.spaces.claimNextIngestionDeletionOwnerDelivery, { now: Date.now(), workerId: args.workerId });
    if (!claimed) return { status: "idle" as const };
    const release = async (reason: string) => {
      const retryAt = await ctx.runMutation(internal.spaces.releaseIngestionDeletionOwnerDelivery, { eventId: claimed.eventId, workerId: args.workerId, now: Date.now(), error: reason });
      await ctx.scheduler.runAfter(Math.max(0, retryAt - Date.now()), internal.spaceDeletionIngestion.deliverOne, { workerId: nextWorker(claimed.eventId) });
      return { status: "retry_scheduled" as const, retryAt };
    };
    if (!INGESTION_SPACE_DELETION_URL.trim() || !CONTROL_SPACE_DELETION_INGESTION_BEARER.trim()) return release("Ingestion Space deletion endpoint or dedicated bearer is not configured");
    let response: Response;
    try {
      response = await fetch(INGESTION_SPACE_DELETION_URL, {
        method: "POST", headers: { "Authorization": `Bearer ${CONTROL_SPACE_DELETION_INGESTION_BEARER}`, "Content-Type": "application/json" },
        body: JSON.stringify({ deletion_request_id: claimed.request.requestId, space_ref: claimed.request.spaceRef, reason: "Control-authorized Space deletion queued-job cancellation" }),
      });
    } catch { return release("Ingestion Space deletion request failed"); }
    if (response.status !== 200) return release(`Ingestion Space deletion returned HTTP ${response.status}`);
    let receipt: unknown;
    try { receipt = await response.json(); } catch { return release("Ingestion Space deletion returned an invalid receipt"); }
    const outcome = ingestionDeletionReceipt(receipt, claimed.request.requestId);
    if (!outcome) return release("Ingestion Space deletion receipt did not match the claimed request");
    await ctx.runMutation(internal.spaces.acknowledgeIngestionDeletionOwnerDelivery, { eventId: claimed.eventId, workerId: args.workerId, now: Date.now(), ...outcome });
    return { eventId: claimed.eventId, status: outcome.status };
  },
});
