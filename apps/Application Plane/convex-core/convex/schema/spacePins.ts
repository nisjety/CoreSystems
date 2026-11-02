/**
 * Room pins (WS13-S8).
 *
 * Table definition only; composed by `../schema.ts` (P-CX1). Kept in its own file so the slice
 * does not touch the shared `spaces.ts` table file other Space slices edit.
 */

import { defineTable } from "convex/server";
import { v } from "convex/values";

export const spacePinTables = {
  // A room pin: "this post matters to the room", set by an editor or above and shown to every
  // member. Ids and a timestamp only, never a title and never content, so it is not
  // ZDR-sensitive. Application owns it because "what the room has pinned" is a workspace
  // projection; Session Core's owner-only `pinned_at` is a different, personal fact and stays
  // untouched. The gateway checks the caller's role and that the thread belongs to the room
  // (through a Control read decision) before it writes here.
  spacePins: defineTable({
    spaceRef: v.string(),
    externalOrgId: v.string(),
    threadId: v.string(),
    pinnedByExternalAuthId: v.string(),
    pinnedAt: v.number(),
  })
    .index("by_space", ["spaceRef", "pinnedAt"])
    .index("by_space_and_thread", ["spaceRef", "threadId"]),
};
