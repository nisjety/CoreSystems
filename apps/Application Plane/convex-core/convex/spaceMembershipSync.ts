/**
 * Organization room membership sync: org-core roster -> Control Space roster.
 *
 * The product rule is that every active organization user belongs to the one
 * organization room. org-core is the roster's owner, so this reads from there
 * and declares the result to Control rather than Application maintaining a
 * second list that would drift.
 *
 * At-least-once and idempotent by construction: Control's endpoint is
 * declarative, so re-running with the same roster changes nothing and does not
 * advance the membership revision.
 */
import { v } from "convex/values";
import { internalAction, internalMutation } from "./_generated/server";
import { internal } from "./_generated/api";

const ORG_CORE_URL = process.env.ORG_CORE_URL || "";
const APPLICATION_ORG_CORE_SERVICE_TOKEN = process.env.APPLICATION_ORG_CORE_SERVICE_TOKEN || "";
const CONTROL_SPACE_MEMBERSHIP_BASE_URL =
  process.env.CONTROL_SPACE_MEMBERSHIP_BASE_URL || "";
const APPLICATION_SPACE_LIFECYCLE_TOKEN = process.env.APPLICATION_SPACE_LIFECYCLE_TOKEN || "";

const SERVICE_PRINCIPAL = "application-space-lifecycle";

/** Quiet period that coalesces a burst of org-core membership events into one sync. */
export const ORG_ROOM_SYNC_DEBOUNCE_MS = 5000;

/**
 * Kill switch `space_org_room_reconcile_sync`, read at handler time (never at
 * module load) so `convex env set` plus a functions push takes effect without a
 * code change. It is a rollout switch only: Control still decides who is a
 * member, and this flag is never an authorization input.
 */
export function orgRoomReconcileEnabled(): boolean {
  const raw = (process.env.SPACE_ORG_ROOM_RECONCILE || "").trim().toLowerCase();
  return raw === "1" || raw === "true";
}

type OrgRoomSyncRequestRow = { state: "scheduled" | "running"; dueAt: number } | null;

/**
 * Debounce decision for `requestOrganizationRoomSync`. A request that is still
 * waiting for its quiet period absorbs further events; anything else (no row, a
 * run in flight, or a scheduled run that is overdue because it was lost) gets a
 * fresh run so no change made after the last org-core read is left behind.
 */
export function planOrgRoomSyncRequest(
  row: OrgRoomSyncRequestRow,
  now: number,
): { action: "skip" } | { action: "schedule"; dueAt: number; delayMs: number } {
  if (row && row.state === "scheduled" && row.dueAt > now) return { action: "skip" };
  return {
    action: "schedule",
    dueAt: now + ORG_ROOM_SYNC_DEBOUNCE_MS,
    delayMs: ORG_ROOM_SYNC_DEBOUNCE_MS,
  };
}

function configurationError(): string | null {
  if (!ORG_CORE_URL.trim()) return "ORG_CORE_URL is not configured";
  if (!APPLICATION_ORG_CORE_SERVICE_TOKEN.trim()) return "APPLICATION_ORG_CORE_SERVICE_TOKEN is not configured";
  if (!CONTROL_SPACE_MEMBERSHIP_BASE_URL.trim()) return "CONTROL_SPACE_MEMBERSHIP_BASE_URL is not configured";
  if (!APPLICATION_SPACE_LIFECYCLE_TOKEN.trim()) return "APPLICATION_SPACE_LIFECYCLE_TOKEN is not configured";
  return null;
}

type OrgMember = { user_id?: unknown; status?: unknown };

/**
 * Only `active` members become Space members.
 *
 * org-core's list also returns `invited` and `suspended`. An invitation is not
 * yet a membership, and a suspension is a deliberate removal of access —
 * putting either into a shared room would hand out content on the strength of
 * a state that specifically withholds it.
 *
 * Every member maps to `editor`, never to `manager` or `owner`. Being in the
 * organization is what grants a place in its room; an org admin's authority is
 * over the organization, not automatically over this Space's roster. Inferring
 * elevation here would create Space authority nobody granted. Control preserves
 * the registered owner separately, so this never needs to name one.
 */
