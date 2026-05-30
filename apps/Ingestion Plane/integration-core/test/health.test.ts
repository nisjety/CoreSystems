import { describe, expect, it } from 'vitest';

import { createConfig } from '../src/common/config/app-config';
import { buildHealthResponse } from '../src/modules/health/http';

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

describe('buildHealthResponse', () => {
  it('returns service health metadata', () => {
    const response = buildHealthResponse(buildConfig());

    expect(response).toMatchObject({
      success: true,
      data: {
        service: 'integration-core',
        status: 'ok'
      }
    });
    expect(Date.parse(response.data.timestamp)).not.toBeNaN();
  });
});