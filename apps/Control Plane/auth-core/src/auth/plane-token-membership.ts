import { and, eq } from 'drizzle-orm';

import { db } from '../db';
import { member, user } from '../db/schema';

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

/**
 * Whether the account is banned right now: `user.banned` is true and the ban
 * has no expiry or its expiry is still ahead of `now`. Only the ban flag and
 * its expiry are read (never the email or name). An unknown user id reads as
 * not banned: the membership check already fails for it. A database error
 * propagates so the caller can answer 503 like the membership lookup.
 */
export async function isUserBanned(
  userId: string,
  now: Date = new Date(),
): Promise<boolean> {
  if (!userId.trim()) return false;

  const [row] = await db
    .select({ banned: user.banned, banExpires: user.banExpires })
    .from(user)
    .where(eq(user.id, userId))
    .limit(1);
  if (!row || row.banned !== true) return false;
  return row.banExpires == null || row.banExpires.getTime() > now.getTime();
}
