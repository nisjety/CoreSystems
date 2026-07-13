import { v } from "convex/values";
import { internalMutation } from "./_generated/server";

export const claim = internalMutation({
  args: {
    nonce: v.string(),
    externalOrgId: v.string(),
    requestTimestamp: v.number(),
  },
  handler: async (ctx, args) => {
    const existing = await ctx.db
      .query("reconciliationNonces")
      .withIndex("by_nonce", (q) => q.eq("nonce", args.nonce))
      .first();
    if (existing) throw new Error("Reconciliation request replayed");

    const now = Date.now();
    const expired = await ctx.db
      .query("reconciliationNonces")
      .withIndex("by_expires_at", (q) => q.lt("expiresAt", now))
      .take(100);
    for (const item of expired) await ctx.db.delete(item._id);

    return await ctx.db.insert("reconciliationNonces", {
      nonce: args.nonce,
      externalOrgId: args.externalOrgId,
      requestTimestamp: args.requestTimestamp,
      createdAt: now,
      expiresAt: now + 24 * 60 * 60 * 1000,
    });
  },
});
