import { internalMutation, query } from "./_generated/server";
import { v } from "convex/values";
import { assertServiceKey } from "./authz";

type ProjectionEvent = {
  id?: string;
  type?: string;
  org_id?: string;
  conversation_id?: string;
  message_id?: string;
  data?: {
    conversation?: any;
    message?: any;
    status?: string;
    assignee_user_id?: string;
    assignee_name?: string;
    tag?: string;
    ai_action_id?: string;
    decision?: string;
  };
  occurred_at?: string;
};

export const applyEvent = internalMutation({
  args: { event: v.any() },
  handler: async (ctx, args) => {
    const event = args.event as ProjectionEvent;
    const eventId = clean(event.id) || `${clean(event.type)}:${clean(event.org_id)}:${clean(event.conversation_id)}:${clean(event.message_id)}:${Date.now()}`;
    const existing = await ctx.db
      .query("conversationProjectionEvents")
      .withIndex("by_event_id", (q) => q.eq("eventId", eventId))
      .first();
    if (existing) return { status: "already_applied", eventId };

    const externalOrgId = clean(event.org_id);
    const conversationId = clean(event.conversation_id);
    const messageId = clean(event.message_id);
    const type = clean(event.type);
    const now = Date.now();

    await ctx.db.insert("conversationProjectionEvents", {
      eventId,
      type,
      externalOrgId,
      conversationId: conversationId || undefined,
      messageId: messageId || undefined,
      createdAt: now,
    });

    const conversation = event.data?.conversation;
    if (conversation || conversationId) {
      await upsertInboxItem(ctx, externalOrgId, conversationId, conversation, event);
    }

    const message = event.data?.message;
    if (message || messageId) {
      await upsertMessage(ctx, externalOrgId, conversationId, messageId, message, event);
    }

    if (event.data?.ai_action_id) {
      await upsertAiAction(ctx, externalOrgId, conversationId, event.data);
    }

    return { status: "applied", eventId };
  },
});

export const listInbox = query({
  args: {
    externalOrgId: v.string(),
    status: v.optional(v.string()),
    limit: v.optional(v.number()),
    serviceKey: v.string(),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    const limit = Math.min(Math.max(args.limit ?? 50, 1), 100);
    const rows = args.status
      ? await ctx.db
        .query("conversationInboxItems")
        .withIndex("by_org_and_status", (q) => q.eq("externalOrgId", args.externalOrgId).eq("status", args.status))
        .take(limit)
      : await ctx.db
        .query("conversationInboxItems")
        .withIndex("by_org", (q) => q.eq("externalOrgId", args.externalOrgId))
        .take(limit);
    return rows.sort((left, right) => right.updatedAt - left.updatedAt);
  },
});

export const messagesByConversation = query({
  args: {
    conversationId: v.string(),
    serviceKey: v.string(),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    return await ctx.db
      .query("conversationProjectedMessages")
      .withIndex("by_conversation_and_occurred", (q) => q.eq("conversationId", args.conversationId))
      .order("asc")
      .collect();
  },
});

async function upsertInboxItem(ctx: any, externalOrgId: string, conversationId: string, conversation: any, event: ProjectionEvent) {
  if (!externalOrgId || !conversationId) return;
  const existing = await ctx.db
    .query("conversationInboxItems")
    .withIndex("by_conversation", (q: any) => q.eq("conversationId", conversationId))
    .first();
  const updatedAt = toMillis(conversation?.updated_at) || Date.now();
  const patch = {
    externalOrgId,
    conversationId,
    inboxId: clean(conversation?.inbox_id) || clean(existing?.inboxId) || "inbox_email",
    title: clean(conversation?.title) || clean(existing?.title) || "Conversation",
    status: clean(event.data?.status) || clean(conversation?.status) || clean(existing?.status) || "open",
    priority: clean(conversation?.priority) || clean(existing?.priority) || "normal",
    channel: clean(conversation?.channel) || clean(existing?.channel) || "email",
    provider: clean(conversation?.provider) || clean(existing?.provider) || undefined,
    providerThreadId: clean(conversation?.provider_thread_id) || clean(existing?.providerThreadId) || undefined,
    assigneeUserId: clean(event.data?.assignee_user_id) || clean(conversation?.assignee_user_id) || clean(existing?.assigneeUserId) || undefined,
    assigneeName: clean(event.data?.assignee_name) || clean(conversation?.assignee_name) || clean(existing?.assigneeName) || undefined,
    contactName: clean(conversation?.contact?.name) || clean(existing?.contactName) || undefined,
    contactEmail: clean(conversation?.contact?.email) || clean(existing?.contactEmail) || undefined,
    tags: resolveTags(existing?.tags ?? [], conversation?.tags, event),
    lastMessagePreview: clean(conversation?.last_message_preview) || clean(existing?.lastMessagePreview) || undefined,
    lastMessageAt: toMillis(conversation?.last_message_at) || existing?.lastMessageAt || undefined,
    createdAt: toMillis(conversation?.created_at) || existing?.createdAt || updatedAt,
    updatedAt,
  };
  if (existing) {
    await ctx.db.patch(existing._id, patch);
  } else {
    await ctx.db.insert("conversationInboxItems", patch);
  }
}