export function spaceMembersFromOrgRoster(
  members: readonly OrgMember[],
): { subject_type: "user"; subject_id: string; role: "editor" }[] {
  const seen = new Set<string>();
  const grants: { subject_type: "user"; subject_id: string; role: "editor" }[] = [];
  for (const member of members) {
    if (member?.status !== "active") continue;
    const subjectId = typeof member?.user_id === "string" ? member.user_id.trim() : "";
    if (!subjectId || seen.has(subjectId)) continue;
    seen.add(subjectId);
    grants.push({ subject_type: "user", subject_id: subjectId, role: "editor" });
  }
  return grants;
}

export const syncOrganizationRoom = internalAction({
  args: { spaceRef: v.string(), externalOrgId: v.string() },
  handler: async (ctx, args) => {
    if (!orgRoomReconcileEnabled()) {
      return { status: "disabled" as const };
    }
    const misconfigured = configurationError();
    if (misconfigured) {
      // Deliberately not retried on a schedule: a missing credential is an
      // operator fact, and a retry storm would only bury it. The room stays
      // owner-only, which is the truthful subset.
      console.warn(`organization room membership sync skipped: ${misconfigured}`);
      return { status: "not_configured" as const, reason: misconfigured };
    }

    let roster: Response;
    try {
      roster = await fetch(
        `${ORG_CORE_URL.replace(/\/$/, "")}/orgs/${encodeURIComponent(args.externalOrgId)}/members`,
        {
          headers: {
            "X-Service-Id": SERVICE_PRINCIPAL,
            "X-Service-Token": APPLICATION_ORG_CORE_SERVICE_TOKEN,
          },
        },
      );
    } catch {
      return { status: "roster_unavailable" as const };
    }
    if (!roster.ok) {
      return { status: "roster_unavailable" as const, httpStatus: roster.status };
    }
    let payload: { members?: readonly OrgMember[] };
    try {
      payload = await roster.json();
    } catch {
      return { status: "roster_unavailable" as const };
    }
    // An unreadable roster must never be treated as an empty one: Control's
    // endpoint is declarative, so posting [] would revoke every member.
    if (!Array.isArray(payload?.members)) {
      return { status: "roster_unavailable" as const };
    }
    const members = spaceMembersFromOrgRoster(payload.members);

    let applied: Response;
    try {
      applied = await fetch(
        `${CONTROL_SPACE_MEMBERSHIP_BASE_URL.replace(/\/$/, "")}/${encodeURIComponent(args.spaceRef)}/memberships`,
        {
          method: "PUT",
          headers: {
            "Content-Type": "application/json",
            "X-Service-Id": SERVICE_PRINCIPAL,
            "X-Service-Token": APPLICATION_SPACE_LIFECYCLE_TOKEN,
          },
          // org-core's roster is people and nothing else. Saying so keeps
          // Control's convergence from revoking the room's agents as a side
          // effect of a human roster syncing — they are bound by a different
          // decision that this call knows nothing about.
          //
          // `roster_mode: "org_derived"` declares that org-core owns this
          // roster (WS04-S4 makes Control record it and refuse manual member
          // writes to the room). Control ignores the field until then.
          body: JSON.stringify({
            members,
            managed_subject_types: ["user"],
            roster_mode: "org_derived",
          }),
        },
      );
    } catch {
      return { status: "control_unavailable" as const };
    }
    if (!applied.ok) {
      return { status: "rejected" as const, httpStatus: applied.status };
    }
    return { status: "applied" as const, members: members.length };
  },
});

/** Re-sync every registered organization room. Safe to call repeatedly. */
export const syncAllOrganizationRooms = internalAction({
  args: {},
  handler: async (ctx) => {
    if (!orgRoomReconcileEnabled()) {
      return { status: "disabled" as const, rooms: 0 };
    }
    const rooms = await ctx.runQuery(internal.spaces.listActiveOrganizationRooms, {});
    for (const room of rooms) {
      await ctx.runAction(internal.spaceMembershipSync.syncOrganizationRoom, {
        spaceRef: room.spaceRef,
        externalOrgId: room.externalOrgId,
      });
    }
    return { status: "ok" as const, rooms: rooms.length };
  },
});

