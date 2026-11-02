/**
 * Cross-plane ingestion job tracking.
 *
 * Table definitions only; composed by `../schema.ts` (P-CX1). Add new
 * tables for a feature to that feature's file, not to `schema.ts`.
 */

import { defineTable } from "convex/server";
import { v } from "convex/values";

export const ingestTables = {
  // IngestJobs - Cross-plane ingestion job tracking (quarry crawl/scrape events)
  // Separate from user-scoped jobs; entries driven by Ingestion Plane NATS events.
  ingestJobs: defineTable({
    externalJobId: v.string(),
    externalOrgId: v.optional(v.string()),
    type: v.union(
      v.literal("crawl"),
      v.literal("scrape"),
      v.literal("import")
    ),
    status: v.union(
      v.literal("pending"),
      v.literal("running"),
      v.literal("indexing"),
      v.literal("completed"),
      v.literal("failed")
    ),
    url: v.optional(v.string()),
    progress: v.number(),
    progressMessage: v.optional(v.string()),
    pageCount: v.optional(v.number()),
    documentCount: v.optional(v.number()),
    sourceType: v.optional(v.string()),
    error: v.optional(v.string()),
    startedAt: v.optional(v.number()),
    completedAt: v.optional(v.number()),
    failedAt: v.optional(v.number()),
    createdAt: v.number(),
    updatedAt: v.number(),
  })
    .index("by_external_id", ["externalJobId"])
    .index("by_org", ["externalOrgId"])
    .index("by_status", ["status"])
    .index("by_org_and_status", ["externalOrgId", "status"]),
};
