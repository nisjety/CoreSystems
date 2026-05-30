import { createHash } from 'node:crypto';

import { describe, expect, it, vi } from 'vitest';

import { createConfig } from '../src/common/config/app-config';
import { integrationSubjects } from '../src/common/nats/subjects';
import type { ConnectionIntroIngestService } from '../src/modules/data-plane/connection-intro-ingest-service';
import { NangoWebhookService } from '../src/modules/webhooks/nango-webhook-service';

function buildConfig() {
  return createConfig({
    AUTH_CORE_URL: 'http://auth-core:3011',
    AUTH_CORE_INTERNAL_API_KEY: 'test-internal-key',
    BILLING_CORE_URL: 'http://billing-core:3014',
    CONNECTOR_RUNTIME_BASE_URL: 'http://connector-runtime-engine:3003',
    CONNECTOR_RUNTIME_SECRET: 'secret',
    CONNECTOR_RUNTIME_WEBHOOK_SECRET: 'webhook-secret',
    DATABASE_URL: 'postgres://user:password@localhost:5432/integration',
    ORG_CORE_URL: 'http://org-core:8080',
    USER_CORE_URL: 'http://user-core:3012'
  });
}

describe('NangoWebhookService', () => {
  it('accepts a signed Nango auth webhook payload', async () => {
    const payload = {
      type: 'auth',
      operation: 'creation',
      success: true,
      connectionId: 'nango-1',
      tags: {
        end_user_email: 'user@example.com',
        end_user_id: 'user-1',
        organization_id: 'org-1',
        provider_key: 'microsoft',
        selected_sources: 'teams,sharepoint',
        workspace_id: 'ws-1'
      }
    };
    const rawBody = JSON.stringify(payload);
    const signature = createHash('sha256')
      .update('webhook-secret')
      .update(rawBody)
      .digest('hex');

    const repository = {
      getById: vi.fn(),
      list: vi.fn().mockResolvedValue([]),
      markDeleted: vi.fn(),
      recordWebhookEvent: vi.fn().mockResolvedValue(undefined),
      upsertFromAuthWebhook: vi.fn().mockResolvedValue({
        id: 'conn-1',
        organizationId: 'org-1',
        workspaceId: 'ws-1',
        userId: 'user-1',
        userEmail: 'user@example.com',
        providerKey: 'microsoft',
        providerLabel: 'Microsoft 365',
        nangoConnectionId: 'nango-1',
        nangoIntegrationId: 'microsoft-graph',
        status: 'connected',
        lastSyncStatus: null,
        lastSyncSummary: null,
        createdAt: '2026-01-01T00:00:00.000Z',
        updatedAt: '2026-01-01T00:00:00.000Z',
        deletedAt: null
      })
    };
    const eventPublisher = {
      close: vi.fn(),
      publish: vi.fn().mockResolvedValue(undefined)
    };
    const introIngestService = {
      buildIntroSnapshot: vi.fn().mockResolvedValue({
        fetchedAt: '2026-01-01T00:00:00.000Z',
        items: [
          {
            id: 'team-1',
            kind: 'team',
            label: 'Support',
            source: 'microsoft'
          }
        ]
      }),
      ingestConnectionIntro: vi.fn().mockResolvedValue(undefined)
    } as unknown as ConnectionIntroIngestService;
    const service = new NangoWebhookService(
      buildConfig(),
      repository,
      eventPublisher,
      introIngestService
    );

    const result = await service.handle(rawBody, {
      'x-nango-signature': signature
    });

    expect(repository.recordWebhookEvent).toHaveBeenCalledTimes(1);
    expect(repository.upsertFromAuthWebhook).toHaveBeenCalledWith(
      expect.objectContaining({
        organizationId: 'org-1',
        lastSyncSummary: expect.objectContaining({
          introItems: [
            {
              id: 'team-1',
              kind: 'team',
              label: 'Support',
              source: 'microsoft'
            }
          ],
          selectedSources: ['teams', 'sharepoint']
        }),
        providerKey: 'microsoft',
        userId: 'user-1',
        workspaceId: 'ws-1'
      })
    );
    expect(introIngestService.buildIntroSnapshot).toHaveBeenCalledWith({
      nangoConnectionId: 'nango-1',
      nangoIntegrationId: 'microsoft-graph',
      providerKey: 'microsoft',
      selectedSources: ['teams', 'sharepoint']
    });
    expect(eventPublisher.publish).toHaveBeenCalledWith(
      integrationSubjects.connectionCreated,
      expect.objectContaining({
        connectionId: 'conn-1',
        providerKey: 'microsoft'
      })
    );
    expect(introIngestService.ingestConnectionIntro).toHaveBeenCalledWith({
      connection: expect.objectContaining({ id: 'conn-1' }),
      intro: {
        fetchedAt: '2026-01-01T00:00:00.000Z',
        items: [
          {
            id: 'team-1',
            kind: 'team',
            label: 'Support',
            source: 'microsoft'
          }
        ]
      },
      selectedSources: ['teams', 'sharepoint'],
      webhookEventId: expect.any(String)
    });
    expect(result).toMatchObject({
      accepted: true,
      connection: {
        id: 'conn-1'
      }
    });
  });
});
