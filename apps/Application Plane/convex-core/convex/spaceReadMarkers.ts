/**
 * Where a member last caught up with a room.
 *
 * # One marker per member per room, not per thread
 *
 * Slack's channel model: opening the channel is reading it, and "new" means
 * "since you last had it open". A per-thread marker would need a write for
 * every post rendered in the timeline — the room renders them all inline, so
 * that is a write per post per visit for no extra truth. The room-level marker
 * gives the same answer the reference products give and costs one write per
 * visit.
 *
 * # Application owns this, deliberately
 *
 * Threads live in Model Plane and membership in Control; neither owns "what
 * this person has seen". That is a workspace projection, which is this plane's
 * job. It stores only a timestamp against identifiers — never a thread id list
 * and never content — so it says nothing about *what* was read, only *when*.
 *
 * # The derivation is a pure function, on purpose
 *
 * `unreadThreadIds` is exported and tested directly (`test/space-read-markers
 * .test.cjs`), the way every Space-side decision in this package is: the
 * handlers below are thin shells around it plus the auth checks and indexed
 * reads a unit test cannot exercise without a real deployment.
 */
import { v } from "convex/values";
import { mutation, query } from "./_generated/server";
import { assertServiceKey, requireGatewayMember } from "./authz";

export interface ThreadActivityLike {
  readonly thread_id: string;
  readonly updated_at?: string | null;
  readonly latest_run_updated_at?: string | null;
  readonly owner_subject_id?: string | null;
  /** Who wrote the latest message (WS01-S1). Absent means unknown: the owner rule applies. */
  readonly last_author_subject_id?: string | null;
}

/**
 * Which threads changed after the member last caught up.
 *
 * - No marker means the member has never opened the room: nothing is "new" in
 *   the sense the badge means, because there is no last visit to be new since.
 *   Marking everything unread on a first visit would badge a hundred posts at
 *   once and teach people to ignore the badge.
 * - Activity the member wrote themselves is never unread to them. With
 *   `last_author_subject_id` that is "the latest message is theirs", so someone
 *   else's reply in a thread they started badges; without the field the older
 *   rule applies (a thread they started is never unread). Mirrors the SPA's
 *   `space-unread.ts`; they must agree.
 * - A thread with no readable timestamp cannot be placed relative to the
 *   marker and is left alone rather than guessed at.
 */
export function unreadThreadIds(
  threads: readonly ThreadActivityLike[],
  lastReadAt: number | null | undefined,
  viewerSubjectId: string | null | undefined,
): string[] {
  if (lastReadAt === null || lastReadAt === undefined || !Number.isFinite(lastReadAt)) return [];
  const viewer = (viewerSubjectId ?? "").trim();
  const out: string[] = [];
  for (const thread of threads) {
    const id = (thread.thread_id ?? "").trim();
    if (!id) continue;
    if (viewer && lastWriterOf(thread) === viewer) continue;
    const at = latestActivityMillis(thread);
    if (at === undefined) continue;
    if (at > lastReadAt) out.push(id);
  }
  return out;
}

function lastWriterOf(thread: ThreadActivityLike): string {
  const last = (thread.last_author_subject_id ?? "").trim();
  return last || (thread.owner_subject_id ?? "").trim();
}

function latestActivityMillis(thread: ThreadActivityLike): number | undefined {
  const candidates = [thread.updated_at, thread.latest_run_updated_at]
    .map((value) => (typeof value === "string" && value.trim() ? Date.parse(value) : Number.NaN))
    .filter((value) => Number.isFinite(value));
  if (candidates.length === 0) return undefined;
  return Math.max(...candidates);
}

/**
 * The marker never moves backwards. A slow poll landing after a fresh visit
 * must not un-read what the fresh visit read.
 */
export function advanceMarker(existing: number | null | undefined, now: number): number {
  if (existing === null || existing === undefined || !Number.isFinite(existing)) return now;
  return Math.max(existing, now);
}

