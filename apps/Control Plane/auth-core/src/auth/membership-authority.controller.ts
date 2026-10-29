import {
  BadRequestException,
  Body,
  Controller,
  Get,
  Headers,
  HttpCode,
  HttpStatus,
  Param,
  Post,
  ServiceUnavailableException,
  UnauthorizedException,
} from '@nestjs/common';
import { createHash, timingSafeEqual } from 'node:crypto';
import { and, asc, eq } from 'drizzle-orm';

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

interface MembershipListRecord extends MembershipRecord {
  userId: string;
}

export interface MembershipDecision {
  version: 'v1';
  member: boolean;
  role: string | null;
}

export interface MembershipList {
  version: 'v1';
  organizationId: string;
  members: Array<{
    user_id: string;
    role: string;
    status: 'active';
  }>;
}

function requireDedicatedSecret(
  name: string,
  value?: string,
  forbiddenValues: Array<string | undefined> = [],
): string {
  const secret = (value ?? '').trim();
  if (secret.length < 32) {
    throw new Error(`${name} must contain at least 32 characters`);
  }
  const lowered = secret.toLowerCase();
  if (placeholderPrefixes.some((prefix) => lowered.startsWith(prefix))) {
    throw new Error(`${name} must not be a placeholder`);
  }
  if (
    forbiddenValues.some(
      (forbidden) =>
        (forbidden ?? '').trim() !== '' && credentialMatches(secret, forbidden),
    )
  ) {
    throw new Error(`${name} must be a dedicated credential`);
  }
  return secret;
}

export function requireMembershipAuthoritySecret(
  value?: string,
  forbiddenValues: Array<string | undefined> = [],
): string {
  return requireDedicatedSecret(
    'USER_CORE_MEMBERSHIP_SERVICE_TOKEN',
    value,
    forbiddenValues,
  );
}

export function requireApplicationReconcilerSecret(
  value?: string,
  forbiddenValues: Array<string | undefined> = [],
): string {
  return requireDedicatedSecret(
    'APPLICATION_RECONCILER_AUTH_TOKEN',
    value,
    forbiddenValues,
  );
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

export function buildMembershipList(
  organizationId: string,
  records: MembershipListRecord[],
): MembershipList {
  const normalizedOrgID = organizationId.trim();
  if (!normalizedOrgID) {
    throw new Error('organization id is required');
  }
  const seenUsers = new Set<string>();
  const members = records.map((record) => {
    const userID = record.userId.trim();
    const role = record.role.trim().toLowerCase();
    if (!userID) {
      throw new Error('canonical membership has an invalid user id');
    }
    if (seenUsers.has(userID)) {
      throw new Error('canonical membership is ambiguous');
    }
    seenUsers.add(userID);
    if (!allowedRoles.has(role)) {
      throw new Error('canonical membership has an unsupported role');
    }
    return { user_id: userID, role, status: 'active' as const };
  });
  return {
    version: 'v1',
    organizationId: normalizedOrgID,
    members: members.toSorted((left, right) =>
      left.user_id.localeCompare(right.user_id),
    ),
  };
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
      process.env.ORG_CORE_SERVICE_TOKEN,
      process.env.BILLING_CORE_SERVICE_TOKEN,
      process.env.APPLICATION_RECONCILER_AUTH_TOKEN,
    ],
  );

  private readonly applicationReconcilerToken =
    requireApplicationReconcilerSecret(
      process.env.APPLICATION_RECONCILER_AUTH_TOKEN,
      [
        process.env.INTERNAL_API_KEY,
        process.env.INTERNAL_SERVICE_SECRET,
        process.env.USER_CORE_SERVICE_TOKEN,
        process.env.USER_CORE_MEMBERSHIP_SERVICE_TOKEN,
        process.env.ORG_CORE_SERVICE_TOKEN,
        process.env.BILLING_CORE_SERVICE_TOKEN,
      ],
    );

  // This is a membership *decision* lookup, not a resource creation. NestJS
  // defaults @Post to 201; user-core's canonical-authority client treats any
  // non-200 as "authority unavailable" (-> 503), so pin it to 200.
  @Post('decision')
  @HttpCode(HttpStatus.OK)
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

  @Get('organizations/:orgId/members')
  async listOrganizationMembers(
    @Headers('x-service-id') callerServiceID: string | undefined,
    @Headers('x-service-token') callerToken: string | undefined,
    @Param('orgId') orgID: string,
  ): Promise<MembershipList> {
    if (
      callerServiceID !== 'application-reconciler' ||
      !credentialMatches(this.applicationReconcilerToken, callerToken)
    ) {
      throw new UnauthorizedException(
        'Valid Application membership reconciler credential required',
      );
    }
    const normalizedOrgID = orgID.trim();
    if (!normalizedOrgID || normalizedOrgID.length > 128) {
      throw new BadRequestException('Valid organization id required');
    }

    try {
      const rows = await db
        .select({ userId: member.userId, role: member.role })
        .from(member)
        .where(eq(member.organizationId, normalizedOrgID))
        .orderBy(asc(member.userId));
      return buildMembershipList(normalizedOrgID, rows);
    } catch {
      throw new ServiceUnavailableException(
        'Canonical membership authority unavailable',
      );
    }
  }
}
