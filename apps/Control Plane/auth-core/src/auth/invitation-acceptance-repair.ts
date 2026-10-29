import { randomUUID } from 'node:crypto';
import type postgres from 'postgres';

import { sqlClient } from '../db';

const CLAIM_LIMIT = 100;
const MAX_ATTEMPTS = 5;
const MAX_ERROR_LENGTH = 120;

export interface InvitationAcceptanceActor {
  userId: string;
  email: string;
}

interface NormalizedInvitationAcceptanceActor {
  userId: string;
  normalizedEmail: string;
}

export interface InvitationAcceptanceRepairResponse {
  invitationId: string;
  organizationId: string;
  memberId: string;
  memberRole: string;
}

export interface InvitationAcceptanceRepairClaim {
  invitationId: string;
  attempts: number;
}

export type InvitationAcceptanceRepairResult =
  | {
      kind: 'completed';
      response: InvitationAcceptanceRepairResponse;
    }
  | { kind: 'superseded' }
  | { kind: 'not_repairable' };

export interface InvitationAcceptanceRepairRepository {
  claimPending(limit: number): Promise<InvitationAcceptanceRepairClaim[]>;
  repair(
    invitationId: string,
    expectedActor?: NormalizedInvitationAcceptanceActor,
  ): Promise<InvitationAcceptanceRepairResult>;
  recordFailure(
    claim: InvitationAcceptanceRepairClaim,
    errorCode: string,
  ): Promise<void>;
}

export interface InvitationAcceptanceRepairSweep {
  claimed: number;
  completed: number;
  superseded: number;
  notRepairable: number;
  retried: number;
  deadLettered: number;
}

type InvitationRow = {
  invitation_id: string;
  organization_id: string;
  invitation_email: string;
  invited_role: string;
  invitation_status: string;
};

type UserRow = { user_id: string };
type MemberRow = { member_id: string; member_role: string };
type RepairIntentRow = {
  organization_id: string;
  normalized_email: string;
  invited_role: string;
  state: string;
  repaired_member_id: string | null;
};

class RepairDeferredError extends Error {
  constructor(readonly code: string) {
    super(code);
  }
}

function normalizedEmail(value: string): string {
  return value.trim().toLowerCase();
}

function boundedFailureCode(error: unknown): string {
  if (error instanceof RepairDeferredError) {
    return error.code.slice(0, MAX_ERROR_LENGTH);
  }
  return 'repair_failed';
}

