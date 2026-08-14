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
import { internalAction } from "./_generated/server";
import { internal } from "./_generated/api";

const ORG_CORE_URL = process.env.ORG_CORE_URL || "";
const APPLICATION_ORG_CORE_SERVICE_TOKEN = process.env.APPLICATION_ORG_CORE_SERVICE_TOKEN || "";
const CONTROL_SPACE_MEMBERSHIP_BASE_URL =
  process.env.CONTROL_SPACE_MEMBERSHIP_BASE_URL || "";
const APPLICATION_SPACE_LIFECYCLE_TOKEN = process.env.APPLICATION_SPACE_LIFECYCLE_TOKEN || "";

const SERVICE_PRINCIPAL = "application-space-lifecycle";

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
          body: JSON.stringify({ members }),
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
    const rooms = await ctx.runQuery(internal.spaces.listActiveOrganizationRooms, {});
    for (const room of rooms) {
      await ctx.runAction(internal.spaceMembershipSync.syncOrganizationRoom, {
        spaceRef: room.spaceRef,
        externalOrgId: room.externalOrgId,
      });
    }
    return { rooms: rooms.length };
  },
});