export type MarkReadReason = "enter" | "while_open";

export interface MarkerRowLike {
  readonly lastReadAt: number;
  readonly unreadFloor?: number | null;
}

/**
 * The marker unread is measured against: the manual floor when one is set,
 * otherwise `lastReadAt`. The floor is always <= `lastReadAt`.
 */
export function effectiveMarker(row: MarkerRowLike | null | undefined): number | null {
  if (!row) return null;
  const floor = row.unreadFloor;
  if (typeof floor === "number" && Number.isFinite(floor)) return Math.min(floor, row.lastReadAt);
  return row.lastReadAt;
}

export interface MarkReadPlan {
  readonly lastReadAt: number;
  /** `null` when no floor remains. */
  readonly unreadFloor: number | null;
  readonly write: boolean;
}

/**
 * What one "I have the room open" call does.
 *
 * - `while_open` (the default, and what every poll of an open room sends): while
 *   a manual floor is set it is a no-op, so a slow poll cannot undo "mark
 *   unread"; otherwise the marker advances, never backwards.
 * - `enter` (the first call after navigating to the room): clears the floor and
 *   advances. This is the only way a floor goes away besides read-all.
 */
export function planMarkRead(
  existing: MarkerRowLike | null | undefined,
  now: number,
  reason: MarkReadReason,
): MarkReadPlan {
  if (!existing) return { lastReadAt: now, unreadFloor: null, write: true };
  const hasFloor =
    typeof existing.unreadFloor === "number" && Number.isFinite(existing.unreadFloor);
  if (hasFloor && reason !== "enter") {
    return {
      lastReadAt: existing.lastReadAt,
      unreadFloor: existing.unreadFloor as number,
      write: false,
    };
  }
  const lastReadAt = advanceMarker(existing.lastReadAt, now);
  return {
    lastReadAt,
    unreadFloor: null,
    write: lastReadAt !== existing.lastReadAt || hasFloor,
  };
}

/**
 * "Mark unread from here": the floor becomes `min(existing floor ?? lastReadAt,
 * beforeMs)`. Ignored (`null`) when there is no marker or `beforeMs` is not
 * earlier than the effective marker, because then nothing would become unread.
 * This is the one deliberate backward move, and it is an explicit user action.
 */
export function planMarkUnread(
  existing: MarkerRowLike | null | undefined,
  beforeMs: number,
): number | null {
  if (!existing) return null;
  if (typeof beforeMs !== "number" || !Number.isFinite(beforeMs) || beforeMs <= 0) return null;
  const current = effectiveMarker(existing) as number;
  if (beforeMs >= current) return null;
  return Math.floor(beforeMs);
}

async function loadMarker(ctx: any, spaceRef: string, externalAuthId: string) {
  return ctx.db
    .query("spaceReadMarkers")
    .withIndex("by_space_and_subject", (q: any) =>
      q.eq("spaceRef", spaceRef).eq("externalAuthId", externalAuthId),
    )
    .unique();
}

export interface MarkerView {
  /**
   * The EFFECTIVE marker (floor if set, else the real one). Keeping this name
   * means a caller written before the floor existed stays correct.
   */
  lastReadAt: number | null;
  /** The real catch-up time, ignoring the floor. */
  readThroughAt: number | null;
  manualUnread: boolean;
}

function viewOf(row: MarkerRowLike | null | undefined): MarkerView {
  if (!row) return { lastReadAt: null, readThroughAt: null, manualUnread: false };
  const manual = typeof row.unreadFloor === "number" && Number.isFinite(row.unreadFloor);
  return {
    lastReadAt: effectiveMarker(row),
    readThroughAt: row.lastReadAt,
    manualUnread: manual,
  };
}

