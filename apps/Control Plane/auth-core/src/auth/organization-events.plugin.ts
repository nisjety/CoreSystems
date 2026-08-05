/**
 * Organization Events Plugin
 *
 * Publishes organization lifecycle events to NATS when organizations are created,
 * members are added, or members are removed.
 */

import { randomUUID } from 'node:crypto';
import type { BetterAuthPlugin } from 'better-auth';
import {
  APIError,
  createAuthMiddleware,
  getSessionFromCtx,
} from 'better-auth/api';
import type postgres from 'postgres';
import { AuthEventPublisher } from '../internal/auth-event.publisher';
import { sqlClient } from '../db';
import {
  normalizeOutboxRevision,
  type OutboxRevision,
} from './outbox-revision';
import { fetchReconciliation } from './reconciliation-http';
import { requiredScopedServiceToken } from './control-service-credentials';
import {
  authGdprEmailIdentity,
  authGdprUserIdentity,
  lockAuthGdprIdentities,
} from '../internal/auth-gdpr-publish-fence';
import {
  membershipAuditBearerSessionToken,
  verifiedAuthoritativeMembershipAuditCredentialActorUserId,
  verifiedMembershipAuditActorUserId,
  verifiedMembershipAuditSessionActorUserId,
} from './membership-audit-hook-evidence';
import {
  captureMembershipAuditOperationResult,
  isMembershipMutationPath,
  type CapturedMembershipAuditOperation,
  type MembershipAuditMemberSnapshot,
} from './membership-audit-operation';

let eventPublisher: AuthEventPublisher | null = null;

type OrganizationOutboxRow = {
  organization_id: string;
  name: string;
  slug: string | null;
  metadata: Record<string, unknown> | null;
  owner_user_id: string;
  revision: OutboxRevision;
};

type MembershipOutboxRow = {
  organization_id: string;
  user_id: string;
  role: string;
  desired_action: 'upsert' | 'remove';
  revision: OutboxRevision;
};

type MembershipAuditOutboxRow = {
  organization_id: string;
  user_id: string;
  member_id: string;
  invitation_id: string | null;
  invitation_causality_pending: boolean;
  role: string;
  previous_role: string | null;
  applied_role: string | null;
  actor_user_id: string | null;
  actor_classification:
    | 'pending'
    | 'verified_user'
    | 'system_repair'
    | 'operator'
    | 'unresolved'
    | 'erased_actor';
  actor_resolution_attempts: number;
  actor_resolution_last_error: string | null;
  actor_resolution_dead_lettered_at: Date | null;
  action: 'member_added' | 'role_changed' | 'member_removed';
  revision: OutboxRevision;
  occurred_at: Date;
  published_at: Date | null;
};

type InvitationAuditOutboxRow = {
  invitation_id: string;
  organization_id: string;
  inviter_user_id: string;
  invitee_email: string;
  role: string;
  action: 'member_invited';
  occurred_at: Date;
  published_at: Date | null;
};

type OrganizationDeletionOutboxRow = {
  organization_id: string;
  name: string;
  revision: OutboxRevision;
  billing_synced_at: Date | null;
  org_synced_at: Date | null;
  event_synced_at: Date | null;
};

type OrganizationHookValue = {
  id: string;
  name: string;
  slug?: string | null;
  metadata?: unknown;
  members?: ReadonlyArray<{ userId: string }>;
};

type MemberHookValue = {
  id?: string;
  organizationId: string;
  userId: string;
  role: string;
};

type DurableMembershipAuditOperation = CapturedMembershipAuditOperation &
  Readonly<{ operationId: string }>;

async function findMembershipAuditMember(
  organizationId: string,
  memberIdOrEmail: string,
): Promise<ReadonlyArray<MembershipAuditMemberSnapshot>> {
  return sqlClient<MembershipAuditMemberSnapshot[]>`
    SELECT member.id,
           member.organization_id AS "organizationId",
           member.user_id AS "userId",
           member.role,
           projection.revision
    FROM member
    JOIN "user" canonical_user ON canonical_user.id = member.user_id
    JOIN organization_membership_outbox projection
      ON projection.organization_id = member.organization_id
     AND projection.user_id = member.user_id
    WHERE member.organization_id = ${organizationId}
      AND (
        member.id = ${memberIdOrEmail} OR
        member.user_id = ${memberIdOrEmail} OR
        LOWER(BTRIM(canonical_user.email)) = LOWER(BTRIM(${memberIdOrEmail}))
      )
    ORDER BY member.id
    LIMIT 2
  `;
}

async function readAuthoritativeMembershipAuditSession(
  context: Parameters<typeof getSessionFromCtx>[0],
): Promise<Readonly<{ sessionToken: string; value: unknown }> | null> {
  const authorization =
    context.request?.headers.get('authorization') ??
    context.headers?.get('authorization') ??
    null;
  let sessionToken: string | null;
  if (authorization !== null) {
    sessionToken = membershipAuditBearerSessionToken(authorization);
  } else {
    const signedCookieToken = await context.getSignedCookie(
      context.context.authCookies.sessionToken.name,
      context.context.secret,
    );
    sessionToken = signedCookieToken
      ? membershipAuditBearerSessionToken(`Bearer ${signedCookieToken}`)
      : null;
  }
  if (!sessionToken) return null;

  const authoritativeSession =
    await context.context.internalAdapter.findSession(sessionToken);
  return verifiedAuthoritativeMembershipAuditCredentialActorUserId(
    sessionToken,
    authoritativeSession,
  )
    ? Object.freeze({ sessionToken, value: authoritativeSession })
    : null;
}

