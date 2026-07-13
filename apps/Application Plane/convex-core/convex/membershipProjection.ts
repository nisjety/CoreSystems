export type ProjectionRole = "admin" | "member" | "viewer";

export function normalizeProjectionRole(role: string): ProjectionRole {
  switch (role.trim().toLowerCase()) {
    case "owner":
    case "admin":
      return "admin";
    case "member":
      return "member";
    case "viewer":
      return "viewer";
    default:
      throw new Error(`Unsupported Control Plane membership role: ${role}`);
  }
}

export function shouldApplyMembershipAdd(
  tombstoneSourceUpdatedAt: number | undefined,
  membershipSourceUpdatedAt: number | undefined,
  incomingSourceUpdatedAt: number,
): boolean {
  if (
    tombstoneSourceUpdatedAt !== undefined &&
    tombstoneSourceUpdatedAt >= incomingSourceUpdatedAt
  ) {
    return false;
  }
  return !(
    membershipSourceUpdatedAt !== undefined &&
    membershipSourceUpdatedAt > incomingSourceUpdatedAt
  );
}

export function shouldApplyMembershipRemoval(
  membershipSourceUpdatedAt: number | undefined,
  incomingSourceUpdatedAt: number,
): boolean {
  return !(
    membershipSourceUpdatedAt !== undefined &&
    membershipSourceUpdatedAt > incomingSourceUpdatedAt
  );
}
