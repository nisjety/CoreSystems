/** Durable Application -> Control recipient-audience registration worker. */
import { v } from "convex/values";
import { internalAction } from "./_generated/server";
import { internal } from "./_generated/api";

const CONTROL_SPACE_AUDIENCE_URL = process.env.CONTROL_SPACE_AUDIENCE_URL || "";
const APPLICATION_SPACE_AUDIENCE_TOKEN = process.env.APPLICATION_SPACE_AUDIENCE_TOKEN || "";

function workerRef(eventId: string): string {
  return `space-audience-registration:${eventId}:${crypto.randomUUID()}`;
}

export const deliverOne = internalAction({
  args: { workerId: v.string() },
  handler: async (ctx, args) => {
    const claimed = await ctx.runMutation(internal.spaces.claimNextRecipientAudienceRegistration, { now: Date.now(), workerId: args.workerId });
    if (!claimed) return { status: "idle" as const };
    const release = async (reason: string) => {
      const retryAt = await ctx.runMutation(internal.spaces.releaseRecipientAudienceRegistrationClaim, {
        error: reason, eventId: claimed.eventId, now: Date.now(), workerId: args.workerId,
      });
      await ctx.scheduler.runAfter(Math.max(0, retryAt - Date.now()), internal.spaceAudienceRegistration.deliverOne, { workerId: workerRef(claimed.eventId) });
      return { status: "retry_scheduled" as const };
    };
    if (!CONTROL_SPACE_AUDIENCE_URL.trim() || !APPLICATION_SPACE_AUDIENCE_TOKEN.trim()) {
      return release("recipient audience Control registration is not configured");
    }
    let response: Response;
    try {
      response = await fetch(CONTROL_SPACE_AUDIENCE_URL, {
        method: "POST",
        headers: { "Content-Type": "application/json", "X-Service-Id": "application-space-lifecycle", "X-Service-Token": APPLICATION_SPACE_AUDIENCE_TOKEN },
        body: JSON.stringify({
          audience_ref: claimed.audience.audienceRef, audience_hash: claimed.audience.audienceHash,
          org_id: claimed.audience.externalOrgId, recipients: claimed.audience.recipientExternalAuthIds,
          revision: claimed.audience.revision, space_ref: claimed.audience.spaceRef,
        }),
      });
    } catch { return release("recipient audience Control registration request failed"); }
    if (response.status === 409) {
      await ctx.runMutation(internal.spaces.rejectRecipientAudienceRegistrationClaim, { eventId: claimed.eventId, now: Date.now(), workerId: args.workerId });
      return { status: "rejected" as const };
    }
    if (response.status !== 201) return release(`recipient audience Control registration returned HTTP ${response.status}`);
    let receipt: any;
    try { receipt = await response.json(); } catch { return release("recipient audience Control registration returned an invalid receipt"); }
    const data = receipt?.data;
    if (data?.space_ref !== claimed.audience.spaceRef || data?.audience_ref !== claimed.audience.audienceRef || data?.audience_hash !== claimed.audience.audienceHash || data?.revision !== claimed.audience.revision) {
      return release("recipient audience Control receipt did not match the claimed snapshot");
    }
    await ctx.runMutation(internal.spaces.acknowledgeRecipientAudienceRegistrationClaim, { eventId: claimed.eventId, now: Date.now(), workerId: args.workerId });
    return { status: "acknowledged" as const };
  },
});
