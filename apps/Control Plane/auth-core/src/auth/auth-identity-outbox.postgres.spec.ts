import { randomUUID } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

import postgres from 'postgres';

import { verifyLifecycleFixtureMarker } from './lifecycle-test-fixture';

const testDatabaseUrl = process.env.TEST_DATABASE_URL?.trim();
const postgresDescribe = testDatabaseUrl ? describe : describe.skip;

postgresDescribe('auth identity outbox with Postgres', () => {
  const schema = `identity_outbox_${randomUUID().replaceAll('-', '')}`;
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
        name TEXT NOT NULL,
        email TEXT NOT NULL UNIQUE,
        email_verified BOOLEAN NOT NULL DEFAULT FALSE
      );
      CREATE TABLE account (
        id TEXT PRIMARY KEY,
        account_id TEXT NOT NULL,
        provider_id TEXT NOT NULL,
        user_id TEXT NOT NULL REFERENCES "user"(id) ON DELETE CASCADE,
        scope TEXT,
        created_at TIMESTAMP NOT NULL,
        updated_at TIMESTAMP NOT NULL
      );
    `);
    await fixture.unsafe(
      readFileSync(
        join(process.cwd(), 'migrations', '020_auth_identity_event_outbox.sql'),
        'utf8',
      ),
    );
  });

  afterAll(async () => {
    if (fixture) await fixture.end({ timeout: 5 });
    if (admin) {
      await admin`DROP SCHEMA IF EXISTS ${admin(schema)} CASCADE`;
      await admin.end({ timeout: 5 });
    }
  });

  it('captures OAuth signup events in one transaction with stable ids, dependency order, and split scopes', async () => {
    await fixture.begin(async (tx) => {
      await tx`
        INSERT INTO "user" (id, name, email, email_verified)
        VALUES ('user-1', 'User One', ' USER-1@Example.Invalid ', TRUE)
      `;
      await tx`
        INSERT INTO account (
          id, account_id, provider_id, user_id, scope, created_at, updated_at
        ) VALUES (
          'account-1', 'external-1', 'microsoft', 'user-1',
          'openid profile email', NOW(), NOW()
        )
      `;
    });

    const rows = await fixture<
      Array<{
        event_id: string;
        event_type: string;
        provider: string;
        scopes: string[] | null;
      }>
    >`
      SELECT event_id, event_type, payload->>'provider' AS provider,
             ARRAY(SELECT jsonb_array_elements_text(payload->'scopesGranted')) AS scopes
      FROM auth_identity_event_outbox
      ORDER BY event_type DESC
    `;
    expect(rows).toEqual([
      {
        event_id: 'user:user-1:registered',
        event_type: 'user_registered',
        provider: 'microsoft',
        scopes: [],
      },
      {
        event_id: 'account:account-1:provider_linked',
        event_type: 'provider_linked',
        provider: 'microsoft',
        scopes: ['openid', 'profile', 'email'],
      },
    ]);

    const providerClaimBeforeRegistration = await fixture`
      SELECT candidate.event_id
      FROM auth_identity_event_outbox candidate
      WHERE candidate.event_type = 'provider_linked'
        AND EXISTS (
          SELECT 1 FROM auth_identity_event_outbox dependency
          WHERE dependency.user_id = candidate.user_id
            AND dependency.event_type = 'user_registered'
            AND dependency.published_at IS NOT NULL
        )
    `;
    expect(providerClaimBeforeRegistration).toEqual([]);

    await fixture`
      UPDATE auth_identity_event_outbox
      SET published_at = NOW()
      WHERE event_id = 'user:user-1:registered'
    `;
    const providerClaimAfterRegistration = await fixture`
      SELECT candidate.event_id
      FROM auth_identity_event_outbox candidate
      WHERE candidate.event_type = 'provider_linked'
        AND EXISTS (
          SELECT 1 FROM auth_identity_event_outbox dependency
          WHERE dependency.user_id = candidate.user_id
            AND dependency.event_type = 'user_registered'
            AND dependency.published_at IS NOT NULL
        )
    `;
    expect(providerClaimAfterRegistration).toEqual([
      { event_id: 'account:account-1:provider_linked' },
    ]);

    await fixture`
      INSERT INTO account (
        id, account_id, provider_id, user_id, scope, created_at, updated_at
      ) VALUES (
        'account-1', 'external-1', 'microsoft', 'user-1',
        'openid profile email', NOW(), NOW()
      )
      ON CONFLICT (id) DO NOTHING
    `;
    const count = await fixture<Array<{ count: string }>>`
      SELECT COUNT(*)::TEXT AS count FROM auth_identity_event_outbox
    `;
    expect(count).toEqual([{ count: '2' }]);
  });

  it('does not create provider-link events for credential accounts', async () => {
    await fixture`
      INSERT INTO "user" (id, name, email, email_verified)
      VALUES ('user-credential', 'Credential User', 'credential@example.invalid', TRUE)
    `;
    await fixture`
      INSERT INTO account (
        id, account_id, provider_id, user_id, scope, created_at, updated_at
      ) VALUES (
        'account-password', 'user-credential', 'credential', 'user-credential',
        NULL, NOW(), NOW()
      )
    `;
    const rows = await fixture`
      SELECT event_id FROM auth_identity_event_outbox
      WHERE event_id = 'account:account-password:provider_linked'
    `;
    expect(rows).toEqual([]);
  });

  it('does not relabel a historical pending registration after a later provider link', async () => {
    await fixture`
      INSERT INTO "user" (id, name, email, email_verified)
      VALUES ('user-later', 'Later User', 'later@example.invalid', TRUE)
    `;
    await fixture`
      UPDATE auth_identity_event_outbox
      SET created_at = NOW() - INTERVAL '1 minute'
      WHERE event_id = 'user:user-later:registered'
    `;
    await fixture`
      INSERT INTO account (
        id, account_id, provider_id, user_id, scope, created_at, updated_at
      ) VALUES (
        'account-later', 'external-later', 'microsoft', 'user-later',
        'openid', NOW(), NOW()
      )
    `;

    const registration = await fixture`
      SELECT payload->>'provider' AS provider
      FROM auth_identity_event_outbox
      WHERE event_id = 'user:user-later:registered'
    `;
    expect(registration).toEqual([{ provider: 'email' }]);
  });
});
