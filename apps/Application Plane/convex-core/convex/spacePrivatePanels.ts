/**
 * Private panel pointers (WS17-S6): the gateway's read and get-or-create for one person's private
 * assistant thread in one room. The table and its rationale are in `schema/spacePrivatePanels.ts`.
 *
 * Callers are the gateway only, with the service key, after Control has confirmed the caller is a
 * current member of the room (the gateway owns that check and the ZDR refusal, which runs before
 * any call here). Convex checks the service key and the caller's organization membership, exactly
 * like the other `ForGateway` functions. Nothing here reads or writes a message, a memory row or a
 * Space membership.
 *
 * Errors are stable codes the gateway maps: `private_panel_invalid:<field>`.
 */
import { v } from "convex/values";
import { mutation, query } from "./_generated/server";
import { assertServiceKey, requireGatewayMember } from "./authz";

const MAX_ID_CHARS = 200;

function requireId(value: string, field: string): string {
  const trimmed = (value ?? "").trim();
  if (!trimmed || trimmed.length > MAX_ID_CHARS || /[\u0000-\u001f\u007f]/.test(trimmed)) {
    throw new Error(`private_panel_invalid:${field}`);
  }
  return trimmed;
}

/** The caller's pointer for one room in this organization, or null. */
async function pointerRow(ctx: any, externalOrgId: string, externalAuthId: string, spaceRef: string) {
  return await ctx.db
    .query("spacePrivatePanels")
    .withIndex("by_user_and_space", (q: any) =>
      q.eq("externalOrgId", externalOrgId).eq("externalAuthId", externalAuthId).eq("spaceRef", spaceRef),
    )
    .first();
}

/**
 * The caller's panel pointer for one room. Null when none exists yet. Reads only the ids; the thread
 * is read through the chat route, which enforces its owner.
 */
export const getSpacePrivatePanelForGateway = query({
  args: {
    serviceKey: v.string(),
    externalAuthId: v.string(),
    externalOrgId: v.string(),
    spaceRef: v.string(),
  },
  handler: async (ctx, args): Promise<{ threadId: string; agentSubjectId: string | null } | null> => {
    assertServiceKey(args.serviceKey);
    await requireGatewayMember(ctx, args.externalAuthId, args.externalOrgId);
    const spaceRef = requireId(args.spaceRef, "space_ref");
    const row = await pointerRow(ctx, args.externalOrgId, args.externalAuthId, spaceRef);
    if (!row) return null;
    return {
      threadId: row.threadId as string,
      agentSubjectId: (row.agentSubjectId ?? null) as string | null,
    };
  },
});

/**
 * Record the caller's panel thread for one room, or return the one already recorded. First writer
 * wins: a second record for the same caller and room returns the stored thread and never replaces
 * it (the gateway then uses that thread; its own new thread is an unlisted orphan, the same as any
 * other unused chat). The agent preference may move, and only that is updated on a repeat.
 */
export const recordSpacePrivatePanelForGateway = mutation({
  args: {
    serviceKey: v.string(),
    externalAuthId: v.string(),
    externalOrgId: v.string(),
    spaceRef: v.string(),
    threadId: v.string(),
    agentSubjectId: v.optional(v.string()),
  },
  handler: async (
    ctx,
    args,
  ): Promise<{ threadId: string; agentSubjectId: string | null; created: boolean }> => {
    assertServiceKey(args.serviceKey);
    await requireGatewayMember(ctx, args.externalAuthId, args.externalOrgId);
    const spaceRef = requireId(args.spaceRef, "space_ref");
    const threadId = requireId(args.threadId, "thread_id");
    const agentSubjectId = args.agentSubjectId?.trim() ? requireId(args.agentSubjectId, "agent_subject_id") : undefined;

    const existing = await pointerRow(ctx, args.externalOrgId, args.externalAuthId, spaceRef);
    if (existing) {
      let storedAgent = (existing.agentSubjectId ?? null) as string | null;
      if (agentSubjectId !== undefined && agentSubjectId !== existing.agentSubjectId) {
        await ctx.db.patch(existing._id, { agentSubjectId, updatedAt: Date.now() });
        storedAgent = agentSubjectId;
      }
      return { threadId: existing.threadId as string, agentSubjectId: storedAgent, created: false };
    }

    const now = Date.now();
    await ctx.db.insert("spacePrivatePanels", {
      externalAuthId: args.externalAuthId,
      externalOrgId: args.externalOrgId,
      spaceRef,
      threadId,
      ...(agentSubjectId !== undefined ? { agentSubjectId } : {}),
      createdAt: now,
      updatedAt: now,
    });
    return { threadId, agentSubjectId: agentSubjectId ?? null, created: true };
  },
});
