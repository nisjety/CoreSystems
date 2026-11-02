/**
 * conversation-core projection tables (support inbox, projected messages, presence, AI actions).
 *
 * Table definitions only; composed by `../schema.ts` (P-CX1). Add new
 * tables for a feature to that feature's file, not to `schema.ts`.
 */

import { defineTable } from "convex/server";
import { v } from "convex/values";

export const conversationProjectionTables = {
  // First-party Application Plane conversation-core projection.
  // These tables are separate from the existing AI chat `conversations` /
  // `messages` tables because conversation-core is the durable support inbox
  // truth while Convex only powers live UI projection.
  conversationProjectionEvents: defineTable({
    eventId: v.string(),
    type: v.string(),
    externalOrgId: v.string(),
    conversationId: v.optional(v.string()),
    messageId: v.optional(v.string()),
    createdAt: v.number(),
  })
    .index("by_event_id", ["eventId"])
    .index("by_conversation", ["conversationId"])
    .index("by_org", ["externalOrgId"]),

  conversationInboxItems: defineTable({
    externalOrgId: v.string(),
    conversationId: v.string(),
    inboxId: v.string(),
    title: v.string(),
    status: v.string(),
    priority: v.string(),
    channel: v.string(),
    provider: v.optional(v.string()),
    providerThreadId: v.optional(v.string()),
    assigneeUserId: v.optional(v.string()),
    assigneeName: v.optional(v.string()),
    contactName: v.optional(v.string()),
    contactEmail: v.optional(v.string()),
    tags: v.array(v.string()),
    lastMessagePreview: v.optional(v.string()),
    lastMessageAt: v.optional(v.number()),
    createdAt: v.number(),
    updatedAt: v.number(),
  })
    .index("by_org", ["externalOrgId"])
    .index("by_conversation", ["conversationId"])
    .index("by_org_and_status", ["externalOrgId", "status"])
    .index("by_org_and_updated", ["externalOrgId", "updatedAt"]),

  conversationProjectedMessages: defineTable({
    externalOrgId: v.string(),
    conversationId: v.string(),
    messageId: v.string(),
    direction: v.string(),
    senderType: v.string(),
    senderName: v.optional(v.string()),
    senderEmail: v.optional(v.string()),
    bodyText: v.string(),
    bodyHtml: v.optional(v.string()),
    internal: v.boolean(),
    provider: v.optional(v.string()),
    occurredAt: v.number(),
    createdAt: v.number(),
  })
    .index("by_conversation", ["conversationId"])
    .index("by_message", ["messageId"])
    .index("by_conversation_and_occurred", ["conversationId", "occurredAt"]),

  conversationPresence: defineTable({
    externalOrgId: v.string(),
    conversationId: v.string(),
    externalUserId: v.string(),
    status: v.union(v.literal("online"), v.literal("typing"), v.literal("away"), v.literal("offline")),
    updatedAt: v.number(),
  })
    .index("by_conversation", ["conversationId"])
    .index("by_user", ["externalUserId"]),

  conversationAiActions: defineTable({
    externalOrgId: v.string(),
    conversationId: v.string(),
    aiActionId: v.string(),
    kind: v.string(),
    status: v.string(),
    payload: v.optional(v.any()),
    updatedAt: v.number(),
    createdAt: v.number(),
  })
    .index("by_conversation", ["conversationId"])
    .index("by_ai_action", ["aiActionId"]),
};
