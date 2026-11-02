import {
  BadRequestException,
  Body,
  ConflictException,
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

import { sqlClient } from '../db';
import { auth } from './auth';

/**
 * Internal SCIM membership surface (WS05-S8), called by org-core only.
 *
 * org-core owns the SCIM tokens, the SCIM user records and the verified-domain
 * check; auth-core owns the Better Auth `member` table (no status column), so
 * the actual membership change happens here. Two operations, both idempotent:
 *
 *   POST /api/v1/internal/scim/memberships/provision
 *     find or create the user (no password: sign-in is SSO only), add the
 *     member row with the role org-core names (never `owner`), and bind the
 *     membership audit actor in the same transaction.
 *   POST /api/v1/internal/scim/memberships/deprovision
 *     remove the member row through `apply_membership_mutation` (kind
 *     `scim_deprovision`, migration 030) so the operation row, the projection
 *     revision, the audit outbox and the session cleanup commit atomically.
 *     The last owner is refused (409 last_owner).
 *
 * Authentication is the dedicated ORG_CORE_IDENTITY_SERVICE_TOKEN credential in
 * `x-sso-admin-token` (the pattern of sso-provider-admin.controller.ts). No
 * profile attribute is accepted, stored or logged; only ids and the identity key.
 */

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
const EMAIL = /^[^\s@]{1,64}@[^\s@]{1,255}\.[^\s@]{2,63}$/;
// owner is never granted through SCIM; admin only restores a prior admin.
const PROVISION_ROLES = new Set(['member', 'viewer', 'admin']);

function credentialMatches(expected: string, received?: string): boolean {
  const expectedDigest = createHash('sha256').update(expected).digest();
  const receivedDigest = createHash('sha256')
    .update((received ?? '').trim())
    .digest();
  return timingSafeEqual(expectedDigest, receivedDigest);
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === 'object' && !Array.isArray(value);
}

function text(value: unknown, max = 255): string {
  return typeof value === 'string' ? value.trim().slice(0, max) : '';
}

const userAuth = auth as unknown as {
  $context: Promise<{
    internalAdapter: {
      createUser(user: Record<string, unknown>): Promise<{ id: string }>;
    };
  }>;
};

type MemberRow = { id: string; role: string };

@Controller('api/v1/internal/scim/memberships')
export class ScimProvisioningController {
  private readonly logger = new Logger(ScimProvisioningController.name);
  private readonly expectedToken = (
    process.env.ORG_CORE_IDENTITY_SERVICE_TOKEN ?? ''
  ).trim();

  private authenticate(callerToken: string | undefined): void {
    if (this.expectedToken.length < 32) {
      this.logger.error(
        'ORG_CORE_IDENTITY_SERVICE_TOKEN is not configured; refusing SCIM provisioning',
      );
      throw new ServiceUnavailableException('SCIM provisioning unavailable');
    }
    if (!credentialMatches(this.expectedToken, callerToken)) {
      throw new UnauthorizedException('Valid SCIM provisioning credential required');
    }
  }

  private unavailable(action: string, error: unknown): never {
    this.logger.error(
      `scim ${action} failed: ${error instanceof Error ? error.name : 'unknown'}`,
    );
    throw new ServiceUnavailableException('SCIM provisioning unavailable');
  }

  private async findUserId(email: string): Promise<string | null> {
    const rows = await sqlClient<Array<{ id: string }>>`
      SELECT id FROM "user" WHERE lower(email) = ${email} LIMIT 2
    `;
    return rows.length === 1 ? rows[0].id : null;
  }

  @Post('provision')
  @HttpCode(HttpStatus.OK)
  async provision(
    @Headers('x-sso-admin-token') token: string | undefined,
    @Body() body: unknown,
  ) {
    this.authenticate(token);
    if (!isRecord(body)) throw new BadRequestException('an object is required');
    const operationId = text(body.operationId, 64);
    const organizationId = text(body.organizationId, 128);
    const actorUserId = text(body.actorUserId, 128);
    const email = text(body.email, 320).toLowerCase();
    const role = text(body.role, 16).toLowerCase() || 'member';
    if (!UUID.test(operationId) || !organizationId || !actorUserId) {
      throw new BadRequestException('operationId, organizationId and actorUserId are required');
    }
    if (!EMAIL.test(email)) {
      throw new BadRequestException({ code: 'invalid_email', message: 'email is not valid' });
    }
    if (!PROVISION_ROLES.has(role)) {
      throw new BadRequestException({ code: 'invalid_role', message: 'role is not allowed' });
    }
    try {
      const orgRows = await sqlClient<Array<{ id: string }>>`
        SELECT id FROM organization WHERE id = ${organizationId} LIMIT 1
      `;
      if (orgRows.length === 0) {
        throw new BadRequestException({ code: 'organization_not_found', message: 'organization not found' });
      }
      let userId = await this.findUserId(email);
      let userCreated = false;
      if (!userId) {
        // createUser runs the Better Auth database hooks, so user-core learns
        // of the person exactly as it does for any other sign-up. No password
        // account is created: the only way in is the organisation's SSO.
        const context = await userAuth.$context;
        const created = await context.internalAdapter.createUser({
          email,
          name: email.split('@')[0].slice(0, 64),
          emailVerified: false,
        });
        userId = created.id;
        userCreated = true;
      }
      const resolvedUserId = userId;
      const result = await sqlClient.begin(async (tx) => {
        const inserted = await tx<MemberRow[]>`
          INSERT INTO member (id, organization_id, user_id, role, created_at)
          VALUES (${randomUUID()}, ${organizationId}, ${resolvedUserId}, ${role}, NOW())
          ON CONFLICT (organization_id, user_id) DO NOTHING
          RETURNING id, role
        `;
        if (inserted.length === 0) {
          const existing = await tx<MemberRow[]>`
            SELECT id, role FROM member
            WHERE organization_id = ${organizationId} AND user_id = ${resolvedUserId}
            LIMIT 1
          `;
          if (existing.length !== 1) throw new Error('canonical member unavailable');
          return { created: false, member: existing[0] };
        }
        const member = inserted[0];
        // The projection trigger wrote a pending audit row for this exact
        // transition; bind its actor to the person who authorised the SCIM token.
        const bound = await tx`
          UPDATE organization_membership_audit_outbox audit
          SET actor_user_id = ${actorUserId},
              actor_classification = 'verified_user',
              updated_at = NOW()
          WHERE audit.organization_id = ${organizationId}
            AND audit.user_id = ${resolvedUserId}
            AND audit.member_id = ${member.id}
            AND audit.action = 'member_added'
            AND audit.applied_role = ${member.role}
            AND audit.actor_classification = 'pending'
            AND audit.published_at IS NULL
            AND audit.revision = (
              SELECT projection.revision FROM organization_membership_outbox projection
              WHERE projection.organization_id = audit.organization_id
                AND projection.user_id = audit.user_id
            )
        `;
        if (bound.count !== 1) {
          throw new Error('exact membership audit transition is unavailable');
        }
        return { created: true, member };
      });
      this.logger.log(
        `scim provision org=${organizationId} user=${resolvedUserId} created=${result.created} userCreated=${userCreated}`,
      );
      return {
        userId: resolvedUserId,
        memberId: result.member.id,
        role: result.member.role,
        memberCreated: result.created,
        userCreated,
      };
    } catch (error) {
      if (error instanceof BadRequestException) throw error;
      this.unavailable('provision', error);
    }
  }

  @Post('deprovision')
  @HttpCode(HttpStatus.OK)
  async deprovision(
    @Headers('x-sso-admin-token') token: string | undefined,
    @Body() body: unknown,
  ) {
    this.authenticate(token);
    if (!isRecord(body)) throw new BadRequestException('an object is required');
    const operationId = text(body.operationId, 64);
    const organizationId = text(body.organizationId, 128);
    const actorUserId = text(body.actorUserId, 128);
    const userId = text(body.userId, 128);
    if (!UUID.test(operationId) || !organizationId || !actorUserId || !userId) {
      throw new BadRequestException(
        'operationId, organizationId, userId and actorUserId are required',
      );
    }
    try {
      // A retry of an applied operation answers from the stored result.
      const replay = await sqlClient<Array<{ previous_role: string; audit_revision: string | null }>>`
        SELECT previous_role, audit_revision
        FROM organization_membership_mutation_operation
        WHERE operation_id = ${operationId}::UUID
          AND organization_id = ${organizationId}
          AND user_id = ${userId}
          AND kind = 'scim_deprovision'
          AND state = 'completed'
        LIMIT 1
      `;
      if (replay.length === 1) {
        return {
          removed: true,
          replay: true,
          role: replay[0].previous_role,
          revision: replay[0].audit_revision === null ? 0 : Number(replay[0].audit_revision),
        };
      }
      const rows = await sqlClient<Array<{ id: string; role: string; revision: string }>>`
        SELECT m.id, m.role, p.revision
        FROM member m
        JOIN organization_membership_outbox p
          ON p.organization_id = m.organization_id AND p.user_id = m.user_id
        WHERE m.organization_id = ${organizationId} AND m.user_id = ${userId}
        LIMIT 2
      `;
      if (rows.length === 0) {
        // Already not a member: the desired state holds.
        return { removed: false, replay: false, role: '', revision: 0 };
      }
      if (rows.length !== 1) throw new Error('canonical member unavailable');
      const row = rows[0];
      const applied = await sqlClient<Array<{ result: { revision?: number } }>>`
        SELECT apply_membership_mutation(
          ${operationId}::UUID,
          ${organizationId},
          ${userId},
          ${row.id},
          'scim_deprovision',
          ${Number(row.revision)},
          ${row.role},
          ${row.role},
          ${actorUserId}
        ) AS result
      `;
      const result = applied[0]?.result;
      this.logger.log(`scim deprovision org=${organizationId} user=${userId}`);
      return {
        removed: true,
        replay: false,
        role: row.role,
        revision: Number(result?.revision ?? 0),
      };
    } catch (error) {
      const message = error instanceof Error ? error.message : '';
      if (message.includes('only owner')) {
        throw new ConflictException({
          code: 'last_owner',
          message: 'The only organization owner cannot be deprovisioned.',
        });
      }
      if (message.includes('membership mutation conflict') || message.includes('revision conflicted')) {
        throw new ConflictException({
          code: 'membership_conflict',
          message: 'Membership changed concurrently; retry.',
        });
      }
      this.unavailable('deprovision', error);
    }
  }
}
