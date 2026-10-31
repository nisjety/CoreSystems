import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import test from 'node:test';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
test('canonical Studio subset preserves grants, strict auth and owner-plane boundaries', () => {
  const fixture = mkdtempSync(resolve(tmpdir(), 'studio-ingestion-test-'));
  try {
    const secrets = resolve(fixture, 'secrets');
    mkdirSync(secrets, { mode: 0o700 });
    const source = resolve(fixture, 'integration.env');
    const env = {
      INTERNAL_API_KEY: 'synthetic-internal', AUTH_CORE_INTERNAL_API_KEY: 'synthetic-auth',
      INTEGRATION_ORG_CORE_SERVICE_TOKEN: 'synthetic-org', INTEGRATION_BILLING_CORE_SERVICE_TOKEN: 'synthetic-billing',
      INTEGRATION_AUDIT_CORE_SERVICE_TOKEN: 'synthetic-audit', INTEGRATION_CREDENTIALS_ENCRYPTION_KEY: 'synthetic-vault',
      INTEGRATION_PROVIDER_WRITE_ATTESTATION_KEYS_JSON: '[]', GOOGLE_CLIENT_ID: 'synthetic-google',
      GOOGLE_CLIENT_SECRET: 'synthetic-google-secret', INTEGRATION_PUBLIC_BASE_URL: 'http://localhost:3026',
    };
    writeFileSync(source, Object.entries(env).map(([key, value]) => `${key}=${value}`).join('\n'), { mode: 0o600 });
    writeFileSync(resolve(secrets, 'integration_db_password'), 'synthetic-db', { mode: 0o600 });
    writeFileSync(resolve(secrets, 'integration_google_client_secret'), env.GOOGLE_CLIENT_SECRET, { mode: 0o600 });
    const required = [...new Set([...readFileSync(resolve(root, 'docker-compose.yml'), 'utf8')
      .matchAll(/\$\{([A-Z_0-9]+):\?/g)].map((match) => match[1]))];
    const interpolation = resolve(fixture, 'canonical.env');
    writeFileSync(interpolation, required.map((key) => `${key}=synthetic-required-value`).join('\n'), { mode: 0o600 });
    const result = spawnSync('sh', [resolve(root, 'scripts/run-engagement-ingestion.sh'), 'config', '--format', 'json'], {
      encoding: 'utf8', env: { ...process.env, INTEGRATION_LOCAL_SOURCE_ENV: source,
        STUDIO_LOCAL_SECRETS_DIR: secrets, INTEGRATION_CANONICAL_ENV_FILE: interpolation },
    });
    assert.equal(result.status, 0, result.stderr);
    const config = JSON.parse(result.stdout);
    assert.equal(config.name, 'ingestion-plane');
    const active = Object.entries(config.services).filter(([, service]) => !service.profiles?.length).map(([name]) => name).sort();
    assert.deepEqual(active, ['dragonfly', 'integration-api', 'integration-postgres', 'nats', 'postgres',
      'quarry-control', 'quarry-edge', 'quarry-orchestrator', 'searxng', 'temporal', 'temporal-postgres'].sort());
    const integration = config.services['integration-api'];
    assert.equal(integration.environment.DATABASE_URL, 'postgresql://integration_user:synthetic-db@integration-postgres:5432/integration');
    assert.equal(integration.environment.GOOGLE_CLIENT_SECRET, '');
    assert.equal(integration.environment.GOOGLE_CLIENT_SECRET_FILE, '/run/secrets/google_client_secret');
    assert.equal(integration.environment.INTEGRATION_ALLOWED_RETURN_ORIGINS, 'http://localhost:5173');
    assert.equal(integration.environment.INTEGRATION_CREDENTIALS_ENCRYPTION_KEY, 'synthetic-vault');
    assert.deepEqual(Object.keys(integration.depends_on), ['integration-postgres']);
    assert.equal(config.volumes['integration-postgres-data'].external, true);
    assert.equal(config.volumes['integration-postgres-data'].name, 'studio-integration-local_integration-postgres-data');
    assert.equal(config.services['quarry-edge'].environment.QUARRY_EDGE_AUTH_DEV_BYPASS, '0');
    assert.equal(config.services['quarry-edge'].environment.QUARRY_EDGE__MODEL_PLANE_URL, '');
    assert.equal(config.services['quarry-edge'].environment.QUARRY_EDGE__DATA_PLANE_INGEST_URL, '');
    assert.equal(integration.ports[0].host_ip, '127.0.0.1');
    writeFileSync(resolve(secrets, 'integration_pagespeed_api_key'), 'synthetic-pagespeed', { mode: 0o600 });
    writeFileSync(resolve(secrets, 'integration_clarity_bindings.json'), '{"version":1,"bindings":[]}', { mode: 0o600 });
    writeFileSync(resolve(secrets, 'integration_clarity_export_token'), 'synthetic-clarity', { mode: 0o600 });
    writeFileSync(resolve(secrets, 'integration_clarity_triodelab_export_token'), 'synthetic-triodelab', { mode: 0o600 });
    const providers = spawnSync('sh', [resolve(root, 'scripts/run-engagement-ingestion.sh'), 'config', '--format', 'json'], {
      encoding: 'utf8', env: { ...process.env, INTEGRATION_LOCAL_SOURCE_ENV: source,
        STUDIO_LOCAL_SECRETS_DIR: secrets, INTEGRATION_CANONICAL_ENV_FILE: interpolation,
        INTEGRATION_LOCAL_PAGESPEED: '1', INTEGRATION_LOCAL_CLARITY: '1', INTEGRATION_LOCAL_CLARITY_TRIODELAB: '1' },
    });
    assert.equal(providers.status, 0, providers.stderr);
    const configured = JSON.parse(providers.stdout).services['integration-api'];
    assert.equal(configured.environment.PAGESPEED_API_KEY_FILE, '/run/secrets/pagespeed_api_key');
    assert.equal(configured.environment.CLARITY_BINDINGS_FILE, '/run/secrets/clarity_bindings');
    assert.equal(configured.environment.CLARITY_API_TOKEN, undefined);
    assert.equal(configured.secrets.length, 5);
  } finally { rmSync(fixture, { recursive: true, force: true }); }
});
