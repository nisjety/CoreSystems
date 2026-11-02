/**
 * Room notices (WS15-S12): the Application half of the room timeline's system rows.
 *
 * Control's membership ledger answers "who joined, left or changed role". The changes Application
 * owns the presentation of (an agent bound, paused, resumed or revoked; the room renamed; the topic
 * changed) have no ledger, so each of those mutations writes one `spaceRoomNotices` row in its own
 * transaction. The gateway reads them for the interval a timeline page covers and merges them with
 * Control's rows; the SPA builds the sentence from the kind and ids.
 *
 * # What a notice is
 *
 * An id, a closed kind word, the acting person's id, an optional subject id and a time. No names
 * (the SPA resolves those from the roster and the agent list), no room text, no content, so there
 * is nothing for a Zero Data Retention organization to withhold. A notice grants nothing: it is
 * presentation, and Control still decides who may read the room or act in it.
 *
 * # Retention
 *
 * Notices older than 90 days are deleted by the hourly `pruneRoomNotices` cron (deletes only).
 */
import { v } from "convex/values";
import { internal } from "./_generated/api";
import { internalMutation, query } from "./_generated/server";
import { assertServiceKey, requireGatewayMember } from "./authz";

export const ROOM_NOTICE_KINDS = [
  "agent_bound",
  "agent_paused",
  "agent_resumed",
  "agent_revoked",
  "room_renamed",
  "topic_changed",
] as const;

export type RoomNoticeKind = (typeof ROOM_NOTICE_KINDS)[number];

/** Most notices one read returns; the gateway asks for at most this many per timeline page. */
export const MAX_NOTICES_PER_READ = 100;

/** Notices older than this are deleted. */
export const NOTICE_RETENTION_MS = 90 * 24 * 60 * 60 * 1000;

const PRUNE_BATCH = 200;
const MAX_ID_CHARS = 200;

/**
 * Appends one notice inside the CALLER's transaction. It is a plain database insert, so it commits
 * or rolls back with the change it describes. Call it only after that change is certain.
 */
export async function recordRoomNotice(
  ctx: any,
  notice: {
    spaceRef: string;
    externalOrgId: string;
    kind: RoomNoticeKind;
    actorExternalAuthId?: string;
    subjectId?: string;
  },
): Promise<void> {
  await ctx.db.insert("spaceRoomNotices", {
    noticeId: `rn_${crypto.randomUUID()}`,
    spaceRef: notice.spaceRef,
    externalOrgId: notice.externalOrgId,
    kind: notice.kind,
    actorExternalAuthId: notice.actorExternalAuthId,
    subjectId: notice.subjectId,
    createdAt: Date.now(),
  });
}

/** The notice kind for a binding moving to `status`, or `null` for a status that has none. */
export function noticeKindForBindingStatus(status: string): RoomNoticeKind | null {
  switch (status) {
    case "paused":
      return "agent_paused";
    case "active":
      return "agent_resumed";
    case "revoked":
      return "agent_revoked";
    default:
      return null;
  }
}

function cleanRef(value: string): string {
  const trimmed = (value ?? "").trim();
  if (!trimmed || trimmed.length > MAX_ID_CHARS || /[\u0000-\u001f\u007f]/.test(trimmed)) {
    throw new Error("room_notices_invalid:space_ref");
  }
  return trimmed;
}

/**
 * The room's notices in `[after, before)` (unix ms, either end open), newest first, at most
 * `limit` rows. `truncated` is true when more rows matched than were returned. Rows of another
 * organization are never returned.
 *
 * Service-key gated like the other `*ForGateway` functions. The gateway has already checked the
 * caller's live Control membership and the Space lifecycle; this verifies the organization
 * membership and scopes every row by organization and Space.
 */
export const roomNoticesForGateway = query({
  args: {
    serviceKey: v.string(),
    externalAuthId: v.string(),
    externalOrgId: v.string(),
    spaceRef: v.string(),
    after: v.optional(v.number()),
    before: v.optional(v.number()),
    limit: v.optional(v.number()),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    await requireGatewayMember(ctx, args.externalAuthId, args.externalOrgId);
    const spaceRef = cleanRef(args.spaceRef);
    const limit = Math.min(
      Math.max(Math.floor(args.limit ?? MAX_NOTICES_PER_READ), 1),
      MAX_NOTICES_PER_READ,
    );
    const rows = await ctx.db
      .query("spaceRoomNotices")
      .withIndex("by_space_and_created", (q: any) => {
        const scoped = q.eq("spaceRef", spaceRef);
        if (args.after !== undefined && args.before !== undefined) {
          return scoped.gte("createdAt", args.after).lt("createdAt", args.before);
        }
        if (args.after !== undefined) return scoped.gte("createdAt", args.after);
        if (args.before !== undefined) return scoped.lt("createdAt", args.before);
        return scoped;
      })
      .order("desc")
      .take(limit + 1);
    const inOrg = rows.filter((row: any) => row.externalOrgId === args.externalOrgId);
    return {
      notices: inOrg.slice(0, limit).map((row: any) => ({
        noticeId: row.noticeId as string,
        kind: row.kind as RoomNoticeKind,
        actorExternalAuthId: (row.actorExternalAuthId ?? null) as string | null,
        subjectId: (row.subjectId ?? null) as string | null,
        createdAt: row.createdAt as number,
      })),
      truncated: rows.length > limit,
    };
  },
});

/**
 * Hourly housekeeping: delete notices older than 90 days. Deletes only; safe to run repeatedly.
 * A full batch schedules the next one.
 */
export const pruneRoomNotices = internalMutation({
  args: {},
  handler: async (ctx): Promise<{ deleted: number }> => {
    const cutoff = Date.now() - NOTICE_RETENTION_MS;
    const old = await ctx.db
      .query("spaceRoomNotices")
      .withIndex("by_created", (q: any) => q.lt("createdAt", cutoff))
      .take(PRUNE_BATCH);
    for (const row of old) await ctx.db.delete(row._id);
    if (old.length >= PRUNE_BATCH) {
      await ctx.scheduler.runAfter(0, internal.spaceRoomNotices.pruneRoomNotices, {});
    }
    return { deleted: old.length };
  },
});
