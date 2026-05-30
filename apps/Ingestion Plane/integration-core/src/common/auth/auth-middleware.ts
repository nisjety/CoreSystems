/**
 * Fastify request authentication middleware.
 *
 * Supports two modes, tried in order:
 *
 * 1. Bearer JWT mode – `Authorization: Bearer <token>`
 *    Delegates to auth-core for session verification.
 *    Resolves to a full AuthPrincipal with org/workspace/role.
 *
 * 2. Internal API-key mode – `x-internal-api-key: <key>`
 *    Validated locally against AUTH_CORE_INTERNAL_API_KEY.
 *    The caller must supply organizationId + workspaceId + userId in the
 *    request body; these are promoted into a synthetic principal.
 *    Used for service-to-service calls from trusted internal planes.
 *
 * Routes that do NOT call `requireAuth()` or `requireInternalOrBearerAuth()`
 * remain publicly accessible (health, provider catalog, Nango webhook intake).
 */
import {
  FastifyInstance,
  FastifyReply,
  FastifyRequest
} from 'fastify';

import { AuthClient, AuthPrincipal } from './auth-client';
import { HttpError } from '../http/http-error';

// ─── Module augmentation ──────────────────────────────────────────────────────

declare module 'fastify' {
  interface FastifyRequest {
    /**
     * Present on routes protected by `requireAuth` or
     * `requireInternalOrBearerAuth`.  Absent on public routes.
     */
    principal?: AuthPrincipal;
    /**
     * True when the request was authenticated via the internal API key rather
     * than a user Bearer token.  Internal callers bypass organization scoping
     * on list queries, subject to additional trust context.
     */
    isInternalCall?: boolean;
  }
}

// ─── registerAuthDecorators ───────────────────────────────────────────────────

export function registerAuthDecorators(
  app: FastifyInstance,
  authClient: AuthClient,
  internalApiKey: string
): void {
  app.decorateRequest('principal', undefined);
  app.decorateRequest('isInternalCall', false);

  /**
   * Extracts and validates the caller's identity from the request.
   * Resolves `request.principal`; sets `request.isInternalCall` for
   * x-internal-api-key authenticated requests.
   *
   * Throws HttpError(401) if no valid credential is present.
   * Throws HttpError(503) if auth-core is unreachable during Bearer validation.
   */
  async function resolveIdentity(request: FastifyRequest): Promise<void> {
    const authHeader = request.headers.authorization;

    // ── Mode 1: Bearer JWT ────────────────────────────────────────────────────
    if (authHeader) {
      if (!authHeader.startsWith('Bearer ')) {
        throw new HttpError(401, 'unauthorized', 'Authorization header must use Bearer scheme');
      }

      const token = authHeader.slice('Bearer '.length).trim();

      if (!token) {
        throw new HttpError(401, 'unauthorized', 'Bearer token is empty');
      }

      request.principal = await authClient.verifyToken(token);
      request.isInternalCall = false;
      return;
    }

    // ── Mode 2: Internal API key ──────────────────────────────────────────────
    const providedKey = request.headers['x-internal-api-key'];

    if (providedKey) {
      if (providedKey !== internalApiKey) {
        throw new HttpError(401, 'unauthorized', 'Invalid internal API key');
      }

      // Internal callers embed org/user context in the request body.
      // We build a synthetic principal from those fields; the caller is
      // responsible for supplying correct context values.
      const body = request.body as Record<string, unknown> | undefined;
      const organizationId = typeof body?.organizationId === 'string' ? body.organizationId : '';
      const workspaceId = typeof body?.workspaceId === 'string' ? body.workspaceId : '';
      const userId = typeof body?.userId === 'string' ? body.userId : 'internal-service';

      request.principal = {
        userId,
        organizationId,
        workspaceId,
        role: 'service',
        email: ''
      };
      request.isInternalCall = true;
      return;
    }

    throw new HttpError(401, 'unauthorized', 'Authentication required. Provide an Authorization header or x-internal-api-key header.');
  }

  /**
   * preHandler that enforces authentication via Bearer token ONLY.
   * Use for user-facing routes where internal keys must not be accepted.
   */
  app.decorate(
    'requireAuth',
    async function requireAuth(request: FastifyRequest, reply: FastifyReply): Promise<void> {
      const authHeader = request.headers.authorization;

      if (!authHeader || !authHeader.startsWith('Bearer ')) {
        throw new HttpError(401, 'unauthorized', 'Authentication required');
      }

      const token = authHeader.slice('Bearer '.length).trim();

      if (!token) {
        throw new HttpError(401, 'unauthorized', 'Bearer token is empty');
      }

      request.principal = await authClient.verifyToken(token);
      request.isInternalCall = false;

      void reply; // satisfies Fastify's preHandler signature
    }
  );

  /**
   * preHandler that accepts either a Bearer token or the internal API key.
   * Use for routes callable both by users and trusted internal services.
   */
  app.decorate(
    'requireInternalOrBearerAuth',
    async function requireInternalOrBearerAuth(request: FastifyRequest, reply: FastifyReply): Promise<void> {
      await resolveIdentity(request);
      void reply;
    }
  );
}

// ─── TypeScript declarations for decorators ──────────────────────────────────

declare module 'fastify' {
  interface FastifyInstance {
    requireAuth: (request: FastifyRequest, reply: FastifyReply) => Promise<void>;
    requireInternalOrBearerAuth: (request: FastifyRequest, reply: FastifyReply) => Promise<void>;
  }
}

// ─── Organization scoping helper ─────────────────────────────────────────────

/**
 * Asserts that the principal has access to the given organizationId.
 * Internal service calls (isInternalCall === true) bypass this check.
 */
export function assertOrgAccess(request: FastifyRequest, targetOrgId: string): void {
  if (request.isInternalCall) return;

  const principal = request.principal;

  if (!principal) {
    throw new HttpError(401, 'unauthorized', 'Request is not authenticated');
  }

  if (principal.organizationId !== targetOrgId) {
    throw new HttpError(403, 'forbidden', 'Access to this organization is not allowed');
  }
}
