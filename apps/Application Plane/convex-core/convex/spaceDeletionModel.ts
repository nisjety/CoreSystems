/**
 * Application -> Model deletion adapter. The two credentials are Control-minted
 * service identities for different audiences; neither is an Application key or
 * a user bearer. A remote timeout remains a leased retry and never becomes a
 * successful deletion receipt. A typed legal-hold refusal is a definitive answer,
 * so it parks the delivery instead of retrying it. With SPACE_DELETION_PERMANENT_REFUSAL_PARK
 * on, a permanent refusal (400, 401, 403, 412) parks it as `failed_permanent`; with it off
 * that refusal retries as before. Any throw after the claim releases it for retry: deletion
 * deliveries have no sweep to pick up an abandoned claim.
 */
import { v } from "convex/values";
import { internalAction } from "./_generated/server";
import { internal } from "./_generated/api";
import { isOwnerLegalHoldRefusal, isPermanentOwnerRefusal, permanentRefusalParkEnabled } from "./spaceDeletionReceipts";

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

/**
 * The gateway's typed refusal for an erase that a legal hold covers: the exact
 * code with `details.retryable: false`. Only those fields are read. The upstream
 * message is never read or stored, and any other body is not a hold.
 */
export function isModelLegalHoldRefusal(body: unknown): boolean {
  return isOwnerLegalHoldRefusal(body);
}

async function isLegalHoldRefusal(response: Response): Promise<boolean> {
  try {
    return isModelLegalHoldRefusal(await response.json());
  } catch {
    return false;
  }
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
    const blockByLegalHold = async () => {
      await ctx.runMutation(internal.spaces.blockModelDeletionOwnerDeliveryForLegalHold, {
        eventId: claimed.eventId, workerId: args.workerId, now: Date.now(),
      });
      return { eventId: claimed.eventId, status: "blocked_legal_hold" as const };
    };
    const failPermanently = async (reason: string) => {
      await ctx.runMutation(internal.spaces.failModelDeletionOwnerDeliveryPermanently, {
        eventId: claimed.eventId, workerId: args.workerId, now: Date.now(), reason,
      });
      return { eventId: claimed.eventId, status: "failed_permanent" as const };
    };
    try {
      const misconfigured = configurationError();
      if (misconfigured) return await release(misconfigured);
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
        return await release("Model Space deletion request failed");
      }
      if (response.status !== 200) {
        if (await isLegalHoldRefusal(response)) return await blockByLegalHold();
        if (permanentRefusalParkEnabled() && isPermanentOwnerRefusal(response.status)) {
          return await failPermanently(`permanent_refusal_http_${response.status}`);
        }
        return await release(`Model Space deletion returned HTTP ${response.status}`);
      }
      let receipt: unknown;
      try { receipt = await response.json(); } catch { return await release("Model Space deletion returned an invalid receipt"); }
      const outcome = modelDeletionReceipt(receipt, claimed.request.requestId);
      if (!outcome) return await release("Model Space deletion receipt did not match the claimed request");
      await ctx.runMutation(internal.spaces.acknowledgeModelDeletionOwnerDelivery, {
        eventId: claimed.eventId, workerId: args.workerId, now: Date.now(), ...outcome,
      });
      return { eventId: claimed.eventId, status: outcome.status };
    } catch {
      // A throw after the claim (a failed mutation, a receipt the state no longer
      // accepts) must not leave the delivery claimed: only a release schedules the
      // next attempt. The logged text is fixed, so no upstream body is recorded.
      console.error("[Space deletion] Model delivery failed after the claim; releasing it for retry");
      return await release("Model Space deletion delivery failed before an owner receipt was recorded");
    }
  },
});
