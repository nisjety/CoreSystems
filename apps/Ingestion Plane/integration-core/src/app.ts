import Fastify, { FastifyInstance } from 'fastify';
import { ZodError } from 'zod';

import { AppConfig } from './common/config/app-config';
import { AuthClient } from './common/auth/auth-client';
import { registerAuthDecorators } from './common/auth/auth-middleware';
import { OrgClient } from './common/auth/org-client';
import { BillingClient } from './common/billing/billing-client';
import { errorResponse, HttpError } from './common/http/http-error';
import { IntegrationEventPublisher } from './common/nats/event-publisher';
import { ConnectSessionService } from './modules/connect-sessions/service';
import { ConnectionMappingRepository } from './modules/connections/connection-mapping-repository';
import { registerConnectionRoutes } from './modules/connections/http';
import { registerConnectSessionRoutes } from './modules/connect-sessions/http';
import { registerHealthRoutes } from './modules/health/http';
import { registerProviderRoutes } from './modules/providers/http';
import { NangoWebhookService } from './modules/webhooks/nango-webhook-service';
import { ZammadWebhookService } from './modules/webhooks/zammad-webhook-service';
import { NovuWebhookService } from './modules/webhooks/novu-webhook-service';
import { registerWebhookRoutes } from './modules/webhooks/http';
import { NangoSdkClient } from './common/runtime/nango-runtime-client';
import { registerConnectorRoutes } from './modules/connectors/http';
import { NangoManagementService } from './modules/connectors/nango-management-service';
import { NovuManagementService } from './modules/connectors/novu-management-service';
import { SupportIntelligenceService } from './modules/connectors/support-intelligence-service';

export interface IntegrationCoreDependencies {
  config: AppConfig;
  authClient: AuthClient;
  orgClient: OrgClient;
  billingClient: BillingClient;
  connectSessionService: ConnectSessionService;
  connectionRepository: ConnectionMappingRepository;
  eventPublisher: IntegrationEventPublisher;
  nangoWebhookService: NangoWebhookService;
  zammadWebhookService: ZammadWebhookService;
  novuWebhookService: NovuWebhookService;
  nangoMgmt: NangoManagementService;
  novuMgmt: NovuManagementService;
  intelligence: SupportIntelligenceService;
  nango: NangoSdkClient;
}

export async function createApp(dependencies: IntegrationCoreDependencies): Promise<FastifyInstance> {
  const app = Fastify({
    bodyLimit: 1024 * 1024,
    logger: dependencies.config.logLevel === 'silent'
      ? false
      : {
          level: dependencies.config.logLevel
        }
  });

  app.addContentTypeParser(
    /^application\/(?:[\w.+-]+\+)?json(?:;.*)?$/,
    { parseAs: 'string' },
    (request, body, done) => {
      const rawRequest = request as { rawBody?: string };
      const rawBody = body.toString();
      rawRequest.rawBody = rawBody;

      try {
        done(null, JSON.parse(rawBody));
      } catch (error) {
        done(error as Error, undefined);
      }
    }
  );

  app.setErrorHandler((error, request, reply) => {
    if (error instanceof ZodError) {
      const validationError = new HttpError(400, 'validation_error', 'Request validation failed', {
        issues: error.issues.map((issue) => ({
          message: issue.message,
          path: issue.path.join('.')
        }))
      });
      reply.status(validationError.statusCode).send(errorResponse(validationError));
      return;
    }

    if (error instanceof HttpError) {
      reply.status(error.statusCode).send(errorResponse(error));
      return;
    }

    request.log.error({ err: error }, 'Unhandled integration-core error');
    const internalError = new HttpError(500, 'internal_error', 'Internal server error');
    reply.status(internalError.statusCode).send(errorResponse(internalError));
  });

  // Register auth decorators before any routes that reference them.
  registerAuthDecorators(app, dependencies.authClient, dependencies.config.authCoreInternalApiKey);

  registerHealthRoutes(app, dependencies.config);
  registerProviderRoutes(app, dependencies.config, dependencies.nangoMgmt);
  registerConnectSessionRoutes(app, dependencies.connectSessionService, dependencies.orgClient, dependencies.billingClient);
  registerConnectionRoutes(app, dependencies.connectionRepository, dependencies.eventPublisher, dependencies.billingClient);
  // Webhook intake is authenticated per-service (Nango HMAC, Zammad bearer token, Novu HMAC-SHA256).
  registerWebhookRoutes(
    app,
    dependencies.nangoWebhookService,
    dependencies.zammadWebhookService,
    dependencies.novuWebhookService
  );
  registerConnectorRoutes(
    app,
    dependencies.nango,
    dependencies.connectionRepository,
    dependencies.nangoMgmt,
    dependencies.novuMgmt,
    dependencies.intelligence
  );

  return app;
}
