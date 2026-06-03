import { defineSchema, defineTable } from "convex/server";
import { v } from "convex/values";

// Persistence for the Velion search/answer experience, served by the LOCAL
// self-hosted convex-backend (`coresystem-local`). Every row is org-scoped;
// queries/mutations MUST filter by the server-asserted orgId so one tenant can
// never read another's threads.
const citation = v.object({
  url: v.string(),
  title: v.optional(v.union(v.string(), v.null())),
});

export default defineSchema({
  // One row per initial search ("turn 0"): the query + the synthesized answer +
  // its citations. Follow-up turns live in `searchTurns`.
  searchThreads: defineTable({
    orgId: v.string(),
    userId: v.string(),
    query: v.string(),
    answer: v.string(),
    citations: v.array(citation),
    createdAt: v.number(),
    updatedAt: v.number(),
  })
    // Recent searches for a user within an org.
    .index("by_org_user", ["orgId", "userId"])
    // Org-wide recency (admin / shared views).
    .index("by_org_created", ["orgId", "createdAt"]),

  // Conversational follow-up turns under a thread (user question + assistant
  // answer), in order.
  searchTurns: defineTable({
    threadId: v.id("searchThreads"),
    orgId: v.string(),
    role: v.union(v.literal("user"), v.literal("assistant")),
    text: v.string(),
    createdAt: v.number(),
  }).index("by_thread", ["threadId"]),
});
