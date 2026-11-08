/**
 * Room notices (WS15-S12).
 *
 * Table definition only; composed by `../schema.ts` (P-CX1). Kept in its own file so the slice
 * does not touch the shared `spaces.ts` table file other Space slices edit.
 */

import { defineTable } from "convex/server";
import { v } from "convex/values";

export const spaceRoomNoticeTables = {
  // A room-level change Application owns the presentation of: an agent bound, paused, resumed or
  // revoked, the room renamed, the topic changed. The room timeline shows one system row per
  // notice. Ids, a closed kind word and a timestamp only: no names (the SPA resolves those from
  // the roster and the agent list), no content, so it is not ZDR-sensitive. A notice never grants
  // anything; Control still decides who may read or act. Written in the same transaction as the
  // change it describes, pruned after 90 days.
  spaceRoomNotices: defineTable({
    noticeId: v.string(),
    spaceRef: v.string(),
    externalOrgId: v.string(),
    kind: v.union(
      v.literal("agent_bound"),
      v.literal("agent_paused"),
      v.literal("agent_resumed"),
      v.literal("agent_revoked"),
      v.literal("room_renamed"),
      v.literal("topic_changed"),
    ),
    actorExternalAuthId: v.optional(v.string()),
    subjectId: v.optional(v.string()),
    createdAt: v.number(),
  })
    .index("by_space_and_created", ["spaceRef", "createdAt"])
    .index("by_created", ["createdAt"]),
};
