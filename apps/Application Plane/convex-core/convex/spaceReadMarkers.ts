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
}

/**
 * Which threads changed after the member last caught up.
 *
 * - No marker means the member has never opened the room: nothing is "new" in
 *   the sense the badge means, because there is no last visit to be new since.
 *   Marking everything unread on a first visit would badge a hundred posts at
 *   once and teach people to ignore the badge.
 * - A thread the member started themselves is never unread to them. Their own
 *   post arriving is not news.
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
    if (viewer && (thread.owner_subject_id ?? "").trim() === viewer) continue;
    const at = latestActivityMillis(thread);
    if (at === undefined) continue;
    if (at > lastReadAt) out.push(id);
  }
  return out;
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

async function loadMarker(ctx: any, spaceRef: string, externalAuthId: string) {
  return ctx.db
    .query("spaceReadMarkers")
    .withIndex("by_space_and_subject", (q: any) =>
      q.eq("spaceRef", spaceRef).eq("externalAuthId", externalAuthId),
    )
    .unique();
}

/** The caller's own marker for a room. `null` when they have never caught up. */
export const spaceReadMarkerForGateway = query({
  args: {
    serviceKey: v.string(),
    externalAuthId: v.string(),
    externalOrgId: v.string(),
    spaceRef: v.string(),
  },
  handler: async (ctx, args): Promise<{ lastReadAt: number | null }> => {
    assertServiceKey(args.serviceKey);
    await requireGatewayMember(ctx, args.externalAuthId, args.externalOrgId);
    const spaceRef = args.spaceRef.trim();
    if (!spaceRef) throw new Error("A Space reference is required");
    const marker = await loadMarker(ctx, spaceRef, args.externalAuthId);
    if (!marker || marker.externalOrgId !== args.externalOrgId) return { lastReadAt: null };
    return { lastReadAt: marker.lastReadAt };
  },
});

/**
 * The caller has the room open now. Records nothing about which threads exist
 * or what they say — only that this person caught up at this moment.
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
  },
  handler: async (ctx, args): Promise<{ lastReadAt: number }> => {
    assertServiceKey(args.serviceKey);
    await requireGatewayMember(ctx, args.externalAuthId, args.externalOrgId);
    const spaceRef = args.spaceRef.trim();
    if (!spaceRef) throw new Error("A Space reference is required");
    const now = Date.now();
    const existing = await loadMarker(ctx, spaceRef, args.externalAuthId);
    if (existing) {
      const lastReadAt = advanceMarker(existing.lastReadAt, now);
      if (lastReadAt !== existing.lastReadAt) {
        await ctx.db.patch(existing._id, { lastReadAt });
      }
      return { lastReadAt };
    }
    await ctx.db.insert("spaceReadMarkers", {
      spaceRef,
      externalOrgId: args.externalOrgId,
      externalAuthId: args.externalAuthId,
      lastReadAt: now,
    });
    return { lastReadAt: now };
  },
});