async function enrichMembershipAuditActor(
  member: MemberHookValue,
  action: MembershipAuditOutboxRow['action'],
  actorUserId: string | null,
): Promise<void> {
  if (!actorUserId || !member.id) return;
  await sqlClient`
    WITH exact_transition AS (
      SELECT organization_id, user_id, revision
      FROM organization_membership_audit_outbox audit
      JOIN organization_membership_outbox projection
        ON projection.organization_id = audit.organization_id
       AND projection.user_id = audit.user_id
       AND projection.revision = audit.revision
      WHERE audit.organization_id = ${member.organizationId}
        AND audit.user_id = ${member.userId}
        AND audit.member_id = ${member.id}
        AND audit.action = ${action}
        AND audit.actor_classification IN ('pending', 'unresolved')
        AND audit.published_at IS NULL
        AND (
          (${action} IN ('member_added', 'role_changed') AND audit.applied_role = ${member.role}) OR
          (${action} = 'member_removed' AND audit.previous_role = ${member.role})
        )
      FOR UPDATE OF audit
    )
    UPDATE organization_membership_audit_outbox audit
    SET actor_user_id = ${actorUserId}, actor_classification = 'verified_user',
        actor_resolution_last_error = NULL,
        actor_resolution_dead_lettered_at = NULL,
        updated_at = NOW()
    FROM exact_transition
    WHERE audit.organization_id = exact_transition.organization_id
      AND audit.user_id = exact_transition.user_id
      AND audit.revision = exact_transition.revision
      AND audit.actor_classification IN ('pending', 'unresolved')
      AND audit.published_at IS NULL
      AND audit.action = ${action}
      AND audit.member_id = ${member.id}
      AND (
        (${action} IN ('member_added', 'role_changed') AND audit.applied_role = ${member.role}) OR
        (${action} = 'member_removed' AND audit.previous_role = ${member.role})
      )
  `;
}

type AppliedMembershipMutation = Readonly<{
  member: Readonly<{
    id: string;
    organizationId: string;
    userId: string;
    role: string;
  }>;
  mutationApplied: boolean;
  revision: number;
}>;

async function applyMembershipMutation(
  operation: DurableMembershipAuditOperation,
): Promise<AppliedMembershipMutation> {
  const rows = await sqlClient<Array<{ result: AppliedMembershipMutation }>>`
    SELECT apply_membership_mutation(
      ${operation.operationId}::UUID,
      ${operation.member.organizationId},
      ${operation.member.userId},
      ${operation.member.id},
      ${operation.kind},
      ${operation.observedRevision},
      ${operation.previousRole},
      ${operation.member.role},
      ${operation.actorUserId}
    ) AS result
  `;
  const result = rows[0]?.result;
  if (
    rows.length !== 1 ||
    !result ||
    typeof result.mutationApplied !== 'boolean' ||
    !Number.isSafeInteger(result.revision) ||
    !result.member ||
    !result.member.id ||
    !result.member.organizationId ||
    !result.member.userId ||
    !result.member.role
  ) {
    throw new Error('membership mutation returned an invalid result');
  }
  return Object.freeze({
    ...result,
    member: Object.freeze({ ...result.member }),
  });
}

function membershipMutationAPIError(error: unknown): APIError {
  const message = error instanceof Error ? error.message : '';
  if (message.includes('membership mutation conflict')) {
    return new APIError('CONFLICT', {
      message: 'Membership changed concurrently; retry from current state',
    });
  }
  if (message.includes('target not found')) {
    return new APIError('BAD_REQUEST', { message: 'Member not found' });
  }
  if (message.includes('only owner')) {
    return new APIError('BAD_REQUEST', {
      message: 'The only organization owner cannot leave or be demoted',
    });
  }
  if (message.includes('not authorized')) {
    return new APIError('FORBIDDEN', {
      message: 'Membership mutation is not authorized',
    });
  }
  return new APIError('INTERNAL_SERVER_ERROR', {
    message: 'Membership mutation could not be completed safely',
  });
}

function orgCoreConfig(): { url: string; token: string } {
  const url = (
    process.env.ORG_SERVICE_URL ||
    process.env.ORG_CORE_URL ||
    ''
  ).replace(/\/$/, '');
  const token = requiredScopedServiceToken('ORG_CORE_SERVICE_TOKEN');
  if (!url) throw new Error('Org Core reconciliation is not configured');
  return { url, token };
}

async function postOrgCore(
  path: string,
  body: unknown,
): Promise<{ applied?: boolean }> {
  const config = orgCoreConfig();
  const response = await fetchReconciliation(`${config.url}${path}`, {
    method: 'POST',
    headers: {
      'content-type': 'application/json',
      'x-service-id': 'auth-core',
      'x-service-token': config.token,
    },
    body: JSON.stringify(body),
  });
  if (!response.ok) {
    throw new Error(`Org Core reconciliation returned ${response.status}`);
  }
  return (await response.json()) as { applied?: boolean };
}

