/**
 * Conversation Mutations
 * 
 * Write functions that modify data transactionally.
 * All clients subscribed to affected data get updates automatically.
 */

import { mutation, query } from "./_generated/server";
import { v } from "convex/values";

import {
  assertServiceKey,
  requireConversationViewer,
  requireViewerMembership,
} from "./authz";

async function getConversationOrThrow(ctx: any, conversationId: string) {
  const conversation = await ctx.db.get(conversationId);
  if (!conversation) {
    throw new Error("Conversation not found");
  }
  return conversation;
}

function assertConversationAccess(
  conversation: { orgId: string; userId: string; status?: string },
  args: { orgId: string; userId: string },
) {
  if (conversation.orgId !== args.orgId || conversation.userId !== args.userId) {
    throw new Error("Conversation access denied");
  }

  if (conversation.status === "deleted") {
    throw new Error("Conversation not found");
  }
}

/**
 * Get a full conversation with messages for a specific user in an organization
 */
export const get = query({
  args: {
    conversationId: v.id("conversations"),
    orgId: v.id("organizations"),
    userId: v.id("users"),
    serviceKey: v.string(),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    const conversation = await ctx.db.get(args.conversationId);

    if (!conversation || conversation.status === "deleted") {
      return null;
    }

    if (conversation.orgId !== args.orgId || conversation.userId !== args.userId) {
      return null;
    }

    const messages = await ctx.db
      .query("messages")
      .withIndex("by_conversation_and_created", (q) =>
        q.eq("conversationId", args.conversationId)
      )
      .order("asc")
      .collect();

    return {
      ...conversation,
      messages,
    };
  },
});

/**
 * Get a conversation by ID without expanding messages
 * Intended for trusted server-side actions.
 */
export const getById = query({
  args: {
    conversationId: v.id("conversations"),
    serviceKey: v.string(),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    const conversation = await ctx.db.get(args.conversationId);
    if (!conversation || conversation.status === "deleted") {
      return null;
    }
    return conversation;
  },
});

/**
 * List conversations for a specific user in an organization
 */
export const listByUser = query({
  args: {
    orgId: v.id("organizations"),
    userId: v.id("users"),
    includeArchived: v.optional(v.boolean()),
    serviceKey: v.string(),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    const includeArchived = args.includeArchived ?? false;

    const conversations = await ctx.db
      .query("conversations")
      .withIndex("by_user", (q) => q.eq("userId", args.userId))
      .collect();

    const filtered = conversations
      .filter((conversation) => {
        if (conversation.orgId !== args.orgId) {
          return false;
        }

        if (conversation.status === "deleted") {
          return false;
        }

        if (!includeArchived && conversation.status === "archived") {
          return false;
        }

        return true;
      })
      .sort((left, right) => right.updatedAt - left.updatedAt);

    return await Promise.all(
      filtered.map(async (conversation) => {
        const messages = await ctx.db
          .query("messages")
          .withIndex("by_conversation", (q) =>
            q.eq("conversationId", conversation._id)
          )
          .collect();

        return {
          ...conversation,
          messageCount: messages.length,
        };
      }),
    );
  },
});

/**
 * Create a new conversation
 */
export const create = mutation({
  args: {
    orgId: v.id("organizations"),
    userId: v.id("users"),
    title: v.string(),
    serviceKey: v.string(),
    sessionId: v.optional(v.string()),
    createdAt: v.optional(v.number()),
    updatedAt: v.optional(v.number()),
    metadata: v.optional(v.object({
      model: v.optional(v.string()),
      temperature: v.optional(v.number()),
      maxTokens: v.optional(v.number()),
      tags: v.optional(v.array(v.string())),
    })),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    const createdAt = args.createdAt ?? Date.now();
    const updatedAt = args.updatedAt ?? createdAt;

    const conversationId = await ctx.db.insert("conversations", {
      orgId: args.orgId,
      userId: args.userId,
      title: args.title,
      sessionId: args.sessionId,
      metadata: args.metadata,
      status: "active",
      createdAt,
      updatedAt,
    });
    
    // Create audit log
    await ctx.db.insert("auditLog", {
      orgId: args.orgId,
      userId: args.userId,
      action: "conversation.created",
      resource: "conversation",
      resourceId: conversationId,
      createdAt: Date.now(),
    });
    
    return conversationId;
  },
});

/**
 * Send a message to a conversation
 */
