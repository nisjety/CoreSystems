import { randomUUID } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { createServer, type Server } from 'node:http';
import { join } from 'node:path';

jest.mock('better-auth/api', () => ({
  createAuthMiddleware: <T>(handler: T): T => handler,
}));

import { sqlClient } from '../db';
import { verifyLifecycleFixtureMarker } from './lifecycle-test-fixture';
import {
  flushOrganizationDeletionOutbox,
  setOrganizationEventPublisher,
} from './organization-events.plugin';

const lifecycleDatabaseUrl = process.env.CONTROL_LIFECYCLE_TEST_DATABASE_URL;
const postgresDescribe =
  lifecycleDatabaseUrl && process.env.DATABASE_URL === lifecycleDatabaseUrl
    ? describe
    : describe.skip;

type RecordedRequest = {
  path: string;
  serviceId: string | undefined;
  serviceToken: string | undefined;
};

async function listen(server: Server): Promise<string> {
  await new Promise<void>((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', () => resolve());
  });
  const address = server.address();
  if (!address || typeof address === 'string') {
    throw new Error('fixture server did not bind a TCP port');
  }
  return `http://127.0.0.1:${address.port}`;
}

async function close(server: Server): Promise<void> {
  await new Promise<void>((resolve, reject) => {
    server.close((error) => (error ? reject(error) : resolve()));
  });
}

postgresDescribe('organization deletion lifecycle with Postgres', () => {
  const orgRequests: RecordedRequest[] = [];
  const billingRequests: RecordedRequest[] = [];
  const orgToken = `${randomUUID()}${randomUUID()}`;
  const billingToken = `${randomUUID()}${randomUUID()}`;
  let billingFailuresRemaining = 1;
  const publishDeletion = jest.fn().mockResolvedValue(undefined);

  const orgServer = createServer((request, response) => {
    orgRequests.push({
      path: request.url || '',
      serviceId: request.headers['x-service-id'] as string | undefined,
      serviceToken: request.headers['x-service-token'] as string | undefined,
    });
    response.writeHead(200, { 'content-type': 'application/json' });
    response.end(JSON.stringify({ applied: true }));
  });
  const billingServer = createServer((request, response) => {
    billingRequests.push({
      path: request.url || '',
      serviceId: request.headers['x-service-id'] as string | undefined,
      serviceToken: request.headers['x-service-token'] as string | undefined,
    });
    if (billingFailuresRemaining > 0) {
      billingFailuresRemaining -= 1;
      response.writeHead(503, { 'content-type': 'application/json' });
      response.end(JSON.stringify({ error: 'fixture transient failure' }));
      return;
    }
    response.writeHead(200, { 'content-type': 'application/json' });
    response.end(JSON.stringify({ ok: true }));
  });

  beforeAll(async () => {
    await verifyLifecycleFixtureMarker(
      sqlClient,
      lifecycleDatabaseUrl!,
      'auth_outbox',
      process.env.CONTROL_LIFECYCLE_FIXTURE_ID ?? '',
    );
    process.env.ORG_SERVICE_URL = await listen(orgServer);
    process.env.BILLING_CORE_URL = await listen(billingServer);
    process.env.ORG_CORE_SERVICE_TOKEN = orgToken;
    process.env.BILLING_CORE_SERVICE_TOKEN = billingToken;
    setOrganizationEventPublisher({
      publishOrganizationDeletionProjection: publishDeletion,
    } as never);

    await sqlClient.unsafe(`
      CREATE TABLE organization (
        id TEXT PRIMARY KEY,
        name TEXT NOT NULL,
        slug TEXT,
        metadata TEXT
      );
      CREATE TABLE member (
        id TEXT PRIMARY KEY,
        organization_id TEXT NOT NULL REFERENCES organization(id) ON DELETE CASCADE,
        user_id TEXT NOT NULL,
        role TEXT NOT NULL DEFAULT 'member',
        created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
      );
    `);
    await sqlClient.unsafe(
      readFileSync(
        join(
          process.cwd(),
          'migrations',
          '015_organization_projection_outbox.sql',
        ),
        'utf8',
      ),
    );
    await sqlClient.unsafe(
      readFileSync(
        join(
          process.cwd(),
          'migrations',
          '024_revisioned_cross_plane_projection.sql',
        ),
        'utf8',
      ),
    );
  });

  afterAll(async () => {
    await close(orgServer);
    await close(billingServer);
    await sqlClient.end({ timeout: 5 });
  });

  it('resumes only the failed sink and completes deletion exactly once', async () => {
    await sqlClient.unsafe(`
      INSERT INTO organization (id, name, slug, metadata)
      VALUES ('org_auth_deletion', 'Disposable Organization', 'disposable', '{}');
      INSERT INTO member (id, organization_id, user_id, role)
      VALUES ('member_auth_deletion', 'org_auth_deletion', 'owner_auth_deletion', 'owner');
      DELETE FROM organization WHERE id = 'org_auth_deletion';
    `);

    await expect(flushOrganizationDeletionOutbox()).resolves.toBe(0);
    let rows = await sqlClient<
      Array<{
        attempts: number;
        billing_synced: boolean;
        org_synced: boolean;
        event_synced: boolean;
        completed: boolean;
      }>
    >`
      SELECT attempts,
             billing_synced_at IS NOT NULL AS billing_synced,
             org_synced_at IS NOT NULL AS org_synced,
             event_synced_at IS NOT NULL AS event_synced,
             completed_at IS NOT NULL AS completed
      FROM organization_deletion_outbox
      WHERE organization_id = 'org_auth_deletion'
    `;
    expect(rows).toEqual([
      {
        attempts: 1,
        billing_synced: false,
        org_synced: true,
        event_synced: true,
        completed: false,
      },
    ]);
    expect(billingRequests).toHaveLength(1);
    expect(orgRequests).toHaveLength(1);

    await expect(flushOrganizationDeletionOutbox()).resolves.toBe(1);
    rows = await sqlClient`
      SELECT attempts,
             billing_synced_at IS NOT NULL AS billing_synced,
             org_synced_at IS NOT NULL AS org_synced,
             event_synced_at IS NOT NULL AS event_synced,
             completed_at IS NOT NULL AS completed
      FROM organization_deletion_outbox
      WHERE organization_id = 'org_auth_deletion'
    `;
    expect(rows).toEqual([
      {
        attempts: 2,
        billing_synced: true,
        org_synced: true,
        event_synced: true,
        completed: true,
      },
    ]);
    expect(billingRequests).toHaveLength(2);
    expect(orgRequests).toHaveLength(1);
    expect(publishDeletion).toHaveBeenCalledTimes(1);
    expect(publishDeletion).toHaveBeenCalledWith(
      { organizationId: 'org_auth_deletion', revision: 2 },
      'organization:org_auth_deletion:2:deleted',
    );

    await expect(flushOrganizationDeletionOutbox()).resolves.toBe(0);
    expect(billingRequests).toHaveLength(2);
    expect(orgRequests).toHaveLength(1);

    expect(billingRequests[0]).toEqual({
      path: '/api/v1/billing/orgs/org_auth_deletion/deactivate',
      serviceId: 'auth-core',
      serviceToken: billingToken,
    });
    expect(orgRequests[0]).toEqual({
      path: '/internal/orgs/org_auth_deletion/reconcile-delete',
      serviceId: 'auth-core',
      serviceToken: orgToken,
    });
  });
});
