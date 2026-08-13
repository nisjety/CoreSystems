/**
 * Application -> Model deletion adapter. The two credentials are Control-minted
 * service identities for different audiences; neither is an Application key or
 * a user bearer. A remote timeout remains a leased retry and never becomes a
 * successful deletion receipt.
 */
import { v } from "convex/values";
import { internalAction } from "./_generated/server";
import { internal } from "./_generated/api";

const MODEL_SPACE_DELETION_URL = process.env.MODEL_SPACE_DELETION_URL || "";
const CONTROL_SPACE_DELETION_MODEL_BEARER = process.env.CONTROL_SPACE_DELETION_MODEL_BEARER || "";
const CONTROL_SPACE_DELETION_SESSION_BEARER = process.env.CONTROL_SPACE_DELETION_SESSION_BEARER || "";
const CONTROL_SPACE_DELETION_CAPABILITY_BEARER = process.env.CONTROL_SPACE_DELETION_CAPABILITY_BEARER || "";

type ModelOutcome = "succeeded" | "partial" | "failed" | "unknown";

export function modelDeletionReceipt(receipt: unknown, requestId: string): { status: ModelOutcome; receiptRef: string; detail: string } | null {
  if (!receipt || typeof receipt !== "object") return null;
  const body = receipt as { request_id?: unknown; owner_plane?: unknown; owner_outcome?: unknown; remaining_work?: unknown };
  if (body.request_id !== requestId || body.owner_plane !== "model") return null;
  if (body.owner_outcome !== "succeeded" && body.owner_outcome !== "partial" && body.owner_outcome !== "failed" && body.owner_outcome !== "unknown") return null;
  const remaining = Array.isArray(body.remaining_work) ? body.remaining_work.filter((value) => typeof value === "string").slice(0, 4) : [];
  return {
    status: body.owner_outcome,
    receiptRef: `model:${requestId}`,
    detail: remaining.length > 0 ? `Model owner receipt: ${remaining.join(", ")}` : "Model owner receipt accepted",
  };
}

function configurationError(): string | null {
  if (!MODEL_SPACE_DELETION_URL.trim()) return "MODEL_SPACE_DELETION_URL is not configured";
  if (!CONTROL_SPACE_DELETION_MODEL_BEARER.trim()) return "CONTROL_SPACE_DELETION_MODEL_BEARER is not configured";
  if (!CONTROL_SPACE_DELETION_SESSION_BEARER.trim()) return "CONTROL_SPACE_DELETION_SESSION_BEARER is not configured";
  if (!CONTROL_SPACE_DELETION_CAPABILITY_BEARER.trim()) return "CONTROL_SPACE_DELETION_CAPABILITY_BEARER is not configured";
  return null;
}

function nextWorker(eventId: string): string {
  return `space-deletion-model:${eventId}:${crypto.randomUUID()}`;
}

export const deliverOne = internalAction({
  args: { workerId: v.string() },
  handler: async (ctx, args) => {
    const claimed = await ctx.runMutation(internal.spaces.claimNextModelDeletionOwnerDelivery, {
      now: Date.now(), workerId: args.workerId,
    });
    if (!claimed) return { status: "idle" as const };
    const release = async (reason: string) => {
      const retryAt = await ctx.runMutation(internal.spaces.releaseModelDeletionOwnerDelivery, {
        eventId: claimed.eventId, workerId: args.workerId, now: Date.now(), error: reason,
      });
      await ctx.scheduler.runAfter(Math.max(0, retryAt - Date.now()), internal.spaceDeletionModel.deliverOne, {
        workerId: nextWorker(claimed.eventId),
      });
      return { status: "retry_scheduled" as const, retryAt };
    };
    const misconfigured = configurationError();
    if (misconfigured) return release(misconfigured);
    let response: Response;
    try {
      response = await fetch(MODEL_SPACE_DELETION_URL, {
        method: "POST",
        headers: {
          "Authorization": `Bearer ${CONTROL_SPACE_DELETION_MODEL_BEARER}`,
          "Content-Type": "application/json",
          "X-Session-Authorization": `Bearer ${CONTROL_SPACE_DELETION_SESSION_BEARER}`,
          "X-Capability-Authorization": `Bearer ${CONTROL_SPACE_DELETION_CAPABILITY_BEARER}`,
        },
        body: JSON.stringify({
          org_id: claimed.request.externalOrgId,
          space_id: claimed.request.spaceRef,
          owner_principal_id: claimed.request.ownerExternalAuthId,
          deletion_request_id: claimed.request.requestId,
        }),
      });
    } catch {
      return release("Model Space deletion request failed");
    }
    if (response.status !== 200) return release(`Model Space deletion returned HTTP ${response.status}`);
    let receipt: unknown;
    try { receipt = await response.json(); } catch { return release("Model Space deletion returned an invalid receipt"); }
    const outcome = modelDeletionReceipt(receipt, claimed.request.requestId);
    if (!outcome) return release("Model Space deletion receipt did not match the claimed request");
    await ctx.runMutation(internal.spaces.acknowledgeModelDeletionOwnerDelivery, {
      eventId: claimed.eventId, workerId: args.workerId, now: Date.now(), ...outcome,
    });
    return { eventId: claimed.eventId, status: outcome.status };
  },
});
