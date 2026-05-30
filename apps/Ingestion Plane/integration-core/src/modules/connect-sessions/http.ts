import { FastifyInstance, FastifyReply, FastifyRequest } from 'fastify';
import { z } from 'zod';

import { assertOrgAccess } from '../../common/auth/auth-middleware';
import { requirePlan } from '../../common/auth/entitlement-guard';
import { OrgClient } from '../../common/auth/org-client';
import { BillingClient } from '../../common/billing/billing-client';
import { HttpError, successResponse } from '../../common/http/http-error';
import { ConnectSessionService } from './service';

const paramsSchema = z.object({
  provider: z.string().trim().min(1)
});

const bodySchema = z.object({
  organizationId: z.string().trim().min(1),
  selectedSources: z.array(z.string().trim().min(1)).optional(),
  workspaceId: z.string().trim().min(1),
  userId: z.string().trim().min(1),
  userEmail: z.string().trim().email()
});

export function registerConnectSessionRoutes(
  app: FastifyInstance,
  service: ConnectSessionService,
  orgClient: OrgClient,
  billingClient: BillingClient
): void {
  /**
   * Protected: requires a valid user Bearer token or internal API key.
   *
   * The caller must be acting on behalf of their own organization.
   * An internal service call (x-internal-api-key) bypasses org-scope enforcement
   * and may supply any organizationId in the body.
   */
  const handler = async (
    request: FastifyRequest,
    reply: FastifyReply
  ) => {
    const params = paramsSchema.parse(request.params);
    const rawBody = bodySchema.parse(request.body);

    // Enforce organization ownership for user Bearer calls.
    // Internal callers (isInternalCall) are trusted to supply any org.
    assertOrgAccess(request, rawBody.organizationId);

    // Derive org/user context: for Bearer calls use the verified principal
    // to prevent a caller from creating sessions for other users.
    const principal = request.principal;

    if (!request.isInternalCall && !principal) {
      throw new HttpError(401, 'unauthorized', 'Authentication required');
    }

    const organizationId = rawBody.organizationId;
    const workspaceId = rawBody.workspaceId || (principal?.workspaceId ?? '');
    const userId = request.isInternalCall ? rawBody.userId : (principal?.userId ?? rawBody.userId);
    const userEmail = request.isInternalCall ? rawBody.userEmail : (principal?.email || rawBody.userEmail);

    const session = await service.createSession({
      providerKey: params.provider,
      organizationId,
      selectedSources: rawBody.selectedSources ?? [],
      workspaceId,
      userId,
      userEmail
    });

    // Record usage for billing (fire-and-forget).
    billingClient.recordUsage(organizationId, 'connect_session_created', 1, {
      providerKey: params.provider
    });

    reply.code(201);
    return successResponse(session);
  };

  const authOptions = {
    preHandler: [app.requireInternalOrBearerAuth, requirePlan(orgClient, 'pro')]
  };

  app.post('/api/v1/providers/:provider/connect-session', authOptions, handler);
  app.post('/api/v1/providers/:provider/connect', authOptions, handler);
}
