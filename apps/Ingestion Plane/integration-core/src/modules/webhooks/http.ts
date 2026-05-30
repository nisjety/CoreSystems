import { FastifyInstance, FastifyRequest } from 'fastify';

import { successResponse } from '../../common/http/http-error';
import { NangoWebhookService } from './nango-webhook-service';
import { ZammadWebhookService } from './zammad-webhook-service';
import { NovuWebhookService } from './novu-webhook-service';

type RawBodyRequest = FastifyRequest & {
  rawBody?: string;
};

function rawBody(request: FastifyRequest): string {
  const raw = (request as RawBodyRequest).rawBody;
  return typeof raw === 'string' ? raw : JSON.stringify(request.body ?? {});
}

export function registerWebhookRoutes(
  app: FastifyInstance,
  nangoService: NangoWebhookService,
  zammadService: ZammadWebhookService,
  novuService: NovuWebhookService
): void {
  app.post('/api/v1/webhooks/nango', async (request) => {
    const result = await nangoService.handle(
      rawBody(request),
      request.headers as Record<string, string | string[] | undefined>
    );
    return successResponse(result);
  });

  app.post('/api/v1/webhooks/zammad', async (request) => {
    const result = await zammadService.handle(
      rawBody(request),
      request.headers as Record<string, string | string[] | undefined>
    );
    return successResponse(result);
  });

  app.post('/api/v1/webhooks/novu', async (request) => {
    const result = await novuService.handle(
      rawBody(request),
      request.headers as Record<string, string | string[] | undefined>
    );
    return successResponse(result);
  });
}