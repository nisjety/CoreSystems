/**
 * A member's personal Spaces sidebar layout (WS16-S7).
 *
 * # Application owns this, as a workspace projection
 *
 * Which rooms a person starred, grouped or hid is neither identity nor
 * authorization: Control still decides who can open a room, and nothing in an
 * authority path reads this table. It is the same class of data as
 * `spaceReadMarkers`: one row per (organization, member), refs and short
 * labels only, never room names, thread ids or content.
 *
 * # Shape of the calls
 *
 * Both functions are the service-key gateway path: `assertServiceKey` proves
 * the caller is the gateway, `requireGatewayMember` proves the subject belongs
 * to the organization. The row is addressed by the caller's own
 * `(externalOrgId, externalAuthId)`; there is no way to name another member's
 * row. The gateway takes the identity from the session, never from the body.
 *
 * `putForGateway` is compare-and-set on `revision` (see `decideSidebarPut`).
 * The handlers are thin shells around the pure rules in `spaceSidebarRules.ts`.
 *
 * # Orphan sweep (WS16-S11)
 *
 * A row outlives its owner when the person leaves the organization or the
 * organization is deleted. Such a row is already inert (`requireGatewayMember`
 * refuses its owner), so the sweep is housekeeping, not an authority control.
 * `sweepOrphans` is an internal mutation run by `crons.ts`; it walks the table
 * in bounded pages and deletes rows whose `(externalOrgId, externalAuthId)` has
 * no non-deleted membership, using the same test as `requireGatewayMember`.
 */
import { v } from "convex/values";
import { internal } from "./_generated/api";
import { internalMutation, mutation, query } from "./_generated/server";
import { assertServiceKey, requireGatewayMember } from "./authz";
import { spaceSidebarDocValidator } from "./schema/spaces";
import { decideSidebarPut, validateSidebarDoc } from "./spaceSidebarRules";

async function loadRow(ctx: any, externalOrgId: string, externalAuthId: string) {
  return ctx.db
    .query("spaceSidebarPreferences")
    .withIndex("by_subject", (q: any) =>
      q.eq("externalOrgId", externalOrgId).eq("externalAuthId", externalAuthId),
    )
    .unique();
}

/** The caller's own sidebar document, or `null` when they have never saved one. */
export const getForGateway = query({
  args: {
    serviceKey: v.string(),
    externalAuthId: v.string(),
    externalOrgId: v.string(),
  },
  handler: async (ctx, args): Promise<{ revision: number; doc: unknown } | null> => {
    assertServiceKey(args.serviceKey);
    await requireGatewayMember(ctx, args.externalAuthId, args.externalOrgId);
    const row = await loadRow(ctx, args.externalOrgId, args.externalAuthId);
    if (!row) return null;
    return { revision: row.revision, doc: row.doc };
  },
});

/**
 * Save the caller's sidebar document if `baseRevision` is still current.
 *
 * Returns `{status:"ok", revision}` on success (including a replayed identical
 * document) or `{status:"conflict", revision, doc}` carrying the stored
 * document so the caller can rebase. An invalid document throws
 * `invalid_sidebar_doc:<reason>`; the gateway maps that to a 400.
 */
export const putForGateway = mutation({
  args: {
    serviceKey: v.string(),
    externalAuthId: v.string(),
    externalOrgId: v.string(),
    baseRevision: v.number(),
    doc: spaceSidebarDocValidator,
  },
  handler: async (
    ctx,
    args,
  ): Promise<
    { status: "ok"; revision: number } | { status: "conflict"; revision: number; doc: unknown }
  > => {
    assertServiceKey(args.serviceKey);
    await requireGatewayMember(ctx, args.externalAuthId, args.externalOrgId);
    if (!Number.isInteger(args.baseRevision) || args.baseRevision < 0) {
      throw new Error("invalid_sidebar_doc:bad_base_revision");
    }
    const doc = validateSidebarDoc(args.doc);
    const existing = await loadRow(ctx, args.externalOrgId, args.externalAuthId);
    const decision = decideSidebarPut(
      existing ? { revision: existing.revision, doc: existing.doc } : null,
      args.baseRevision,
      doc,
    );
    switch (decision.action) {
      case "conflict":
        return {
          status: "conflict",
          revision: decision.revision,
          doc: existing ? existing.doc : null,
        };
      case "noop":
        return { status: "ok", revision: decision.revision };
      case "create": {
        const now = Date.now();
        await ctx.db.insert("spaceSidebarPreferences", {
          externalOrgId: args.externalOrgId,
          externalAuthId: args.externalAuthId,
          revision: decision.revision,
          doc,
          createdAt: now,
          updatedAt: now,
        });
        return { status: "ok", revision: decision.revision };
      }
      case "update": {
        await ctx.db.patch(existing._id, {
          revision: decision.revision,
          doc,
          updatedAt: Date.now(),
        });
        return { status: "ok", revision: decision.revision };
      }
    }
  },
});

/** Rows examined per sweep page; each page is one transaction. */
const SWEEP_PAGE_SIZE = 100;

/**
 * Delete sidebar rows whose owner no longer has a non-deleted membership in the
 * organization (the organization is missing or deleted, or the member's `users`
 * row is missing or deleted). Safe to run repeatedly; a live member's row is
 * never touched. Pages through the table and schedules the next page itself, so
 * one cron tick covers the whole table without a long transaction.
 */
export const sweepOrphans = internalMutation({
  args: {
    cursor: v.optional(v.union(v.string(), v.null())),
  },
  handler: async (ctx, args): Promise<{ scanned: number; deleted: number; done: boolean }> => {
    const page = await ctx.db
      .query("spaceSidebarPreferences")
      .paginate({ numItems: SWEEP_PAGE_SIZE, cursor: args.cursor ?? null });

    // One organization lookup per distinct org on this page.
    const orgIdByExternal = new Map<string, any | null>();
    let deleted = 0;
    for (const row of page.page) {
      let organizationId = orgIdByExternal.get(row.externalOrgId);
      if (organizationId === undefined) {
        const organizations = await ctx.db
          .query("organizations")
          .withIndex("by_external_id", (q: any) => q.eq("externalOrgId", row.externalOrgId))
          .collect();
        const organization = organizations.find((candidate: any) => candidate.syncStatus !== "deleted");
        organizationId = organization ? organization._id : null;
        orgIdByExternal.set(row.externalOrgId, organizationId);
      }
      let isMember = false;
      if (organizationId !== null) {
        const members = await ctx.db
          .query("users")
          .withIndex("by_external_and_org", (q: any) =>
            q.eq("externalAuthId", row.externalAuthId).eq("orgId", organizationId),
          )
          .collect();
        isMember = members.some((candidate: any) => candidate.syncStatus !== "deleted");
      }
      if (!isMember) {
        await ctx.db.delete(row._id);
        deleted += 1;
      }
    }

    if (!page.isDone) {
      await ctx.scheduler.runAfter(0, internal.spaceSidebar.sweepOrphans, {
        cursor: page.continueCursor,
      });
    }
    return { scanned: page.page.length, deleted, done: page.isDone };
  },
});
