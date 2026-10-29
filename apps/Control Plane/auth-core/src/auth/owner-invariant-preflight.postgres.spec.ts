import { randomUUID } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

import postgres from 'postgres';

import { verifyLifecycleFixtureMarker } from './lifecycle-test-fixture';

const testDatabaseUrl = process.env.TEST_DATABASE_URL?.trim();
const postgresDescribe = testDatabaseUrl ? describe : describe.skip;

postgresDescribe('reviewed historical owner preflight with Postgres', () => {
  const schema = `owner_preflight_${randomUUID().replaceAll('-', '')}`;
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

  beforeEach(async () => {
    await fixture.unsafe(`
      TRUNCATE owner_invariant_repair_audit,
               owner_invariant_reviewed_mapping,
               organization_invitation_audit_outbox,
               organization_membership_audit_outbox,
               organization_membership_outbox,
               organization_projection_outbox,
               member, organization, "user" CASCADE;
    `);
  });

  afterAll(async () => {
    if (fixture) await fixture.end({ timeout: 5 });
    if (admin) {
      await admin`DROP SCHEMA IF EXISTS ${admin(schema)} CASCADE`;
      await admin.end({ timeout: 5 });
    }
  });

  async function seed(
    suffix: string,
    role = 'member',
  ): Promise<{ organizationId: string; userId: string; memberId: string }> {
    const organizationId = `org_${suffix}`;
    const userId = `user_${suffix}`;
    const memberId = `member_${suffix}`;
    await fixture.begin(async (transaction) => {
      await transaction`
        INSERT INTO "user" (id, email) VALUES (${userId}, ${`${suffix}@example.invalid`})
      `;
      await transaction`
        INSERT INTO organization (id, name) VALUES (${organizationId}, ${`Organization ${suffix}`})
      `;
      await transaction`
        INSERT INTO member (id, organization_id, user_id, role)
        VALUES (${memberId}, ${organizationId}, ${userId}, ${role})
      `;
    });
    return { organizationId, userId, memberId };
  }

  async function review(
    mappingId: string,
    organizationId: string,
    userId: string,
    expectedRole: string,
  ): Promise<void> {
    await fixture`
      INSERT INTO owner_invariant_reviewed_mapping (
        mapping_id, organization_id, owner_user_id,
        expected_organization_created_at, expected_member_role,
        reviewed_by, reviewed_at
      )
      SELECT ${mappingId}, o.id, ${userId}, o.created_at, ${expectedRole},
             'security-review@example.invalid', NOW()
      FROM organization o WHERE o.id = ${organizationId}
    `;
  }

  it('reports and stops on an ownerless organization without a reviewed mapping', async () => {
    const seeded = await seed('unmapped');
    const report = await fixture<Array<{ issue: string }>>`
      SELECT issue FROM owner_invariant_preflight_report
      WHERE organization_id = ${seeded.organizationId}
    `;
    expect(report).toEqual([{ issue: 'ownerless_unmapped' }]);
    await expect(
      fixture`SELECT apply_reviewed_owner_repairs()`,
    ).rejects.toThrow(/OWNER_INVARIANT_PREFLIGHT_FAILED/);
    const roles = await fixture<Array<{ role: string }>>`
      SELECT role FROM member WHERE id = ${seeded.memberId}
    `;
    expect(roles).toEqual([{ role: 'member' }]);
  });

  it('makes the read-only lifecycle preflight a real process-level release gate', async () => {
    const seeded = await seed('release_gate');
    const script = join(
      process.cwd(),
      'scripts',
      'validate-lifecycle-preflight.sh',
    );
    const blocked = spawnSync('sh', [script], {
      encoding: 'utf8',
      env: {
        ...process.env,
        DATABASE_URL: testDatabaseUrl!,
        PGOPTIONS: `-c search_path=${schema},public`,
      },
    });
    expect(blocked.status).toBe(42);
    expect(blocked.stderr).toContain('OWNER_INVARIANT_PREFLIGHT_FAILED');
    expect(blocked.stderr).not.toContain(seeded.organizationId);

    await review(
      'map_release_gate',
      seeded.organizationId,
      seeded.userId,
      'member',
    );
    const stillBlocked = spawnSync('sh', [script], {
      encoding: 'utf8',
      env: {
        ...process.env,
        DATABASE_URL: testDatabaseUrl!,
        PGOPTIONS: `-c search_path=${schema},public`,
      },
    });
    expect(stillBlocked.status).toBe(42);
    expect(stillBlocked.stderr).toContain('OWNER_INVARIANT_PREFLIGHT_FAILED');
    expect(stillBlocked.stderr).not.toContain(seeded.organizationId);

    await fixture`SELECT apply_reviewed_owner_repairs()`;
    const passed = spawnSync('sh', [script], {
      encoding: 'utf8',
      env: {
        ...process.env,
        DATABASE_URL: testDatabaseUrl!,
        PGOPTIONS: `-c search_path=${schema},public`,
      },
    });
    expect(passed.status).toBe(0);
    expect(passed.stdout).toContain('Auth lifecycle preflight passed');
  });

  it('blocks release on an accepted historical invite without canonical membership', async () => {
    const owner = await seed('accepted_gap_owner', 'owner');
    await fixture`
      INSERT INTO "user" (id, email)
      VALUES ('user_accepted_gap_invitee', 'accepted-gap@example.invalid')
    `;
    await fixture`
      INSERT INTO invitation (
        id, organization_id, email, role, status, expires_at, inviter_id
      ) VALUES (
        'inv_accepted_gap', ${owner.organizationId},
        'accepted-gap@example.invalid', 'member', 'accepted',
        NOW() + INTERVAL '1 day', ${owner.userId}
      )
    `;

    const blocked = spawnSync(
      'sh',
      [join(process.cwd(), 'scripts', 'validate-lifecycle-preflight.sh')],
      {
        encoding: 'utf8',
        env: {
          ...process.env,
          DATABASE_URL: testDatabaseUrl!,
          PGOPTIONS: `-c search_path=${schema},public`,
        },
      },
    );
    expect(blocked.status).toBe(43);
    expect(blocked.stderr).toContain('INVITATION_MEMBERSHIP_PREFLIGHT_FAILED');
    expect(blocked.stderr).not.toContain('inv_accepted_gap');
  });

  it('reports and stops when reviewers supplied ambiguous mappings', async () => {
    const first = await seed('ambiguous_first');
    const secondUserId = 'user_ambiguous_second';
    await fixture.begin(async (transaction) => {
      await transaction`
        INSERT INTO "user" (id, email) VALUES (${secondUserId}, 'ambiguous-second@example.invalid')
      `;
      await transaction`
        INSERT INTO member (id, organization_id, user_id, role)
        VALUES ('member_ambiguous_second', ${first.organizationId}, ${secondUserId}, 'admin')
      `;
    });
    await review(
      'map_ambiguous_first',
      first.organizationId,
      first.userId,
      'member',
    );
    await review(
      'map_ambiguous_second',
      first.organizationId,
      secondUserId,
      'admin',
    );
    const report = await fixture<Array<{ issue: string }>>`
      SELECT issue FROM owner_invariant_preflight_report
      WHERE organization_id = ${first.organizationId}
    `;
    expect(report).toEqual([{ issue: 'ownerless_ambiguous_mapping' }]);
    await expect(
      fixture`SELECT apply_reviewed_owner_repairs()`,
    ).rejects.toThrow(/OWNER_INVARIANT_PREFLIGHT_FAILED/);
  });

  it('stops when canonical membership changed after review', async () => {
    const seeded = await seed('stale');
    await review('map_stale', seeded.organizationId, seeded.userId, 'member');
    await fixture`
      UPDATE organization SET created_at = created_at + INTERVAL '1 second'
      WHERE id = ${seeded.organizationId}
    `;
    const report = await fixture<Array<{ issue: string }>>`
      SELECT issue FROM owner_invariant_preflight_report
      WHERE organization_id = ${seeded.organizationId}
    `;
    expect(report).toEqual([{ issue: 'stale_organization_precondition' }]);
    await expect(
      fixture`SELECT apply_reviewed_owner_repairs()`,
    ).rejects.toThrow(/OWNER_INVARIANT_PREFLIGHT_FAILED/);
  });

  it('leaves a valid multi-owner organization unaffected', async () => {
    const first = await seed('multi_owner_first', 'owner');
    await fixture.begin(async (transaction) => {
      await transaction`
        INSERT INTO "user" (id, email)
        VALUES ('user_multi_owner_second', 'multi-owner-second@example.invalid')
      `;
      await transaction`
        INSERT INTO member (id, organization_id, user_id, role)
        VALUES (
          'member_multi_owner_second', ${first.organizationId},
          'user_multi_owner_second', 'owner'
        )
      `;
    });
    const report = await fixture`
      SELECT issue FROM owner_invariant_preflight_report
      WHERE organization_id = ${first.organizationId}
    `;
    expect(report).toEqual([]);
    await expect(
      fixture`SELECT apply_reviewed_owner_repairs() AS applied`,
    ).resolves.toEqual([{ applied: 0 }]);
    const owners = await fixture<Array<{ owners: number }>>`
      SELECT COUNT(*)::INT AS owners FROM member
      WHERE organization_id = ${first.organizationId} AND role = 'owner'
    `;
    expect(owners).toEqual([{ owners: 2 }]);
  });

  it('applies one reviewed existing member atomically and records durable evidence', async () => {
    const seeded = await seed('safe', 'admin');
    await review('map_safe', seeded.organizationId, seeded.userId, 'admin');
    await expect(
      fixture`SELECT apply_reviewed_owner_repairs() AS applied`,
    ).resolves.toEqual([{ applied: 1 }]);
    await expect(
      fixture`SELECT apply_reviewed_owner_repairs() AS applied`,
    ).resolves.toEqual([{ applied: 0 }]);

    const state = await fixture`
      SELECT m.role, map.applied_at IS NOT NULL AS mapped,
             map.applied_member_id,
             outbox.desired_action, outbox.role AS outbox_role,
             outbox.revision AS outbox_revision,
             outbox.synced_at IS NULL AS pending,
             projection.owner_user_id,
             audit.previous_role AS audit_previous_role,
             audit.applied_role AS audit_applied_role,
             audit.reviewed_by AS audit_reviewed_by,
             audit.applied_at >= audit.reviewed_at AS audit_ordered,
             membership_audit.actor_user_id AS membership_actor_user_id,
             membership_audit.actor_classification
      FROM member m
      JOIN owner_invariant_reviewed_mapping map
        ON map.organization_id = m.organization_id AND map.owner_user_id = m.user_id
      JOIN organization_membership_outbox outbox
        ON outbox.organization_id = m.organization_id AND outbox.user_id = m.user_id
      JOIN organization_projection_outbox projection
        ON projection.organization_id = m.organization_id
      JOIN owner_invariant_repair_audit audit
        ON audit.mapping_id = map.mapping_id
      JOIN organization_membership_audit_outbox membership_audit
        ON membership_audit.organization_id = m.organization_id
       AND membership_audit.user_id = m.user_id
       AND membership_audit.revision = outbox.revision
      WHERE m.id = ${seeded.memberId}
    `;
    expect(state).toEqual([
      {
        role: 'owner',
        mapped: true,
        applied_member_id: seeded.memberId,
        desired_action: 'upsert',
        outbox_role: 'owner',
        outbox_revision: '2',
        pending: true,
        owner_user_id: seeded.userId,
        audit_previous_role: 'admin',
        audit_applied_role: 'owner',
        audit_reviewed_by: 'security-review@example.invalid',
        audit_ordered: true,
        membership_actor_user_id: 'security-review@example.invalid',
        actor_classification: 'operator',
      },
    ]);
    await expect(
      fixture`
        UPDATE owner_invariant_repair_audit
        SET applied_role = 'member'
        WHERE mapping_id = 'map_safe'
      `,
    ).rejects.toThrow(/append-only/);
    await expect(
      fixture`
        DELETE FROM owner_invariant_repair_audit
        WHERE mapping_id = 'map_safe'
      `,
    ).rejects.toThrow(/append-only/);
  });
});
