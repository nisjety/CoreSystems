import { v } from "convex/values";
import { internalMutation } from "./_generated/server";

/** Idempotently project a completed Ingestion Plane import into ingestJobs. */
export const recordCompleted = internalMutation({
  args: {
    externalOrgId: v.string(),
    externalImportId: v.string(),
    sourceType: v.string(),
    totalDocuments: v.number(),
    completedAt: v.number(),
  },
  handler: async (ctx, args) => {
    if (!args.externalOrgId.trim() || !args.externalImportId.trim()) {
      throw new Error("externalOrgId and externalImportId are required");
    }
    if (!Number.isFinite(args.totalDocuments) || args.totalDocuments < 0) {
      throw new Error("totalDocuments must be a non-negative number");
    }

    const existing = await ctx.db
      .query("ingestJobs")
      .withIndex("by_external_id", (q) =>
        q.eq("externalJobId", args.externalImportId)
      )
      .first();

    if (existing) {
      if (
        existing.externalOrgId &&
        existing.externalOrgId !== args.externalOrgId
      ) {
        throw new Error("Import identifier already belongs to another organization");
      }
      if (existing.updatedAt > args.completedAt) {
        return { status: "stale_ignored", jobId: existing._id };
      }
      await ctx.db.patch(existing._id, {
        externalOrgId: args.externalOrgId,
        type: "import",
        status: "completed",
        progress: 100,
        progressMessage: `${args.totalDocuments} documents imported`,
        documentCount: args.totalDocuments,
        sourceType: args.sourceType,
        completedAt: args.completedAt,
        updatedAt: args.completedAt,
      });
      return { status: "updated", jobId: existing._id };
    }

    const jobId = await ctx.db.insert("ingestJobs", {
      externalJobId: args.externalImportId,
      externalOrgId: args.externalOrgId,
      type: "import",
      status: "completed",
      progress: 100,
      progressMessage: `${args.totalDocuments} documents imported`,
      documentCount: args.totalDocuments,
      sourceType: args.sourceType,
      completedAt: args.completedAt,
      createdAt: args.completedAt,
      updatedAt: args.completedAt,
    });
    return { status: "created", jobId };
  },
});
