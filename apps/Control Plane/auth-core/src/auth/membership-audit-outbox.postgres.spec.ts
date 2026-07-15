import { randomUUID } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

import postgres from 'postgres';

import { verifyLifecycleFixtureMarker } from './lifecycle-test-fixture';

const testDatabaseUrl = process.env.TEST_DATABASE_URL?.trim();
const postgresDescribe = testDatabaseUrl ? describe : describe.skip;

postgresDescribe('membership audit outbox with Postgres', () => {
  const schema = `membership_audit_${randomUUID().replaceAll('-', '')}`;
  let admin: ReturnType<typeof postgres>;
  let fixture: ReturnType<typeof postgres>;

  beforeAll(async () => {
    admin = postgres(testDatabaseUrl!, { max: 1, prepare: false });
    await verifyLifecycleFixtureMarker(
      admin,
      testDatabaseUrl!,
      'auth_invitation',
      process.env.CONTROL_LIFECYCLE_FIXTURE_ID ?? '',
    );
    await admin`CREATE SCHEMA ${admin(schema)}`;
    fixture = postgres(testDatabaseUrl!, {
      max: 2,
      prepare: false,
      connection: { search_path: `${schema},public` },
    });
    await fixture.unsafe(`
      CREATE TABLE "user" (id TEXT PRIMARY KEY, email TEXT NOT NULL);
      CREATE TABLE account (
        id TEXT PRIMARY KEY,
        provider_id TEXT NOT NULL,
        account_id TEXT NOT NULL
      );
      CREATE TABLE organization (
        id TEXT PRIMARY KEY, name TEXT NOT NULL, slug TEXT,
        metadata TEXT NOT NULL DEFAULT '{}',
        created_at TIMESTAMP NOT NULL DEFAULT NOW()
      );
      CREATE TABLE member (
        id TEXT PRIMARY KEY,
        organization_id TEXT NOT NULL REFERENCES organization(id) ON DELETE CASCADE,
        user_id TEXT NOT NULL REFERENCES "user"(id) ON DELETE CASCADE,
        role TEXT NOT NULL DEFAULT 'member',
        created_at TIMESTAMP NOT NULL DEFAULT NOW()
      );
      CREATE TABLE session (
        id TEXT PRIMARY KEY,
        token TEXT NOT NULL UNIQUE,
        user_id TEXT NOT NULL REFERENCES "user"(id) ON DELETE CASCADE,
        active_organization_id TEXT
      );
      CREATE TABLE invitation (
        id TEXT PRIMARY KEY,
        organization_id TEXT NOT NULL REFERENCES organization(id) ON DELETE CASCADE,
        email TEXT NOT NULL,
        role TEXT,
        status TEXT NOT NULL DEFAULT 'pending',
        expires_at TIMESTAMP NOT NULL,
        inviter_id TEXT NOT NULL REFERENCES "user"(id) ON DELETE CASCADE,
        team_id TEXT
      );
    `);
    for (const migration of [
      '014_normalized_identity_email.sql',
      '015_organization_projection_outbox.sql',
      '016_unique_organization_membership.sql',
      '017_invitation_acceptance_repair.sql',
      '018_historical_owner_preflight.sql',
      '019_membership_audit_outbox.sql',
      '022_membership_audit_actor_recovery.sql',
      '027_membership_mutation_intents.sql',
    ]) {
      await fixture.unsafe(
        readFileSync(join(process.cwd(), 'migrations', migration), 'utf8'),
      );
    }
  });

  it('rolls back an interrupted operation and lets a retry bind only its own actor', async () => {
    await fixture.unsafe(`
      INSERT INTO "user" (id, email) VALUES
        ('actor-lease-a', 'actor-lease-a@example.invalid'),
        ('actor-lease-b', 'actor-lease-b@example.invalid'),
        ('user-lease', 'user-lease@example.invalid');
      INSERT INTO organization (id, name) VALUES ('org-lease', 'Org Lease');
      INSERT INTO member (id, organization_id, user_id, role)
      VALUES
        ('member-lease-owner-a', 'org-lease', 'actor-lease-a', 'owner'),
        ('member-lease-owner-b', 'org-lease', 'actor-lease-b', 'owner'),
        ('member-lease', 'org-lease', 'user-lease', 'member');
    `);

    await expect(
      fixture.begin(async (transaction) => {
        await transaction`
          SELECT apply_membership_mutation(
          '00000000-0000-4000-8000-000000000001'::UUID,
          'org-lease', 'user-lease', 'member-lease', 'role_change',
          1, 'member', 'admin', 'actor-lease-a'
        ) AS result
        `;
        throw new Error('simulate process loss before commit');
      }),
    ).rejects.toThrow('simulate process loss');

    await expect(
      fixture`SELECT role FROM member WHERE id = 'member-lease'`,
    ).resolves.toEqual([{ role: 'member' }]);
    await expect(
      fixture`
        SELECT COUNT(*)::INT AS operations
        FROM organization_membership_mutation_operation
        WHERE member_id = 'member-lease'
      `,
    ).resolves.toEqual([{ operations: 0 }]);

    const retried = await fixture<Array<{ result: Record<string, unknown> }>>`
      SELECT apply_membership_mutation(
        '00000000-0000-4000-8000-000000000002'::UUID,
        'org-lease', 'user-lease', 'member-lease', 'role_change',
        1, 'member', 'admin', 'actor-lease-b'
      ) AS result
    `;
    expect(retried[0].result).toMatchObject({
      mutationApplied: true,
      revision: 2,
      member: { id: 'member-lease', role: 'admin' },
    });

    const evidence = await fixture`
      SELECT audit.revision, audit.actor_user_id, audit.actor_classification
      FROM organization_membership_audit_outbox audit
      WHERE audit.organization_id = 'org-lease'
        AND audit.user_id = 'user-lease'
        AND audit.action = 'role_changed'
    `;
    expect(evidence).toEqual([
      {
        revision: '2',
        actor_user_id: 'actor-lease-b',
        actor_classification: 'verified_user',
      },
    ]);
    await expect(
      fixture`UPDATE member SET role = 'viewer' WHERE id = 'member-lease'`,
    ).rejects.toThrow(/verified membership mutation operation is required/i);
  });

  it('rejects stale same-role reordering and replays a completed no-op without reverting newer state', async () => {
    await fixture.unsafe(`
      INSERT INTO "user" (id, email) VALUES
        ('actor-noop', 'actor-noop@example.invalid'),
        ('user-noop-stale', 'user-noop-stale@example.invalid'),
        ('user-noop-replay', 'user-noop-replay@example.invalid');
      INSERT INTO organization (id, name) VALUES ('org-noop', 'Org Noop');
      INSERT INTO member (id, organization_id, user_id, role)
      VALUES
        ('member-noop-owner', 'org-noop', 'actor-noop', 'owner'),
        ('member-noop-stale', 'org-noop', 'user-noop-stale', 'member'),
        ('member-noop-replay', 'org-noop', 'user-noop-replay', 'member');
    `);

    await fixture`
      SELECT apply_membership_mutation(
        '00000000-0000-4000-8000-000000000101'::UUID,
        'org-noop', 'user-noop-stale', 'member-noop-stale', 'role_change',
        1, 'member', 'admin', 'actor-noop'
      )
    `;
    await expect(
      fixture`
        SELECT apply_membership_mutation(
          '00000000-0000-4000-8000-000000000102'::UUID,
          'org-noop', 'user-noop-stale', 'member-noop-stale', 'role_change',
          1, 'member', 'member', 'actor-noop'
        )
      `,
    ).rejects.toThrow(/membership mutation conflict/i);
    await expect(
      fixture`SELECT role FROM member WHERE id = 'member-noop-stale'`,
    ).resolves.toEqual([{ role: 'admin' }]);

    const noOp = await fixture<Array<{ result: Record<string, unknown> }>>`
      SELECT apply_membership_mutation(
        '00000000-0000-4000-8000-000000000103'::UUID,
        'org-noop', 'user-noop-replay', 'member-noop-replay', 'role_change',
        1, 'member', 'member', 'actor-noop'
      ) AS result
    `;
    expect(noOp[0].result).toMatchObject({
      mutationApplied: false,
      revision: 1,
      member: { role: 'member' },
    });
    await fixture`
      SELECT apply_membership_mutation(
        '00000000-0000-4000-8000-000000000104'::UUID,
        'org-noop', 'user-noop-replay', 'member-noop-replay', 'role_change',
        1, 'member', 'admin', 'actor-noop'
      )
    `;
    await expect(
      fixture`
        SELECT apply_membership_mutation(
          '00000000-0000-4000-8000-000000000103'::UUID,
          'org-noop', 'user-noop-replay', 'member-noop-replay', 'role_change',
          1, 'member', 'member', 'actor-noop'
        ) AS result
      `,
    ).resolves.toEqual(noOp);
    await expect(
      fixture`SELECT role FROM member WHERE id = 'member-noop-replay'`,
    ).resolves.toEqual([{ role: 'admin' }]);
    await expect(
      fixture`
        SELECT COUNT(*)::INT AS role_audits
        FROM organization_membership_audit_outbox
        WHERE organization_id = 'org-noop'
          AND user_id IN ('user-noop-stale', 'user-noop-replay')
          AND action = 'role_changed'
      `,
    ).resolves.toEqual([{ role_audits: 2 }]);
  });

  it('applies self-leave exactly once, clears active sessions, and protects the sole owner', async () => {
    await fixture.unsafe(`
      INSERT INTO "user" (id, email) VALUES
        ('owner-leave', 'owner-leave@example.invalid'),
        ('user-leave', 'user-leave@example.invalid');
      INSERT INTO organization (id, name) VALUES ('org-leave', 'Org Leave');
      INSERT INTO member (id, organization_id, user_id, role)
      VALUES
        ('member-leave-owner', 'org-leave', 'owner-leave', 'owner'),
        ('member-leave', 'org-leave', 'user-leave', 'member');
      INSERT INTO session (id, token, user_id, active_organization_id)
      VALUES ('session-leave', 'token-leave', 'user-leave', 'org-leave');
    `);

    const first = await fixture<Array<{ result: Record<string, unknown> }>>`
      SELECT apply_membership_mutation(
        '00000000-0000-4000-8000-000000000105'::UUID,
        'org-leave', 'user-leave', 'member-leave', 'self_leave',
        1, 'member', 'member', 'user-leave'
      ) AS result
    `;
    expect(first[0].result).toMatchObject({
      mutationApplied: true,
      revision: 2,
      member: { id: 'member-leave', userId: 'user-leave' },
    });
    await expect(
      fixture`
        SELECT apply_membership_mutation(
          '00000000-0000-4000-8000-000000000105'::UUID,
          'org-leave', 'user-leave', 'member-leave', 'self_leave',
          1, 'member', 'member', 'user-leave'
        ) AS result
      `,
    ).resolves.toEqual(first);
    await expect(
      fixture`
        SELECT
          (SELECT COUNT(*)::INT FROM member WHERE id = 'member-leave') AS members,
          (SELECT COUNT(*)::INT FROM organization_membership_audit_outbox
           WHERE organization_id = 'org-leave' AND user_id = 'user-leave'
             AND action = 'member_removed') AS removal_audits,
          (SELECT active_organization_id FROM session
           WHERE id = 'session-leave') AS active_organization_id
      `,
    ).resolves.toEqual([
      { members: 0, removal_audits: 1, active_organization_id: null },
    ]);

    await expect(
      fixture`
        SELECT apply_membership_mutation(
          '00000000-0000-4000-8000-000000000106'::UUID,
          'org-leave', 'owner-leave', 'member-leave-owner', 'self_leave',
          1, 'owner', 'owner', 'owner-leave'
        )
      `,
    ).rejects.toThrow(/only owner/i);
  });

  afterAll(async () => {
    if (fixture) await fixture.end({ timeout: 5 });
    if (admin) {
      await admin`DROP SCHEMA IF EXISTS ${admin(schema)} CASCADE`;
      await admin.end({ timeout: 5 });
    }
  });

  it('preserves invite, add, role, and removal evidence in canonical order', async () => {
    await fixture.begin(async (tx) => {
      await tx`
        INSERT INTO "user" (id, email) VALUES
          ('actor-1', 'actor-1@example.invalid'),
          ('user-1', ' USER-1@Example.Invalid '),
          ('user-2', 'user-2@example.invalid')
      `;
      await tx`
        INSERT INTO organization (id, name) VALUES
          ('org-1', 'Org 1'),
          ('org-2', 'Org 2')
      `;
      await tx`
        INSERT INTO invitation (
          id, organization_id, email, role, status, expires_at, inviter_id
        ) VALUES (
          'invitation-1', 'org-1', ' USER-1@Example.Invalid ', 'member',
          'pending', NOW() + INTERVAL '1 day', 'actor-1'
        )
      `;
      await tx`
        INSERT INTO member (id, organization_id, user_id, role)
        VALUES
          ('member-actor-1', 'org-1', 'actor-1', 'owner'),
          ('member-1', 'org-1', 'user-1', 'member')
      `;
    });

    const initialProjection = await fixture<
      Array<{ revision: string; audit_count: string }>
    >`
      SELECT projection.revision,
             COUNT(audit.*)::TEXT AS audit_count
      FROM organization_membership_outbox projection
      JOIN organization_membership_audit_outbox audit
        ON audit.organization_id = projection.organization_id
       AND audit.user_id = projection.user_id
      WHERE projection.organization_id = 'org-1'
        AND projection.user_id = 'user-1'
      GROUP BY projection.revision
    `;
    expect(initialProjection).toEqual([{ revision: '1', audit_count: '1' }]);

    await fixture`UPDATE member SET role = role WHERE id = 'member-1'`;
    const afterNoOp = await fixture<
      Array<{ revision: string; audit_count: string }>
    >`
      SELECT projection.revision,
             COUNT(audit.*)::TEXT AS audit_count
      FROM organization_membership_outbox projection
      JOIN organization_membership_audit_outbox audit
        ON audit.organization_id = projection.organization_id
       AND audit.user_id = projection.user_id
      WHERE projection.organization_id = 'org-1'
        AND projection.user_id = 'user-1'
      GROUP BY projection.revision
    `;
    expect(afterNoOp).toEqual(initialProjection);

    await expect(
      fixture`UPDATE member SET organization_id = 'org-2' WHERE id = 'member-1'`,
    ).rejects.toThrow('canonical membership identity is immutable');
    await expect(
      fixture`UPDATE member SET user_id = 'user-2' WHERE id = 'member-1'`,
    ).rejects.toThrow('canonical membership identity is immutable');

    await expect(
      fixture`
        SELECT apply_membership_mutation(
          '00000000-0000-4000-8000-000000000201'::UUID,
          'org-1', 'user-1', 'member-1', 'role_change',
          1, 'member', 'admin', 'actor-1'
        ) AS result
      `,
    ).resolves.toHaveLength(1);
    await expect(
      fixture`
        SELECT apply_membership_mutation(
          '00000000-0000-4000-8000-000000000202'::UUID,
          'org-1', 'user-1', 'member-1', 'admin_remove',
          2, 'admin', 'admin', 'actor-1'
        ) AS result
      `,
    ).resolves.toHaveLength(1);

    const rows = await fixture`
      SELECT action, revision, member_id, role, previous_role, applied_role,
             actor_user_id, actor_classification,
             published_at IS NULL AS pending
      FROM organization_membership_audit_outbox
      WHERE organization_id = 'org-1' AND user_id = 'user-1'
      ORDER BY revision
    `;
    expect(rows).toEqual([
      {
        action: 'member_added',
        revision: '1',
        member_id: 'member-1',
        role: 'member',
        previous_role: null,
        applied_role: 'member',
        actor_user_id: null,
        actor_classification: 'pending',
        pending: true,
      },
      {
        action: 'role_changed',
        revision: '2',
        member_id: 'member-1',
        role: 'admin',
        previous_role: 'member',
        applied_role: 'admin',
        actor_user_id: 'actor-1',
        actor_classification: 'verified_user',
        pending: true,
      },
      {
        action: 'member_removed',
        revision: '3',
        member_id: 'member-1',
        role: 'admin',
        previous_role: 'admin',
        applied_role: null,
        actor_user_id: 'actor-1',
        actor_classification: 'verified_user',
        pending: true,
      },
    ]);

    // Creation is the only membership write without a request mutation
    // intent. Simulate its verified Better Auth database-hook evidence; role
    // and removal actors were already bound atomically by migration 027.
    await fixture`
      UPDATE organization_membership_audit_outbox
      SET actor_user_id = 'actor-add', actor_classification = 'verified_user'
      WHERE organization_id = 'org-1' AND user_id = 'user-1'
        AND revision = 1 AND action = 'member_added'
        AND actor_classification = 'pending'
    `;
    const actors = await fixture`
      SELECT revision, actor_user_id, actor_classification
      FROM organization_membership_audit_outbox
      WHERE organization_id = 'org-1' AND user_id = 'user-1'
      ORDER BY revision
    `;
    expect(actors).toEqual([
      {
        revision: '1',
        actor_user_id: 'actor-add',
        actor_classification: 'verified_user',
      },
      {
        revision: '2',
        actor_user_id: 'actor-1',
        actor_classification: 'verified_user',
      },
      {
        revision: '3',
        actor_user_id: 'actor-1',
        actor_classification: 'verified_user',
      },
    ]);

    const invitations = await fixture`
      SELECT invitation_id, organization_id, inviter_user_id, invitee_email,
             role, action, published_at IS NULL AS pending
      FROM organization_invitation_audit_outbox
    `;
    expect(invitations).toEqual([
      {
        invitation_id: 'invitation-1',
        organization_id: 'org-1',
        inviter_user_id: 'actor-1',
        invitee_email: 'user-1@example.invalid',
        role: 'member',
        action: 'member_invited',
        pending: true,
      },
    ]);
  });

  it('gates delivery by invitation state and membership revision', async () => {
    const eligibleBeforeInvitation = await fixture<{ eligible: boolean }[]>`
      SELECT NOT EXISTS (
        SELECT 1
        FROM organization_invitation_audit_outbox invitation_audit
        JOIN "user" invited_user
          ON LOWER(BTRIM(invited_user.email)) = invitation_audit.invitee_email
        WHERE invitation_audit.organization_id = 'org-1'
          AND invited_user.id = 'user-1'
          AND invitation_audit.published_at IS NULL
      ) AS eligible
    `;
    expect(eligibleBeforeInvitation).toEqual([{ eligible: false }]);

    await fixture`
      UPDATE organization_invitation_audit_outbox
      SET published_at = NOW(), attempts = attempts + 1
      WHERE invitation_id = 'invitation-1'
    `;

    const eligibleMemberships = async () =>
      fixture<{ revision: string }[]>`
        SELECT candidate.revision
        FROM organization_membership_audit_outbox candidate
        WHERE candidate.organization_id = 'org-1'
          AND candidate.user_id = 'user-1'
          AND candidate.published_at IS NULL
          AND candidate.actor_classification <> 'pending'
          AND NOT EXISTS (
            SELECT 1 FROM organization_membership_audit_outbox prior
            WHERE prior.organization_id = candidate.organization_id
              AND prior.user_id = candidate.user_id
              AND prior.revision < candidate.revision
              AND prior.published_at IS NULL
          )
        ORDER BY candidate.revision
      `;

    await expect(eligibleMemberships()).resolves.toEqual([{ revision: '1' }]);
    await fixture`
      UPDATE organization_membership_audit_outbox
      SET published_at = NOW(), attempts = attempts + 1
      WHERE organization_id = 'org-1' AND user_id = 'user-1' AND revision = 1
    `;
    await expect(eligibleMemberships()).resolves.toEqual([{ revision: '2' }]);
    await fixture`
      UPDATE organization_membership_audit_outbox
      SET published_at = NOW(), attempts = attempts + 1
      WHERE organization_id = 'org-1' AND user_id = 'user-1' AND revision = 2
    `;
    await expect(eligibleMemberships()).resolves.toEqual([{ revision: '3' }]);
  });

  it('orders an accepted membership by exact invitation id even after email changes', async () => {
    await fixture.unsafe(`
      INSERT INTO "user" (id, email) VALUES
        ('actor-causal', 'actor-causal@example.invalid'),
        ('user-causal', 'causal-before@example.invalid');
      INSERT INTO organization (id, name) VALUES ('org-causal', 'Org Causal');
      INSERT INTO invitation (
        id, organization_id, email, role, status, expires_at, inviter_id
      ) VALUES (
        'invitation-causal', 'org-causal', 'causal-before@example.invalid',
        'member', 'pending', NOW() + INTERVAL '1 day', 'actor-causal'
      );
      UPDATE invitation SET status = 'accepted'
      WHERE id = 'invitation-causal';
      INSERT INTO member (id, organization_id, user_id, role)
      VALUES ('member-causal', 'org-causal', 'user-causal', 'member');
      UPDATE organization_membership_audit_outbox
      SET actor_user_id = 'actor-causal', actor_classification = 'verified_user'
      WHERE organization_id = 'org-causal' AND user_id = 'user-causal';
      UPDATE "user" SET email = 'causal-after@example.invalid'
      WHERE id = 'user-causal';
    `);

    const eligibility = async () => fixture`
      SELECT candidate.invitation_id,
             NOT EXISTS (
               SELECT 1
               FROM organization_invitation_audit_outbox invitation_audit
               WHERE invitation_audit.invitation_id = candidate.invitation_id
                 AND invitation_audit.published_at IS NULL
             ) AS eligible
      FROM organization_membership_audit_outbox candidate
      WHERE candidate.organization_id = 'org-causal'
        AND candidate.user_id = 'user-causal'
        AND candidate.action = 'member_added'
    `;
    await expect(eligibility()).resolves.toEqual([
      { invitation_id: 'invitation-causal', eligible: false },
    ]);
    await fixture`
      UPDATE organization_invitation_audit_outbox
      SET published_at = NOW()
      WHERE invitation_id = 'invitation-causal'
    `;
    await expect(eligibility()).resolves.toEqual([
      { invitation_id: 'invitation-causal', eligible: true },
    ]);
  });

  it('rejects unaudited live-organization role and removal mutations', async () => {
    await fixture.unsafe(`
      INSERT INTO "user" (id, email)
      VALUES ('user-unguarded', 'user-unguarded@example.invalid');
      INSERT INTO organization (id, name)
      VALUES ('org-unguarded', 'Org Unguarded');
      INSERT INTO member (id, organization_id, user_id, role)
      VALUES ('member-unguarded', 'org-unguarded', 'user-unguarded', 'member');
    `);

    await expect(
      fixture`
        UPDATE member SET role = 'admin' WHERE id = 'member-unguarded'
      `,
    ).rejects.toThrow(/verified membership mutation operation is required/i);
    await expect(
      fixture`DELETE FROM member WHERE id = 'member-unguarded'`,
    ).rejects.toThrow(/verified membership mutation operation is required/i);
  });

  it('dead-letters missing creation actor evidence without fabricating a user', async () => {
    await fixture.unsafe(`
      INSERT INTO "user" (id, email)
      VALUES ('user-hol', 'user-hol@example.invalid');
      INSERT INTO organization (id, name) VALUES ('org-hol', 'Org HOL');
      INSERT INTO member (id, organization_id, user_id, role)
      VALUES ('member-hol', 'org-hol', 'user-hol', 'member');
      UPDATE organization_membership_audit_outbox
      SET actor_resolution_not_before = NOW()
      WHERE organization_id = 'org-hol' AND user_id = 'user-hol';
    `);

    for (const expected of [
      { retried: 1, dead_lettered: 0 },
      { retried: 1, dead_lettered: 0 },
      { retried: 0, dead_lettered: 1 },
    ]) {
      await expect(
        fixture`
          SELECT retried, dead_lettered
          FROM recover_pending_membership_audit_actors(10, 3)
        `,
      ).resolves.toEqual([expected]);
      await fixture`
        UPDATE organization_membership_audit_outbox
        SET actor_resolution_not_before = NOW()
        WHERE organization_id = 'org-hol'
          AND user_id = 'user-hol'
          AND actor_classification = 'pending'
      `;
    }

    const recovered = await fixture`
      SELECT revision, actor_user_id, actor_classification,
             actor_resolution_attempts,
             actor_resolution_dead_lettered_at IS NOT NULL AS dead_lettered,
             actor_resolution_last_error
      FROM organization_membership_audit_outbox
      WHERE organization_id = 'org-hol' AND user_id = 'user-hol'
      ORDER BY revision
    `;
    expect(recovered).toEqual([
      {
        revision: '1',
        actor_user_id: null,
        actor_classification: 'unresolved',
        actor_resolution_attempts: 3,
        dead_lettered: true,
        actor_resolution_last_error:
          'verified actor evidence unavailable after 3 attempts',
      },
    ]);

    const eligibleMemberships = async () =>
      fixture<{ revision: string }[]>`
        SELECT candidate.revision
        FROM organization_membership_audit_outbox candidate
        WHERE candidate.organization_id = 'org-hol'
          AND candidate.user_id = 'user-hol'
          AND candidate.published_at IS NULL
          AND candidate.actor_classification <> 'pending'
          AND NOT EXISTS (
            SELECT 1 FROM organization_membership_audit_outbox prior
            WHERE prior.organization_id = candidate.organization_id
              AND prior.user_id = candidate.user_id
              AND prior.revision < candidate.revision
              AND prior.published_at IS NULL
          )
        ORDER BY candidate.revision
      `;
    await expect(eligibleMemberships()).resolves.toEqual([{ revision: '1' }]);
    await fixture`
      UPDATE organization_membership_audit_outbox
      SET published_at = NOW()
      WHERE organization_id = 'org-hol' AND user_id = 'user-hol'
        AND revision = 1
    `;
    await expect(eligibleMemberships()).resolves.toEqual([]);
  });

  it('rejects logical audit mutation and deletion while allowing delivery state', async () => {
    await expect(
      fixture`
        UPDATE organization_membership_audit_outbox
        SET invitation_id = 'forged-invitation',
            invitation_causality_pending = TRUE
        WHERE organization_id = 'org-1' AND user_id = 'user-1' AND revision = 1
      `,
    ).rejects.toThrow('organization membership audit identity is immutable');
    await expect(
      fixture`
        UPDATE organization_membership_audit_outbox
        SET applied_role = 'owner'
        WHERE organization_id = 'org-1' AND user_id = 'user-1' AND revision = 3
      `,
    ).rejects.toThrow('organization membership audit identity is immutable');
    await expect(
      fixture`
        DELETE FROM organization_membership_audit_outbox
        WHERE organization_id = 'org-1' AND user_id = 'user-1' AND revision = 3
      `,
    ).rejects.toThrow('organization membership audit is append-only');
    await expect(
      fixture`
        UPDATE organization_invitation_audit_outbox
        SET invitee_email = 'forged@example.invalid'
        WHERE invitation_id = 'invitation-1'
      `,
    ).rejects.toThrow('organization invitation audit identity is immutable');
    await expect(
      fixture`
        DELETE FROM organization_invitation_audit_outbox
        WHERE invitation_id = 'invitation-1'
      `,
    ).rejects.toThrow('organization invitation audit is append-only');

    const deliveryState = await fixture`
      SELECT revision, published_at IS NOT NULL AS published, attempts
      FROM organization_membership_audit_outbox
      WHERE organization_id = 'org-1' AND user_id = 'user-1'
      ORDER BY revision
    `;
    expect(deliveryState).toEqual([
      { revision: '1', published: true, attempts: 1 },
      { revision: '2', published: true, attempts: 1 },
      { revision: '3', published: false, attempts: 0 },
    ]);
  });
});
