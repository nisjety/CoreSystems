import { describe, expect, it } from 'vitest';

import { createConfig } from '../src/common/config/app-config';

const baseEnv = {
  AUTH_CORE_URL: 'http://auth-core:3011',
  AUTH_CORE_INTERNAL_API_KEY: 'test-internal-key',
  BILLING_CORE_URL: 'http://billing-core:3014',
  CONNECTOR_RUNTIME_BASE_URL: 'http://connector-runtime-engine:3003',
  CONNECTOR_RUNTIME_SECRET: 'super-secret',
  CONNECTOR_RUNTIME_WEBHOOK_SECRET: 'webhook-secret',
  DATABASE_URL: 'postgres://user:password@localhost:5432/integration',
  ORG_CORE_URL: 'http://org-core:8080',
  USER_CORE_URL: 'http://user-core:3012'
};

describe('createConfig', () => {
  it('parses explicit values and defaults correctly', () => {
    const config = createConfig({
      ...baseEnv,
      INTEGRATION_SERVICE_NAME: 'integration-core',
      INTEGRATION_SERVICE_PORT: '3026',
      LOG_LEVEL: 'debug'
    });

    expect(config.serviceName).toBe('integration-core');
    expect(config.port).toBe(3026);
    expect(config.logLevel).toBe('debug');
    expect(config.connectorRuntimePublicBaseUrl).toBe('http://localhost:3003');
    expect(config.dataPlaneDocumentsUrl).toBe('http://dpv2-documents-api:8010');
    expect(config.connectorRuntimeSecret).toBe('super-secret');
    expect(config.authCoreInternalApiKey).toBe('test-internal-key');
    expect(config.velionNatsUrl).toBe('nats://velion-nats:4222');
  });

  it('throws when the connector runtime secret is missing', () => {
    expect(() => createConfig({
      ...baseEnv,
      CONNECTOR_RUNTIME_SECRET: ''
    })).toThrow(/CONNECTOR_RUNTIME_SECRET/i);
  });

  it('throws when auth core internal api key is missing', () => {
    expect(() => createConfig({
      ...baseEnv,
      AUTH_CORE_INTERNAL_API_KEY: ''
    })).toThrow(/AUTH_CORE_INTERNAL_API_KEY/i);
  });

  it('throws when database url is missing', () => {
    expect(() => createConfig({
      ...baseEnv,
      DATABASE_URL: ''
    })).toThrow(/DATABASE_URL/i);
  });

  it('throws when webhook secret is missing', () => {
    expect(() => createConfig({
      ...baseEnv,
      CONNECTOR_RUNTIME_WEBHOOK_SECRET: ''
    })).toThrow(/CONNECTOR_RUNTIME_WEBHOOK_SECRET/i);
  });
});
