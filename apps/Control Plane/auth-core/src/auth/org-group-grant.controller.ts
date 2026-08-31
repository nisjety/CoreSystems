import {
  BadRequestException,
  Body,
  Controller,
  Headers,
  HttpCode,
  HttpStatus,
  Logger,
  Post,
  ServiceUnavailableException,
  UnauthorizedException,
} from '@nestjs/common';
import { createHash, randomUUID, timingSafeEqual } from 'node:crypto';
import { and, eq } from 'drizzle-orm';

import { db } from '../db';
import { member, orgGroup, orgGroupGrant } from '../db/schema';
import { getOrganizationEventPublisher } from './organization-hooks';

/**
 * D-A grant write path. Without this the org_group_grant table is unreachable
 * except by hand-written SQL, so nothing could ever publish a change and any
 * downstream consumer (billing-core's plan inheritance) would sit inert.
 *
 * Deliberately an /internal service-to-service surface, not an end-user route:
 * granting another organization standing access to your data is an
 * administrative act, and the interactive surface for it is a separate product
 * decision that has not been made.
 */

function credentialMatches(expected: string, received?: string): boolean {
  const expectedDigest = createHash('sha256').update(expected).digest();
  const receivedDigest = createHash('sha256')
    .update((received ?? '').trim())
    .digest();
  return timingSafeEqual(expectedDigest, receivedDigest);
}

export interface OrgGroupGrantState {
  organizationId: string;
  orgGroupId: string | null;
  hostOrganizationId: string | null;
  dataAccess: boolean;
  billingConsolidation: boolean;
}

interface GrantRequestBody {
  organizationId?: string;
  hostOrganizationId?: string;
  groupName?: string;
  dataAccess?: boolean;
  billingConsolidation?: boolean;
  actorUserId?: string;
}

function parseGrantRequest(
  body: unknown,
): Required<
  Pick<
    GrantRequestBody,
    'organizationId' | 'hostOrganizationId' | 'actorUserId'
  >
> & { groupName: string; dataAccess: boolean; billingConsolidation: boolean } {
  if (!body || typeof body !== 'object' || Array.isArray(body)) {
    throw new BadRequestException('a grant request object is required');
  }
  const b = body as GrantRequestBody;
  const organizationId = (b.organizationId ?? '').trim();
  const hostOrganizationId = (b.hostOrganizationId ?? '').trim();
  const actorUserId = (b.actorUserId ?? '').trim();
  const dataAccess = b.dataAccess === true;
  const billingConsolidation = b.billingConsolidation === true;

  if (!organizationId || !hostOrganizationId || !actorUserId) {
    throw new BadRequestException(
      'organizationId, hostOrganizationId, and actorUserId are required',
    );
  }
  if (organizationId === hostOrganizationId) {
    // Self-hosting is structurally valid in the schema but meaningless as a
    // product action, and it would let a caller silently no-op an inheritance
    // grant onto itself.
    throw new BadRequestException(
      'an organization cannot grant to a group it hosts itself',
    );
  }
  if (!dataAccess && !billingConsolidation) {
    // Matches the org_group_grant_has_a_grant CHECK: a row must carry at least
    // one grant. Revocation is the dedicated /revoke route, not an empty grant.
    throw new BadRequestException(
      'at least one of dataAccess or billingConsolidation must be granted',
    );
  }
  return {
    organizationId,
    hostOrganizationId,
    actorUserId,
    groupName: (b.groupName ?? '').trim() || 'Account',
    dataAccess,
    billingConsolidation,
  };
}

@Controller('api/v1/internal/org-groups')
export class OrgGroupGrantController {
  private readonly logger = new Logger(OrgGroupGrantController.name);
  private readonly expectedToken = (
    process.env.ORG_GROUP_GRANT_SERVICE_TOKEN ??
    process.env.INTERNAL_SERVICE_SECRET ??
    process.env.INTERNAL_API_KEY ??
    ''
  ).trim();

  private authenticate(callerToken: string | undefined): void {
    if (this.expectedToken === '') {
      // Fail closed rather than accepting every caller when unconfigured.
      this.logger.error(
        'ORG_GROUP_GRANT_SERVICE_TOKEN is not configured; refusing grant mutations',
      );
      throw new ServiceUnavailableException(
        'Grant administration is unavailable',
      );
    }
    if (!credentialMatches(this.expectedToken, callerToken)) {
      throw new UnauthorizedException('Valid grant credential required');
    }
  }

