import { randomUUID } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

import postgres from 'postgres';

import {
  flushInvitationAcceptanceRepairs,
  PostgresInvitationAcceptanceRepairRepository,
  repairAcceptedInvitationForActor,
} from './invitation-acceptance-repair';
import { verifyLifecycleFixtureMarker } from './lifecycle-test-fixture';

const testDatabaseUrl = process.env.TEST_DATABASE_URL?.trim();
const postgresDescribe = testDatabaseUrl ? describe : describe.skip;

postgresDescribe('invitation acceptance repair with Postgres', () => {
  const schema = `invitation_repair_${randomUUID().replaceAll('-', '')}`;
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
      max: 1,
      prepare: false,
      connection: { search_path: `${schema},public` },
    });
    await fixture.unsafe(`
      CREATE TABLE "user" (
        id TEXT PRIMARY KEY,
        email TEXT NOT NULL
      );
      CREATE TABLE account (
        id TEXT PRIMARY KEY,
        provider_id TEXT NOT NULL,
        account_id TEXT NOT NULL
      );
      CREATE TABLE organization (
        id TEXT PRIMARY KEY,
        name TEXT NOT NULL DEFAULT 'Lifecycle fixture',
        slug TEXT,
        metadata TEXT NOT NULL DEFAULT '{}',
        created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
      );
      CREATE TABLE member (
        id TEXT PRIMARY KEY,
        organization_id TEXT NOT NULL REFERENCES organization(id) ON DELETE CASCADE,
        user_id TEXT NOT NULL REFERENCES "user"(id) ON DELETE CASCADE,
        role TEXT NOT NULL DEFAULT 'member',
        created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
      );
      CREATE TABLE invitation (
        id TEXT PRIMARY KEY,
        organization_id TEXT NOT NULL REFERENCES organization(id) ON DELETE CASCADE,
        email TEXT NOT NULL,
        role TEXT,
        status TEXT NOT NULL DEFAULT 'pending',
        inviter_id TEXT NOT NULL DEFAULT 'fixture_inviter'
          REFERENCES "user"(id) ON DELETE CASCADE
      );
    `);
    for (const migration of [
      '014_normalized_identity_email.sql',
      '015_organization_projection_outbox.sql',
      '016_unique_organization_membership.sql',
    ]) {
      await fixture.unsafe(
        readFileSync(join(process.cwd(), 'migrations', migration), 'utf8'),
      );
    }
    await fixture.unsafe(`
      INSERT INTO "user" (id, email)
      VALUES
        ('fixture_inviter', 'inviter@example.com'),
        ('user_historical', 'historical@example.com');
      INSERT INTO organization (id) VALUES ('org_historical');
      INSERT INTO invitation (id, organization_id, email, role, status)
      VALUES (
        'inv_historical',
        'org_historical',
        'historical@example.com',
        'member',
        'accepted'
      );
    `);
    await fixture.unsafe(
      readFileSync(
        join(
          process.cwd(),
          'migrations',
          '017_invitation_acceptance_repair.sql',
        ),
        'utf8',
      ),
    );
    for (const migration of [
      '018_historical_owner_preflight.sql',
      '019_membership_audit_outbox.sql',
      '021_invitation_created_at.sql',
      '026_invitation_removal_preflight.sql',
    ]) {
      await fixture.unsafe(
        readFileSync(join(process.cwd(), 'migrations', migration), 'utf8'),
      );
    }
  });

  afterAll(async () => {
    if (fixture) await fixture.end({ timeout: 5 });
    if (admin) {
      await admin`DROP SCHEMA IF EXISTS ${admin(schema)} CASCADE`;
      await admin.end({ timeout: 5 });
    }
  });

  it('does not infer repair intent for a historical accepted invitation', async () => {
    const repository = new PostgresInvitationAcceptanceRepairRepository(
      fixture as never,
    );
    const repairs = await fixture<Array<{ count: number }>>`
      SELECT COUNT(*)::INT AS count
      FROM invitation_acceptance_repair
      WHERE invitation_id = 'inv_historical'
    `;
    expect(repairs[0].count).toBe(0);
    const historicalGap = await fixture<
      Array<{ invitation_id: string; organization_id: string }>
    >`
      SELECT invitation_id, organization_id
      FROM accepted_invitation_membership_gap_report
      WHERE invitation_id = 'inv_historical'
    `;
    expect(historicalGap).toEqual([
      { invitation_id: 'inv_historical', organization_id: 'org_historical' },
    ]);
    const invitationTimestamp = await fixture<
      Array<{ backfilled: boolean; nullable: string; defaulted: boolean }>
    >`
      SELECT i.created_at IS NOT NULL AS backfilled,
             column_info.is_nullable AS nullable,
             column_info.column_default IS NOT NULL AS defaulted
      FROM invitation i
      JOIN information_schema.columns column_info
        ON column_info.table_schema = ${schema}
       AND column_info.table_name = 'invitation'
       AND column_info.column_name = 'created_at'
      WHERE i.id = 'inv_historical'
    `;
    expect(invitationTimestamp).toEqual([
      { backfilled: true, nullable: 'NO', defaulted: true },
    ]);

    await expect(
      repairAcceptedInvitationForActor(
        'inv_historical',
        { userId: 'user_historical', email: 'historical@example.com' },
        repository,
      ),
    ).resolves.toBeNull();
    const members = await fixture<Array<{ count: number }>>`
      SELECT COUNT(*)::INT AS count
      FROM member
      WHERE organization_id = 'org_historical'
    `;
    expect(members[0].count).toBe(0);
  });

  it('does not report an accepted invitation after an explicit canonical member removal', async () => {
    await fixture.unsafe(`
      INSERT INTO "user" (id, email)
      VALUES ('user_explicitly_removed', 'removed@example.com');
      INSERT INTO organization (id) VALUES ('org_explicitly_removed');
      INSERT INTO invitation (id, organization_id, email, role, status)
      VALUES (
        'inv_explicitly_removed',
        'org_explicitly_removed',
        'removed@example.com',
        'member',
        'pending'
      );
      UPDATE invitation SET status = 'accepted'
      WHERE id = 'inv_explicitly_removed';
      INSERT INTO member (id, organization_id, user_id, role)
      VALUES (
        'member_explicitly_removed',
        'org_explicitly_removed',
        'user_explicitly_removed',
        'member'
      );
      UPDATE invitation_acceptance_repair
      SET state = 'completed', completed_at = NOW()
      WHERE invitation_id = 'inv_explicitly_removed';
      DELETE FROM member WHERE id = 'member_explicitly_removed';
    `);

    const repair = await fixture<
      Array<{ state: string; repaired_member_id: string | null }>
    >`
      SELECT state, repaired_member_id
      FROM invitation_acceptance_repair
      WHERE invitation_id = 'inv_explicitly_removed'
    `;
    expect(repair).toEqual([
      {
        state: 'superseded',
        repaired_member_id: 'member_explicitly_removed',
      },
    ]);
    const removalAudit = await fixture<Array<{ count: number }>>`
      SELECT COUNT(*)::INT AS count
      FROM organization_membership_audit_outbox
      WHERE organization_id = 'org_explicitly_removed'
        AND user_id = 'user_explicitly_removed'
        AND member_id = 'member_explicitly_removed'
        AND action = 'member_removed'
    `;
    expect(removalAudit[0].count).toBe(1);
    const gaps = await fixture<Array<{ invitation_id: string }>>`
      SELECT invitation_id
      FROM accepted_invitation_membership_gap_report
      WHERE invitation_id = 'inv_explicitly_removed'
    `;
    expect(gaps).toEqual([]);
  });

  it('repairs the committed status after member and compensation failures', async () => {
    await fixture.unsafe(`
      INSERT INTO "user" (id, email) VALUES ('user_fault', 'Invitee@Example.com');
      INSERT INTO organization (id) VALUES ('org_fault');
      INSERT INTO invitation (id, organization_id, email, role, status)
      VALUES ('inv_fault', 'org_fault', 'invitee@example.com', 'member', 'pending')
    `);
    await fixture.unsafe(`
      CREATE FUNCTION reject_fault_member() RETURNS TRIGGER
      LANGUAGE plpgsql AS $$
      BEGIN
        IF NEW.user_id = 'user_fault' THEN
          RAISE EXCEPTION 'fixture member failure';
        END IF;
        RETURN NEW;
      END;
      $$;
      CREATE TRIGGER reject_fault_member_trigger
      BEFORE INSERT ON member
      FOR EACH ROW EXECUTE FUNCTION reject_fault_member();

      CREATE FUNCTION reject_fault_compensation() RETURNS TRIGGER
      LANGUAGE plpgsql AS $$
      BEGIN
        IF OLD.id = 'inv_fault'
          AND OLD.status = 'accepted'
          AND NEW.status = 'pending' THEN
          RAISE EXCEPTION 'fixture compensation failure';
        END IF;
        RETURN NEW;
      END;
      $$;
      CREATE TRIGGER reject_fault_compensation_trigger
      BEFORE UPDATE OF status ON invitation
      FOR EACH ROW EXECUTE FUNCTION reject_fault_compensation();
    `);

    // This sequence mirrors Better Auth 1.6.x: accepted commits first, the
    // separate member transaction fails, then its compensating reset fails.
    await fixture`
      UPDATE invitation SET status = 'accepted' WHERE id = 'inv_fault'
    `;
    await expect(
      fixture`
        INSERT INTO member (id, organization_id, user_id, role)
        VALUES ('failed_member', 'org_fault', 'user_fault', 'member')
      `,
    ).rejects.toThrow('fixture member failure');
    await expect(
      fixture`
        UPDATE invitation SET status = 'pending' WHERE id = 'inv_fault'
      `,
    ).rejects.toThrow('fixture compensation failure');

    const partial = await fixture<
      Array<{ status: string; members: number; repair_state: string }>
    >`
      SELECT i.status,
             (SELECT COUNT(*)::INT FROM member m WHERE m.organization_id = i.organization_id) AS members,
             r.state AS repair_state
      FROM invitation i
      JOIN invitation_acceptance_repair r ON r.invitation_id = i.id
      WHERE i.id = 'inv_fault'
    `;
    expect(partial).toEqual([
      { status: 'accepted', members: 0, repair_state: 'pending' },
    ]);

    const repository = new PostgresInvitationAcceptanceRepairRepository(
      fixture as never,
    );
    await expect(repository.claimPending(100)).resolves.toEqual([]);

    await fixture.unsafe(`
      DROP TRIGGER reject_fault_member_trigger ON member;
      DROP FUNCTION reject_fault_member();
      DROP TRIGGER reject_fault_compensation_trigger ON invitation;
      DROP FUNCTION reject_fault_compensation();
      UPDATE invitation_acceptance_repair
      SET not_before = NOW() WHERE invitation_id = 'inv_fault';
    `);

    await expect(flushInvitationAcceptanceRepairs(repository)).resolves.toEqual(
      {
        claimed: 1,
        completed: 1,
        superseded: 0,
        notRepairable: 0,
        retried: 0,
        deadLettered: 0,
      },
    );
    const members = await fixture<
      Array<{ member_id: string; member_role: string }>
    >`
      SELECT id AS member_id, role AS member_role
      FROM member
      WHERE organization_id = 'org_fault' AND user_id = 'user_fault'
    `;
    expect(members).toHaveLength(1);
    expect(members[0].member_role).toBe('member');

    const membershipOutbox = await fixture<
      Array<{
        desired_action: string;
        role: string;
        synced_at: Date | null;
      }>
    >`
      SELECT desired_action, role, synced_at
      FROM organization_membership_outbox
      WHERE organization_id = 'org_fault' AND user_id = 'user_fault'
    `;
    expect(membershipOutbox).toEqual([
      { desired_action: 'upsert', role: 'member', synced_at: null },
    ]);

    // A retry returns the canonical row and never overwrites its existing role.
    await fixture`
      UPDATE member SET role = 'owner'
      WHERE organization_id = 'org_fault' AND user_id = 'user_fault'
    `;
    await expect(
      repairAcceptedInvitationForActor(
        'inv_fault',
        { userId: 'user_fault', email: ' INVITEE@example.com ' },
        repository,
      ),
    ).resolves.toMatchObject({ memberRole: 'owner' });
    const count = await fixture<Array<{ count: number }>>`
      SELECT COUNT(*)::INT AS count FROM member
      WHERE organization_id = 'org_fault' AND user_id = 'user_fault'
    `;
    expect(count[0].count).toBe(1);

    // Removing the canonical member is an explicit revocation signal. The
    // completed acceptance intent must become terminal and must never recreate
    // that membership while the old invitation remains accepted.
    await fixture`
      DELETE FROM member
      WHERE organization_id = 'org_fault' AND user_id = 'user_fault'
    `;
    await expect(
      repairAcceptedInvitationForActor(
        'inv_fault',
        { userId: 'user_fault', email: 'invitee@example.com' },
        repository,
      ),
    ).resolves.toBeNull();
    const revoked = await fixture<Array<{ state: string; members: number }>>`
      SELECT r.state,
             (SELECT COUNT(*)::INT FROM member m
              WHERE m.organization_id = r.organization_id
                AND m.user_id = 'user_fault') AS members
      FROM invitation_acceptance_repair r
      WHERE r.invitation_id = 'inv_fault'
    `;
    expect(revoked).toEqual([{ state: 'superseded', members: 0 }]);

    // Even a very late successful Better Auth compensation must supersede the
    // repair and remove only the exact member row the repair inserted.
    await fixture`
      UPDATE invitation SET status = 'pending' WHERE id = 'inv_fault'
    `;
    const compensated = await fixture<
      Array<{ state: string; members: number }>
    >`
      SELECT r.state,
             (SELECT COUNT(*)::INT FROM member m
              WHERE m.organization_id = r.organization_id
                AND m.user_id = 'user_fault') AS members
      FROM invitation_acceptance_repair r
      WHERE r.invitation_id = 'inv_fault'
    `;
    expect(compensated).toEqual([{ state: 'superseded', members: 0 }]);
  });

  it('keeps canonical membership creation available while resolving ambiguous invitation causality by exact id', async () => {
    await fixture.unsafe(`
      INSERT INTO "user" (id, email)
      VALUES ('user_ambiguous_invite', 'ambiguous@example.invalid');
      INSERT INTO organization (id) VALUES ('org_ambiguous_invite');
      INSERT INTO invitation (id, organization_id, email, role, status)
      VALUES
        ('inv_ambiguous_a', 'org_ambiguous_invite', 'ambiguous@example.invalid', 'member', 'pending'),
        ('inv_ambiguous_b', 'org_ambiguous_invite', 'ambiguous@example.invalid', 'member', 'pending');
      UPDATE invitation SET status = 'accepted'
      WHERE id IN ('inv_ambiguous_a', 'inv_ambiguous_b');
      INSERT INTO member (id, organization_id, user_id, role)
      VALUES (
        'member_ambiguous_invite', 'org_ambiguous_invite',
        'user_ambiguous_invite', 'member'
      );
    `);

    await expect(
      fixture`
        SELECT invitation_id, invitation_causality_pending
        FROM organization_membership_audit_outbox
        WHERE organization_id = 'org_ambiguous_invite'
          AND user_id = 'user_ambiguous_invite'
      `,
    ).resolves.toEqual([
      { invitation_id: null, invitation_causality_pending: true },
    ]);
    await expect(
      fixture`
        SELECT invitation_id, repaired_member_id
        FROM invitation_acceptance_repair
        WHERE invitation_id IN ('inv_ambiguous_a', 'inv_ambiguous_b')
        ORDER BY invitation_id
      `,
    ).resolves.toEqual([
      { invitation_id: 'inv_ambiguous_a', repaired_member_id: null },
      { invitation_id: 'inv_ambiguous_b', repaired_member_id: null },
    ]);

    const repository = new PostgresInvitationAcceptanceRepairRepository(
      fixture as never,
    );
    await expect(
      repairAcceptedInvitationForActor(
        'inv_ambiguous_a',
        {
          userId: 'user_ambiguous_invite',
          email: 'ambiguous@example.invalid',
        },
        repository,
      ),
    ).resolves.toMatchObject({
      invitationId: 'inv_ambiguous_a',
      memberId: 'member_ambiguous_invite',
    });
    await expect(
      fixture`
        SELECT invitation_id, invitation_causality_pending,
               actor_classification
        FROM organization_membership_audit_outbox
        WHERE organization_id = 'org_ambiguous_invite'
          AND user_id = 'user_ambiguous_invite'
      `,
    ).resolves.toEqual([
      {
        invitation_id: 'inv_ambiguous_a',
        invitation_causality_pending: false,
        actor_classification: 'verified_user',
      },
    ]);
  });

  it('supersedes invitation deletion without deleting an established member', async () => {
    const repository = new PostgresInvitationAcceptanceRepairRepository(
      fixture as never,
    );
    await fixture.unsafe(`
      INSERT INTO "user" (id, email) VALUES
        ('user_existing', 'existing@example.com'),
        ('user_repaired_delete', 'repaired-delete@example.com');
      INSERT INTO organization (id) VALUES
        ('org_existing'), ('org_repaired_delete');
      INSERT INTO invitation (id, organization_id, email, role, status)
      VALUES
        ('inv_existing', 'org_existing', 'existing@example.com', 'member', 'pending'),
        ('inv_repaired_delete', 'org_repaired_delete', 'repaired-delete@example.com', 'member', 'pending');
      UPDATE invitation SET status = 'accepted'
      WHERE id IN ('inv_existing', 'inv_repaired_delete');
      INSERT INTO member (id, organization_id, user_id, role)
      VALUES ('member_existing_owner', 'org_existing', 'user_existing', 'owner');
    `);

    await expect(
      repairAcceptedInvitationForActor(
        'inv_existing',
        { userId: 'user_existing', email: 'existing@example.com' },
        repository,
      ),
    ).resolves.toMatchObject({
      memberId: 'member_existing_owner',
      memberRole: 'owner',
    });
    await expect(
      repairAcceptedInvitationForActor(
        'inv_repaired_delete',
        {
          userId: 'user_repaired_delete',
          email: 'repaired-delete@example.com',
        },
        repository,
      ),
    ).resolves.toMatchObject({ memberRole: 'member' });

    await fixture`
      DELETE FROM invitation
      WHERE id IN ('inv_existing', 'inv_repaired_delete')
    `;
    const deleted = await fixture<
      Array<{ invitation_id: string; state: string; members: number }>
    >`
      SELECT r.invitation_id, r.state,
             (SELECT COUNT(*)::INT FROM member m
              WHERE m.organization_id = r.organization_id) AS members
      FROM invitation_acceptance_repair r
      WHERE r.invitation_id IN ('inv_existing', 'inv_repaired_delete')
      ORDER BY r.invitation_id
    `;
    expect(deleted).toEqual([
      { invitation_id: 'inv_existing', state: 'superseded', members: 1 },
      {
        invitation_id: 'inv_repaired_delete',
        state: 'superseded',
        members: 1,
      },
    ]);
    const existingRole = await fixture<Array<{ role: string }>>`
      SELECT role FROM member WHERE id = 'member_existing_owner'
    `;
    expect(existingRole).toEqual([{ role: 'owner' }]);
  });

  it('does not revoke a repaired invitee membership when deleting the inviter cascades the invitation', async () => {
    const repository = new PostgresInvitationAcceptanceRepairRepository(
      fixture as never,
    );
    await fixture.unsafe(`
      INSERT INTO "user" (id, email) VALUES
        ('user_cascade_inviter', 'cascade-inviter@example.com'),
        ('user_cascade_invitee', 'cascade-invitee@example.com');
      INSERT INTO organization (id) VALUES ('org_cascade_invite');
      INSERT INTO invitation (
        id, organization_id, email, role, status, inviter_id
      ) VALUES (
        'inv_cascade', 'org_cascade_invite', 'cascade-invitee@example.com',
        'member', 'pending', 'user_cascade_inviter'
      );
      UPDATE invitation SET status = 'accepted' WHERE id = 'inv_cascade';
    `);

    await expect(
      repairAcceptedInvitationForActor(
        'inv_cascade',
        {
          userId: 'user_cascade_invitee',
          email: 'cascade-invitee@example.com',
        },
        repository,
      ),
    ).resolves.toMatchObject({ memberRole: 'member' });

    await fixture`DELETE FROM "user" WHERE id = 'user_cascade_inviter'`;
    const retained = await fixture<
      Array<{ invitation_rows: number; repair_state: string; members: number }>
    >`
      SELECT
        (SELECT COUNT(*)::INT FROM invitation WHERE id = 'inv_cascade')
          AS invitation_rows,
        repair.state AS repair_state,
        (SELECT COUNT(*)::INT FROM member
         WHERE organization_id = 'org_cascade_invite'
           AND user_id = 'user_cascade_invitee') AS members
      FROM invitation_acceptance_repair repair
      WHERE repair.invitation_id = 'inv_cascade'
    `;
    expect(retained).toEqual([
      { invitation_rows: 0, repair_state: 'superseded', members: 1 },
    ]);
  });

  it('suppresses an uncommitted acceptance without revoking a completed membership', async () => {
    const repository = new PostgresInvitationAcceptanceRepairRepository(
      fixture as never,
    );
    await fixture.unsafe(`
      INSERT INTO "user" (id, email) VALUES
        ('user_cancel_pending', 'cancel-pending@example.com'),
        ('user_cancel_repaired', 'cancel-repaired@example.com');
      INSERT INTO organization (id) VALUES
        ('org_cancel_pending'), ('org_cancel_repaired');
      INSERT INTO invitation (id, organization_id, email, role, status)
      VALUES
        ('inv_cancel_pending', 'org_cancel_pending', 'cancel-pending@example.com', 'member', 'pending'),
        ('inv_cancel_repaired', 'org_cancel_repaired', 'cancel-repaired@example.com', 'admin', 'pending');
      UPDATE invitation SET status = 'accepted'
      WHERE id IN ('inv_cancel_pending', 'inv_cancel_repaired');
    `);

    await expect(
      repairAcceptedInvitationForActor(
        'inv_cancel_repaired',
        {
          userId: 'user_cancel_repaired',
          email: 'cancel-repaired@example.com',
        },
        repository,
      ),
    ).resolves.toMatchObject({ memberRole: 'admin' });

    await fixture`
      UPDATE invitation SET status = 'canceled'
      WHERE id IN ('inv_cancel_pending', 'inv_cancel_repaired')
    `;
    await fixture`
      UPDATE invitation_acceptance_repair SET not_before = NOW()
      WHERE invitation_id IN ('inv_cancel_pending', 'inv_cancel_repaired')
    `;
    await expect(flushInvitationAcceptanceRepairs(repository)).resolves.toEqual(
      {
        claimed: 0,
        completed: 0,
        superseded: 0,
        notRepairable: 0,
        retried: 0,
        deadLettered: 0,
      },
    );
    await expect(
      repairAcceptedInvitationForActor(
        'inv_cancel_repaired',
        {
          userId: 'user_cancel_repaired',
          email: 'cancel-repaired@example.com',
        },
        repository,
      ),
    ).resolves.toBeNull();

    const canceled = await fixture<
      Array<{ invitation_id: string; state: string; members: number }>
    >`
      SELECT r.invitation_id, r.state,
             (SELECT COUNT(*)::INT FROM member m
              WHERE m.organization_id = r.organization_id) AS members
      FROM invitation_acceptance_repair r
      WHERE r.invitation_id IN ('inv_cancel_pending', 'inv_cancel_repaired')
      ORDER BY r.invitation_id
    `;
    expect(canceled).toEqual([
      { invitation_id: 'inv_cancel_pending', state: 'superseded', members: 0 },
      { invitation_id: 'inv_cancel_repaired', state: 'superseded', members: 1 },
    ]);
  });

  it('does not recreate a normally inserted member revoked before delayed repair', async () => {
    const repository = new PostgresInvitationAcceptanceRepairRepository(
      fixture as never,
    );
    await fixture.unsafe(`
      INSERT INTO "user" (id, email)
      VALUES ('user_revoked_before_repair', 'revoked-before-repair@example.com');
      INSERT INTO organization (id) VALUES ('org_revoked_before_repair');
      INSERT INTO invitation (id, organization_id, email, role, status)
      VALUES (
        'inv_revoked_before_repair',
        'org_revoked_before_repair',
        'revoked-before-repair@example.com',
        'admin',
        'pending'
      );
      UPDATE invitation SET status = 'accepted'
      WHERE id = 'inv_revoked_before_repair';
      INSERT INTO member (id, organization_id, user_id, role)
      VALUES (
        'member_revoked_before_repair',
        'org_revoked_before_repair',
        'user_revoked_before_repair',
        'owner'
      );
      DELETE FROM member WHERE id = 'member_revoked_before_repair';
      UPDATE invitation_acceptance_repair
      SET not_before = NOW()
      WHERE invitation_id = 'inv_revoked_before_repair';
    `);

    await expect(flushInvitationAcceptanceRepairs(repository)).resolves.toEqual(
      {
        claimed: 0,
        completed: 0,
        superseded: 0,
        notRepairable: 0,
        retried: 0,
        deadLettered: 0,
      },
    );
    const revoked = await fixture<
      Array<{
        state: string;
        members: number;
        repaired_member_id: string | null;
        inserted_member: boolean;
      }>
    >`
      SELECT r.state, r.repaired_member_id, r.inserted_member,
             (SELECT COUNT(*)::INT FROM member m
              WHERE m.organization_id = r.organization_id) AS members
      FROM invitation_acceptance_repair r
      WHERE r.invitation_id = 'inv_revoked_before_repair'
    `;
    expect(revoked).toEqual([
      {
        state: 'superseded',
        members: 0,
        repaired_member_id: 'member_revoked_before_repair',
        inserted_member: false,
      },
    ]);
  });

  it('fails closed for missing state, actor mismatch, and exhausted resolution retries', async () => {
    const repository = new PostgresInvitationAcceptanceRepairRepository(
      fixture as never,
    );
    await expect(repository.repair('missing_invitation')).resolves.toEqual({
      kind: 'superseded',
    });

    await fixture.unsafe(`
      INSERT INTO organization (id) VALUES
        ('org_pending'), ('org_mismatch'), ('org_tampered'), ('org_missing_user');
      INSERT INTO "user" (id, email) VALUES
        ('user_mismatch', 'match@example.com'),
        ('user_tampered', 'tampered@example.com'),
        ('user_evil', 'evil@example.com');
      INSERT INTO invitation (id, organization_id, email, role, status)
      VALUES
        ('inv_pending', 'org_pending', 'pending@example.com', 'member', 'pending'),
        ('inv_mismatch', 'org_mismatch', 'match@example.com', 'member', 'pending'),
        ('inv_tampered', 'org_tampered', 'tampered@example.com', 'member', 'pending'),
        ('inv_missing_user', 'org_missing_user', 'missing@example.com', 'member', 'pending');
      UPDATE invitation SET status = 'accepted'
      WHERE id IN ('inv_mismatch', 'inv_tampered', 'inv_missing_user');
      UPDATE invitation SET email = 'evil@example.com'
      WHERE id = 'inv_tampered';
      UPDATE invitation_acceptance_repair SET not_before = NOW()
      WHERE invitation_id = 'inv_missing_user';
    `);

    await expect(repository.repair('inv_pending')).resolves.toEqual({
      kind: 'superseded',
    });
    await expect(
      repairAcceptedInvitationForActor(
        'inv_mismatch',
        { userId: 'wrong_user', email: 'match@example.com' },
        repository,
      ),
    ).resolves.toBeNull();
    const mismatchedMembers = await fixture<Array<{ count: number }>>`
      SELECT COUNT(*)::INT AS count FROM member
      WHERE organization_id = 'org_mismatch'
    `;
    expect(mismatchedMembers[0].count).toBe(0);
    await expect(
      repairAcceptedInvitationForActor(
        'inv_tampered',
        { userId: 'user_evil', email: 'evil@example.com' },
        repository,
      ),
    ).resolves.toBeNull();
    const tamperedMembers = await fixture<Array<{ count: number }>>`
      SELECT COUNT(*)::INT AS count FROM member
      WHERE organization_id = 'org_tampered'
    `;
    expect(tamperedMembers[0].count).toBe(0);

    await expect(flushInvitationAcceptanceRepairs(repository)).resolves.toEqual(
      {
        claimed: 1,
        completed: 0,
        superseded: 0,
        notRepairable: 0,
        retried: 1,
        deadLettered: 0,
      },
    );
    let failed = await fixture<
      Array<{ state: string; attempts: number; last_error: string }>
    >`
      SELECT state, attempts, last_error
      FROM invitation_acceptance_repair
      WHERE invitation_id = 'inv_missing_user'
    `;
    expect(failed).toEqual([
      {
        state: 'pending',
        attempts: 1,
        last_error: 'canonical_user_unavailable',
      },
    ]);

    await fixture`
      UPDATE invitation_acceptance_repair
      SET attempts = 4, not_before = NOW()
      WHERE invitation_id = 'inv_missing_user'
    `;
    await expect(flushInvitationAcceptanceRepairs(repository)).resolves.toEqual(
      {
        claimed: 1,
        completed: 0,
        superseded: 0,
        notRepairable: 0,
        retried: 0,
        deadLettered: 1,
      },
    );
    failed = await fixture`
      SELECT state, attempts, last_error
      FROM invitation_acceptance_repair
      WHERE invitation_id = 'inv_missing_user'
    `;
    expect(failed).toEqual([
      {
        state: 'dead_letter',
        attempts: 5,
        last_error: 'canonical_user_unavailable',
      },
    ]);
  });
});
