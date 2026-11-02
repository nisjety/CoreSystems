/**
 * A person's hide of a direct message (WS12-S2).
 *
 * Hide, not delete, is the whole "delete a DM" semantics: the DM, its roster
 * and its history stay exactly as they were; only this viewer's sidebar stops
 * listing it. Any accepted post in the DM clears every viewer's row so it
 * resurfaces (the gateway calls `clearForSpaceForGateway` after Session Core
 * accepts the post, best effort).
 *
 * Presentation state like `spaceReadMarkers`: ids and a timestamp, no content,
 * and never an authorization input. It never touches Control membership, and
 * the gateway still proves the caller is a participant (Control's index) before
 * it calls these. Each mutation re-verifies org membership and that the Space
 * is a DM of that organization. Failures throw a stable code the gateway maps
 * to its typed envelope: `space_not_found`, `space_not_hideable`.
 */
import { v } from "convex/values";
import { mutation, query } from "./_generated/server";
import { assertServiceKey, requireGatewayMember } from "./authz";
import { isDmKind } from "./spaceParticipants";

async function requireDmSpace(ctx: any, spaceRef: string, externalOrgId: string) {
  const ref = spaceRef.trim();
  if (!ref) throw new Error("space_not_found");
  const space = await ctx.db
    .query("spaces")
    .withIndex("by_space_ref", (q: any) => q.eq("spaceRef", ref))
    .first();
  // Same answer for an unknown Space and one in another organization.
  if (!space || space.externalOrgId !== externalOrgId) throw new Error("space_not_found");
  if (!isDmKind(space.kind)) throw new Error("space_not_hideable");
  return space;
}

async function viewerRows(ctx: any, spaceRef: string, externalAuthId: string) {
  return await ctx.db
    .query("spaceDmHides")
    .withIndex("by_space_and_viewer", (q: any) =>
      q.eq("spaceRef", spaceRef).eq("externalAuthId", externalAuthId),
    )
    .collect();
}

/** Hide one DM for the caller. Idempotent: a second hide keeps the first time. */
export const hideForGateway = mutation({
  args: {
    serviceKey: v.string(),
    externalAuthId: v.string(),
    externalOrgId: v.string(),
    spaceRef: v.string(),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    await requireGatewayMember(ctx, args.externalAuthId, args.externalOrgId);
    const space = await requireDmSpace(ctx, args.spaceRef, args.externalOrgId);
    const existing = await viewerRows(ctx, space.spaceRef, args.externalAuthId);
    if (existing[0]) return { spaceRef: space.spaceRef, hidden: true, hiddenAt: existing[0].hiddenAt };
    const hiddenAt = Date.now();
    await ctx.db.insert("spaceDmHides", {
      externalOrgId: args.externalOrgId,
      spaceRef: space.spaceRef,
      externalAuthId: args.externalAuthId,
      hiddenAt,
    });
    return { spaceRef: space.spaceRef, hidden: true, hiddenAt };
  },
});

/** Show a hidden DM again for the caller. Idempotent. */
export const unhideForGateway = mutation({
  args: {
    serviceKey: v.string(),
    externalAuthId: v.string(),
    externalOrgId: v.string(),
    spaceRef: v.string(),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    await requireGatewayMember(ctx, args.externalAuthId, args.externalOrgId);
    const space = await requireDmSpace(ctx, args.spaceRef, args.externalOrgId);
    for (const row of await viewerRows(ctx, space.spaceRef, args.externalAuthId)) {
      await ctx.db.delete(row._id);
    }
    return { spaceRef: space.spaceRef, hidden: false };
  },
});

/** Every DM the caller has hidden in this organization: refs and times only. */
export const hiddenForViewerForGateway = query({
  args: {
    serviceKey: v.string(),
    externalAuthId: v.string(),
    externalOrgId: v.string(),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    await requireGatewayMember(ctx, args.externalAuthId, args.externalOrgId);
    const rows = await ctx.db
      .query("spaceDmHides")
      .withIndex("by_viewer", (q: any) =>
        q.eq("externalOrgId", args.externalOrgId).eq("externalAuthId", args.externalAuthId),
      )
      .collect();
    return rows.map((row: any) => ({ spaceRef: row.spaceRef, hiddenAt: row.hiddenAt }));
  },
});

/**
 * New activity in a DM: delete every viewer's hide row so it resurfaces. The
 * gateway calls this after Session Core accepts a post (best effort). The caller
 * must be an organization member and the Space a DM of that organization.
 */
export const clearForSpaceForGateway = mutation({
  args: {
    serviceKey: v.string(),
    externalAuthId: v.string(),
    externalOrgId: v.string(),
    spaceRef: v.string(),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    await requireGatewayMember(ctx, args.externalAuthId, args.externalOrgId);
    const space = await requireDmSpace(ctx, args.spaceRef, args.externalOrgId);
    const rows = await ctx.db
      .query("spaceDmHides")
      .withIndex("by_space", (q: any) => q.eq("spaceRef", space.spaceRef))
      .collect();
    for (const row of rows) await ctx.db.delete(row._id);
    return { spaceRef: space.spaceRef, cleared: rows.length };
  },
});