async function deactivateOrganizationBilling(
  organizationId: string,
  reason: string,
): Promise<void> {
  const url = (process.env.BILLING_CORE_URL || '').replace(/\/$/, '');
  const config = {
    token: requiredScopedServiceToken('BILLING_CORE_SERVICE_TOKEN'),
  };
  if (!url) throw new Error('Billing Core reconciliation is not configured');

  const response = await fetchReconciliation(
    `${url}/api/v1/billing/orgs/${encodeURIComponent(organizationId)}/deactivate`,
    {
      method: 'POST',
      headers: {
        'content-type': 'application/json',
        'x-service-id': 'auth-core',
        'x-service-token': config.token,
      },
      body: JSON.stringify({ reason }),
    },
  );
  if (!response.ok) {
    throw new Error(`Billing Core reconciliation returned ${response.status}`);
  }
}

async function recordOrganizationProjection(organization: {
  id: string;
  name: string;
  slug?: string | null;
  metadata?: unknown;
}): Promise<void> {
  const metadata =
    organization.metadata && typeof organization.metadata === 'object'
      ? (organization.metadata as Record<string, unknown>)
      : {};
  const jsonMetadata = JSON.parse(
    JSON.stringify(metadata),
  ) as postgres.JSONValue;
  await sqlClient`
    INSERT INTO organization_projection_outbox (organization_id, name, slug, metadata)
    VALUES (${organization.id}, ${organization.name}, ${organization.slug || null}, ${sqlClient.json(jsonMetadata)})
    ON CONFLICT (organization_id) DO UPDATE SET
      name = EXCLUDED.name,
      slug = EXCLUDED.slug,
      metadata = EXCLUDED.metadata,
      revision = organization_projection_outbox.revision + 1,
      published_at = NULL,
      processing_at = NULL,
      last_error = NULL,
      updated_at = NOW()
  `;
}

async function recordOrganizationOwner(
  organizationId: string,
  ownerUserId: string,
): Promise<void> {
  await sqlClient`
    UPDATE organization_projection_outbox
    SET owner_user_id = ${ownerUserId},
        revision = revision + 1,
        published_at = NULL,
        processing_at = NULL,
        last_error = NULL,
        updated_at = NOW()
    WHERE organization_id = ${organizationId}
  `;
}

export async function flushOrganizationProjectionOutbox(
  organizationId?: string,
): Promise<number> {
  const claimToken = randomUUID();
  await sqlClient`
    UPDATE organization_projection_outbox o
    SET owner_user_id = (
      SELECT m.user_id
      FROM member m
      WHERE m.organization_id = o.organization_id
        AND 'owner' = ANY(string_to_array(m.role, ','))
      ORDER BY m.created_at
      LIMIT 1
    ), updated_at = NOW()
    WHERE o.owner_user_id IS NULL
      AND EXISTS (
        SELECT 1 FROM member m
        WHERE m.organization_id = o.organization_id
          AND 'owner' = ANY(string_to_array(m.role, ','))
      )
  `;
  const rows = organizationId
    ? await sqlClient<OrganizationOutboxRow[]>`
        WITH claimed AS (
          SELECT organization_id
          FROM organization_projection_outbox
          WHERE published_at IS NULL AND owner_user_id IS NOT NULL
            AND (processing_at IS NULL OR processing_at < NOW() - INTERVAL '5 minutes')
            AND organization_id = ${organizationId}
          FOR UPDATE SKIP LOCKED
        )
        UPDATE organization_projection_outbox o
            SET processing_at = NOW(), claim_token = ${claimToken}, updated_at = NOW()
        FROM claimed
        WHERE o.organization_id = claimed.organization_id
        RETURNING o.organization_id, o.name, o.slug, o.metadata, o.owner_user_id, o.revision
      `
    : await sqlClient<OrganizationOutboxRow[]>`
        WITH claimed AS (
          SELECT organization_id
          FROM organization_projection_outbox
          WHERE published_at IS NULL AND owner_user_id IS NOT NULL
            AND (processing_at IS NULL OR processing_at < NOW() - INTERVAL '5 minutes')
          ORDER BY created_at
          LIMIT 100
          FOR UPDATE SKIP LOCKED
        )
        UPDATE organization_projection_outbox o
            SET processing_at = NOW(), claim_token = ${claimToken}, updated_at = NOW()
        FROM claimed
        WHERE o.organization_id = claimed.organization_id
        RETURNING o.organization_id, o.name, o.slug, o.metadata, o.owner_user_id, o.revision
      `;

  let published = 0;
  for (const row of rows) {
    try {
      const revision = normalizeOutboxRevision(row.revision);
      const acknowledged = await sqlClient.begin(async (tx) => {
        await lockAuthGdprIdentities(tx, [
          authGdprUserIdentity(row.owner_user_id),
        ]);
        const current = await tx<OrganizationOutboxRow[]>`
          SELECT organization_id, name, slug, metadata, owner_user_id, revision
          FROM organization_projection_outbox
          WHERE organization_id = ${row.organization_id}
            AND revision = ${revision}
            AND claim_token = ${claimToken}
            AND published_at IS NULL
          FOR UPDATE
        `;
        if (current.length !== 1) return false;
        const projection = current[0];
        await postOrgCore(
          `/internal/orgs/${encodeURIComponent(projection.organization_id)}/reconcile`,
          {
            name: projection.name,
            slug: projection.slug,
            metadata: projection.metadata || {},
            ownerUserId: projection.owner_user_id,
            revision,
          },
        );
        if (!eventPublisher) {
          throw new Error('organization event publisher unavailable');
        }
        await eventPublisher.publishOrganizationProjection(
          {
            organizationId: projection.organization_id,
            name: projection.name,
            slug: projection.slug || '',
            ownerUserId: projection.owner_user_id,
            metadata: projection.metadata || {},
            revision,
          },
          `organization:${projection.organization_id}:${revision}:upsert`,
        );
        const updated = await tx<Array<{ organization_id: string }>>`
          UPDATE organization_projection_outbox
          SET published_at = NOW(), attempts = attempts + 1,
              last_error = NULL, processing_at = NULL, claim_token = NULL,
              updated_at = NOW()
          WHERE organization_id = ${projection.organization_id}
            AND revision = ${revision}
            AND claim_token = ${claimToken}
            AND published_at IS NULL
          RETURNING organization_id
        `;
        return updated.length === 1;
      });
      if (!acknowledged) continue;
      published++;
    } catch (error) {
      await sqlClient`
        UPDATE organization_projection_outbox
        SET attempts = attempts + 1,
            last_error = ${String(error).slice(0, 1000)},
            processing_at = NULL, claim_token = NULL,
            updated_at = NOW()
        WHERE organization_id = ${row.organization_id}
          AND revision = ${row.revision}
          AND claim_token = ${claimToken}
      `;
    }
  }
  return published;
}

