import { v } from "convex/values";
import { internalMutation } from "./_generated/server";
import { normalizeProjectionRole } from "./membershipProjection";
import { planMembershipReconciliation } from "./reconciliation";

export const reconcile = internalMutation({
  args: {
    externalOrgId: v.string(),
    authoritativeMembers: v.array(
      v.object({ userId: v.string(), role: v.string() })
    ),
    observedAt: v.number(),
    apply: v.boolean(),
  },
  handler: async (ctx, args) => {
    if (!args.externalOrgId.trim()) throw new Error("externalOrgId is required");
    if (!Number.isFinite(args.observedAt) || args.observedAt <= 0) {
      throw new Error("observedAt must be a valid timestamp");
    }
    if (args.observedAt > Date.now() + 5 * 60 * 1000) {
      throw new Error("observedAt cannot be in the future");
    }

    const organizations = await ctx.db
      .query("organizations")
      .withIndex("by_external_id", (q) =>
        q.eq("externalOrgId", args.externalOrgId)
      )
      .collect();
    const organization = organizations.find((item) => item.syncStatus !== "deleted");
    if (!organization) throw new Error("Organization projection not found");

    const memberships = await ctx.db
      .query("users")
      .withIndex("by_org", (q) => q.eq("orgId", organization._id))
      .collect();
    const byExternalUser = new Map<string, any>(
      memberships.map((membership: any) => [membership.externalAuthId, membership])
    );
    const plan = planMembershipReconciliation(
      memberships.map((membership: any) => ({
        userId: membership.externalAuthId,
        role: membership.role,
        syncStatus: membership.syncStatus,
      })),
      args.authoritativeMembers
    );
    const compensation = [
      ...plan.removals.map((userId) => {
        const membership = byExternalUser.get(userId);
        return {
          userId,
          previousRole: membership?.role,
          previousSyncStatus: membership?.syncStatus,
          previousSourceUpdatedAt: membership?.sourceUpdatedAt,
        };
      }),
      ...plan.roleChanges.map((change) => {
        const membership = byExternalUser.get(change.userId);
        return {
          userId: change.userId,
          previousRole: membership?.role,
          previousSyncStatus: membership?.syncStatus,
          previousSourceUpdatedAt: membership?.sourceUpdatedAt,
        };
      }),
    ];

    if (!args.apply) {
      return { dryRun: true, applied: false, plan, compensation };
    }

    const staleSkipped: string[] = [];
    for (const userId of plan.removals) {
      const membership = byExternalUser.get(userId);
      if (!membership) continue;
      if ((membership.sourceUpdatedAt ?? 0) > args.observedAt) {
        staleSkipped.push(userId);
        continue;
      }

      await ctx.db.patch(membership._id, {
        syncStatus: "deleted",
        deletedAt: args.observedAt,
        lastSyncedAt: args.observedAt,
        sourceUpdatedAt: args.observedAt,
      });
      const controlSessions = await ctx.db
        .query("controlSessions")
        .withIndex("by_external_user_and_org", (q) =>
          q.eq("externalUserId", userId).eq("externalOrgId", args.externalOrgId)
        )
        .collect();
      for (const session of controlSessions) await ctx.db.delete(session._id);
      const tombstones = await ctx.db
        .query("membershipTombstones")
        .withIndex("by_external_org_and_user", (q) =>
          q.eq("externalOrgId", args.externalOrgId).eq("externalAuthId", userId)
        )
        .collect();
      const [first, ...duplicates] = tombstones;
      if (first) {
        await ctx.db.patch(first._id, {
          sourceUpdatedAt: args.observedAt,
          removedAt: args.observedAt,
        });
      } else {
        await ctx.db.insert("membershipTombstones", {
          externalOrgId: args.externalOrgId,
          externalAuthId: userId,
          sourceUpdatedAt: args.observedAt,
          removedAt: args.observedAt,
        });
      }
      for (const duplicate of duplicates) await ctx.db.delete(duplicate._id);
    }

    for (const change of plan.roleChanges) {
      const membership = byExternalUser.get(change.userId);
      if (!membership) continue;
      if ((membership.sourceUpdatedAt ?? 0) > args.observedAt) {
        staleSkipped.push(change.userId);
        continue;
      }
      await ctx.db.patch(membership._id, {
        role: normalizeProjectionRole(change.nextRole),
        lastSyncedAt: args.observedAt,
        sourceUpdatedAt: args.observedAt,
      });
    }

    await ctx.db.insert("auditLog", {
      orgId: organization._id,
      action: "membership.projection.reconciled",
      resource: "organization_memberships",
      resourceId: args.externalOrgId,
      changes: {
        removals: plan.removals,
        roleChanges: plan.roleChanges,
        unsafePromotionsReportedOnly: plan.unsafePromotions,
        missingReportedOnly: plan.missing,
        staleSkipped,
      },
      createdAt: Date.now(),
    });

    return {
      dryRun: false,
      applied: true,
      plan,
      staleSkipped: staleSkipped.sort(),
      compensation,
    };
  },
});
