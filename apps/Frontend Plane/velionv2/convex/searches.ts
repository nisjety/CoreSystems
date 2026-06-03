import { mutation, query } from "./_generated/server";
import { v } from "convex/values";

// All functions are org-scoped: the caller passes the server-asserted orgId
// (from the verified session) and every read/write filters by it. A thread can
// only be mutated/read by its owning org.

const citation = v.object({
  url: v.string(),
  title: v.optional(v.union(v.string(), v.null())),
});

/** Persist a completed initial search (turn 0). Returns the new thread id. */
export const createThread = mutation({
  args: {
    orgId: v.string(),
    userId: v.string(),
    query: v.string(),
    answer: v.string(),
    citations: v.array(citation),
  },
  handler: async (ctx, args) => {
    const now = Date.now();
    return await ctx.db.insert("searchThreads", {
      orgId: args.orgId,
      userId: args.userId,
      query: args.query,
      answer: args.answer,
      citations: args.citations,
      createdAt: now,
      updatedAt: now,
    });
  },
});

/** Append a follow-up turn (user question or assistant answer) to a thread. */
export const appendTurn = mutation({
  args: {
    threadId: v.id("searchThreads"),
    orgId: v.string(),
    role: v.union(v.literal("user"), v.literal("assistant")),
    text: v.string(),
  },
  handler: async (ctx, args) => {
    const thread = await ctx.db.get(args.threadId);
    if (!thread || thread.orgId !== args.orgId) {
      throw new Error("thread not found for this org");
    }
    await ctx.db.insert("searchTurns", {
      threadId: args.threadId,
      orgId: args.orgId,
      role: args.role,
      text: args.text,
      createdAt: Date.now(),
    });
    await ctx.db.patch(args.threadId, { updatedAt: Date.now() });
  },
});

/** Recent search threads for a user within an org (most recent first). */
export const listThreads = query({
  args: {
    orgId: v.string(),
    userId: v.string(),
    limit: v.optional(v.number()),
  },
  handler: async (ctx, args) => {
    return await ctx.db
      .query("searchThreads")
      .withIndex("by_org_user", (q) =>
        q.eq("orgId", args.orgId).eq("userId", args.userId),
      )
      .order("desc")
      .take(args.limit ?? 20);
  },
});

/** A single thread + its follow-up turns, org-guarded. */
export const getThread = query({
  args: { threadId: v.id("searchThreads"), orgId: v.string() },
  handler: async (ctx, args) => {
    const thread = await ctx.db.get(args.threadId);
    if (!thread || thread.orgId !== args.orgId) return null;
    const turns = await ctx.db
      .query("searchTurns")
      .withIndex("by_thread", (q) => q.eq("threadId", args.threadId))
      .order("asc")
      .collect();
    return { thread, turns };
  },
});
