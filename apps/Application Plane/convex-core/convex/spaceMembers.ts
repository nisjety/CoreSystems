/**
 * Room audience maintenance for named rooms.
 *
 * WS04-S12: this file used to hold Application's own member list for named
 * rooms (`spaceMemberGrants`, the add/remove mutations and the declarative
 * sync to Control). That list is retired. Control's membership ledger is the
 * only roster for a named room and the gateway writes it through Control's
 * source-tagged writer (WS04-S6); the organization room is still derived from
 * org-core by `spaceMembershipSync`. What remains here is the audience
 * reconcile the gateway triggers after a Control membership write.
 */
import { v } from "convex/values";
import { action } from "./_generated/server";
import { internal } from "./_generated/api";
import { assertServiceKey } from "./authz";

/**
 * Bring the room's recipient audience up to date after a membership write
 * (WS04-S5). The gateway calls this after each Control membership change so the
 * person added or removed is reflected in the next audience revision without
 * waiting for the 10-minute cron. The Space must belong to the caller's
 * organization; the plan itself is derived from Control's candidates, never
 * from anything the caller sends. Flag-gated and idempotent.
 */
export const reconcileRoomAudienceForGateway = action({
  args: {
    serviceKey: v.string(),
    externalOrgId: v.string(),
    spaceRef: v.string(),
  },
  handler: async (ctx, args): Promise<{ status: string }> => {
    assertServiceKey(args.serviceKey);
    const current: any = await ctx.runQuery(internal.spaces.activeAudienceForReconcile, {
      spaceRef: args.spaceRef.trim(),
    });
    // Same answer for an unknown Space and one in another organization.
    if (!current.space || current.space.externalOrgId !== args.externalOrgId) {
      return { status: "skipped" };
    }
    const result: { status: string } = await ctx.runAction(
      internal.spaceAudienceReconcile.reconcileRoomAudience,
      { spaceRef: args.spaceRef.trim() },
    );
    return { status: result.status };
  },
});
