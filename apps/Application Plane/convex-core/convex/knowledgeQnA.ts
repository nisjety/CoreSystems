/**
 * Wave 11 §2.1 — Q&A first-class entity (see schema.ts `knowledgeQnA`).
 *
 * Org-scoped CRUD plus a few helpers for the playground citation
 * surface. All mutations require both `orgId` (for RLS-like scoping)
 * and `createdBy`/`userId` for audit; the velion server-side `qa-store`
 * passes the resolved `actor.convexOrgId` / `convexUserId` from
 * `resolveChatActor()`.
 */
import { v } from "convex/values";
import { mutation, query } from "./_generated/server";
import {
  requireEditorMembershipByOrgId,
  requireViewerMembershipByOrgId,
} from "./authz";

export const listByOrg = query({
  args: { orgId: v.id("organizations") },
  handler: async (ctx, { orgId }) => {
    await requireViewerMembershipByOrgId(ctx, orgId);
    return await ctx.db
      .query("knowledgeQnA")
      .withIndex("by_org", (q) => q.eq("orgId", orgId))
      .order("desc")
      .collect();
  },
});

export const listPublishedByOrg = query({
  args: { orgId: v.id("organizations") },
  handler: async (ctx, { orgId }) => {
    await requireViewerMembershipByOrgId(ctx, orgId);
    return await ctx.db
      .query("knowledgeQnA")
      .withIndex("by_org_and_status", (q) =>
        q.eq("orgId", orgId).eq("status", "published")
      )
      .collect();
  },
});

export const create = mutation({
  args: {
    orgId: v.id("organizations"),
    createdBy: v.id("users"),
    question: v.string(),
    answer: v.string(),
    status: v.optional(
      v.union(v.literal("draft"), v.literal("published"))
    ),
  },
  handler: async (ctx, args) => {
    const viewer = await requireEditorMembershipByOrgId(ctx, args.orgId);
    if (args.createdBy !== viewer.membership._id) throw new Error("Unauthorized");
    const now = Date.now();
    const entryId = await ctx.db.insert("knowledgeQnA", {
      orgId: args.orgId,
      createdBy: viewer.membership._id,
      question: args.question,
      answer: args.answer,
      status: args.status ?? "draft",
      citationCount: 0,
      createdAt: now,
      updatedAt: now,
    });
    const entry = await ctx.db.get(entryId);
    if (!entry) throw new Error("Failed to read just-inserted Q&A entry");
    return entry;
  },
});

export const update = mutation({
  args: {
    entryId: v.id("knowledgeQnA"),
    orgId: v.id("organizations"),
    question: v.optional(v.string()),
    answer: v.optional(v.string()),
    status: v.optional(
      v.union(v.literal("draft"), v.literal("published"), v.literal("deprecated"))
    ),
  },
  handler: async (ctx, args) => {
    const existing = await ctx.db.get(args.entryId);
    if (!existing) throw new Error("Q&A entry not found");
    await requireEditorMembershipByOrgId(ctx, existing.orgId);
    if (existing.orgId !== args.orgId) {
      throw new Error("Q&A entry belongs to a different organization");
    }
    const patch: Record<string, unknown> = { updatedAt: Date.now() };
    if (typeof args.question === "string") patch.question = args.question;
    if (typeof args.answer === "string") patch.answer = args.answer;
    if (args.status) patch.status = args.status;
    await ctx.db.patch(args.entryId, patch);
    const updated = await ctx.db.get(args.entryId);
    if (!updated) throw new Error("Failed to read updated Q&A entry");
    return updated;
  },
});

export const remove = mutation({
  args: {
    entryId: v.id("knowledgeQnA"),
    orgId: v.id("organizations"),
  },
  handler: async (ctx, args) => {
    const existing = await ctx.db.get(args.entryId);
    if (!existing) return null;
    await requireEditorMembershipByOrgId(ctx, existing.orgId);
    if (existing.orgId !== args.orgId) {
      throw new Error("Q&A entry belongs to a different organization");
    }
    await ctx.db.delete(args.entryId);
    return { success: true };
  },
});

export const incrementCitation = mutation({
  args: {
    entryId: v.id("knowledgeQnA"),
    rating: v.optional(
      v.union(v.literal("good"), v.literal("acceptable"), v.literal("poor"))
    ),
  },
  handler: async (ctx, args) => {
    const existing = await ctx.db.get(args.entryId);
    if (!existing) return null;
    await requireViewerMembershipByOrgId(ctx, existing.orgId);
    const patch: Record<string, unknown> = {
      citationCount: (existing.citationCount ?? 0) + 1,
      updatedAt: Date.now(),
    };
    if (args.rating) patch.rating = args.rating;
    await ctx.db.patch(args.entryId, patch);
    return { success: true };
  },
});
