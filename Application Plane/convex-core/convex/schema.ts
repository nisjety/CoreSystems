/**
 * Convex Schema Definition
 * 
 * This defines the shape of data in the Convex database.
 * TypeScript types are automatically generated from this schema.
 */

import { defineSchema, defineTable } from "convex/server";
import { v } from "convex/values";

export default defineSchema({
  // Organizations - Multi-tenant support
  organizations: defineTable({
    // External sync fields
    externalOrgId: v.string(),  // ID from auth-core/org-core
    
    name: v.string(),
    slug: v.string(),
    settings: v.optional(v.object({
      maxTokens: v.optional(v.number()),
      defaultModel: v.optional(v.string()),
      allowedModels: v.optional(v.array(v.string())),
    })),
    
    // Sync tracking
    syncStatus: v.union(v.literal("syncing"), v.literal("synced"), v.literal("deleted")),
    lastSyncedAt: v.number(),
    
    createdAt: v.number(),
    updatedAt: v.number(),
    deletedAt: v.optional(v.number()),
  })
    .index("by_slug", ["slug"])
    .index("by_external_id", ["externalOrgId"]),

  // Users - Connected to auth system
  users: defineTable({
    // External sync fields
    externalAuthId: v.string(),  // ID from auth-core
    
    email: v.string(),
    name: v.optional(v.string()),
    orgId: v.id("organizations"),
    role: v.union(v.literal("admin"), v.literal("member"), v.literal("viewer")),
    
    // Sync tracking
    syncStatus: v.union(v.literal("syncing"), v.literal("synced"), v.literal("deleted")),
    lastSyncedAt: v.number(),
    
    createdAt: v.number(),
    lastSeenAt: v.number(),
    deletedAt: v.optional(v.number()),
  })
    .index("by_external_auth_id", ["externalAuthId"])
    .index("by_org", ["orgId"])
    .index("by_email", ["email"])
    .index("by_external_and_org", ["externalAuthId", "orgId"]),

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
    .index("by_conversation_and_created", ["conversationId", "createdAt"]),

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

  // Planner documents - lightweight metadata for BlockSuite-backed notes/canvases.
  plannerDocuments: defineTable({
    workspaceId: v.string(),
    documentId: v.string(),
    title: v.string(),
    ownerExternalAuthId: v.optional(v.string()),
    parentDocumentId: v.optional(v.string()),
    isFavorite: v.optional(v.boolean()),
    lastViewedAt: v.optional(v.number()),
    space: v.optional(v.union(v.literal("private"), v.literal("shared"), v.literal("collection"))),
    createdAt: v.number(),
    updatedAt: v.number(),
    archivedAt: v.optional(v.number()),
  })
    .index("by_workspace", ["workspaceId"])
    .index("by_workspace_and_document", ["workspaceId", "documentId"])
    .index("by_workspace_and_updated", ["workspaceId", "updatedAt"])
    .index("by_workspace_and_parent", ["workspaceId", "parentDocumentId"])
    .index("by_workspace_and_last_viewed", ["workspaceId", "lastViewedAt"]),

  plannerDocumentStates: defineTable({
    workspaceId: v.string(),
    documentId: v.string(),
    stateBase64: v.string(),
    updatedAt: v.number(),
  })
    .index("by_workspace_and_document", ["workspaceId", "documentId"]),

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
});