export async function flushOrganizationMembershipOutbox(): Promise<number> {
  await purgeCompletedGdprMembershipOutbox();
  const claimToken = randomUUID();
  const rows = await sqlClient<MembershipOutboxRow[]>`
    WITH claimed AS (
      SELECT organization_id, user_id
      FROM organization_membership_outbox
      WHERE synced_at IS NULL
        AND (processing_at IS NULL OR processing_at < NOW() - INTERVAL '5 minutes')
      ORDER BY updated_at
      LIMIT 200
      FOR UPDATE SKIP LOCKED
    )
    UPDATE organization_membership_outbox o
    SET processing_at = NOW(), claim_token = ${claimToken}, updated_at = NOW()
    FROM claimed
    WHERE o.organization_id = claimed.organization_id
      AND o.user_id = claimed.user_id
    RETURNING o.organization_id, o.user_id, o.role, o.desired_action, o.revision
  `;

  let synced = 0;
  for (const row of rows) {
    try {
      const revision = normalizeOutboxRevision(row.revision);
      if (!eventPublisher) {
        throw new Error('organization membership event publisher unavailable');
      }
      const eventID = `organization:${row.organization_id}:member:${row.user_id}:${revision}:${row.desired_action}`;
      let claimedEmail: string | undefined;
      if (row.desired_action === 'upsert') {
        const users = await sqlClient<Array<{ email: string }>>`
          SELECT LOWER(BTRIM(email)) AS email
          FROM "user"
          WHERE id = ${row.user_id}
        `;
        if (users.length !== 1 || !users[0].email) {
          throw new Error('canonical membership email is unavailable');
        }
        claimedEmail = users[0].email;
      }
      const acknowledged = await sqlClient.begin(async (tx) => {
        await lockAuthGdprIdentities(tx, [
          authGdprEmailIdentity(claimedEmail),
          authGdprUserIdentity(row.user_id),
        ]);
        const current = await tx<MembershipOutboxRow[]>`
          SELECT organization_id, user_id, role, desired_action, revision
          FROM organization_membership_outbox
          WHERE organization_id = ${row.organization_id}
            AND user_id = ${row.user_id}
            AND revision = ${revision}
            AND desired_action = ${row.desired_action}
            AND claim_token = ${claimToken}
            AND synced_at IS NULL
          FOR UPDATE
        `;
        if (current.length !== 1) return false;

        const organizationRevisions = await tx<
          Array<{ revision: OutboxRevision | null }>
        >`
          SELECT COALESCE(
            (
              SELECT projection.revision
              FROM organization_projection_outbox projection
              WHERE projection.organization_id = ${row.organization_id}
            ),
            (
              SELECT deletion.revision
              FROM organization_deletion_outbox deletion
              WHERE deletion.organization_id = ${row.organization_id}
            )
          ) AS revision
        `;
        if (
          organizationRevisions.length !== 1 ||
          organizationRevisions[0].revision == null
        ) {
          throw new Error('organization revision is unavailable');
        }
        const organizationRevision = normalizeOutboxRevision(
          organizationRevisions[0].revision,
        );

        let userEmail: string | undefined;
        if (row.desired_action === 'upsert') {
          const users = await tx<Array<{ email: string }>>`
            SELECT LOWER(BTRIM(email)) AS email
            FROM "user"
            WHERE id = ${row.user_id}
            FOR UPDATE
          `;
          if (users.length !== 1 || !users[0].email) {
            throw new Error('canonical membership email is unavailable');
          }
          userEmail = users[0].email;
          if (userEmail !== claimedEmail) {
            throw new Error('canonical membership email changed during claim');
          }
        }

        await postOrgCore(
          `/internal/orgs/${encodeURIComponent(row.organization_id)}/members/reconcile`,
          {
            userId: row.user_id,
            role: current[0].role,
            action: row.desired_action,
            revision,
          },
        );

        await eventPublisher!.publishOrganizationMembershipProjection(
          {
            organizationId: row.organization_id,
            userId: row.user_id,
            role: current[0].role,
            action: row.desired_action,
            revision,
            organizationRevision,
            userEmail,
          },
          eventID,
        );
        const updated = await tx<Array<{ organization_id: string }>>`
          UPDATE organization_membership_outbox
          SET synced_at = NOW(), processing_at = NULL,
              claim_token = NULL, attempts = attempts + 1,
              last_error = NULL, updated_at = NOW()
          WHERE organization_id = ${row.organization_id}
            AND user_id = ${row.user_id}
            AND revision = ${revision}
            AND desired_action = ${row.desired_action}
            AND claim_token = ${claimToken}
            AND synced_at IS NULL
          RETURNING organization_id
        `;
        return updated.length === 1;
      });
      if (!acknowledged) continue;
      synced++;
    } catch (error) {
      await sqlClient`
        UPDATE organization_membership_outbox
        SET processing_at = NULL, claim_token = NULL, attempts = attempts + 1,
            last_error = ${String(error).slice(0, 1000)}, updated_at = NOW()
        WHERE organization_id = ${row.organization_id} AND user_id = ${row.user_id}
          AND revision = ${row.revision} AND desired_action = ${row.desired_action}
          AND claim_token = ${claimToken}
      `;
    }
  }
  await purgeCompletedGdprMembershipOutbox();
  return synced;
}