/** The caller's own marker for a room. `lastReadAt` is `null` when they have never caught up. */
export const spaceReadMarkerForGateway = query({
  args: {
    serviceKey: v.string(),
    externalAuthId: v.string(),
    externalOrgId: v.string(),
    spaceRef: v.string(),
  },
  handler: async (ctx, args): Promise<MarkerView> => {
    assertServiceKey(args.serviceKey);
    await requireGatewayMember(ctx, args.externalAuthId, args.externalOrgId);
    const spaceRef = args.spaceRef.trim();
    if (!spaceRef) throw new Error("A Space reference is required");
    const marker = await loadMarker(ctx, spaceRef, args.externalAuthId);
    if (!marker || marker.externalOrgId !== args.externalOrgId) return viewOf(null);
    return viewOf(marker);
  },
});

/**
 * The caller has the room open now. Records nothing about which threads exist
 * or what they say — only that this person caught up at this moment.
 *
 * `reason` (WS11-S3): absent means `while_open`. While a manual unread floor is
 * set, `while_open` changes nothing and answers with the floor; `enter` clears
 * the floor and advances (see {@link planMarkRead}).
 *
 * Space membership is the calling gateway's check (it verifies lifecycle and
 * current Control membership before calling, as it does for every Space read);
 * this plane verifies organization membership and refuses to write for anyone
 * else's identity, since `externalAuthId` is the caller's own.
 */
export const markSpaceReadForGateway = mutation({
  args: {
    serviceKey: v.string(),
    externalAuthId: v.string(),
    externalOrgId: v.string(),
    spaceRef: v.string(),
    reason: v.optional(v.union(v.literal("enter"), v.literal("while_open"))),
  },
  handler: async (ctx, args): Promise<MarkerView> => {
    assertServiceKey(args.serviceKey);
    await requireGatewayMember(ctx, args.externalAuthId, args.externalOrgId);
    const spaceRef = args.spaceRef.trim();
    if (!spaceRef) throw new Error("A Space reference is required");
    const now = Date.now();
    const existing = await loadMarker(ctx, spaceRef, args.externalAuthId);
    const plan = planMarkRead(existing, now, args.reason ?? "while_open");
    if (existing) {
      if (plan.write) {
        await ctx.db.patch(existing._id, {
          lastReadAt: plan.lastReadAt,
          unreadFloor: plan.unreadFloor ?? undefined,
        });
      }
    } else {
      await ctx.db.insert("spaceReadMarkers", {
        spaceRef,
        externalOrgId: args.externalOrgId,
        externalAuthId: args.externalAuthId,
        lastReadAt: plan.lastReadAt,
      });
    }
    return viewOf({ lastReadAt: plan.lastReadAt, unreadFloor: plan.unreadFloor });
  },
});

/**
 * "Mark unread": sets the manual floor so everything newer than `beforeMs`
 * reads as unread again. Ignored (answers the unchanged marker) when the caller
 * has no marker or `beforeMs` is not earlier than the effective marker.
 */
export const markUnreadForGateway = mutation({
  args: {
    serviceKey: v.string(),
    externalAuthId: v.string(),
    externalOrgId: v.string(),
    spaceRef: v.string(),
    beforeMs: v.number(),
  },
  handler: async (ctx, args): Promise<MarkerView> => {
    assertServiceKey(args.serviceKey);
    await requireGatewayMember(ctx, args.externalAuthId, args.externalOrgId);
    const spaceRef = args.spaceRef.trim();
    if (!spaceRef) throw new Error("A Space reference is required");
    const existing = await loadMarker(ctx, spaceRef, args.externalAuthId);
    if (!existing || existing.externalOrgId !== args.externalOrgId) return viewOf(null);
    const floor = planMarkUnread(existing, args.beforeMs);
    if (floor === null) return viewOf(existing);
    await ctx.db.patch(existing._id, { unreadFloor: floor });
    return viewOf({ lastReadAt: existing.lastReadAt, unreadFloor: floor });
  },
});
