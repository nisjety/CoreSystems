/**
 * Application -> Control Space registration delivery.
 *
 * This is intentionally an at-least-once worker: Control deduplicates the
 * immutable `(space_ref, lifecycle_revision)` registration tuple. No browser
 * request can invoke it.
 */
import { v } from "convex/values";
import { internalAction } from "./_generated/server";
import { internal } from "./_generated/api";
import { controlRegistrationResourceRef } from "./spaceLifecycle";

const CONTROL_SPACE_REGISTRATION_URL = process.env.CONTROL_SPACE_REGISTRATION_URL || "";
const APPLICATION_SPACE_LIFECYCLE_TOKEN = process.env.APPLICATION_SPACE_LIFECYCLE_TOKEN || "";

function configurationError(): string | null {
  if (!CONTROL_SPACE_REGISTRATION_URL.trim()) return "CONTROL_SPACE_REGISTRATION_URL is not configured";
  if (!APPLICATION_SPACE_LIFECYCLE_TOKEN.trim()) return "APPLICATION_SPACE_LIFECYCLE_TOKEN is not configured";
  return null;
}

function deliveryWorkerRef(eventId: string): string {
  return `space-registration:${eventId}:${crypto.randomUUID()}`;
}

export const deliverOne = internalAction({
  args: { workerId: v.string() },
  handler: async (ctx, args) => {
    const now = Date.now();
    const claimed = await ctx.runMutation(internal.spaces.claimNextRegistration, {
      now,
      workerId: args.workerId,
    });
    if (!claimed) return { status: "idle" as const };

    const release = async (reason: string) => {
      const retryAt = await ctx.runMutation(internal.spaces.releaseRegistrationClaim, {
        error: reason,
        eventId: claimed.event.eventId,
        now: Date.now(),
        workerId: args.workerId,
      });
      await ctx.scheduler.runAfter(
        Math.max(0, retryAt - Date.now()),
        internal.spaceRegistration.deliverOne,
        { workerId: deliveryWorkerRef(claimed.event.eventId) },
      );
      return { retryAt, status: "retry_scheduled" as const };
    };

    const misconfigured = configurationError();
    if (misconfigured) return release(misconfigured);

    let response: Response;
    try {
      response = await fetch(CONTROL_SPACE_REGISTRATION_URL, {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          "X-Service-Id": "application-space-lifecycle",
          "X-Service-Token": APPLICATION_SPACE_LIFECYCLE_TOKEN,
        },
        body: JSON.stringify({
          space_ref: claimed.event.spaceRef,
          org_id: claimed.event.externalOrgId,
          kind: claimed.space.kind,
	          lifecycle: claimed.event.lifecycle,
          lifecycle_revision: claimed.event.revision,
          owner_principal_id: claimed.space.ownerPrincipalId,
        }),
      });
    } catch {
      return release("Control Space registration request failed");
    }
    if (response.status !== 201) {
      if (response.status === 409) {
        await ctx.runMutation(internal.spaces.rejectRegistrationClaim, {
          eventId: claimed.event.eventId,
          now: Date.now(),
          workerId: args.workerId,
        });
        return { eventId: claimed.event.eventId, status: "rejected" as const };
      }
      return release(`Control Space registration returned HTTP ${response.status}`);
    }

    let receipt: unknown;
    try {
      receipt = await response.json();
    } catch {
      return release("Control Space registration returned an invalid receipt");
    }
    let controlResourceRef: string;
    try {
      controlResourceRef = controlRegistrationResourceRef(receipt, claimed.event.spaceRef);
    } catch {
      return release("Control Space registration receipt did not match the claimed Space");
    }

    await ctx.runMutation(internal.spaces.acknowledgeRegistrationClaim, {
      controlResourceRef,
      eventId: claimed.event.eventId,
      now: Date.now(),
      workerId: args.workerId,
    });
    return { eventId: claimed.event.eventId, status: "acknowledged" as const };
  },
});