export const sendMessage = mutation({
  args: {
    conversationId: v.id("conversations"),
    content: v.string(),
    serviceKey: v.string(),
    userId: v.optional(v.id("users")),
    metadata: v.optional(v.object({
      model: v.optional(v.string()),
      tokens: v.optional(v.number()),
    })),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    const conversation = await getConversationOrThrow(ctx, args.conversationId as any);
    
    // Insert message
    const messageId = await ctx.db.insert("messages", {
      conversationId: args.conversationId,
      role: "user",
      content: args.content,
      userId: args.userId,
      metadata: args.metadata,
      createdAt: Date.now(),
      updatedAt: Date.now(),
    });
    
    // Update conversation's lastMessageAt
    await ctx.db.patch(args.conversationId, {
      lastMessageAt: Date.now(),
      updatedAt: Date.now(),
    });
    
    return messageId;
  },
});

/**
 * Update conversation metadata
 */
export const updateMetadata = mutation({
  args: {
    conversationId: v.id("conversations"),
    serviceKey: v.string(),
    metadata: v.object({
      model: v.optional(v.string()),
      temperature: v.optional(v.number()),
      maxTokens: v.optional(v.number()),
      tags: v.optional(v.array(v.string())),
    }),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    const conversation = await getConversationOrThrow(ctx, args.conversationId as any);

    await ctx.db.patch(args.conversationId, {
      metadata: args.metadata,
      updatedAt: Date.now(),
    });

    return conversation;
  },
});

/**
 * Update conversation title
 */
export const updateTitle = mutation({
  args: {
    conversationId: v.id("conversations"),
    orgId: v.id("organizations"),
    userId: v.id("users"),
    serviceKey: v.string(),
    title: v.string(),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    const conversation = await getConversationOrThrow(ctx, args.conversationId as any);
    assertConversationAccess(conversation as any, args as any);

    await ctx.db.patch(args.conversationId, {
      title: args.title,
      updatedAt: Date.now(),
    });

    return await ctx.db.get(args.conversationId);
  },
});

/**
 * Archive a conversation
 */
export const archive = mutation({
  args: {
    conversationId: v.id("conversations"),
    orgId: v.id("organizations"),
    userId: v.id("users"),
    serviceKey: v.string(),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    const conversation = await getConversationOrThrow(ctx, args.conversationId as any);
    assertConversationAccess(conversation as any, args as any);

    await ctx.db.patch(args.conversationId, {
      status: "archived",
      updatedAt: Date.now(),
    });
  },
});

/**
 * Delete a conversation (soft delete)
 */
export const remove = mutation({
  args: {
    conversationId: v.id("conversations"),
    orgId: v.id("organizations"),
    userId: v.id("users"),
    serviceKey: v.string(),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    const conversation = await getConversationOrThrow(ctx, args.conversationId as any);
    assertConversationAccess(conversation as any, args as any);

    await ctx.db.patch(args.conversationId, {
      status: "deleted",
      updatedAt: Date.now(),
    });
  },
});

/**
 * List conversations for the authenticated user within a validated organization.
 */
export const listForCurrentUser = query({
  args: {
    externalOrgId: v.string(),
    includeArchived: v.optional(v.boolean()),
  },
  handler: async (ctx, args) => {
    const includeArchived = args.includeArchived ?? false;
    const viewer = await requireViewerMembership(ctx, args.externalOrgId);

    const conversations = await ctx.db
      .query("conversations")
      .withIndex("by_user", (q) => q.eq("userId", viewer.membership._id))
      .collect();

    const filtered = conversations
      .filter((conversation) => {
        if (conversation.orgId !== viewer.organization._id) {
          return false;
        }

        if (conversation.status === "deleted") {
          return false;
        }

        if (!includeArchived && conversation.status === "archived") {
          return false;
        }

        return true;
      })
      .sort((left, right) => right.updatedAt - left.updatedAt);

    return await Promise.all(
      filtered.map(async (conversation) => {
        const messages = await ctx.db
          .query("messages")
          .withIndex("by_conversation", (q) =>
            q.eq("conversationId", conversation._id),
          )
          .collect();

        return {
          ...conversation,
          messageCount: messages.length,
        };
      }),
    );
  },
});

/**
 * Get a full conversation with messages for the authenticated user.
 */
export const getForCurrentUser = query({
  args: {
    conversationId: v.id("conversations"),
    externalOrgId: v.string(),
  },
  handler: async (ctx, args) => {
    const { conversation } = await requireConversationViewer(
      ctx,
      args.conversationId,
      args.externalOrgId,
    );

    const messages = await ctx.db
      .query("messages")
      .withIndex("by_conversation_and_created", (q) =>
        q.eq("conversationId", args.conversationId),
      )
      .order("asc")
      .collect();

    return {
      ...conversation,
      messages,
    };
  },
});
