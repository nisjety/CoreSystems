/**
 * WS05-S8: the SCIM membership surface against a real Postgres. The controller runs
 * unchanged; only its database client is pointed at a fixture schema that carries the
 * production membership migrations (014 through 027, then 029 and 030). Skipped unless
 * TEST_DATABASE_URL names the lifecycle fixture database.
 */
import { randomUUID } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

import { ConflictException, HttpException } from '@nestjs/common';
import postgres from 'postgres';

import { verifyLifecycleFixtureMarker } from './lifecycle-test-fixture';

type SqlCall = (...args: unknown[]) => unknown;
const mockFixture: { client?: SqlCall & { begin: SqlCall } } = {};

jest.mock('./auth', () => ({
  auth: {
    $context: Promise.resolve({
      internalAdapter: { createUser: jest.fn() },
    }),
  },
}));
jest.mock('../db', () => ({
  db: {},
  sqlClient: Object.assign((...args: unknown[]) => mockFixture.client!(...args), {
    begin: (...args: unknown[]) => mockFixture.client!.begin(...args),
  }),
}));

import { ScimProvisioningController } from './scim-provisioning.controller';

const testDatabaseUrl = process.env.TEST_DATABASE_URL?.trim();
const postgresDescribe = testDatabaseUrl ? describe : describe.skip;

const TOKEN = 'scim-live-token-0123456789abcdef0123456789';
const ORG_A = 'org-scim-a';
const ORG_B = 'org-scim-b';
const ADA = 'scim-ada';
const OWNER_A = 'scim-owner-a';
const SOLE_OWNER_B = 'scim-sole-owner-b';

function controller(): ScimProvisioningController {
  process.env.ORG_CORE_IDENTITY_SERVICE_TOKEN = TOKEN;
  return new ScimProvisioningController();
}

async function rejectionOf(work: Promise<unknown>): Promise<unknown> {
  try {
    await work;
  } catch (error) {
    return error;
  }
  throw new Error('expected the call to reject');
}

