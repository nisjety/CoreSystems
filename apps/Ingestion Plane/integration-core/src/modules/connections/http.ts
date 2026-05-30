import { FastifyInstance, FastifyRequest } from 'fastify';
import { z } from 'zod';

import { assertOrgAccess } from '../../common/auth/auth-middleware';
import { BillingClient } from '../../common/billing/billing-client';
import { HttpError, successResponse } from '../../common/http/http-error';
import { IntegrationEventPublisher } from '../../common/nats/event-publisher';
import { integrationSubjects } from '../../common/nats/subjects';
import { ConnectionMappingRepository } from './connection-mapping-repository';

const listQuerySchema = z.object({
  organizationId: z.string().trim().min(1).optional(),
  workspaceId: z.string().trim().min(1).optional(),
  userId: z.string().trim().min(1).optional(),
  providerKey: z.string().trim().min(1).optional()
});

const idParamsSchema = z.object({
  id: z.string().trim().min(1)
});

export function registerConnectionRoutes(
  app: FastifyInstance,
  repository: ConnectionMappingRepository,
  eventPublisher: IntegrationEventPublisher,
  billingClient: BillingClient
): void {
  const authOptions = { preHandler: [app.requireInternalOrBearerAuth] };

  /**
   * List connections.
   *
   * - Bearer callers: automatically scoped to their organizationId from the
   *   verified principal.  Any organizationId filter in the querystring is
   *   ignored and overridden by the principal's org.
   * - Internal callers (x-internal-api-key): may supply an explicit
   *   organizationId query param; if absent, all records are returned.
   */
  app.get('/api/v1/connections', authOptions, async (request: FastifyRequest) => {
    const query = listQuerySchema.parse(request.query);

    const filters = {
      ...query,
      organizationId: request.isInternalCall
        ? query.organizationId
        : (request.principal?.organizationId ?? query.organizationId)
    };

    const connections = await repository.list(filters);
    return successResponse({ connections });
  });

  app.get('/api/v1/connections/:id', authOptions, async (request: FastifyRequest) => {
    const params = idParamsSchema.parse(request.params);
    const connection = await repository.getById(params.id);

    if (!connection || connection.deletedAt) {
      throw new HttpError(404, 'connection_not_found', `Connection ${params.id} was not found`);
    }

    assertOrgAccess(request, connection.organizationId);

    return successResponse({ connection });
  });

  app.get('/api/v1/connections/:id/status', authOptions, async (request: FastifyRequest) => {
    const params = idParamsSchema.parse(request.params);
    const connection = await repository.getById(params.id);

    if (!connection || connection.deletedAt) {
      throw new HttpError(404, 'connection_not_found', `Connection ${params.id} was not found`);
    }

    assertOrgAccess(request, connection.organizationId);

    return successResponse({
      connectionId: connection.id,
      lastSyncStatus: connection.lastSyncStatus,
      status: connection.status
    });
  });

  app.delete('/api/v1/connections/:id', authOptions, async (request: FastifyRequest, reply) => {
    const params = idParamsSchema.parse(request.params);
    const connection = await repository.getById(params.id);

    if (!connection) {
      throw new HttpError(404, 'connection_not_found', `Connection ${params.id} was not found`);
    }

    assertOrgAccess(request, connection.organizationId);

    const deleted = await repository.markDeleted(params.id);

    if (!deleted) {
      throw new HttpError(404, 'connection_not_found', `Connection ${params.id} was not found`);
    }

    try {
      await eventPublisher.publish(integrationSubjects.connectionDeleted, {
        connectionId: deleted.id,
        nangoConnectionId: deleted.nangoConnectionId,
        organizationId: deleted.organizationId,
        providerKey: deleted.providerKey,
        workspaceId: deleted.workspaceId
      });
    } catch (error) {
      request.log.error({ err: error, connectionId: deleted.id }, 'Failed to publish connection deleted event');
    }

    reply.code(204);
    return null;
  });

  app.post('/api/v1/connections/:id/sync', authOptions, async (request: FastifyRequest, reply) => {
    const params = idParamsSchema.parse(request.params);
    const connection = await repository.getById(params.id);

    if (!connection || connection.deletedAt) {
      throw new HttpError(404, 'connection_not_found', `Connection ${params.id} was not found`);
    }

    assertOrgAccess(request, connection.organizationId);

    try {
      await eventPublisher.publish(integrationSubjects.syncStarted, {
        connectionId: connection.id,
        nangoConnectionId: connection.nangoConnectionId,
        organizationId: connection.organizationId,
        providerKey: connection.providerKey,
        workspaceId: connection.workspaceId
      });
    } catch (error) {
      request.log.error({ err: error, connectionId: connection.id }, 'Failed to publish connection sync event');
    }

    // Record usage for billing (fire-and-forget).
    billingClient.recordUsage(connection.organizationId, 'connection_sync_triggered', 1, {
      connectionId: connection.id,
      providerKey: connection.providerKey
    });

    reply.code(202);
    return successResponse({
      connectionId: connection.id,
      status: 'queued'
    });
  });
}
