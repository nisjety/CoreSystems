/**
 * Search history — verevonv2 search-v2 persistence.
 *
 * Mirrors the split used across convex-core (see `controlSessions.ts` /
 * `conversations.ts`):
 *
 *   - WRITES (`createThread`, `appendTurn`) are service-key gated mutations.
 *     verevonv2's BFF calls them after a search answer completes, so search
 *     authority stays server-side — a browser cannot forge history.
 *   - READS (`listThreads`, `getThread`) are public, arg-scoped queries for
 *     the browser's `useQuery` reactive subscription. They are scoped by the
 *     Better Auth `externalUserId` / `externalOrgId`, the same trust model as
 *     `controlSessions.byUser`.
 *
 * External-ID scoped (no join through `organizations` / `users`).
 */

import { mutation, query } from "./_generated/server";
import { v } from "convex/values";

import { assertServiceKey } from "./authz";

const citation = v.object({
  url: v.string(),
  title: v.optional(v.union(v.string(), v.null())),
});

async function requireSearchViewer(
  ctx: {
    auth: {
      getUserIdentity: () => Promise<Record<string, unknown> | null>;
    };
  },
  externalUserId: string,
  externalOrgId?: string,
) {
  const identity = await ctx.auth.getUserIdentity();
  if (!identity) {
    throw new Error("Not authenticated");
  }

  const identityExternalAuthId = identity["properties.externalAuthId"];
  if (
    typeof identityExternalAuthId !== "string" ||
    identityExternalAuthId.length === 0 ||
    identityExternalAuthId !== externalUserId
  ) {
    throw new Error("Unauthorized");
  }

  const identityActiveOrgId = identity["properties.activeOrgId"];
  if (
    externalOrgId &&
    typeof identityActiveOrgId === "string" &&
    identityActiveOrgId.length > 0 &&
    identityActiveOrgId !== externalOrgId
  ) {
    throw new Error("Unauthorized");
  }

  return identity;
}

/**
 * createThread — persist a completed initial search (turn 0). Returns the new
 * thread id so the BFF can thread follow-up turns onto it.
 */
export const createThread = mutation({
  args: {
    serviceKey: v.string(),
    externalOrgId: v.string(),
    externalUserId: v.string(),
    query: v.string(),
    answer: v.string(),
    citations: v.array(citation),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    const now = Date.now();
    return await ctx.db.insert("searchThreads", {
      externalOrgId: args.externalOrgId,
      externalUserId: args.externalUserId,
      query: args.query,
      answer: args.answer,
      citations: args.citations,
      status: "active",
      createdAt: now,
      updatedAt: now,
    });
  },
});

/**
 * appendTurn — append a follow-up turn (user question or assistant answer) to
 * an existing thread. Denormalizes the thread's external ids onto the turn so
 * turn-level reads can scope in one hop, and bumps the thread's updatedAt so it
 * floats to the top of the recent list.
 */
export const appendTurn = mutation({
  args: {
    serviceKey: v.string(),
    threadId: v.id("searchThreads"),
    role: v.union(v.literal("user"), v.literal("assistant")),
    text: v.string(),
    citations: v.optional(v.array(citation)),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    const thread = await ctx.db.get(args.threadId);
    if (!thread || thread.status === "deleted") {
      throw new Error("thread not found");
    }
    const now = Date.now();
    const turnId = await ctx.db.insert("searchTurns", {
      threadId: args.threadId,
      externalOrgId: thread.externalOrgId,
      externalUserId: thread.externalUserId,
      role: args.role,
      text: args.text,
      citations: args.citations,
      createdAt: now,
    });
    await ctx.db.patch(args.threadId, { updatedAt: now });
    return turnId;
  },
});

/**
 * listThreads — recent search threads for a user, most-recent first. Public
 * query: the browser subscribes via `useQuery` with its own session ids.
 * Optionally narrows to a single org (the active workspace).
 */
export const listThreads = query({
  args: {
    externalUserId: v.string(),
    externalOrgId: v.optional(v.string()),
    limit: v.optional(v.number()),
  },
  handler: async (ctx, args) => {
    await requireSearchViewer(ctx, args.externalUserId, args.externalOrgId);
    const limit = args.limit ?? 20;
    const rows = await ctx.db
      .query("searchThreads")
      .withIndex("by_user_and_updated", (q) =>
        q.eq("externalUserId", args.externalUserId),
      )
      .order("desc")
      .collect();

    return rows
      .filter((t) => t.status !== "deleted")
      .filter((t) =>
        args.externalOrgId ? t.externalOrgId === args.externalOrgId : true,
      )
      .slice(0, limit);
  },
});

/**
 * getThread — a single thread plus its follow-up turns (chronological). Public
 * query, scoped by the caller-supplied external ids so a browser can only read
 * a thread it owns.
 */
export const getThread = query({
  args: {
    threadId: v.id("searchThreads"),
    externalUserId: v.string(),
    externalOrgId: v.optional(v.string()),
  },
  handler: async (ctx, args) => {
    await requireSearchViewer(ctx, args.externalUserId, args.externalOrgId);
    const thread = await ctx.db.get(args.threadId);
    if (!thread || thread.status === "deleted") return null;
    if (thread.externalUserId !== args.externalUserId) return null;
    if (args.externalOrgId && thread.externalOrgId !== args.externalOrgId) {
      return null;
    }
    const turns = await ctx.db
      .query("searchTurns")
      .withIndex("by_thread_and_created", (q) =>
        q.eq("threadId", args.threadId),
      )
      .order("asc")
      .collect();
    return { thread, turns };
  },
});

/**
 * deleteThread — remove a thread and all its turns. Service-key gated (called
 * from the verevonv2 BFF "delete from history" action). Hard delete so the
 * user's search history is genuinely gone, not just hidden. Optionally asserts
 * ownership via externalUserId before deleting.
 */
export const deleteThread = mutation({
  args: {
    serviceKey: v.string(),
    threadId: v.id("searchThreads"),
    externalUserId: v.optional(v.string()),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    const thread = await ctx.db.get(args.threadId);
    if (!thread) return { deleted: false };
    if (args.externalUserId && thread.externalUserId !== args.externalUserId) {
      throw new Error("thread not owned by user");
    }
    const turns = await ctx.db
      .query("searchTurns")
      .withIndex("by_thread", (q) => q.eq("threadId", args.threadId))
      .collect();
    for (const turn of turns) {
      await ctx.db.delete(turn._id);
    }
    await ctx.db.delete(args.threadId);
    return { deleted: true };
  },
});
