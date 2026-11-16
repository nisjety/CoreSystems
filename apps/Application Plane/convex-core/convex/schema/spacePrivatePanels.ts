/**
 * Private panel pointers (WS17-S6).
 *
 * Table definition only; composed by `../schema.ts` (P-CX1). Kept in its own file so the slice does
 * not touch the shared `spaces.ts` table file other Space slices edit.
 */

import { defineTable } from "convex/server";
import { v } from "convex/values";

export const spacePrivatePanelTables = {
  // One row says "this person's private assistant for this room is that owner thread". Ids and
  // timestamps only: never a title, message text or agent instructions, so it is not ZDR-sensitive
  // content. The thread itself is Session Core's (an owner thread with `sourceScope:'conversation'`,
  // no memory in or out, SD-22 v1); a ZDR caller never reaches this table because the gateway refuses
  // the panel first. Application owns the pointer because "which thread is my panel for this room" is
  // a workspace projection. The gateway proves room membership through Control before it reads or
  // writes here; nothing in this row widens access to the thread or to the room.
  spacePrivatePanels: defineTable({
    externalAuthId: v.string(),
    externalOrgId: v.string(),
    spaceRef: v.string(),
    threadId: v.string(),
    agentSubjectId: v.optional(v.string()),
    createdAt: v.number(),
    updatedAt: v.number(),
  }).index("by_user_and_space", ["externalOrgId", "externalAuthId", "spaceRef"]),
};