async function purgeCompletedGdprMembershipOutbox(): Promise<number> {
  const rows = await sqlClient<Array<{ purged: number }>>`
    SELECT purge_completed_gdpr_membership_outbox(200) AS purged
  `;
  if (rows.length !== 1) {
    throw new Error('GDPR membership outbox purge returned no checkpoint');
  }
  const purged = Number(rows[0].purged);
  if (!Number.isSafeInteger(purged) || purged < 0) {
    throw new Error('GDPR membership outbox purge returned an invalid count');
  }
  return purged;
}

export function membershipAuditIdempotencyKey(
  row: Pick<
    MembershipAuditOutboxRow,
    'organization_id' | 'user_id' | 'revision' | 'action'
  >,
): string {
  return `membership:${row.organization_id}:${row.user_id}:${row.revision}:${row.action}`;
}

export type MembershipAuditActorRecoveryResult = {
  retried: number;
  deadLettered: number;
};

export async function recoverPendingMembershipAuditActors(): Promise<MembershipAuditActorRecoveryResult> {
  const rows = await sqlClient<
    Array<{ retried: number; dead_lettered: number }>
  >`
    SELECT retried, dead_lettered
    FROM recover_pending_membership_audit_actors(200, 3)
  `;
  if (rows.length !== 1) {
    throw new Error('membership audit actor recovery returned no checkpoint');
  }
  return {
    retried: Number(rows[0].retried),
    deadLettered: Number(rows[0].dead_lettered),
  };
}

