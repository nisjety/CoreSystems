import { setTimeout as delay } from 'node:timers/promises';
import { join } from 'node:path';

import { createConfig } from './common/config/app-config';
import { HttpAuthClient } from './common/auth/auth-client';
import { HttpOrgClient } from './common/auth/org-client';
import { NatsBillingClient } from './common/billing/billing-client';
import { createPostgresPool, runSqlMigrations } from './common/db/postgres';
import { createNatsEventPublisher } from './common/nats/event-publisher';
import { NangoSdkClient } from './common/runtime/nango-runtime-client';
import { createApp } from './app';
import { ConnectSessionService } from './modules/connect-sessions/service';
import { PgConnectionMappingRepository } from './modules/connections/connection-mapping-repository';
import { ConnectionIntroIngestService } from './modules/data-plane/connection-intro-ingest-service';
import { NangoWebhookService } from './modules/webhooks/nango-webhook-service';
import { ZammadWebhookService } from './modules/webhooks/zammad-webhook-service';
import { NovuWebhookService } from './modules/webhooks/novu-webhook-service';
import { NangoManagementService } from './modules/connectors/nango-management-service';
import { NovuManagementService } from './modules/connectors/novu-management-service';
import { SupportIntelligenceService } from './modules/connectors/support-intelligence-service';

async function retry<T>(label: string, operation: () => Promise<T>, attempts = 5): Promise<T> {
  let lastError: unknown;

  for (let attempt = 1; attempt <= attempts; attempt += 1) {
    try {
      return await operation();
    } catch (error) {
      lastError = error;

      if (attempt === attempts) {
        break;
      }

      await delay(attempt * 1000);
    }
  }

  throw new Error(`Failed to initialize ${label}`, {
    cause: lastError instanceof Error ? lastError : undefined
  });
}

async function main(): Promise<void> {
  const config = createConfig(process.env);
  const pool = createPostgresPool(config.databaseUrl);
  await retry('database migrations', () => runSqlMigrations(pool, join(process.cwd(), 'migrations')));

  const eventPublisher = await retry('NATS event publisher', () => createNatsEventPublisher(config));
  const runtimeClient = new NangoSdkClient({
    secretKey: config.connectorRuntimeSecret,
    baseUrl: config.connectorRuntimeBaseUrl,
    publicBaseUrl: config.connectorRuntimePublicBaseUrl
  });
  const connectionRepository = new PgConnectionMappingRepository(pool);
  const introIngestService = new ConnectionIntroIngestService(config, runtimeClient);
  const connectSessionService = new ConnectSessionService(config, runtimeClient);
  const nangoWebhookService = new NangoWebhookService(
    config,
    connectionRepository,
    eventPublisher,
    introIngestService
  );
  const zammadWebhookService = new ZammadWebhookService(config, eventPublisher);
  const novuWebhookService = new NovuWebhookService(config, eventPublisher);
  const nangoMgmt = new NangoManagementService(config);
  const novuMgmt = new NovuManagementService(config);
  const intelligence = new SupportIntelligenceService(config);
  const authClient = new HttpAuthClient(config.authCoreUrl, config.authCoreInternalApiKey);
  const orgClient = new HttpOrgClient(config.orgCoreUrl, config.authCoreInternalApiKey);
  const billingClient = new NatsBillingClient(eventPublisher);
  const app = await createApp({
    config,
    authClient,
    orgClient,
    billingClient,
    connectSessionService,
    connectionRepository,
    eventPublisher,
    nangoWebhookService,
    zammadWebhookService,
    novuWebhookService,
    nangoMgmt,
    novuMgmt,
    intelligence,
    nango: runtimeClient
  });

  const close = async (): Promise<void> => {
    await app.close();
    await eventPublisher.close();
    await pool.end();
  };

  process.on('SIGINT', () => {
    void close().finally(() => process.exit(0));
  });

  process.on('SIGTERM', () => {
    void close().finally(() => process.exit(0));
  });

  await app.listen({
    host: '0.0.0.0',
    port: config.port
  });
}

main().catch((error) => {
  console.error('Failed to start integration-core', error);
  process.exit(1);
});
