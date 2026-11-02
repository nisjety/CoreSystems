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
 * The orphan sweep (WS16-S11) is a later slice and is not defined here.
 */
import { v } from "convex/values";
import { mutation, query } from "./_generated/server";
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
