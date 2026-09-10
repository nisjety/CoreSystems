/**
 * People in a named room: grant, revoke, and declare the result to Control.
 *
 * Deliberately separate from `spaceMembershipSync`, which derives the
 * ORGANIZATION room's roster from org-core. That room's membership is a
 * consequence of being in the organization and is not editable; this one is a
 * list somebody maintains. Keeping them in different files keeps the two rules
 * from being mistaken for variants of one thing — they are different products.
 *
 * Both declare with `managed_subject_types: ["user"]`, so neither can revoke a
 * room's agents as a side effect of a human roster change.
 */
import { v } from "convex/values";
import { action, internalMutation, internalQuery, mutation, query } from "./_generated/server";
import { internal } from "./_generated/api";
import { assertServiceKey } from "./authz";
import { requireGatewayMember } from "./authz";

const SERVICE_PRINCIPAL = "application-space-lifecycle";

/** A room whose people are maintained by hand, rather than derived. */
async function requireManagedRoom(ctx: any, spaceRef: string, externalOrgId: string) {
  const space = await ctx.db
    .query("spaces")
    .withIndex("by_space_ref", (q: any) => q.eq("spaceRef", spaceRef))
    .unique();
  if (!space || space.externalOrgId !== externalOrgId) {
    throw new Error("Space not found");
  }
  if (space.kind === "personal") {
    // Control refuses to replace a personal Space's membership at all, and it
    // is right to: a personal room's content, credentials and memory are the
    // one member's. Failing here rather than at Control keeps the reason
    // legible.
    throw new Error("A personal Space cannot have members added");
  }
  if (space.isOrganizationRoom === true) {
    throw new Error(
      "The organization room's members come from the organization, not from this list",
    );
  }
  return space;
}

export const addSpaceMemberForGateway = mutation({
  args: {
    serviceKey: v.string(),
    externalAuthId: v.string(),
    externalOrgId: v.string(),
    spaceRef: v.string(),
    memberExternalAuthId: v.string(),
  },
  handler: async (ctx, args): Promise<{ added: boolean; memberCount: number }> => {
    assertServiceKey(args.serviceKey);
    const organization = await requireGatewayMember(ctx, args.externalAuthId, args.externalOrgId);
    const spaceRef = args.spaceRef.trim();
    const member = args.memberExternalAuthId.trim();
    if (!spaceRef || !member) throw new Error("A Space reference and a member are required");
    await requireManagedRoom(ctx, spaceRef, args.externalOrgId);

    // The person must already be in the organization. A room grant is not a
    // way into the tenant, and Control would refuse the roster anyway — this
    // just refuses it where the reason can be said plainly.
    const users = await ctx.db
      .query("users")
      .withIndex("by_external_and_org", (q: any) =>
        q.eq("externalAuthId", member).eq("orgId", organization._id),
      )
      .collect();
    if (!users.some((candidate: any) => candidate.syncStatus !== "deleted")) {
      throw new Error("That person is not a member of this organization");
    }

    const existing = await ctx.db
      .query("spaceMemberGrants")
      .withIndex("by_space_and_subject", (q: any) =>
        q.eq("spaceRef", spaceRef).eq("externalAuthId", member),
      )
      .unique();
    if (!existing) {
      await ctx.db.insert("spaceMemberGrants", {
        spaceRef,
        externalOrgId: args.externalOrgId,
        externalAuthId: member,
        role: "editor",
        grantedByExternalAuthId: args.externalAuthId,
        createdAt: Date.now(),
      });
    }
    const all = await ctx.db
      .query("spaceMemberGrants")
      .withIndex("by_space_ref", (q: any) => q.eq("spaceRef", spaceRef))
      .collect();
    return { added: !existing, memberCount: all.length };
  },
});

export const removeSpaceMemberForGateway = mutation({
  args: {
    serviceKey: v.string(),
    externalAuthId: v.string(),
    externalOrgId: v.string(),
    spaceRef: v.string(),
    memberExternalAuthId: v.string(),
  },
  handler: async (ctx, args): Promise<{ removed: boolean; memberCount: number }> => {
    assertServiceKey(args.serviceKey);
    await requireGatewayMember(ctx, args.externalAuthId, args.externalOrgId);
    const spaceRef = args.spaceRef.trim();
    const member = args.memberExternalAuthId.trim();
    if (!spaceRef || !member) throw new Error("A Space reference and a member are required");
    const space = await requireManagedRoom(ctx, spaceRef, args.externalOrgId);

    // The registered owner stays. Control keeps them as the room's owner
    // regardless, so removing the grant would make this list disagree with the
    // roster it is supposed to describe.
    if (space.ownerExternalAuthId === member) {
      throw new Error("The room's owner cannot be removed from it");
    }

    const existing = await ctx.db
      .query("spaceMemberGrants")
      .withIndex("by_space_and_subject", (q: any) =>
        q.eq("spaceRef", spaceRef).eq("externalAuthId", member),
      )
      .unique();
    if (existing) await ctx.db.delete(existing._id);
    const all = await ctx.db
      .query("spaceMemberGrants")
      .withIndex("by_space_ref", (q: any) => q.eq("spaceRef", spaceRef))
      .collect();
    return { removed: Boolean(existing), memberCount: all.length };
  },
});

