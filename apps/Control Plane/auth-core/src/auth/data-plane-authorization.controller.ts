import {
  BadRequestException,
  Body,
  Controller,
  ForbiddenException,
  Headers,
  Logger,
  Post,
  ServiceUnavailableException,
  UnauthorizedException,
} from '@nestjs/common';
import { and, eq } from 'drizzle-orm';

import { db } from '../db';
import { member } from '../db/schema';
import {
  ConvexTokenService,
  PlaneTokenVerificationError,
  type VerifiedPlaneServicePrincipal,
} from './convex-token.service';

export interface MembershipRecord {
  role: string;
  createdAt: Date;
}

interface DataPlaneDecisionRequest {
  userId?: string;
  orgId?: string;
  action?: string;
}

type DataPlaneAction = 'data.read' | 'data.admin';

export interface DataPlaneDecision {
  version: 'v1';
  allowed: boolean;
  role: string | null;
  permissions: string[];
  membershipRevision: string | null;
  reason: 'member' | 'not_member' | 'insufficient_role';
}

export function buildDataPlaneDecision(
  membership: MembershipRecord | null,
  action: DataPlaneAction,
): DataPlaneDecision {
  if (!membership) {
    return {
      version: 'v1',
      allowed: false,
      role: null,
      permissions: [],
      membershipRevision: null,
      reason: 'not_member',
    };
  }

  const role = membership.role.toLowerCase();
  const permissions = ['data:read'];
  if (role === 'owner' || role === 'admin') {
    permissions.push('org:data:read_all');
  }
  if (action === 'data.admin' && role !== 'owner' && role !== 'admin') {
    return {
      version: 'v1',
      allowed: false,
      role,
      permissions,
      membershipRevision: membership.createdAt.toISOString(),
      reason: 'insufficient_role',
    };
  }
  if (action === 'data.admin') {
    permissions.push('data:admin');
  }
  return {
    version: 'v1',
    allowed: true,
    role,
    permissions,
    membershipRevision: membership.createdAt.toISOString(),
    reason: 'member',
  };
}

async function lookupMembership(
  userId: string,
  orgId: string,
): Promise<MembershipRecord | null> {
  const [row] = await db
    .select({ role: member.role, createdAt: member.createdAt })
    .from(member)
    .where(and(eq(member.userId, userId), eq(member.organizationId, orgId)))
    .limit(1);
  return row ?? null;
}

@Controller('api/v1/internal/authorization/data-plane')
export class DataPlaneAuthorizationController {
  private readonly logger = new Logger(DataPlaneAuthorizationController.name);
  private readonly expectedCallerSubject = `service:${(
    process.env.DATA_PLANE_POLICY_CALLER_SERVICE_ID ?? 'retrieval-engine'
  ).trim()}`;
  private readonly requiredScope = 'data:authorization:decide';

  constructor(private readonly convexTokenService: ConvexTokenService) {}

  @Post('decision')
  async decide(
    @Headers('authorization') authorization: string | undefined,
    @Body() body: DataPlaneDecisionRequest,
  ): Promise<DataPlaneDecision> {
    const principal = this.authenticateCaller(authorization);

    const userId = (body.userId ?? '').trim();
    const orgId = (body.orgId ?? '').trim();
    const action = (body.action ?? '').trim();
    if (!userId || !orgId || !action) {
      throw new BadRequestException('userId, orgId, and action are required');
    }
    if (!['data.read', 'data.admin'].includes(action)) {
      throw new BadRequestException('unsupported Data Plane action');
    }
    if (principal.orgId !== orgId) {
      throw new ForbiddenException(
        'Policy caller tenant does not match request',
      );
    }

    try {
      const decision = buildDataPlaneDecision(
        await lookupMembership(userId, orgId),
        action as DataPlaneAction,
      );
      this.logger.log(
        JSON.stringify({
          event: 'data_plane_authorization_decision',
          version: decision.version,
          caller: principal.serviceId,
          callerReason: principal.reason,
          orgId,
          userId,
          action,
          allowed: decision.allowed,
          decisionReason: decision.reason,
        }),
      );
      return decision;
    } catch {
      throw new ServiceUnavailableException(
        'Canonical membership authority unavailable',
      );
    }
  }

  private authenticateCaller(
    authorization: string | undefined,
  ): VerifiedPlaneServicePrincipal {
    const match = authorization?.match(
      /^Bearer ([A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+)$/,
    );
    if (!match) {
      throw new UnauthorizedException('Valid policy caller token required');
    }

    let principal: VerifiedPlaneServicePrincipal;
    try {
      principal = this.convexTokenService.verifyPlaneServiceToken(
        'control-policy',
        match[1],
      );
    } catch (error) {
      if (error instanceof PlaneTokenVerificationError) {
        throw new UnauthorizedException('Valid policy caller token required');
      }
      throw new ServiceUnavailableException(
        'Policy caller verification is unavailable',
      );
    }

    if (principal.subject !== this.expectedCallerSubject) {
      throw new ForbiddenException('Policy caller is not authorized');
    }
    if (!principal.scopes.includes(this.requiredScope)) {
      throw new ForbiddenException('Policy caller scope is not authorized');
    }
    return principal;
  }
}