export class PostgresInvitationAcceptanceRepairRepository
  implements InvitationAcceptanceRepairRepository
{
  constructor(private readonly sql: typeof sqlClient = sqlClient) {}

  async claimPending(
    limit: number,
  ): Promise<InvitationAcceptanceRepairClaim[]> {
    const boundedLimit = Math.max(1, Math.min(limit, CLAIM_LIMIT));
    const rows = await this.sql<
      Array<{ invitation_id: string; attempts: number }>
    >`
      WITH claimed AS (
        SELECT invitation_id
        FROM invitation_acceptance_repair
        WHERE not_before <= NOW()
          AND (
            state = 'pending'
            OR (state = 'processing' AND processing_at < NOW() - INTERVAL '5 minutes')
          )
        ORDER BY not_before, updated_at
        LIMIT ${boundedLimit}
        FOR UPDATE SKIP LOCKED
      )
      UPDATE invitation_acceptance_repair r
      SET state = 'processing', processing_at = NOW(),
          attempts = attempts + 1, updated_at = NOW()
      FROM claimed
      WHERE r.invitation_id = claimed.invitation_id
      RETURNING r.invitation_id, r.attempts
    `;
    return rows.map((row) => ({
      invitationId: row.invitation_id,
      attempts: Number(row.attempts),
    }));
  }

  async repair(
    invitationId: string,
    expectedActor?: NormalizedInvitationAcceptanceActor,
  ): Promise<InvitationAcceptanceRepairResult> {
    return this.sql.begin(async (tx) => {
      const invitations = await tx<InvitationRow[]>`
        SELECT
          i.id AS invitation_id,
          i.organization_id,
          i.email AS invitation_email,
          COALESCE(NULLIF(BTRIM(i.role), ''), 'member') AS invited_role,
          i.status AS invitation_status
        FROM invitation i
        WHERE i.id = ${invitationId}
        FOR UPDATE
      `;
      if (invitations.length !== 1) {
        await this.markSuperseded(tx, invitationId);
        return { kind: 'superseded' } as const;
      }

      const invitation = invitations[0];
      if (invitation.invitation_status !== 'accepted') {
        await this.markSuperseded(tx, invitationId);
        return { kind: 'superseded' } as const;
      }

      const intents = await tx<RepairIntentRow[]>`
        SELECT organization_id, normalized_email, invited_role, state,
               repaired_member_id
        FROM invitation_acceptance_repair
        WHERE invitation_id = ${invitationId}
        FOR UPDATE
      `;
      if (intents.length !== 1) {
        if (expectedActor) return { kind: 'not_repairable' } as const;
        throw new RepairDeferredError('acceptance_intent_unavailable');
      }
      const intent = intents[0];
      if (intent.state === 'superseded') {
        return { kind: 'superseded' } as const;
      }
      if (intent.state === 'dead_letter') {
        return { kind: 'not_repairable' } as const;
      }
      const invitationRole = invitation.invited_role.trim() || 'member';
      if (
        intent.organization_id !== invitation.organization_id ||
        intent.normalized_email !==
          normalizedEmail(invitation.invitation_email) ||
        intent.invited_role !== invitationRole
      ) {
        if (expectedActor) return { kind: 'not_repairable' } as const;
        throw new RepairDeferredError('acceptance_intent_mismatch');
      }

      const users = await tx<UserRow[]>`
        SELECT id AS user_id
        FROM "user"
        WHERE LOWER(BTRIM(email)) = ${intent.normalized_email}
        ORDER BY id
        LIMIT 2
      `;
      if (users.length !== 1) {
        if (expectedActor) return { kind: 'not_repairable' } as const;
        throw new RepairDeferredError('canonical_user_unavailable');
      }
      const userId = users[0].user_id;
      if (
        expectedActor &&
        (expectedActor.userId !== userId ||
          expectedActor.normalizedEmail !== intent.normalized_email)
      ) {
        return { kind: 'not_repairable' } as const;
      }

      if (intent.state === 'completed') {
        if (!intent.repaired_member_id) {
          await this.markSuperseded(tx, invitationId);
          return { kind: 'not_repairable' } as const;
        }
        const completedMembers = await tx<MemberRow[]>`
          SELECT id AS member_id, role AS member_role
          FROM member
          WHERE id = ${intent.repaired_member_id}
            AND organization_id = ${intent.organization_id}
            AND user_id = ${userId}
          LIMIT 2
        `;
        if (completedMembers.length !== 1) {
          await this.markSuperseded(tx, invitationId);
          return { kind: 'not_repairable' } as const;
        }
        return {
          kind: 'completed',
          response: {
            invitationId,
            organizationId: intent.organization_id,
            memberId: completedMembers[0].member_id,
            memberRole: completedMembers[0].member_role,
          },
        } as const;
      }
      if (!['pending', 'processing'].includes(intent.state)) {
        return { kind: 'not_repairable' } as const;
      }

      const insertedMembers = await tx<MemberRow[]>`
        INSERT INTO member (id, organization_id, user_id, role, created_at)
        VALUES (
          ${randomUUID()},
          ${intent.organization_id},
          ${userId},
          ${intent.invited_role},
          NOW()
        )
        ON CONFLICT (organization_id, user_id) DO NOTHING
        RETURNING id AS member_id, role AS member_role
      `;
      const members = await tx<MemberRow[]>`
        SELECT id AS member_id, role AS member_role
        FROM member
        WHERE organization_id = ${intent.organization_id}
          AND user_id = ${userId}
        LIMIT 2
      `;
      if (members.length !== 1) {
        throw new RepairDeferredError('canonical_member_unavailable');
      }

      const actorClassification = expectedActor
        ? 'verified_user'
        : 'system_repair';
      await tx`
        UPDATE organization_membership_audit_outbox audit
        SET actor_user_id = ${expectedActor?.userId ?? null},
            actor_classification = ${actorClassification},
            invitation_id = CASE
              WHEN audit.invitation_causality_pending
                THEN ${invitationId}
              ELSE audit.invitation_id
            END,
            invitation_causality_pending = FALSE,
            updated_at = NOW()
        WHERE audit.organization_id = ${intent.organization_id}
          AND audit.user_id = ${userId}
          AND audit.published_at IS NULL
          AND audit.actor_classification = 'pending'
          AND audit.action = 'member_added'
          AND audit.member_id = ${members[0].member_id}
          AND audit.applied_role = ${members[0].member_role}
          AND (
            audit.invitation_id = ${invitationId} OR
            (audit.invitation_id IS NULL AND audit.invitation_causality_pending)
          )
          AND audit.revision = (
            SELECT projection.revision
            FROM organization_membership_outbox projection
            WHERE projection.organization_id = audit.organization_id
              AND projection.user_id = audit.user_id
          )
      `;

      await tx`
        UPDATE invitation_acceptance_repair
        SET state = 'completed', processing_at = NULL,
            last_error = NULL,
            repaired_member_id = CASE
              WHEN ${insertedMembers.length === 1} THEN ${members[0].member_id}
              ELSE COALESCE(repaired_member_id, ${members[0].member_id})
            END,
            inserted_member = inserted_member OR ${insertedMembers.length === 1},
            completed_at = NOW(), updated_at = NOW()
        WHERE invitation_id = ${invitationId}
          AND state <> 'superseded'
      `;
      return {
        kind: 'completed',
        response: {
          invitationId,
          organizationId: intent.organization_id,
          memberId: members[0].member_id,
          memberRole: members[0].member_role,
        },
      } as const;
    });
  }

  async recordFailure(
    claim: InvitationAcceptanceRepairClaim,
    errorCode: string,
  ): Promise<void> {
    const nextState =
      claim.attempts >= MAX_ATTEMPTS ? 'dead_letter' : 'pending';
    const boundedCode = errorCode.slice(0, MAX_ERROR_LENGTH);
    await this.sql`
      UPDATE invitation_acceptance_repair
      SET state = ${nextState}, processing_at = NULL,
          not_before = NOW() + INTERVAL '1 minute',
          last_error = ${boundedCode}, updated_at = NOW()
      WHERE invitation_id = ${claim.invitationId}
        AND state = 'processing'
    `;
  }

  private async markSuperseded(
    tx: postgres.TransactionSql,
    invitationId: string,
  ): Promise<void> {
    await tx`
      UPDATE invitation_acceptance_repair
      SET state = 'superseded', processing_at = NULL,
          last_error = NULL, updated_at = NOW()
      WHERE invitation_id = ${invitationId}
        AND state <> 'superseded'
    `;
  }
}

