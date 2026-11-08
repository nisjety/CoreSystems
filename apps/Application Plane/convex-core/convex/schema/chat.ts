/**
 * AI chat conversations, messages, jobs, presence, webhooks, audit log, projects and search history.
 *
 * Table definitions only; composed by `../schema.ts` (P-CX1). Add new
 * tables for a feature to that feature's file, not to `schema.ts`.
 */

import { defineTable } from "convex/server";
import { v } from "convex/values";

export const chatTables = {
  // Conversations - Chat sessions
  conversations: defineTable({
    orgId: v.id("organizations"),
    title: v.string(),
    userId: v.id("users"),
    sessionId: v.optional(v.string()),  // Link to Org Core session
    metadata: v.optional(v.object({
      model: v.optional(v.string()),
      temperature: v.optional(v.number()),
      maxTokens: v.optional(v.number()),
      tags: v.optional(v.array(v.string())),
    })),
    status: v.union(
      v.literal("active"),
      v.literal("archived"),
      v.literal("deleted")
    ),
    createdAt: v.number(),
    updatedAt: v.number(),
    lastMessageAt: v.optional(v.number()),
  })
    .index("by_org", ["orgId"])
    .index("by_user", ["userId"])
    .index("by_org_and_status", ["orgId", "status"])
    .index("by_session", ["sessionId"]),

  // Messages - Chat messages with streaming support
  messages: defineTable({
    conversationId: v.id("conversations"),
    role: v.union(v.literal("user"), v.literal("assistant"), v.literal("system")),
    content: v.string(),
    clientId: v.optional(v.string()),
    userId: v.optional(v.id("users")),
    
    // Streaming support
    isStreaming: v.optional(v.boolean()),
    streamedChunks: v.optional(v.array(v.string())),
    
    // Metadata
    metadata: v.optional(v.object({
      source: v.optional(v.string()),
      citations: v.optional(v.array(v.string())),
      model: v.optional(v.string()),
      tokens: v.optional(v.number()),
      latencyMs: v.optional(v.number()),
      ragContext: v.optional(v.array(v.string())),
      error: v.optional(v.string()),
    })),
    
    // Attachments
    attachments: v.optional(v.array(v.object({
      type: v.union(v.literal("file"), v.literal("image"), v.literal("document")),
      storageId: v.string(),
      filename: v.string(),
      size: v.number(),
      mimeType: v.string(),
    }))),
    
    createdAt: v.number(),
    updatedAt: v.number(),
  })
    .index("by_conversation", ["conversationId"])
    .index("by_conversation_and_created", ["conversationId", "createdAt"])
    .index("by_conversation_and_client", ["conversationId", "clientId"]),

  // Jobs - Async operations (RAG indexing, exports, etc.)
  jobs: defineTable({
    orgId: v.id("organizations"),
    userId: v.id("users"),
    type: v.union(
      v.literal("rag_index"),
      v.literal("crawl"),
      v.literal("export"),
      v.literal("batch_delete")
    ),
    status: v.union(
      v.literal("pending"),
      v.literal("running"),
      v.literal("completed"),
      v.literal("failed"),
      v.literal("cancelled")
    ),
    
    // Job details
    payload: v.object({
      // Flexible payload based on job type
      documents: v.optional(v.array(v.string())),
      collection: v.optional(v.string()),
      url: v.optional(v.string()),
      options: v.optional(v.any()),
    }),
    
    // Progress tracking
    progress: v.optional(v.number()),  // 0-100
    progressMessage: v.optional(v.string()),
    
    // Results
    result: v.optional(v.object({
      success: v.optional(v.number()),
      failed: v.optional(v.number()),
      errors: v.optional(v.array(v.string())),
      data: v.optional(v.any()),
    })),
    
    // External job ID (from Org Core)
    externalJobId: v.optional(v.string()),
    
    // Timing
    startedAt: v.optional(v.number()),
    completedAt: v.optional(v.number()),
    createdAt: v.number(),
    updatedAt: v.number(),
  })
    .index("by_org", ["orgId"])
    .index("by_user", ["userId"])
    .index("by_status", ["status"])
    .index("by_org_and_status", ["orgId", "status"])
    .index("by_external_id", ["externalJobId"]),

  // Presence - Who's online, typing, etc.
  presence: defineTable({
    userId: v.id("users"),
    conversationId: v.optional(v.id("conversations")),
    status: v.union(
      v.literal("online"),
      v.literal("typing"),
      v.literal("away"),
      v.literal("offline")
    ),
    lastSeenAt: v.number(),
    metadata: v.optional(v.object({
      device: v.optional(v.string()),
      userAgent: v.optional(v.string()),
    })),
  })
    .index("by_user", ["userId"])
    .index("by_conversation", ["conversationId"])
    .index("by_status", ["status"]),

  // Webhooks - External integrations
  webhooks: defineTable({
    orgId: v.id("organizations"),
    name: v.string(),
    url: v.string(),
    events: v.array(v.string()),
    secret: v.string(),
    active: v.boolean(),
    
    // Stats
    lastTriggeredAt: v.optional(v.number()),
    successCount: v.number(),
    failureCount: v.number(),
    
    createdAt: v.number(),
    updatedAt: v.number(),
  })
    .index("by_org", ["orgId"])
    .index("by_active", ["active"]),

  // Audit Log - Track all actions
  auditLog: defineTable({
    orgId: v.id("organizations"),
    userId: v.optional(v.id("users")),
    action: v.string(),
    resource: v.string(),
    resourceId: v.optional(v.string()),
    changes: v.optional(v.any()),
    metadata: v.optional(v.object({
      ipAddress: v.optional(v.string()),
      userAgent: v.optional(v.string()),
    })),
    createdAt: v.number(),
  })
    .index("by_org", ["orgId"])
    .index("by_user", ["userId"])
    .index("by_resource", ["resource"])
    .index("by_created", ["createdAt"]),

  // Chat composer "Add to project" picker — per-org named buckets users can
  // file conversations under. Backs the ChatSettingsModal Projects view
  // (U2-14 follow-up — was SAMPLE_PROJECTS array of two hardcoded strings).
  //
  // `externalOrgId` mirrors the Better Auth / org-core organization id so
  // we can scope without joining through `organizations` (saves a hop).
  // Conversations link back via `conversations.metadata.projectId`.
  projects: defineTable({
    externalOrgId: v.string(),
    title: v.string(),
    description: v.optional(v.string()),
    createdBy: v.string(),       // user id (Better Auth subject)
    color: v.optional(v.string()), // optional UI hint, e.g. "amber-500"
    archived: v.boolean(),
    createdAt: v.number(),
    updatedAt: v.number(),
  })
    .index("by_external_org", ["externalOrgId"])
    .index("by_org_and_archived", ["externalOrgId", "archived"]),

  // verevonv2 search-v2 history — persisted AI answer-search threads + follow-up
  // turns. External-ID scoped (Better Auth org/user ids) like `projects` /
  // `controlSessions` so reads need no join through organizations/users and a
  // browser can subscribe to its own rows reactively. Writes come from the
  // verevonv2 BFF via the service-key mutations below (search authority stays
  // server-side); reads are public arg-scoped queries for `useQuery`.
  searchThreads: defineTable({
    externalOrgId: v.string(),
    externalUserId: v.string(),
    // The originating query (turn 0) and its synthesized grounded answer.
    query: v.string(),
    answer: v.string(),
    citations: v.array(
      v.object({
        url: v.string(),
        title: v.optional(v.union(v.string(), v.null())),
      }),
    ),
    status: v.union(v.literal("active"), v.literal("deleted")),
    createdAt: v.number(),
    updatedAt: v.number(),
  })
    .index("by_org", ["externalOrgId"])
    .index("by_user", ["externalUserId"])
    .index("by_org_and_user", ["externalOrgId", "externalUserId"])
    .index("by_user_and_updated", ["externalUserId", "updatedAt"]),

  searchTurns: defineTable({
    threadId: v.id("searchThreads"),
    // Denormalized external ids so turn-level reads can scope without a thread
    // lookup, mirroring how `messages` carries enough to authorize in one hop.
    externalOrgId: v.string(),
    externalUserId: v.string(),
    role: v.union(v.literal("user"), v.literal("assistant")),
    text: v.string(),
    citations: v.optional(
      v.array(
        v.object({
          url: v.string(),
          title: v.optional(v.union(v.string(), v.null())),
        }),
      ),
    ),
    createdAt: v.number(),
  })
    .index("by_thread", ["threadId"])
    .index("by_thread_and_created", ["threadId", "createdAt"]),
};
