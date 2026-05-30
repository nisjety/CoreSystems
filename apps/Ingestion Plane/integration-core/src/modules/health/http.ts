import { FastifyInstance } from 'fastify';

import { AppConfig } from '../../common/config/app-config';
import { successResponse } from '../../common/http/http-error';

export function buildHealthResponse(config: AppConfig): {
  success: true;
  data: {
    service: string;
    status: 'ok';
    timestamp: string;
  };
} {
  return successResponse({
    service: config.serviceName,
    status: 'ok' as const,
    timestamp: new Date().toISOString()
  });
}

export function registerHealthRoutes(app: FastifyInstance, config: AppConfig): void {
  app.get('/health', async () => {
    return buildHealthResponse(config);
  });

  // G39 (velion-gap.md §10): cross-service handshake endpoint for the
  // velion startup probe. Requires `x-internal-api-key` or a Bearer JWT.
  // Returns the resolved principal so the caller can confirm the key matches
  // *this* service's view of the cluster secret. Distinct from `/health`
  // because `/health` is unauthenticated and only proves the listener is up.
  app.get('/api/v1/internal/whoami', {
    preHandler: app.requireInternalOrBearerAuth
  }, async (request) => {
    const principal = request.principal;
    return successResponse({
      service: config.serviceName,
      authMode: request.isInternalCall ? 'internal_api_key' : 'bearer_jwt',
      userId: principal?.userId ?? '',
      organizationId: principal?.organizationId ?? '',
      role: principal?.role ?? '',
      timestamp: new Date().toISOString()
    });
  });
}