export async function flushOrganizationMembershipAuditOutbox(): Promise<number> {
  const claimToken = randomUUID();
  const rows = await sqlClient<MembershipAuditOutboxRow[]>`
    WITH claimed AS (
      SELECT organization_id, user_id, revision
      FROM organization_membership_audit_outbox candidate
      WHERE candidate.published_at IS NULL
        AND candidate.actor_classification <> 'pending'
        AND (
          candidate.actor_classification <> 'unresolved' OR
          candidate.actor_resolution_dead_lettered_at < NOW() - INTERVAL '5 minutes'
        )
        AND candidate.invitation_causality_pending = FALSE
        AND (candidate.processing_at IS NULL OR candidate.processing_at < NOW() - INTERVAL '5 minutes')
        AND NOT EXISTS (
          SELECT 1 FROM organization_membership_audit_outbox prior
          WHERE prior.organization_id = candidate.organization_id
            AND prior.user_id = candidate.user_id
            AND prior.revision < candidate.revision
            AND prior.published_at IS NULL
        )
        AND (
          candidate.invitation_id IS NULL OR EXISTS (
            SELECT 1
            FROM organization_invitation_audit_outbox invitation_audit
            WHERE invitation_audit.invitation_id = candidate.invitation_id
              AND invitation_audit.published_at IS NOT NULL
          )
        )
      ORDER BY candidate.created_at, candidate.organization_id,
               candidate.user_id, candidate.revision
      LIMIT 200
      FOR UPDATE SKIP LOCKED
    )
    UPDATE organization_membership_audit_outbox o
    SET processing_at = NOW(), claim_token = ${claimToken}, updated_at = NOW()
    FROM claimed
    WHERE o.organization_id = claimed.organization_id
      AND o.user_id = claimed.user_id AND o.revision = claimed.revision
    RETURNING o.organization_id, o.user_id, o.member_id, o.invitation_id,
              o.invitation_causality_pending, o.role,
              o.previous_role, o.applied_role, o.actor_user_id,
              o.actor_classification, o.actor_resolution_attempts,
              o.actor_resolution_last_error,
              o.actor_resolution_dead_lettered_at,
              o.action, o.revision, o.created_at AS occurred_at, o.published_at
  `;

  let published = 0;
  for (const row of rows) {
    try {
      if (!eventPublisher) {
        throw new Error('membership audit publisher unavailable');
      }
      const revision = normalizeOutboxRevision(row.revision);
      const acknowledged = await sqlClient.begin(async (tx) => {
        await lockAuthGdprIdentities(tx, [
          authGdprUserIdentity(row.actor_user_id),
          authGdprUserIdentity(row.user_id),
        ]);
        const current = await tx<MembershipAuditOutboxRow[]>`
          SELECT organization_id, user_id, member_id, invitation_id,
                 invitation_causality_pending, role, previous_role,
                 applied_role, actor_user_id, actor_classification,
                 actor_resolution_attempts, actor_resolution_last_error,
                 actor_resolution_dead_lettered_at, action, revision,
                 created_at AS occurred_at, published_at
          FROM organization_membership_audit_outbox
          WHERE organization_id = ${row.organization_id}
            AND user_id = ${row.user_id}
            AND revision = ${revision}
            AND action = ${row.action}
            AND claim_token = ${claimToken}
            AND published_at IS NULL
          FOR UPDATE
        `;
        if (current.length !== 1) return false;
        const audit = current[0];
        const idempotencyKey = membershipAuditIdempotencyKey(audit);
        await eventPublisher!.publishVerevonAudit({
          occurred_at: audit.occurred_at,
          org_id: audit.organization_id,
          user_id: audit.actor_user_id || undefined,
          event: audit.action === 'role_changed' ? 'role_change' : audit.action,
          subject: audit.user_id,
          resource_id: audit.user_id,
          outcome: 'ok',
          event_id: idempotencyKey,
          details: {
            target_user_id: audit.user_id,
            member_id: audit.member_id,
            invitation_id: audit.invitation_id,
            role: audit.role,
            previous_role: audit.previous_role,
            applied_role: audit.applied_role,
            actor_classification: audit.actor_classification,
            actor_resolution_attempts: audit.actor_resolution_attempts,
            actor_resolution_last_error: audit.actor_resolution_last_error,
            actor_resolution_dead_lettered_at:
              audit.actor_resolution_dead_lettered_at?.toISOString(),
            revision,
            action: audit.action,
          },
        });
        const updated = await tx<Array<{ organization_id: string }>>`
          UPDATE organization_membership_audit_outbox
          SET published_at = NOW(), processing_at = NULL, claim_token = NULL,
              attempts = attempts + 1, last_error = NULL, updated_at = NOW()
          WHERE organization_id = ${audit.organization_id}
            AND user_id = ${audit.user_id}
            AND revision = ${revision}
            AND action = ${audit.action}
            AND claim_token = ${claimToken}
            AND published_at IS NULL
          RETURNING organization_id
        `;
        return updated.length === 1;
      });
      if (acknowledged) published++;
    } catch (error) {
      await sqlClient`
        UPDATE organization_membership_audit_outbox
        SET processing_at = NULL, claim_token = NULL, attempts = attempts + 1,
            last_error = ${String(error).slice(0, 1000)}, updated_at = NOW()
        WHERE organization_id = ${row.organization_id} AND user_id = ${row.user_id}
          AND revision = ${row.revision} AND action = ${row.action}
          AND claim_token = ${claimToken}
      `;
    }
  }
  return published;
}

export async function flushOrganizationInvitationAuditOutbox(): Promise<number> {
  const claimToken = randomUUID();
  const rows = await sqlClient<InvitationAuditOutboxRow[]>`
    WITH claimed AS (
      SELECT invitation_id
      FROM organization_invitation_audit_outbox
      WHERE published_at IS NULL
        AND (processing_at IS NULL OR processing_at < NOW() - INTERVAL '5 minutes')
      ORDER BY created_at, invitation_id
      LIMIT 200
      FOR UPDATE SKIP LOCKED
    )
    UPDATE organization_invitation_audit_outbox o
    SET processing_at = NOW(), claim_token = ${claimToken}, updated_at = NOW()
    FROM claimed
    WHERE o.invitation_id = claimed.invitation_id
    RETURNING o.invitation_id, o.organization_id, o.inviter_user_id,
              o.invitee_email, o.role, o.action,
              o.created_at AS occurred_at, o.published_at
  `;

  let published = 0;
  for (const row of rows) {
    try {
      if (!eventPublisher) {
        throw new Error('invitation audit publisher unavailable');
      }
      const acknowledged = await sqlClient.begin(async (tx) => {
        await lockAuthGdprIdentities(tx, [
          authGdprEmailIdentity(row.invitee_email),
          authGdprUserIdentity(row.inviter_user_id),
        ]);
        const current = await tx<InvitationAuditOutboxRow[]>`
          SELECT invitation_id, organization_id, inviter_user_id,
                 invitee_email, role, action,
                 created_at AS occurred_at, published_at
          FROM organization_invitation_audit_outbox
          WHERE invitation_id = ${row.invitation_id}
            AND action = ${row.action}
            AND claim_token = ${claimToken}
            AND published_at IS NULL
          FOR UPDATE
        `;
        if (current.length !== 1) return false;
        const audit = current[0];
        const idempotencyKey = `invitation:${audit.invitation_id}:${audit.action}`;
        await eventPublisher!.publishVerevonAudit({
          occurred_at: audit.occurred_at,
          org_id: audit.organization_id,
          user_id: audit.inviter_user_id,
          event: audit.action,
          subject: audit.invitee_email,
          resource_id: audit.invitation_id,
          outcome: 'ok',
          event_id: idempotencyKey,
          details: {
            invitee_email: audit.invitee_email,
            role: audit.role,
            invitation_id: audit.invitation_id,
          },
        });
        const updated = await tx<Array<{ invitation_id: string }>>`
          UPDATE organization_invitation_audit_outbox
          SET published_at = NOW(), processing_at = NULL, claim_token = NULL,
              attempts = attempts + 1, last_error = NULL, updated_at = NOW()
          WHERE invitation_id = ${audit.invitation_id}
            AND action = ${audit.action}
            AND claim_token = ${claimToken}
            AND published_at IS NULL
          RETURNING invitation_id
        `;
        return updated.length === 1;
      });
      if (acknowledged) published++;
    } catch (error) {
      await sqlClient`
        UPDATE organization_invitation_audit_outbox
        SET processing_at = NULL, claim_token = NULL, attempts = attempts + 1,
            last_error = ${String(error).slice(0, 1000)}, updated_at = NOW()
        WHERE invitation_id = ${row.invitation_id} AND action = ${row.action}
          AND claim_token = ${claimToken}
      `;
    }
  }
  return published;
}

