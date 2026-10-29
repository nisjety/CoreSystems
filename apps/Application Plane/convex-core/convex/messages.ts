/**
 * Message Queries
 * 
 * Read-only functions that automatically subscribe to changes.
 * When data changes, subscribed clients receive updates automatically.
 */

import { mutation, query } from "./_generated/server";
import { v } from "convex/values";

import { assertServiceKey } from "./authz";

const messageMetadataValidator = v.optional(v.object({
  source: v.optional(v.string()),
  citations: v.optional(v.array(v.string())),
  model: v.optional(v.string()),
  tokens: v.optional(v.number()),
  latencyMs: v.optional(v.number()),
  ragContext: v.optional(v.array(v.string())),
  error: v.optional(v.string()),
}));

/**
 * Get all messages for a conversation
 * Automatically subscribes - updates when new messages arrive
 */
export const get = query({
  args: { 
    conversationId: v.id("conversations"),
    limit: v.optional(v.number()),
    serviceKey: v.string(),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    const { conversationId, limit = 100 } = args;
    
    // Verify conversation exists
    const conversation = await ctx.db.get(conversationId);
    if (!conversation) {
      throw new Error("Conversation not found");
    }
    
    // Get messages ordered by creation time
    const messages = await ctx.db
      .query("messages")
      .withIndex("by_conversation_and_created", (q) =>
        q.eq("conversationId", conversationId)
      )
      .order("asc")
      .take(limit);
    
    return messages;
  },
});

/**
 * Get a single message by ID
 */
export const getById = query({
  args: {
    messageId: v.id("messages"),
    serviceKey: v.string(),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    return await ctx.db.get(args.messageId);
  },
});

/**
 * Get the latest message in a conversation
 */
export const getLatest = query({
  args: {
    conversationId: v.id("conversations"),
    serviceKey: v.string(),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    const messages = await ctx.db
      .query("messages")
      .withIndex("by_conversation_and_created", (q) =>
        q.eq("conversationId", args.conversationId)
      )
      .order("desc")
      .take(1);
    
    return messages[0] ?? null;
  },
});

/**
 * Get message count for conversation
 */
export const count = query({
  args: {
    conversationId: v.id("conversations"),
    serviceKey: v.string(),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    const messages = await ctx.db
      .query("messages")
      .withIndex("by_conversation", (q) =>
        q.eq("conversationId", args.conversationId)
      )
      .collect();
    
    return messages.length;
  },
});

/**
 * Create a message in a conversation
 */
export const create = mutation({
  args: {
    conversationId: v.id("conversations"),
    role: v.union(v.literal("user"), v.literal("assistant"), v.literal("system")),
    content: v.string(),
    clientId: v.optional(v.string()),
    serviceKey: v.string(),
    userId: v.optional(v.id("users")),
    isStreaming: v.optional(v.boolean()),
    streamedChunks: v.optional(v.array(v.string())),
    metadata: messageMetadataValidator,
    createdAt: v.optional(v.number()),
    updatedAt: v.optional(v.number()),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    const conversation = await ctx.db.get(args.conversationId);
    if (!conversation || conversation.status === "deleted") {
      throw new Error("Conversation not found");
    }

    const createdAt = args.createdAt ?? Date.now();
    const updatedAt = args.updatedAt ?? createdAt;

    const messageId = await ctx.db.insert("messages", {
      conversationId: args.conversationId,
      role: args.role,
      content: args.content,
      clientId: args.clientId,
      userId: args.userId,
      isStreaming: args.isStreaming,
      streamedChunks: args.streamedChunks,
      metadata: args.metadata,
      createdAt,
      updatedAt,
    });

    await ctx.db.patch(args.conversationId, {
      updatedAt,
      lastMessageAt: createdAt,
    });

    return messageId;
  },
});

/**
 * Create an assistant placeholder message for streaming responses
 */
export const createStreaming = mutation({
  args: {
    conversationId: v.id("conversations"),
    serviceKey: v.string(),
    createdAt: v.optional(v.number()),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    const createdAt = args.createdAt ?? Date.now();
    const conversation = await ctx.db.get(args.conversationId);
    if (!conversation || conversation.status === "deleted") {
      throw new Error("Conversation not found");
    }

    const messageId = await ctx.db.insert("messages", {
      conversationId: args.conversationId,
      role: "assistant",
      content: "",
      isStreaming: true,
      streamedChunks: [],
      createdAt,
      updatedAt: createdAt,
    });

    await ctx.db.patch(args.conversationId, {
      updatedAt: createdAt,
      lastMessageAt: createdAt,
    });

    return messageId;
  },
});

/**
 * Append a chunk to a streaming assistant message
 */
export const updateStreaming = mutation({
  args: {
    messageId: v.id("messages"),
    chunk: v.string(),
    serviceKey: v.string(),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    const message = await ctx.db.get(args.messageId);
    if (!message) {
      throw new Error("Message not found");
    }

    const nextChunks = [...(message.streamedChunks ?? []), args.chunk];
    const nextContent = (message.content ?? "") + args.chunk;
    const updatedAt = Date.now();

    await ctx.db.patch(args.messageId, {
      content: nextContent,
      streamedChunks: nextChunks,
      isStreaming: true,
      updatedAt,
    });

    await ctx.db.patch(message.conversationId, {
      updatedAt,
      lastMessageAt: updatedAt,
    });

    return await ctx.db.get(args.messageId);
  },
});

/**
 * Finalize a streaming assistant message
 */
export const finalizeStreaming = mutation({
  args: {
    messageId: v.id("messages"),
    content: v.string(),
    serviceKey: v.string(),
    metadata: messageMetadataValidator,
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    const message = await ctx.db.get(args.messageId);
    if (!message) {
      throw new Error("Message not found");
    }

    const updatedAt = Date.now();

    await ctx.db.patch(args.messageId, {
      content: args.content,
      metadata: args.metadata,
      isStreaming: false,
      updatedAt,
    });

    await ctx.db.patch(message.conversationId, {
      updatedAt,
      lastMessageAt: updatedAt,
    });

    return await ctx.db.get(args.messageId);
  },
});