postgresDescribe('SCIM membership surface with Postgres', () => {
  const schema = `scim_membership_${randomUUID().replaceAll('-', '')}`;
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
    mockFixture.client = fixture as unknown as SqlCall & { begin: SqlCall };

    // The base tables the membership triggers read, as the membership audit
    // fixture declares them: name, banned and ban_reason must exist on "user".
    await fixture.unsafe(`
      CREATE TABLE "user" (
        id TEXT PRIMARY KEY,
        name TEXT,
        email TEXT NOT NULL,
        email_verified BOOLEAN NOT NULL DEFAULT FALSE,
        banned BOOLEAN DEFAULT FALSE,
        ban_reason TEXT
      );
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
    // Production applies gdpr_hard_delete.sql first, then the numbered migrations
    // in order. 029 widens the operation kind; 030 teaches the SQL functions
    // scim_deprovision.
    for (const migration of [
      'gdpr_hard_delete.sql',
      '014_normalized_identity_email.sql',
      '015_organization_projection_outbox.sql',
      '016_unique_organization_membership.sql',
      '017_invitation_acceptance_repair.sql',
      '018_historical_owner_preflight.sql',
      '019_membership_audit_outbox.sql',
      '020_auth_identity_event_outbox.sql',
      '021_invitation_created_at.sql',
      '022_membership_audit_actor_recovery.sql',
      '023_gdpr_outbox_erasure.sql',
      '024_revisioned_cross_plane_projection.sql',
      '025_gdpr_outbox_publish_fence.sql',
      '026_invitation_removal_preflight.sql',
      '027_membership_mutation_intents.sql',
      '029_sso_enforcement.sql',
      '030_scim_deprovision.sql',
    ]) {
      await fixture.unsafe(
        readFileSync(join(process.cwd(), 'migrations', migration), 'utf8'),
      );
    }

    await fixture.unsafe(`
      INSERT INTO "user" (id, email) VALUES
        ('${OWNER_A}', 'owner-a@example.invalid'),
        ('${ADA}', 'ada@example.invalid'),
        ('${SOLE_OWNER_B}', 'owner-b@example.invalid');
      INSERT INTO organization (id, name) VALUES
        ('${ORG_A}', 'SCIM A'),
        ('${ORG_B}', 'SCIM B');
      INSERT INTO member (id, organization_id, user_id, role) VALUES
        ('member-scim-owner-a', '${ORG_A}', '${OWNER_A}', 'owner'),
        ('member-scim-ada-a', '${ORG_A}', '${ADA}', 'member'),
        ('member-scim-ada-b', '${ORG_B}', '${ADA}', 'member'),
        ('member-scim-sole-owner-b', '${ORG_B}', '${SOLE_OWNER_B}', 'owner');
      INSERT INTO session (id, token, user_id, active_organization_id) VALUES
        ('session-ada-a', 'token-ada-a', '${ADA}', '${ORG_A}'),
        ('session-ada-b', 'token-ada-b', '${ADA}', '${ORG_B}');
    `);
  });

  afterAll(async () => {
    if (fixture) await fixture.end({ timeout: 5 });
    if (admin) {
      await admin`DROP SCHEMA IF EXISTS ${admin(schema)} CASCADE`;
      await admin.end({ timeout: 5 });
    }
  });

  it('removes the member through the membership mutation and ends only that organisation\'s sessions', async () => {
    const operationId = randomUUID();
    const result = await controller().deprovision(TOKEN, {
      operationId,
      organizationId: ORG_A,
      actorUserId: OWNER_A,
      userId: ADA,
    });
    expect(result).toEqual({
      removed: true,
      replay: false,
      role: 'member',
      revision: 2,
    });

    const remaining = await fixture`
      SELECT id FROM member WHERE organization_id = ${ORG_A} AND user_id = ${ADA}
    `;
    expect(remaining).toEqual([]);

    const sessions = await fixture`
      SELECT id FROM session WHERE user_id = ${ADA} ORDER BY id
    `;
    expect(sessions).toEqual([{ id: 'session-ada-b' }]);

    const audit = await fixture`
      SELECT action, actor_user_id, actor_classification
      FROM organization_membership_audit_outbox
      WHERE organization_id = ${ORG_A} AND user_id = ${ADA} AND revision = 2
    `;
    expect(audit).toEqual([
      {
        action: 'member_removed',
        actor_user_id: OWNER_A,
        actor_classification: 'verified_user',
      },
    ]);

    const operations = await fixture`
      SELECT kind, state FROM organization_membership_mutation_operation
      WHERE operation_id = ${operationId}::UUID
    `;
    expect(operations).toEqual([{ kind: 'scim_deprovision', state: 'completed' }]);
  });

  it('answers a retried operation from the stored result without a second removal', async () => {
    const operations = await fixture<Array<{ operation_id: string }>>`
      SELECT operation_id::TEXT AS operation_id
      FROM organization_membership_mutation_operation
      WHERE organization_id = ${ORG_A} AND user_id = ${ADA} AND kind = 'scim_deprovision'
    `;
    expect(operations).toHaveLength(1);

    const replay = await controller().deprovision(TOKEN, {
      operationId: operations[0].operation_id,
      organizationId: ORG_A,
      actorUserId: OWNER_A,
      userId: ADA,
    });
    expect(replay).toEqual({
      removed: true,
      replay: true,
      role: 'member',
      revision: 2,
    });

    const removals = await fixture`
      SELECT COUNT(*)::INT AS removals
      FROM organization_membership_audit_outbox
      WHERE organization_id = ${ORG_A} AND user_id = ${ADA} AND action = 'member_removed'
    `;
    expect(removals).toEqual([{ removals: 1 }]);
  });

  it('treats a user who is no longer a member as already removed', async () => {
    const result = await controller().deprovision(TOKEN, {
      operationId: randomUUID(),
      organizationId: ORG_A,
      actorUserId: OWNER_A,
      userId: ADA,
    });
    expect(result).toEqual({ removed: false, replay: false, role: '', revision: 0 });
  });

  it('refuses to remove the only owner and leaves that membership in place', async () => {
    const error = await rejectionOf(
      controller().deprovision(TOKEN, {
        operationId: randomUUID(),
        organizationId: ORG_B,
        actorUserId: OWNER_A,
        userId: SOLE_OWNER_B,
      }),
    );
    expect(error).toBeInstanceOf(ConflictException);
    expect((error as HttpException).getResponse()).toMatchObject({
      code: 'last_owner',
    });

    const owner = await fixture`
      SELECT role FROM member WHERE organization_id = ${ORG_B} AND user_id = ${SOLE_OWNER_B}
    `;
    expect(owner).toEqual([{ role: 'owner' }]);
  });

  it('binds a re-provisioned member to the token creator as the audit actor', async () => {
    const result = await controller().provision(TOKEN, {
      operationId: randomUUID(),
      organizationId: ORG_A,
      actorUserId: OWNER_A,
      email: 'ADA@example.invalid',
    });
    expect(result).toMatchObject({
      userId: ADA,
      role: 'member',
      memberCreated: true,
      userCreated: false,
    });

    const audit = await fixture`
      SELECT action, actor_user_id, actor_classification
      FROM organization_membership_audit_outbox
      WHERE organization_id = ${ORG_A} AND user_id = ${ADA} AND revision = 3
    `;
    expect(audit).toEqual([
      {
        action: 'member_added',
        actor_user_id: OWNER_A,
        actor_classification: 'verified_user',
      },
    ]);

    const again = await controller().provision(TOKEN, {
      operationId: randomUUID(),
      organizationId: ORG_A,
      actorUserId: OWNER_A,
      email: 'ada@example.invalid',
    });
    expect(again).toMatchObject({
      userId: ADA,
      memberCreated: false,
      userCreated: false,
    });
  });
});
