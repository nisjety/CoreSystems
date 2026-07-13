import { and, eq } from 'drizzle-orm';

import { db } from '../db';
import { member } from '../db/schema';

interface CanonicalMembership {
  organizationId: string;
  userId: string;
  role: string;
}

export interface CanonicalTokenContext {
  orgId: string;
  role: string;
}

export function canonicalTokenContext(
  activeOrganizationId: string,
  membership: CanonicalMembership | null,
): CanonicalTokenContext | null {
  if (
    !membership ||
    membership.organizationId !== activeOrganizationId ||
    !membership.userId
  ) {
    return null;
  }
  return { orgId: membership.organizationId, role: membership.role };
}

export async function resolveCanonicalTokenContext(
  userId: string,
  activeOrganizationId: string,
): Promise<CanonicalTokenContext | null> {
  const activeOrg = activeOrganizationId.trim();
  if (!userId.trim() || !activeOrg) return null;

  const [membership] = await db
    .select({
      organizationId: member.organizationId,
      userId: member.userId,
      role: member.role,
    })
    .from(member)
    .where(and(eq(member.userId, userId), eq(member.organizationId, activeOrg)))
    .limit(1);
  return canonicalTokenContext(activeOrg, membership ?? null);
}