/**
 * Debounced entry point for the org-core membership event hook. The first event
 * of a burst schedules one run five seconds out; the rest are absorbed. A lost
 * event or a lost run is healed by the 10-minute cron in `crons.ts`.
 */
export const requestOrganizationRoomSync = internalMutation({
  args: { externalOrgId: v.string() },
  handler: async (ctx, args) => {
    if (!orgRoomReconcileEnabled()) return { status: "disabled" as const };
    const externalOrgId = args.externalOrgId.trim();
    if (!externalOrgId) return { status: "ignored" as const };
    const now = Date.now();
    const existing = await ctx.db
      .query("spaceOrgRoomSyncRequests")
      .withIndex("by_external_org", (q) => q.eq("externalOrgId", externalOrgId))
      .first();
    const plan = planOrgRoomSyncRequest(
      existing ? { state: existing.state, dueAt: existing.dueAt } : null,
      now,
    );
    if (plan.action === "skip") return { status: "coalesced" as const };
    if (existing) {
      await ctx.db.patch(existing._id, { dueAt: plan.dueAt, state: "scheduled" });
    } else {
      await ctx.db.insert("spaceOrgRoomSyncRequests", {
        externalOrgId,
        dueAt: plan.dueAt,
        state: "scheduled",
      });
    }
    await ctx.scheduler.runAfter(
      plan.delayMs,
      internal.spaceMembershipSync.runRequestedOrganizationRoomSync,
      { externalOrgId },
    );
    return { status: "scheduled" as const, dueAt: plan.dueAt };
  },
});

/** Marks the org's debounce row as running. A missing row is fine: it only means the cron got there first. */
export const markOrganizationRoomSyncRunning = internalMutation({
  args: { externalOrgId: v.string() },
  handler: async (ctx, args) => {
    const row = await ctx.db
      .query("spaceOrgRoomSyncRequests")
      .withIndex("by_external_org", (q) => q.eq("externalOrgId", args.externalOrgId))
      .first();
    if (row) await ctx.db.patch(row._id, { state: "running" });
    return { found: row !== null };
  },
});

/**
 * Deletes the debounce row only if it is still `running`. A request that
 * arrived during the run set it back to `scheduled` with its own queued run,
 * and that row must survive.
 */
export const clearOrganizationRoomSyncRequest = internalMutation({
  args: { externalOrgId: v.string() },
  handler: async (ctx, args) => {
    const row = await ctx.db
      .query("spaceOrgRoomSyncRequests")
      .withIndex("by_external_org", (q) => q.eq("externalOrgId", args.externalOrgId))
      .first();
    const cleared = row !== null && row.state === "running";
    if (row && cleared) await ctx.db.delete(row._id);
    return { cleared };
  },
});

/** Runs the debounced sync for every active organization room of one organization. */
export const runRequestedOrganizationRoomSync = internalAction({
  args: { externalOrgId: v.string() },
  handler: async (ctx, args) => {
    if (!orgRoomReconcileEnabled()) {
      await ctx.runMutation(internal.spaceMembershipSync.clearOrganizationRoomSyncRequest, {
        externalOrgId: args.externalOrgId,
      });
      return { status: "disabled" as const };
    }
    await ctx.runMutation(internal.spaceMembershipSync.markOrganizationRoomSyncRunning, {
      externalOrgId: args.externalOrgId,
    });
    try {
      const rooms = await ctx.runQuery(internal.spaces.listActiveOrganizationRooms, {});
      let synced = 0;
      for (const room of rooms) {
        if (room.externalOrgId !== args.externalOrgId) continue;
        await ctx.runAction(internal.spaceMembershipSync.syncOrganizationRoom, {
          spaceRef: room.spaceRef,
          externalOrgId: room.externalOrgId,
        });
        synced += 1;
      }
      return { status: "ok" as const, rooms: synced };
    } finally {
      await ctx.runMutation(internal.spaceMembershipSync.clearOrganizationRoomSyncRequest, {
        externalOrgId: args.externalOrgId,
      });
    }
  },
});
