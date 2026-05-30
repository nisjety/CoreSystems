import { FastifyInstance, FastifyRequest } from 'fastify';
import { z } from 'zod';
import { HttpError, successResponse } from '../../common/http/http-error';
import { NangoSdkClient } from '../../common/runtime/nango-runtime-client';
import { ConnectionMappingRepository } from '../connections/connection-mapping-repository';
import { NangoManagementService } from './nango-management-service';
import { NovuManagementService } from './novu-management-service';
import { SupportIntelligenceService } from './support-intelligence-service';

// Request/response schema validation
const tokenBodySchema = z.object({
  organizationId: z.string().trim().min(1),
  connectorType: z.enum(['microsoft-graph'])
});

const analyzeTicketSchema = z.object({
  ticketId: z.string().min(1),
  title: z.string().min(1),
  description: z.string().optional(),
  createdBy: z.string().optional(),
  status: z.string().optional()
});

const recommendConnectorsSchema = z.object({
  ticketId: z.string().optional(),
  workflowContext: z.record(z.string(), z.unknown()).optional(),
  existingConnectors: z.array(z.string()).optional()
});

const sendNotificationSchema = z.object({
  subscriberId: z.string().min(1),
  templateId: z.string().min(1),
  payload: z.record(z.string(), z.unknown()),
  overrides: z.record(z.string(), z.unknown()).optional()
});

export function registerConnectorRoutes(
  app: FastifyInstance,
  nango: NangoSdkClient,
  connectionRepository: ConnectionMappingRepository,
  nangoMgmt: NangoManagementService,
  novuMgmt: NovuManagementService,
  intelligence: SupportIntelligenceService
): void {
  app.post(
    '/internal/connectors/token',
    { preHandler: [app.requireInternalOrBearerAuth] },
    async (request: FastifyRequest) => {
      const body = tokenBodySchema.parse(request.body);
      const connections = await connectionRepository.list({
        organizationId: body.organizationId,
        providerKey: body.connectorType
      });
      const connection = connections.find(c => !c.deletedAt);
      if (!connection) {
        throw new HttpError(404, 'connection_not_found',
          `No active ${body.connectorType} connection for organization ${body.organizationId}`);
      }
      const accessToken = await nango.getConnectionToken(
        connection.nangoIntegrationId,
        connection.nangoConnectionId
      );
      return successResponse({ accessToken });
    }
  );

  // ─────────────────────────────────────────────────────────────────
  // Nango Integration Management
  // ─────────────────────────────────────────────────────────────────

  app.get(
    '/api/v1/integrations/nango/list',
    { preHandler: [app.requireInternalOrBearerAuth] },
    async () => {
      const integrations = await nangoMgmt.listIntegrations();
      return successResponse({ integrations });
    }
  );

  app.get(
    '/api/v1/integrations/nango/:key',
    { preHandler: [app.requireInternalOrBearerAuth] },
    async (request: FastifyRequest) => {
      const params = z.object({ key: z.string().min(1) }).parse(request.params);
      const integration = await nangoMgmt.getIntegration(params.key);
      if (!integration) {
        throw new HttpError(404, 'integration_not_found', `Integration ${params.key} not found`);
      }
      return successResponse({ integration });
    }
  );

  app.get(
    '/api/v1/connections/nango/:connectionId/status',
    { preHandler: [app.requireInternalOrBearerAuth] },
    async (request: FastifyRequest) => {
      const params = z.object({ connectionId: z.string().min(1) }).parse(request.params);
      const status = await nangoMgmt.checkConnectionStatus(params.connectionId);
      return successResponse({ status });
    }
  );

  // ─────────────────────────────────────────────────────────────────
  // Novu Notification Management
  // ─────────────────────────────────────────────────────────────────

  app.get(
    '/api/v1/notifications/novu/templates',
    { preHandler: [app.requireAuth] },
    async () => {
      const templates = await novuMgmt.listTemplates();
      return successResponse({ templates });
    }
  );

  app.get(
    '/api/v1/notifications/novu/templates/:templateId',
    { preHandler: [app.requireAuth] },
    async (request: FastifyRequest) => {
      const params = z.object({ templateId: z.string().min(1) }).parse(request.params);
      const template = await novuMgmt.getTemplate(params.templateId);
      if (!template) {
        throw new HttpError(404, 'template_not_found', `Template ${params.templateId} not found`);
      }
      return successResponse({ template });
    }
  );

  app.post(
    '/api/v1/notifications/novu/send',
    { preHandler: [app.requireAuth] },
    async (request: FastifyRequest) => {
      const body = sendNotificationSchema.parse(request.body);
      const result = await novuMgmt.sendNotification(body);
      return successResponse(result);
    }
  );

  app.get(
    '/api/v1/notifications/novu/subscribers/:subscriberId/preferences',
    { preHandler: [app.requireAuth] },
    async (request: FastifyRequest) => {
      const params = z.object({ subscriberId: z.string().min(1) }).parse(request.params);
      const preferences = await novuMgmt.getSubscriberPreferences(params.subscriberId);
      return successResponse({ preferences });
    }
  );

  app.put(
    '/api/v1/notifications/novu/subscribers/:subscriberId/preferences',
    { preHandler: [app.requireAuth] },
    async (request: FastifyRequest) => {
      const params = z.object({ subscriberId: z.string().min(1) }).parse(request.params);
      const preferences = await novuMgmt.updateSubscriberPreferences(
        params.subscriberId,
        z.record(z.string(), z.unknown()).parse(request.body)
      );
      return successResponse({ preferences });
    }
  );

  // ─────────────────────────────────────────────────────────────────
  // AI-Powered Intelligence for Support Workflows
  // ─────────────────────────────────────────────────────────────────

  app.post(
    '/api/v1/intelligence/analyze-ticket',
    { preHandler: [app.requireAuth] },
    async (request: FastifyRequest) => {
      const body = analyzeTicketSchema.parse(request.body);
      const analysis = await intelligence.analyzeTicket({
        id: body.ticketId,
        title: body.title,
        description: body.description ?? '',
        createdBy: body.createdBy,
        status: body.status
      });
      return successResponse({ analysis });
    }
  );

  app.post(
    '/api/v1/intelligence/recommend-connectors',
    { preHandler: [app.requireAuth] },
    async (request: FastifyRequest) => {
      const body = recommendConnectorsSchema.parse(request.body);
      const recommendations = await intelligence.recommendConnectors(body);
      return successResponse({ recommendations });
    }
  );

  app.post(
    '/api/v1/intelligence/optimize-notification',
    { preHandler: [app.requireAuth] },
    async (request: FastifyRequest) => {
      const body = z.object({
        subscriberId: z.string().min(1),
        notificationType: z.string().min(1),
        urgency: z.enum(['critical', 'high', 'normal', 'low']),
        context: z.record(z.string(), z.unknown()).optional()
      }).parse(request.body);
      const routing = await intelligence.optimizeNotificationChannels(body);
      return successResponse({ routing });
    }
  );
}
