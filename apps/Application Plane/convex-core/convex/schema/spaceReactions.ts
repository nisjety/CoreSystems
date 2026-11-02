/**
 * Message reactions (WS12-S8).
 *
 * Table definition only; composed by `../schema.ts` (P-CX1). Kept in its own file so the slice
 * does not touch the shared `spaces.ts` table file other Space slices edit.
 */

import { defineTable } from "convex/server";
import { v } from "convex/values";

export const spaceReactionTables = {
  // One person's one emoji on one message. Ids, one pictographic cluster and a timestamp only:
  // never message text, never a display name, so it is not ZDR-sensitive content. Removing a
  // reaction deletes the row (no tombstone). Application owns it because "who reacted to what" is
  // a workspace projection; Session Core stores the message and knows nothing of the reaction. The
  // gateway proves the caller's role, the shared-read decision and that the message exists in
  // that Space thread before it writes here; reading a message still goes through Session Core.
  spaceReactions: defineTable({
    externalOrgId: v.string(),
    spaceRef: v.string(),
    threadId: v.string(),
    messageId: v.string(),
    emoji: v.string(),
    externalAuthId: v.string(),
    createdAt: v.number(),
  })
    .index("by_space_thread", ["spaceRef", "threadId"])
    .index("by_message_subject_emoji", ["messageId", "externalAuthId", "emoji"])
    .index("by_space_subject", ["spaceRef", "externalAuthId"]),
};
