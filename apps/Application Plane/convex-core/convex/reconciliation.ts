import { normalizeProjectionRole, type ProjectionRole } from "./membershipProjection.ts";

export type CurrentMembership = {
  userId: string;
  role: string;
  syncStatus: string;
};

export type AuthoritativeMembership = {
  userId: string;
  role: string;
};

export type MembershipReconciliationPlan = {
  removals: string[];
  roleChanges: Array<{
    userId: string;
    previousRole: ProjectionRole;
    nextRole: ProjectionRole;
  }>;
  missing: string[];
  grants: never[];
  unsafePromotions: Array<{
    userId: string;
    previousRole: ProjectionRole;
    requestedRole: ProjectionRole;
  }>;
};

const roleRank: Record<ProjectionRole, number> = {
  viewer: 0,
  member: 1,
  admin: 2,
};

export function planMembershipReconciliation(
  currentMemberships: CurrentMembership[],
  authoritativeMemberships: AuthoritativeMembership[],
): MembershipReconciliationPlan {
  const authority = new Map<string, ProjectionRole>();
  for (const membership of authoritativeMemberships) {
    if (!membership.userId.trim()) throw new Error("Authority userId is required");
    if (authority.has(membership.userId)) {
      throw new Error(`Duplicate authoritative membership: ${membership.userId}`);
    }
    authority.set(membership.userId, normalizeProjectionRole(membership.role));
  }

  const activeCurrent = currentMemberships.filter(
    (membership) => membership.syncStatus !== "deleted"
  );
  const currentIds = new Set(activeCurrent.map((membership) => membership.userId));
  const removals: string[] = [];
  const roleChanges: MembershipReconciliationPlan["roleChanges"] = [];
  const unsafePromotions: MembershipReconciliationPlan["unsafePromotions"] = [];

  for (const membership of activeCurrent) {
    const authoritativeRole = authority.get(membership.userId);
    if (!authoritativeRole) {
      removals.push(membership.userId);
      continue;
    }
    const currentRole = normalizeProjectionRole(membership.role);
    if (currentRole !== authoritativeRole) {
      if (roleRank[authoritativeRole] < roleRank[currentRole]) {
        roleChanges.push({
          userId: membership.userId,
          previousRole: currentRole,
          nextRole: authoritativeRole,
        });
      } else {
        unsafePromotions.push({
          userId: membership.userId,
          previousRole: currentRole,
          requestedRole: authoritativeRole,
        });
      }
    }
  }

  return {
    removals: removals.sort(),
    roleChanges: roleChanges.sort((left, right) =>
      left.userId.localeCompare(right.userId)
    ),
    missing: [...authority.keys()].filter((userId) => !currentIds.has(userId)).sort(),
    // Reconciliation deliberately never creates a grant. Missing authority
    // members require the normal Control event/backfill path with full profile data.
    grants: [],
    // This endpoint deliberately cannot widen privilege. Promotions must flow
    // through the signed, authoritative Control Plane membership event path.
    unsafePromotions: unsafePromotions.sort((left, right) =>
      left.userId.localeCompare(right.userId)
    ),
  };
}