export async function flushOrganizationDeletionOutbox(): Promise<number> {
  const rows = await sqlClient<OrganizationDeletionOutboxRow[]>`
    WITH claimed AS (
      SELECT organization_id, revision
      FROM organization_deletion_outbox
      WHERE completed_at IS NULL
        AND (processing_at IS NULL OR processing_at < NOW() - INTERVAL '5 minutes')
      ORDER BY created_at
      LIMIT 100
      FOR UPDATE SKIP LOCKED
    )
    UPDATE organization_deletion_outbox o
    SET processing_at = NOW(), updated_at = NOW()
    FROM claimed
    WHERE o.organization_id = claimed.organization_id
      AND o.revision = claimed.revision
    RETURNING o.organization_id, o.name, o.revision, o.billing_synced_at,
              o.org_synced_at, o.event_synced_at
  `;

  let synced = 0;
  for (const row of rows) {
    const errors: string[] = [];
    let staleClaim = false;
    if (!row.billing_synced_at) {
      try {
        await deactivateOrganizationBilling(
          row.organization_id,
          'organization_deleted',
        );
        const checkpoint = await sqlClient<Array<{ organization_id: string }>>`
          UPDATE organization_deletion_outbox
          SET billing_synced_at = NOW(), updated_at = NOW()
          WHERE organization_id = ${row.organization_id}
            AND revision = ${row.revision}
            AND billing_synced_at IS NULL
          RETURNING organization_id
        `;
        staleClaim = checkpoint.length !== 1;
      } catch (error) {
        errors.push(`billing: ${String(error)}`);
      }
    }
    if (!row.org_synced_at) {
      try {
        await postOrgCore(
          `/internal/orgs/${encodeURIComponent(row.organization_id)}/reconcile-delete`,
          { revision: normalizeOutboxRevision(row.revision) },
        );
        const checkpoint = await sqlClient<Array<{ organization_id: string }>>`
          UPDATE organization_deletion_outbox
          SET org_synced_at = NOW(), updated_at = NOW()
          WHERE organization_id = ${row.organization_id}
            AND revision = ${row.revision}
            AND org_synced_at IS NULL
          RETURNING organization_id
        `;
        staleClaim = staleClaim || checkpoint.length !== 1;
      } catch (error) {
        errors.push(`org: ${String(error)}`);
      }
    }
    if (staleClaim) continue;
    if (!row.event_synced_at) {
      try {
        if (!eventPublisher) {
          throw new Error('organization deletion event publisher unavailable');
        }
        const revision = normalizeOutboxRevision(row.revision);
        await eventPublisher.publishOrganizationDeletionProjection(
          { organizationId: row.organization_id, revision },
          `organization:${row.organization_id}:${revision}:deleted`,
        );
        await sqlClient`
          UPDATE organization_deletion_outbox
          SET event_synced_at = NOW(), updated_at = NOW()
          WHERE organization_id = ${row.organization_id}
            AND revision = ${revision}
            AND event_synced_at IS NULL
        `;
      } catch (error) {
        errors.push(`event: ${String(error)}`);
      }
    }

    if (errors.length === 0) {
      const completed = await sqlClient<Array<{ organization_id: string }>>`
        UPDATE organization_deletion_outbox
        SET completed_at = NOW(), processing_at = NULL,
            attempts = attempts + 1, last_error = NULL, updated_at = NOW()
        WHERE organization_id = ${row.organization_id}
          AND revision = ${row.revision}
          AND billing_synced_at IS NOT NULL
          AND org_synced_at IS NOT NULL
          AND event_synced_at IS NOT NULL
          AND completed_at IS NULL
        RETURNING organization_id
      `;
      synced += completed.length;
    } else {
      await sqlClient`
        UPDATE organization_deletion_outbox
        SET processing_at = NULL, attempts = attempts + 1,
            last_error = ${errors.join('; ').slice(0, 1000)}, updated_at = NOW()
        WHERE organization_id = ${row.organization_id}
          AND revision = ${row.revision}
      `;
    }
  }
  return synced;
}

