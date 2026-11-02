/**
 * Space attention, notification preferences, status and reminders (WS11-S3).
 *
 * Table definitions only; composed by `../schema.ts` (P-CX1). Kept in its own
 * file so the slice does not touch the shared `spaces.ts` table file other
 * Space slices edit. The `unreadFloor` field and the `by_org_and_subject`
 * index of the existing `spaceReadMarkers` table live next to that table in
 * `spaces.ts`.
 *
 * Every row here stores ids, closed words and timestamps only. No row has a
 * title, a message body, a display name or a status text that comes from a
 * persistent room (the one free-text field, `spaceMemberStatus.text`, is
 * written only by WS11-S10 and is refused for ZDR organizations at the
 * gateway). Nothing here is an authorization input.
 */

import { defineTable } from "convex/server";
import { v } from "convex/values";

const roomLevel = v.union(v.literal("all"), v.literal("mentions"), v.literal("off"));

export const attentionKind = v.union(
  v.literal("mention"),
  v.literal("reply"),
  v.literal("invite"),
  v.literal("agent_done"),
  v.literal("agent_failed"),
  v.literal("reminder"),
);

export const spaceAttentionTables = {
  // Per-room notification level of one member. Absent row = the default level.
  spaceRoomNotifyPrefs: defineTable({
    externalOrgId: v.string(),
    externalAuthId: v.string(),
    spaceRef: v.string(),
    level: roomLevel,
    updatedAt: v.number(),
  })
    .index("by_subject_and_space", ["externalAuthId", "spaceRef"])
    .index("by_subject", ["externalOrgId", "externalAuthId"]),

  // Per-member notification settings that are not per room (hide previews in
  // the inbox and in out-of-app messages).
  spaceNotifyUserSettings: defineTable({
    externalOrgId: v.string(),
    externalAuthId: v.string(),
    hidePreview: v.boolean(),
    updatedAt: v.number(),
  })
    .index("by_subject", ["externalOrgId", "externalAuthId"]),

  // Thread follow / mute. An explicit `muted` is never overwritten by an
  // auto-follow (the ingest in WS11-S6 enforces it with `mergeFollow`).
  spaceThreadFollows: defineTable({
    externalOrgId: v.string(),
    externalAuthId: v.string(),
    spaceRef: v.string(),
    threadId: v.string(),
    state: v.union(v.literal("following"), v.literal("muted")),
    reason: v.union(
      v.literal("started"),
      v.literal("participated"),
      v.literal("mentioned"),
      v.literal("manual"),
    ),
    updatedAt: v.number(),
  })
    .index("by_subject_and_thread", ["externalAuthId", "threadId"])
    .index("by_thread", ["threadId"])
    .index("by_space", ["spaceRef"]),

  // The inbox rows. Ids and closed words only: `actorRef` is an id, never a
  // name, and there is no text field. Titles are re-read at render under the
  // viewer's current Control decision.
  spaceAttentionItems: defineTable({
    externalOrgId: v.string(),
    recipientAuthId: v.string(),
    spaceRef: v.string(),
    kind: attentionKind,
    threadId: v.optional(v.string()),
    messageId: v.optional(v.string()),
    runId: v.optional(v.string()),
    actorKind: v.union(v.literal("user"), v.literal("agent"), v.literal("system")),
    actorRef: v.optional(v.string()),
    state: v.union(v.literal("open"), v.literal("done"), v.literal("snoozed")),
    snoozedUntil: v.optional(v.number()),
    count: v.number(),
    firstEventId: v.string(),
    lastEventId: v.string(),
    coalesceKey: v.string(),
    createdAt: v.number(),
    updatedAt: v.number(),
    expiresAt: v.number(),
  })
    .index("by_recipient_state_updated", ["recipientAuthId", "state", "updatedAt"])
    .index("by_recipient_coalesce", ["recipientAuthId", "coalesceKey"])
    .index("by_space", ["spaceRef"])
    .index("by_thread", ["threadId"])
    .index("by_expiry", ["expiresAt"]),

  // At-least-once dedupe for the event ingest (WS11-S6); purged after 48 h.
  spaceAttentionEvents: defineTable({
    eventId: v.string(),
    createdAt: v.number(),
  })
    .index("by_event_id", ["eventId"])
    .index("by_created", ["createdAt"]),

  // Out-of-app rate limit: one fixed window per recipient.
  spaceAttentionBudget: defineTable({
    recipientAuthId: v.string(),
    windowStart: v.number(),
    count: v.number(),
  })
    .index("by_recipient", ["recipientAuthId"]),

  // Org-scoped status (WS11-S10). The table exists from S3 so the schema ships once.
  spaceMemberStatus: defineTable({
    externalOrgId: v.string(),
    externalAuthId: v.string(),
    state: v.union(
      v.literal("available"),
      v.literal("away"),
      v.literal("dnd"),
      v.literal("offline"),
    ),
    text: v.optional(v.string()),
    expiresAt: v.optional(v.number()),
    updatedAt: v.number(),
  })
    .index("by_org_and_subject", ["externalOrgId", "externalAuthId"]),

  // Agent availability chips (WS11-S10 reads, WS11-S6 writes).
  spaceAgentActivity: defineTable({
    externalOrgId: v.string(),
    spaceRef: v.string(),
    agentId: v.string(),
    state: v.union(v.literal("working"), v.literal("waiting"), v.literal("idle")),
    runId: v.optional(v.string()),
    updatedAt: v.number(),
  })
    .index("by_space_and_agent", ["spaceRef", "agentId"]),

  // Reminders (WS11-S11).
  spaceReminders: defineTable({
    externalOrgId: v.string(),
    externalAuthId: v.string(),
    spaceRef: v.string(),
    threadId: v.string(),
    messageId: v.optional(v.string()),
    remindAt: v.number(),
    state: v.union(
      v.literal("pending"),
      v.literal("fired"),
      v.literal("cancelled"),
      v.literal("skipped"),
    ),
    createdAt: v.number(),
  })
    .index("by_subject_and_state", ["externalAuthId", "state"])
    .index("by_due", ["state", "remindAt"]),
};
