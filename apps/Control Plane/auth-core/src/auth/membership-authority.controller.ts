import {
  BadRequestException,
  Body,
  Controller,
  Headers,
  Post,
  ServiceUnavailableException,
  UnauthorizedException,
} from '@nestjs/common';
import { createHash, timingSafeEqual } from 'node:crypto';
import { and, eq } from 'drizzle-orm';

import { db } from '../db';
import { member } from '../db/schema';

const allowedRoles = new Set(['owner', 'admin', 'member', 'viewer']);
const placeholderPrefixes = [
  'test',
  'placeholder',
  'change-me',
  'your-',
  'replace-me',
];

interface MembershipRecord {
  role: string;
}

export interface MembershipDecision {
  version: 'v1';
  member: boolean;
  role: string | null;
}

export function requireMembershipAuthoritySecret(
  value?: string,
  forbiddenValues: Array<string | undefined> = [],
): string {
  const secret = (value ?? '').trim();
  if (secret.length < 32) {
    throw new Error(
      'USER_CORE_MEMBERSHIP_SERVICE_TOKEN must contain at least 32 characters',
    );
  }
  const lowered = secret.toLowerCase();
  if (placeholderPrefixes.some((prefix) => lowered.startsWith(prefix))) {
    throw new Error(
      'USER_CORE_MEMBERSHIP_SERVICE_TOKEN must not be a placeholder',
    );
  }
  if (
    forbiddenValues.some(
      (forbidden) =>
        (forbidden ?? '').trim() !== '' && credentialMatches(secret, forbidden),
    )
  ) {
    throw new Error(
      'USER_CORE_MEMBERSHIP_SERVICE_TOKEN must be a dedicated credential',
    );
  }
  return secret;
}

export function buildMembershipDecision(
  membership: MembershipRecord | null,
): MembershipDecision {
  if (!membership) {
    return { version: 'v1', member: false, role: null };
  }
  const role = membership.role.trim().toLowerCase();
  if (!allowedRoles.has(role)) {
    throw new Error('canonical membership has an unsupported role');
  }
  return { version: 'v1', member: true, role };
}

function credentialMatches(expected: string, received?: string): boolean {
  const expectedDigest = createHash('sha256').update(expected).digest();
  const receivedDigest = createHash('sha256')
    .update((received ?? '').trim())
    .digest();
  return timingSafeEqual(expectedDigest, receivedDigest);
}

function parseDecisionRequest(body: unknown): {
  userId: string;
  orgId: string;
} {
  if (!body || typeof body !== 'object' || Array.isArray(body)) {
    throw new BadRequestException('userId and orgId are required');
  }
  const record = body as Record<string, unknown>;
  if (
    Object.keys(record).some((key) => key !== 'userId' && key !== 'orgId') ||
    typeof record.userId !== 'string' ||
    typeof record.orgId !== 'string'
  ) {
    throw new BadRequestException('Only string userId and orgId are accepted');
  }
  const userId = record.userId.trim();
  const orgId = record.orgId.trim();
  if (!userId || !orgId) {
    throw new BadRequestException('userId and orgId are required');
  }
  return { userId, orgId };
}

async function lookupMembership(
  userId: string,
  orgId: string,
): Promise<MembershipRecord | null> {
  const rows = await db
    .select({ role: member.role })
    .from(member)
    .where(and(eq(member.userId, userId), eq(member.organizationId, orgId)))
    .limit(2);
  if (rows.length > 1) {
    throw new Error('canonical membership is ambiguous');
  }
  return rows[0] ?? null;
}

@Controller('api/v1/internal/membership')
export class MembershipAuthorityController {
  private readonly expectedToken = requireMembershipAuthoritySecret(
    process.env.USER_CORE_MEMBERSHIP_SERVICE_TOKEN,
    [
      process.env.INTERNAL_API_KEY,
      process.env.INTERNAL_SERVICE_SECRET,
      process.env.USER_CORE_SERVICE_TOKEN,
    ],
  );

  @Post('decision')
  async decide(
    @Headers('x-user-core-membership-token') callerToken: string | undefined,
    @Body() body: unknown,
  ): Promise<MembershipDecision> {
    if (!credentialMatches(this.expectedToken, callerToken)) {
      throw new UnauthorizedException(
        'Valid membership authority credential required',
      );
    }

    const { userId, orgId } = parseDecisionRequest(body);

    try {
      return buildMembershipDecision(await lookupMembership(userId, orgId));
    } catch {
      throw new ServiceUnavailableException(
        'Canonical membership authority unavailable',
      );
    }
  }
}
