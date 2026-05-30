import { describe, expect, it } from 'vitest';

import { createConfig } from '../src/common/config/app-config';
import { listProviders } from '../src/modules/providers/provider-catalog';

function buildConfig() {
  return createConfig({
    AUTH_CORE_URL: 'http://auth-core:3011',
    AUTH_CORE_INTERNAL_API_KEY: 'test-internal-key',
    BILLING_CORE_URL: 'http://billing-core:3014',
    CONNECTOR_RUNTIME_BASE_URL: 'http://connector-runtime-engine:3003',
    CONNECTOR_RUNTIME_SECRET: 'secret',
    CONNECTOR_RUNTIME_WEBHOOK_SECRET: 'webhook-secret',
    DATABASE_URL: 'postgres://user:password@localhost:5432/integration',
    GOOGLE_DRIVE_INTEGRATION_KEY: 'google-drive',
    GOOGLE_INTEGRATION_KEY: 'google-workspace',
    MICROSOFT_INTEGRATION_KEY: 'microsoft-graph',
    NOTION_INTEGRATION_KEY: 'notion',
    ORG_CORE_URL: 'http://org-core:8080',
    SLACK_INTEGRATION_KEY: 'slack',
    USER_CORE_URL: 'http://user-core:3012'
  });
}

describe('listProviders', () => {
  it('returns the first-party provider catalog', () => {
    const providers = listProviders(buildConfig());

    expect(providers).toHaveLength(9);
    expect(providers).toEqual(expect.arrayContaining([
      expect.objectContaining({ key: 'microsoft', nangoIntegrationId: 'microsoft-graph' }),
      expect.objectContaining({ key: 'google', nangoIntegrationId: 'google-workspace' }),
      expect.objectContaining({ key: 'google-drive', nangoIntegrationId: 'google-drive' }),
      expect.objectContaining({ key: 'notion', nangoIntegrationId: 'notion' }),
      expect.objectContaining({ key: 'slack', nangoIntegrationId: 'slack' })
    ]));
  });
});
