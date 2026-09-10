/**
 * Who is in a room right now, and who is writing.
 *
 * # Why a new table rather than `conversationPresence`
 *
 * `conversationPresence` has existed, dormant, with zero functions and zero
 * readers since the older Application Plane conversation model. It is keyed
 * per *conversation*. A room renders every one of its threads inline, so
 * "who is here" is a fact about the room, not about a thread — the same
 * reasoning that made read markers room-level. Reusing that table would have
 * meant storing a `spaceRef` in a column named `conversationId`, and an index
 * whose name says the opposite of what it holds. It stays dormant.
 *
 * # Application owns this, deliberately
 *
 * Threads are Model Plane's and membership is Control's; neither owns "who is
 * looking at this right now". That is a workspace projection, which is this
 * plane's job by the ownership matrix. A row stores a status and a timestamp
 * against identifiers — never content, never a thread id, never what anyone
 * typed — so Zero Data Retention has nothing to propagate through, and the row
 * cannot say what someone was reading, only that they were here.
 *
 * # Absence is never stored, it is derived
 *
 * A browser that crashes, sleeps or is closed by the operating system sends no
 * goodbye. So presence is a *heartbeat with an expiry*: a row counts as here
 * only while it is fresh. `offline` exists for the polite case where the tab
 * does get to say it is leaving; it is never required for correctness.
 *
 * Rows are bounded by (rooms x members) and are overwritten in place, so
 * nothing accumulates and no cleanup job is needed.
 */
import { v } from "convex/values";
import { mutation } from "./_generated/server";
import { assertServiceKey, requireGatewayMember } from "./authz";

/**
 * How long a heartbeat counts as "here". Five times the room's 6 s poll, so a
 * single dropped request, a slow network or a browser throttling a background
 * timer never makes someone flicker out of the room.
 */
export const PRESENCE_TTL_MS = 30_000;

/**
 * How long a `typing` heartbeat still means "is writing". Much shorter than
 * presence: someone who typed a word and then stopped to think is still in the
 * room, but saying "is writing…" about them thirty seconds later is a lie the
 * room tells on their behalf. They fall back to plain presence.
 */
export const TYPING_TTL_MS = 8_000;

export interface PresenceRowLike {
  readonly externalAuthId: string;
  readonly status: string;
  readonly updatedAt: number;
}

export interface PresentMember {
  readonly subject_id: string;
  readonly status: "online" | "typing";
  readonly last_seen_at: number;
}

/**
 * Who is present, from raw rows.
 *
 * - A row older than {@link PRESENCE_TTL_MS} is gone, whatever it says.
 * - An explicit `offline` (or `away`, from the older schema's vocabulary) is
 *   not present.
 * - `typing` decays to `online` after {@link TYPING_TTL_MS} rather than
 *   vanishing: the person is still here, they just stopped typing.
 * - The viewer never appears in their own list. "Who else is here" is the
 *   question the room is asking.
 * - Ordered by identifier, not by recency, so the line does not reshuffle
 *   itself every six seconds while nobody's state has changed.
 */
export function presentMembers(
  rows: readonly PresenceRowLike[],
  options: { readonly now: number; readonly viewerAuthId?: string | null },
): PresentMember[] {
  const viewer = (options.viewerAuthId ?? "").trim();
  const out: PresentMember[] = [];
  for (const row of rows) {
    const id = (row.externalAuthId ?? "").trim();
    if (!id || id === viewer) continue;
    if (!Number.isFinite(row.updatedAt)) continue;
    const age = options.now - row.updatedAt;
    if (age < 0 || age > PRESENCE_TTL_MS) continue;
    if (row.status !== "online" && row.status !== "typing") continue;
    out.push({
      subject_id: id,
      status: row.status === "typing" && age <= TYPING_TTL_MS ? "typing" : "online",
      last_seen_at: row.updatedAt,
    });
  }
  out.sort((a, b) => a.subject_id.localeCompare(b.subject_id));
  return out;
}

/** The statuses a caller may send. Anything else is refused by the gateway. */
export function normalizeStatus(status: string | null | undefined): "online" | "typing" | "offline" | null {
  const value = (status ?? "").trim().toLowerCase();
  if (value === "" || value === "online") return "online";
  if (value === "typing") return "typing";
  if (value === "offline") return "offline";
  return null;
}

/**
 * "I am here" — and, in the same round trip, "who else is?".
 *
 * A heartbeat that only wrote would need a second request to be useful, on a
 * timer that already runs every six seconds. Answering from the write keeps
 * the room to one request per beat, and the answer is necessarily as fresh as
 * the write that produced it.
 *
 * Space membership is the calling gateway's check (lifecycle and current
 * Control membership, as for every Space read); this plane verifies
 * organization membership and will only ever write the caller's own row —
 * `externalAuthId` is the caller's identity, never a target.
 */
export const recordSpacePresenceForGateway = mutation({
  args: {
    serviceKey: v.string(),
    externalAuthId: v.string(),
    externalOrgId: v.string(),
    spaceRef: v.string(),
    status: v.union(v.literal("online"), v.literal("typing"), v.literal("offline")),
  },
  handler: async (
    ctx,
    args,
  ): Promise<{ present: PresentMember[]; ttlSeconds: number }> => {
    assertServiceKey(args.serviceKey);
    await requireGatewayMember(ctx, args.externalAuthId, args.externalOrgId);
    const spaceRef = args.spaceRef.trim();
    if (!spaceRef) throw new Error("A Space reference is required");

    const now = Date.now();
    const existing = await ctx.db
      .query("spacePresence")
      .withIndex("by_space_and_subject", (q) =>
        q.eq("spaceRef", spaceRef).eq("externalAuthId", args.externalAuthId),
      )
      .unique();
    if (existing) {
      await ctx.db.patch(existing._id, {
        status: args.status,
        updatedAt: now,
        externalOrgId: args.externalOrgId,
      });
    } else {
      await ctx.db.insert("spacePresence", {
        spaceRef,
        externalOrgId: args.externalOrgId,
        externalAuthId: args.externalAuthId,
        status: args.status,
        updatedAt: now,
      });
    }

    const rows = await ctx.db
      .query("spacePresence")
      .withIndex("by_space", (q) => q.eq("spaceRef", spaceRef))
      .collect();
    // Cross-organization rows can only exist if a Space reference were ever
    // reused across orgs; filtered rather than assumed away.
    const scoped = rows.filter((row) => row.externalOrgId === args.externalOrgId);
    return {
      present: presentMembers(scoped, { now, viewerAuthId: args.externalAuthId }),
      ttlSeconds: Math.round(PRESENCE_TTL_MS / 1000),
    };
  },
});
