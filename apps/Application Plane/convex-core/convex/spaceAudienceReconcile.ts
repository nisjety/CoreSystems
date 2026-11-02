/**
 * Recipient audience follows the roster (WS04-S5).
 *
 * Control owns who is a member of a Space. Application owns the versioned
 * recipient snapshot that the registration worker (`spaceAudienceRegistration`)
 * delivers to Control, and that Control checks every shared decision against.
 * Nothing used to call `spaces.replaceRecipientAudience`, so no shared room
 * obtained an audience unless an operator ran it by hand. This module reads
 * Control's audience candidates for a Space and applies the plan from
 * `spaceAudiencePlan` through that mutation's compare-and-set.
 *
 * Control still verifies every recipient independently; this action grants
 * nothing. It is idempotent: with an unchanged roster it does nothing, so it is
 * called from every trigger (room activation, after the org-room sync, after a
 * gateway membership write) and from a 10-minute cron that heals a lost one.
 *
 * Kill switch `space_audience_reconcile_sync` (`SPACE_AUDIENCE_RECONCILE`) is
 * read at handler time. It is a rollout switch, never an authorization input.
 * Statuses carry ids and counts only.
 */
import { v } from "convex/values";
import { internalAction } from "./_generated/server";
import { internal } from "./_generated/api";
import { planAudienceReconcile } from "./spaceAudiencePlan";

const SERVICE_PRINCIPAL = "application-space-lifecycle";

export function audienceReconcileEnabled(): boolean {
  const raw = (process.env.SPACE_AUDIENCE_RECONCILE || "").trim().toLowerCase();
  return raw === "1" || raw === "true";
}

export type AudienceReconcileStatus =
  | { status: "disabled" }
  | { status: "not_configured"; reason: string }
  | { status: "skipped"; reason: "space_not_found" | "space_not_active" | "personal_space" | "control_state" }
  | { status: "control_unavailable"; httpStatus?: number }
  | { status: "candidates_invalid" }
  | { status: "noop" }
  | { status: "waiting" }
  | { status: "refused"; reason: "empty_candidates" | "audience_too_large" }
  | { status: "replaced"; revision: number; recipients: number }
  | { status: "conflict" }
  | { status: "projection_lag" }
  | { status: "failed" };

function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

async function fetchCandidates(
  baseUrl: string,
  token: string,
  spaceRef: string,
): Promise<
  | { status: "ok"; orgId: string; state: string; recipients: unknown[] }
  | { status: "control_unavailable"; httpStatus?: number }
  | { status: "candidates_invalid" }
> {
  let response: Response;
  try {
    response = await fetch(
      `${baseUrl.replace(/\/$/, "")}/${encodeURIComponent(spaceRef)}/audience-candidates`,
      { headers: { "X-Service-Id": SERVICE_PRINCIPAL, "X-Service-Token": token } },
    );
  } catch {
    return { status: "control_unavailable" };
  }
  if (!response.ok) return { status: "control_unavailable", httpStatus: response.status };
  let body: any;
  try {
    body = await response.json();
  } catch {
    return { status: "candidates_invalid" };
  }
  const data = body?.data;
  // An unreadable candidate list is never an empty one: the plan refuses an
  // empty audience, and a malformed body must not even reach the plan.
  if (
    !data ||
    typeof data.org_id !== "string" ||
    typeof data.state !== "string" ||
    !Array.isArray(data.members)
  ) {
    return { status: "candidates_invalid" };
  }
  return {
    status: "ok",
    orgId: data.org_id,
    state: data.state,
    recipients: data.members.map((member: any) => member?.subject_id),
  };
}