/** The full declarative user roster for one managed room: its owner plus every
 * granted member. Not a delta — Control's endpoint converges on exactly this. */
export const roomMembersForControl = internalQuery({
  args: { spaceRef: v.string(), externalOrgId: v.string() },
  handler: async (ctx, args) => {
    const space = await ctx.db
      .query("spaces")
      .withIndex("by_space_ref", (q: any) => q.eq("spaceRef", args.spaceRef))
      .unique();
    if (!space || space.externalOrgId !== args.externalOrgId) return [];
    const grants = await ctx.db
      .query("spaceMemberGrants")
      .withIndex("by_space_ref", (q: any) => q.eq("spaceRef", args.spaceRef))
      .collect();
    const subjects = new Map<string, { subject_type: "user"; subject_id: string; role: string }>();
    if (space.ownerExternalAuthId) {
      subjects.set(space.ownerExternalAuthId, {
        subject_type: "user",
        subject_id: space.ownerExternalAuthId,
        role: "owner",
      });
    }
    for (const grant of grants) {
      if (subjects.has(grant.externalAuthId)) continue;
      subjects.set(grant.externalAuthId, {
        subject_type: "user",
        subject_id: grant.externalAuthId,
        role: grant.role,
      });
    }
    return [...subjects.values()];
  },
});

/** Everyone this room's list names, for the room's own Members tab. */
export const roomMemberGrantsForGateway = query({
  args: {
    serviceKey: v.string(),
    externalAuthId: v.string(),
    externalOrgId: v.string(),
    spaceRef: v.string(),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    await requireGatewayMember(ctx, args.externalAuthId, args.externalOrgId);
    const grants = await ctx.db
      .query("spaceMemberGrants")
      .withIndex("by_space_ref", (q: any) => q.eq("spaceRef", args.spaceRef.trim()))
      .collect();
    return grants
      .filter((grant: any) => grant.externalOrgId === args.externalOrgId)
      .map((grant: any) => ({
        externalAuthId: grant.externalAuthId as string,
        role: grant.role as string,
        createdAt: grant.createdAt as number,
      }));
  },
});

/**
 * Declare this room's people to Control.
 *
 * Called after every grant or revocation. Idempotent by construction: the
 * endpoint is declarative, so re-running with an unchanged roster changes
 * nothing and does not advance the membership revision.
 *
 * A failure leaves Application's list ahead of Control's roster, which is the
 * safe direction — the person is not yet a member and the room says so, rather
 * than the room claiming a membership Control never granted.
 */
export const syncRoomMembersForGateway = action({
  args: {
    serviceKey: v.string(),
    externalOrgId: v.string(),
    spaceRef: v.string(),
  },
  handler: async (ctx, args): Promise<{ status: string; httpStatus?: number }> => {
    assertServiceKey(args.serviceKey);
    const membershipBaseUrl = (process.env.CONTROL_SPACE_MEMBERSHIP_BASE_URL || "").trim();
    const lifecycleToken = (process.env.APPLICATION_SPACE_LIFECYCLE_TOKEN || "").trim();
    if (!membershipBaseUrl || !lifecycleToken) {
      return { status: "not_configured" };
    }

    const members: { subject_type: string; subject_id: string; role: string }[] =
      await ctx.runQuery(internal.spaceMembers.roomMembersForControl, {
        spaceRef: args.spaceRef,
        externalOrgId: args.externalOrgId,
      });
    // An empty set would revoke everyone. The owner is always present for a
    // real room, so an empty answer means the room was not found rather than
    // that it has nobody — refuse instead of converging on nothing.
    if (members.length === 0) {
      return { status: "no_roster" };
    }

    let applied: Response;
    try {
      applied = await fetch(
        `${membershipBaseUrl.replace(/\/$/, "")}/${encodeURIComponent(args.spaceRef)}/memberships`,
        {
          method: "PUT",
          headers: {
            "Content-Type": "application/json",
            "X-Service-Id": SERVICE_PRINCIPAL,
            "X-Service-Token": lifecycleToken,
          },
          body: JSON.stringify({ members, managed_subject_types: ["user"] }),
        },
      );
    } catch {
      return { status: "control_unavailable" };
    }
    if (!applied.ok) {
      return { status: "rejected", httpStatus: applied.status };
    }
    return { status: "applied" };
  },
});
