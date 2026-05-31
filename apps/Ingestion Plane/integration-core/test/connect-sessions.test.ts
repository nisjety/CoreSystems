import { describe, expect, it, vi } from 'vitest';

import { createConfig } from '../src/common/config/app-config';
import { ConnectSessionService } from '../src/modules/connect-sessions/service';

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

describe('ConnectSessionService', () => {
  it('creates a provider-scoped connect session', async () => {
    const runtimeClient = {
      createConnectSession: vi.fn().mockResolvedValue({
        connectLink: 'http://localhost:3009/connect',
        expiresAt: '2026-01-01T00:00:00.000Z',
        token: 'nango-session-token'
      })
    };
    const service = new ConnectSessionService(buildConfig(), runtimeClient);

    const response = await service.createSession({
      organizationId: 'org-1',
      providerKey: 'microsoft',
      selectedSources: ['sharepoint', 'teams'],
      userEmail: 'user@example.com',
      userId: 'user-1',
      workspaceId: 'ws-1'
    });

    expect(runtimeClient.createConnectSession).toHaveBeenCalledWith({
      allowedIntegrations: ['microsoft-graph'],
      tags: {
        end_user_email: 'user@example.com',
        end_user_id: 'user-1',
        organization_id: 'org-1',
        provider_key: 'microsoft',
        selected_sources: 'sharepoint,teams',
        workspace_id: 'ws-1'
      }
    });
    expect(response).toMatchObject({
      sessionToken: 'nango-session-token',
      provider: {
        key: 'microsoft',
        label: 'Microsoft 365'
      }
    });
  });

  it('normalizes provider aliases before creating a session', async () => {
    const runtimeClient = {
      createConnectSession: vi.fn().mockResolvedValue({
        connectLink: 'http://localhost:3009/connect',
        expiresAt: '2026-01-01T00:00:00.000Z',
        token: 'nango-session-token'
      })
    };
    const service = new ConnectSessionService(buildConfig(), runtimeClient);

    const response = await service.createSession({
      organizationId: 'org-1',
      providerKey: 'gdrive',
      userEmail: 'user@example.com',
      userId: 'user-1',
      workspaceId: 'ws-1'
    });

    expect(runtimeClient.createConnectSession).toHaveBeenCalledWith(
      expect.objectContaining({
        allowedIntegrations: ['google-drive'],
        tags: expect.objectContaining({
          provider_key: 'google-drive'
        })
      })
    );
    expect(response.provider).toEqual({
      key: 'google-drive',
      label: 'Google Drive'
    });
  });
});