export const reconcileRoomAudience = internalAction({
  args: { spaceRef: v.string() },
  handler: async (ctx, args): Promise<AudienceReconcileStatus> => {
    if (!audienceReconcileEnabled()) return { status: "disabled" };
    const baseUrl = (process.env.CONTROL_SPACE_MEMBERSHIP_BASE_URL || "").trim();
    const token = (process.env.APPLICATION_SPACE_LIFECYCLE_TOKEN || "").trim();
    if (!baseUrl || !token) {
      console.warn("space audience reconcile skipped: Control membership URL or token is not configured");
      return { status: "not_configured", reason: "control_membership_not_configured" };
    }
    const spaceRef = args.spaceRef.trim();
    if (!spaceRef) return { status: "skipped", reason: "space_not_found" };

    const read = async () => {
      const current: any = await ctx.runQuery(internal.spaces.activeAudienceForReconcile, { spaceRef });
      return current;
    };
    let current = await read();
    if (!current.space) return { status: "skipped", reason: "space_not_found" };
    if (current.space.kind === "personal") return { status: "skipped", reason: "personal_space" };
    if (current.space.lifecycle !== "active") return { status: "skipped", reason: "space_not_active" };

    const candidates = await fetchCandidates(baseUrl, token, spaceRef);
    if (candidates.status === "control_unavailable") {
      return { status: "control_unavailable", httpStatus: candidates.httpStatus };
    }
    if (candidates.status === "candidates_invalid") return { status: "candidates_invalid" };
    // Control has not registered the Space active (or it moved on): its audience
    // read means nothing to Application yet. Registration and the cron retry.
    if (candidates.state !== "active" || candidates.orgId !== current.space.externalOrgId) {
      return { status: "skipped", reason: "control_state" };
    }

    // One retry on a compare-and-set conflict: another trigger registered a new
    // revision between our read and our write.
    for (let attempt = 0; attempt < 2; attempt += 1) {
      const plan = planAudienceReconcile({ candidates: candidates.recipients, active: current.audience });
      if (plan.action === "noop") return { status: "noop" };
      if (plan.action === "wait") return { status: "waiting" };
      if (plan.action === "refuse") {
        console.warn(`space audience reconcile refused: ${plan.reason}`);
        return { status: "refused", reason: plan.reason };
      }
      try {
        const registered: any = await ctx.runMutation(internal.spaces.replaceRecipientAudience, {
          spaceRef,
          recipientExternalAuthIds: plan.recipients,
          ...(plan.expectedRevision === undefined ? {} : { expectedRevision: plan.expectedRevision }),
        });
        return { status: "replaced", revision: registered.revision, recipients: plan.recipients.length };
      } catch (error) {
        const message = errorText(error);
        if (message.includes("recipient audience revision conflict") ||
            message.includes("recipient audience replacement requires expected revision")) {
          current = await read();
          continue;
        }
        // The org-core projection into Application has not caught up with
        // Control for at least one person. The cron retries; nobody is added
        // to an audience Application cannot vouch for.
        if (message.includes("not an active Application organization member")) {
          return { status: "projection_lag" };
        }
        if (message.includes("active Space required")) {
          return { status: "skipped", reason: "space_not_active" };
        }
        console.warn(`space audience reconcile failed: ${message.slice(0, 200)}`);
        return { status: "failed" };
      }
    }
    return { status: "conflict" };
  },
});

/** Reconcile every active shared Space. Safe to call repeatedly; one Space failing never stops the rest. */
export const reconcileAllRoomAudiences = internalAction({
  args: {},
  handler: async (ctx) => {
    if (!audienceReconcileEnabled()) return { status: "disabled" as const, rooms: 0 };
    const spaces: { spaceRef: string }[] = await ctx.runQuery(internal.spaces.listActiveSharedSpaces, {});
    let replaced = 0;
    for (const space of spaces) {
      try {
        const result: AudienceReconcileStatus = await ctx.runAction(
          internal.spaceAudienceReconcile.reconcileRoomAudience,
          { spaceRef: space.spaceRef },
        );
        if (result.status === "replaced") replaced += 1;
      } catch (error) {
        console.warn(`space audience reconcile threw: ${errorText(error).slice(0, 200)}`);
      }
    }
    return { status: "ok" as const, rooms: spaces.length, replaced };
  },
});