  /**
   * Grant (or update) an organization's opt-in toward a host organization's
   * group. Idempotent on (org_group_id, organization_id) so a repeated call
   * settles the same row rather than colliding with the unique constraint.
   */
  @Post('grant')
  @HttpCode(HttpStatus.OK)
  async grant(
    @Headers('x-org-group-grant-token') callerToken: string | undefined,
    @Body() body: unknown,
  ): Promise<OrgGroupGrantState> {
    this.authenticate(callerToken);
    const request = parseGrantRequest(body);

    let state: OrgGroupGrantState;
    try {
      state = await db.transaction(async (tx) => {
        // The actor must be an owner/admin of the HOST org: joining someone's
        // account is authorized by that account, and this mirrors the
        // admin-principal rule the read path already enforces.
        const [actor] = await tx
          .select({ role: member.role })
          .from(member)
          .where(
            and(
              eq(member.userId, request.actorUserId),
              eq(member.organizationId, request.hostOrganizationId),
            ),
          )
          .limit(1);
        const actorRole = actor?.role.toLowerCase();
        if (actorRole !== 'owner' && actorRole !== 'admin') {
          throw new UnauthorizedException(
            'actor is not an administrator of the host organization',
          );
        }

        const [existingGroup] = await tx
          .select({ id: orgGroup.id })
          .from(orgGroup)
          .where(eq(orgGroup.hostOrganizationId, request.hostOrganizationId))
          .limit(1);

        const groupId = existingGroup?.id ?? randomUUID();
        if (!existingGroup) {
          await tx.insert(orgGroup).values({
            id: groupId,
            name: request.groupName,
            hostOrganizationId: request.hostOrganizationId,
          });
        }

        await tx
          .insert(orgGroupGrant)
          .values({
            id: randomUUID(),
            orgGroupId: groupId,
            organizationId: request.organizationId,
            dataAccess: request.dataAccess,
            billingConsolidation: request.billingConsolidation,
            grantedBy: request.actorUserId,
          })
          .onConflictDoUpdate({
            target: [orgGroupGrant.orgGroupId, orgGroupGrant.organizationId],
            set: {
              dataAccess: request.dataAccess,
              billingConsolidation: request.billingConsolidation,
              grantedBy: request.actorUserId,
              updatedAt: new Date(),
            },
          });

        return {
          organizationId: request.organizationId,
          orgGroupId: groupId,
          hostOrganizationId: request.hostOrganizationId,
          dataAccess: request.dataAccess,
          billingConsolidation: request.billingConsolidation,
        };
      });
    } catch (error) {
      if (error instanceof UnauthorizedException) throw error;
      this.logger.error(
        `org group grant failed: ${error instanceof Error ? error.message : 'unknown'}`,
      );
      throw new ServiceUnavailableException(
        'Grant administration is unavailable',
      );
    }

    await this.announce(state, request.actorUserId);
    return state;
  }

  /** Revoke every grant an organization holds toward its host's group. */
  @Post('revoke')
  @HttpCode(HttpStatus.OK)
  async revoke(
    @Headers('x-org-group-grant-token') callerToken: string | undefined,
    @Body() body: unknown,
  ): Promise<OrgGroupGrantState> {
    this.authenticate(callerToken);
    if (!body || typeof body !== 'object' || Array.isArray(body)) {
      throw new BadRequestException('a revoke request object is required');
    }
    const b = body as GrantRequestBody;
    const organizationId = (b.organizationId ?? '').trim();
    const actorUserId = (b.actorUserId ?? '').trim();
    if (!organizationId || !actorUserId) {
      throw new BadRequestException(
        'organizationId and actorUserId are required',
      );
    }

    try {
      await db
        .delete(orgGroupGrant)
        .where(eq(orgGroupGrant.organizationId, organizationId));
    } catch (error) {
      this.logger.error(
        `org group revoke failed: ${error instanceof Error ? error.message : 'unknown'}`,
      );
      throw new ServiceUnavailableException(
        'Grant administration is unavailable',
      );
    }

    // Null host: consumers must fall back to the org's own plan.
    const state: OrgGroupGrantState = {
      organizationId,
      orgGroupId: null,
      hostOrganizationId: null,
      dataAccess: false,
      billingConsolidation: false,
    };
    await this.announce(state, actorUserId);
    return state;
  }

  /**
   * Announce the change so billing-core can mirror it. A publish failure must
   * not fail the mutation the caller already committed — the grant is durable
   * in Postgres, and a missed notification is a staleness problem, not a
   * correctness one. It is logged loudly so it is not silent.
   */
  private async announce(
    state: OrgGroupGrantState,
    actorUserId: string,
  ): Promise<void> {
    try {
      const publisher = getOrganizationEventPublisher();
      if (!publisher) {
        this.logger.warn(
          'org group grant changed but no event publisher is available',
        );
        return;
      }
      await publisher.publishOrganizationBillingGroupChanged({
        organizationId: state.organizationId,
        orgGroupId: state.orgGroupId,
        hostOrganizationId: state.hostOrganizationId,
        billingConsolidation: state.billingConsolidation,
        changedBy: actorUserId,
      });
    } catch (error) {
      this.logger.error(
        `org group grant change was committed but not announced: ${
          error instanceof Error ? error.message : 'unknown'
        }`,
      );
    }
  }
}