const defaultRepository = new PostgresInvitationAcceptanceRepairRepository();

export async function repairAcceptedInvitationForActor(
  invitationId: string,
  actor: InvitationAcceptanceActor,
  repository: InvitationAcceptanceRepairRepository = defaultRepository,
): Promise<InvitationAcceptanceRepairResponse | null> {
  const actorEmail = normalizedEmail(actor.email);
  if (!invitationId || !actor.userId || !actorEmail) return null;
  const result = await repository.repair(invitationId, {
    userId: actor.userId,
    normalizedEmail: actorEmail,
  });
  return result.kind === 'completed' ? result.response : null;
}

export async function flushInvitationAcceptanceRepairs(
  repository: InvitationAcceptanceRepairRepository = defaultRepository,
): Promise<InvitationAcceptanceRepairSweep> {
  const claims = await repository.claimPending(CLAIM_LIMIT);
  const summary: InvitationAcceptanceRepairSweep = {
    claimed: claims.length,
    completed: 0,
    superseded: 0,
    notRepairable: 0,
    retried: 0,
    deadLettered: 0,
  };
  for (const claim of claims) {
    try {
      const result = await repository.repair(claim.invitationId);
      switch (result.kind) {
        case 'completed':
          summary.completed++;
          break;
        case 'superseded':
          summary.superseded++;
          break;
        case 'not_repairable':
          summary.notRepairable++;
          break;
      }
    } catch (error) {
      await repository.recordFailure(claim, boundedFailureCode(error));
      if (claim.attempts >= MAX_ATTEMPTS) {
        summary.deadLettered++;
      } else {
        summary.retried++;
      }
    }
  }
  return summary;
}