export function setOrganizationEventPublisher(publisher: AuthEventPublisher) {
  console.log(
    '🔧 Setting organization event publisher for plugin:',
    !!publisher,
  );
  eventPublisher = publisher;
}

export function organizationEventsPlugin(): BetterAuthPlugin {
  return {
    id: 'organization-events',
    hooks: {
      before: [
        {
          matcher: (context) => isMembershipMutationPath(context.path),
          handler: createAuthMiddleware(async (context) => {
            const authoritativeSession =
              await readAuthoritativeMembershipAuditSession(context);
            const actorUserId = verifiedMembershipAuditSessionActorUserId(
              authoritativeSession?.value,
            );
            if (!actorUserId) {
              throw new APIError('UNAUTHORIZED', {
                message: 'Verified membership actor required',
              });
            }
            const captureResult = await captureMembershipAuditOperationResult(
              context,
              actorUserId,
              findMembershipAuditMember,
            );
            if (captureResult.status === 'not_found') return;
            if (captureResult.status === 'bad_request') {
              throw new APIError('BAD_REQUEST', {
                message: 'Exactly one supported organization role is required',
              });
            }
            if (captureResult.status === 'invalid') {
              throw new APIError('INTERNAL_SERVER_ERROR', {
                message: 'Membership audit evidence unavailable',
              });
            }
            const operation = captureResult.operation;
            const durableOperation = Object.freeze({
              ...operation,
              operationId: randomUUID(),
            });
            let result: AppliedMembershipMutation;
            try {
              result = await applyMembershipMutation(durableOperation);
            } catch (error) {
              throw membershipMutationAPIError(error);
            }
            const response =
              context.path === '/organization/remove-member'
                ? { member: result.member }
                : result.member;
            if (
              context.path === '/organization/leave' &&
              authoritativeSession
            ) {
              try {
                await context.context.internalAdapter.updateSession(
                  authoritativeSession.sessionToken,
                  { activeOrganizationId: null },
                );
              } catch {
                // Canonical membership is already removed. Authorization still
                // fails closed through canonical membership checks; the normal
                // session refresh path repairs this non-authoritative cache.
                console.error(
                  'Failed to clear the departed member active organization cache',
                );
              }
            }
            return context.json(response);
          }),
        },
      ],
    },
    init() {
      console.log('🎉 Organization Events Plugin initialized');

      return {
        options: {
          databaseHooks: {
            organization: {
              create: {
                after: async (organization: OrganizationHookValue) => {
                  console.log(
                    '🎊 Organization created hook triggered:',
                    organization.name,
                  );

                  try {
                    await recordOrganizationProjection(organization);
                    // Find the creator from the members
                    const creatorMember = organization.members?.[0];

                    if (!creatorMember?.userId) {
                      // The durable outbox waits for the following owner-member
                      // commit; a process restart cannot lose this projection.
                      return;
                    }

                    await recordOrganizationOwner(
                      organization.id,
                      creatorMember.userId,
                    );
                    await flushOrganizationProjectionOutbox(organization.id);

                    console.log(
                      '📢 Published organization.created event:',
                      organization.id,
                    );
                  } catch (error) {
                    console.error(
                      '❌ Failed to publish organization created event:',
                      error,
                    );
                  }
                },
              },
            },
            member: {
              create: {
                after: async (
                  member: MemberHookValue,
                  hookContext: unknown,
                ) => {
                  console.log(
                    '👤 Member added hook triggered for org:',
                    member.organizationId,
                  );

                  try {
                    if (
                      member.role
                        .split(',')
                        .some((role) => role.trim() === 'owner')
                    ) {
                      await recordOrganizationOwner(
                        member.organizationId,
                        member.userId,
                      );
                      await flushOrganizationProjectionOutbox(
                        member.organizationId,
                      );
                    }

                    await enrichMembershipAuditActor(
                      member,
                      'member_added',
                      verifiedMembershipAuditActorUserId(hookContext),
                    );
                    await flushOrganizationMembershipOutbox();
                    await flushOrganizationMembershipAuditOutbox();

                    console.log(
                      '📢 Published member_added event for user:',
                      member.userId,
                    );
                  } catch (error) {
                    console.error(
                      '❌ Failed to publish member added event:',
                      error,
                    );
                  }
                },
              },
              update: {
                after: async () => {
                  try {
                    await flushOrganizationMembershipOutbox();
                  } catch (error) {
                    console.error(
                      '❌ Failed to reconcile member role update:',
                      error,
                    );
                  }
                },
              },
              delete: {
                after: async (member: MemberHookValue) => {
                  console.log(
                    '👋 Member removed hook triggered for org:',
                    member.organizationId,
                  );

                  try {
                    await flushOrganizationMembershipOutbox();

                    console.log(
                      '📢 Published member_removed event for user:',
                      member.userId,
                    );
                  } catch (error) {
                    console.error(
                      '❌ Failed to publish member removed event:',
                      error,
                    );
                  }
                },
              },
            },
          },
        },
      } as unknown as BetterAuthPlugin;
    },
  };
}