async function upsertMessage(ctx: any, externalOrgId: string, conversationId: string, messageId: string, message: any, event: ProjectionEvent) {
  const resolvedMessageId = clean(message?.id) || messageId;
  const resolvedConversationId = clean(message?.conversation_id) || conversationId;
  if (!externalOrgId || !resolvedConversationId || !resolvedMessageId) return;

  const existing = await ctx.db
    .query("conversationProjectedMessages")
    .withIndex("by_message", (q: any) => q.eq("messageId", resolvedMessageId))
    .first();
  const occurredAt = toMillis(message?.occurred_at) || toMillis(event.occurred_at) || Date.now();
  const patch = {
    externalOrgId,
    conversationId: resolvedConversationId,
    messageId: resolvedMessageId,
    direction: clean(message?.direction) || "inbound",
    senderType: clean(message?.sender_type) || "customer",
    senderName: clean(message?.sender_name) || undefined,
    senderEmail: clean(message?.sender_email) || undefined,
    bodyText: clean(message?.body_text) || "",
    bodyHtml: clean(message?.body_html) || undefined,
    internal: Boolean(message?.internal),
    provider: clean(message?.provider) || undefined,
    occurredAt,
    createdAt: toMillis(message?.created_at) || occurredAt,
  };
  if (existing) {
    await ctx.db.patch(existing._id, patch);
  } else {
    await ctx.db.insert("conversationProjectedMessages", patch);
  }
}

async function upsertAiAction(ctx: any, externalOrgId: string, conversationId: string, data: any) {
  const aiActionId = clean(data.ai_action_id);
  if (!externalOrgId || !aiActionId) return;
  const existing = await ctx.db
    .query("conversationAiActions")
    .withIndex("by_ai_action", (q: any) => q.eq("aiActionId", aiActionId))
    .first();
  const patch = {
    externalOrgId,
    conversationId,
    aiActionId,
    kind: clean(data.kind) || "review",
    status: clean(data.decision) || clean(data.status) || "reviewed",
    payload: data.payload,
    updatedAt: Date.now(),
    createdAt: existing?.createdAt ?? Date.now(),
  };
  if (existing) {
    await ctx.db.patch(existing._id, patch);
  } else {
    await ctx.db.insert("conversationAiActions", patch);
  }
}

function resolveTags(existing: string[], projected: unknown, event: ProjectionEvent) {
  let tags = Array.isArray(projected) ? projected.filter((tag): tag is string => typeof tag === "string") : existing;
  const tag = clean(event.data?.tag);
  if (tag && event.type === "tag.added" && !tags.includes(tag)) tags = [...tags, tag];
  if (tag && event.type === "tag.removed") tags = tags.filter((current) => current !== tag);
  return tags;
}

function clean(value: unknown) {
  return typeof value === "string" ? value.trim() : "";
}

function toMillis(value: unknown) {
  if (typeof value === "number" && Number.isFinite(value)) return value;
  if (typeof value !== "string" || !value.trim()) return 0;
  const parsed = Date.parse(value);
  return Number.isFinite(parsed) ? parsed : 0;
}
