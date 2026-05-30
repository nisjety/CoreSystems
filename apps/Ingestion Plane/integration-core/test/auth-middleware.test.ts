/**
 * Unit tests for the auth middleware and auth-client.
 *
 * Uses Fastify's inject() so no real network calls are made.
 * The auth-client is replaced with a vi.fn() mock throughout.
 */
import Fastify from 'fastify';
import { ZodError } from 'zod';
import { describe, expect, it, vi } from 'vitest';

import { AuthClient, AuthPrincipal } from '../src/common/auth/auth-client';
import { assertOrgAccess, registerAuthDecorators } from '../src/common/auth/auth-middleware';
import { errorResponse, HttpError } from '../src/common/http/http-error';

// ─── helpers ─────────────────────────────────────────────────────────────────

const INTERNAL_KEY = 'test-internal-secret';

const validPrincipal: AuthPrincipal = {
  userId: 'user-1',
  organizationId: 'org-1',
  workspaceId: 'ws-1',
  role: 'member',
  email: 'user@example.com'
};

function makeAuthClient(result: AuthPrincipal | Error): AuthClient {
  return {
    verifyToken: vi.fn().mockImplementation(() =>
      result instanceof Error ? Promise.reject(result) : Promise.resolve(result)
    )
  };
}

async function buildApp(authClient: AuthClient) {
  const app = Fastify({ logger: false });

  // Match the production error handler so our HttpError responses serialize correctly.
  app.setErrorHandler((error, request, reply) => {
    if (error instanceof ZodError) {
      reply.status(400).send(errorResponse(new HttpError(400, 'validation_error', 'Request validation failed')));
      return;
    }
    if (error instanceof HttpError) {
      reply.status(error.statusCode).send(errorResponse(error));
      return;
    }
    void request;
    reply.status(500).send(errorResponse(new HttpError(500, 'internal_error', 'Internal server error')));
  });

  registerAuthDecorators(app, authClient, INTERNAL_KEY);

  // Guarded route (bearer + internal)
  app.get('/protected', { preHandler: [app.requireInternalOrBearerAuth] }, (request) => {
    return { principal: request.principal, isInternal: request.isInternalCall };
  });

  // Bearer-only route
  app.get('/bearer-only', { preHandler: [app.requireAuth] }, (request) => {
    return { principal: request.principal };
  });

  // Public route (no preHandler)
  app.get('/public', () => ({ ok: true }));

  await app.ready();
  return app;
}

// ─── tests ───────────────────────────────────────────────────────────────────

describe('requireInternalOrBearerAuth', () => {
  it('returns 401 when no credential is present', async () => {
    const app = await buildApp(makeAuthClient(validPrincipal));
    const res = await app.inject({ method: 'GET', url: '/protected' });

    expect(res.statusCode).toBe(401);
    const body = res.json();
    expect(body.error.code).toBe('unauthorized');
  });

  it('returns 401 for a malformed Bearer scheme', async () => {
    const app = await buildApp(makeAuthClient(validPrincipal));
    const res = await app.inject({
      method: 'GET',
      url: '/protected',
      headers: { authorization: 'Basic dXNlcjpwYXNz' }
    });

    expect(res.statusCode).toBe(401);
  });

  it('returns 401 when auth-core rejects the Bearer token', async () => {
    const { HttpError } = await import('../src/common/http/http-error');
    const app = await buildApp(makeAuthClient(new HttpError(401, 'unauthorized', 'Token expired')));

    const res = await app.inject({
      method: 'GET',
      url: '/protected',
      headers: { authorization: 'Bearer expired-token' }
    });

    expect(res.statusCode).toBe(401);
  });

  it('resolves the principal on a valid Bearer token', async () => {
    const authClient = makeAuthClient(validPrincipal);
    const app = await buildApp(authClient);

    const res = await app.inject({
      method: 'GET',
      url: '/protected',
      headers: { authorization: 'Bearer valid-token' }
    });

    expect(res.statusCode).toBe(200);
    const body = res.json<{ principal: AuthPrincipal; isInternal: boolean }>();
    expect(body.principal.userId).toBe('user-1');
    expect(body.principal.organizationId).toBe('org-1');
    expect(body.isInternal).toBe(false);
    expect(authClient.verifyToken).toHaveBeenCalledWith('valid-token');
  });

  it('accepts a valid internal API key', async () => {
    const authClient = makeAuthClient(validPrincipal); // should NOT be called for internal key
    const app = await buildApp(authClient);

    const res = await app.inject({
      method: 'GET',
      url: '/protected',
      headers: { 'x-internal-api-key': INTERNAL_KEY }
    });

    expect(res.statusCode).toBe(200);
    const body = res.json<{ isInternal: boolean }>();
    expect(body.isInternal).toBe(true);
    // Auth client is not consulted for internal key calls
    expect(authClient.verifyToken).not.toHaveBeenCalled();
  });

  it('returns 401 for an incorrect internal API key', async () => {
    const app = await buildApp(makeAuthClient(validPrincipal));

    const res = await app.inject({
      method: 'GET',
      url: '/protected',
      headers: { 'x-internal-api-key': 'wrong-key' }
    });

    expect(res.statusCode).toBe(401);
  });
});

describe('requireAuth (Bearer-only)', () => {
  it('returns 401 when called with an internal API key', async () => {
    const app = await buildApp(makeAuthClient(validPrincipal));

    const res = await app.inject({
      method: 'GET',
      url: '/bearer-only',
      headers: { 'x-internal-api-key': INTERNAL_KEY }
    });

    expect(res.statusCode).toBe(401);
  });

  it('resolves the principal on a valid Bearer token', async () => {
    const app = await buildApp(makeAuthClient(validPrincipal));

    const res = await app.inject({
      method: 'GET',
      url: '/bearer-only',
      headers: { authorization: 'Bearer valid-token' }
    });

    expect(res.statusCode).toBe(200);
  });
});

describe('public routes', () => {
  it('is accessible without credentials', async () => {
    const app = await buildApp(makeAuthClient(validPrincipal));
    const res = await app.inject({ method: 'GET', url: '/public' });

    expect(res.statusCode).toBe(200);
  });
});

describe('assertOrgAccess', () => {
  it('does not throw for a matching org', () => {
    const request = {
      isInternalCall: false,
      principal: validPrincipal
    } as unknown as Parameters<typeof assertOrgAccess>[0];

    expect(() => assertOrgAccess(request, 'org-1')).not.toThrow();
  });

  it('throws 403 for a mismatched org', () => {
    const request = {
      isInternalCall: false,
      principal: validPrincipal
    } as unknown as Parameters<typeof assertOrgAccess>[0];

    expect(() => assertOrgAccess(request, 'org-other')).toThrow('Access to this organization is not allowed');
  });

  it('does not throw for an internal call even with a different org', () => {
    const request = {
      isInternalCall: true,
      principal: validPrincipal
    } as unknown as Parameters<typeof assertOrgAccess>[0];

    expect(() => assertOrgAccess(request, 'org-other')).not.toThrow();
  });

  it('throws 401 when principal is absent on a user call', () => {
    const request = {
      isInternalCall: false,
      principal: undefined
    } as unknown as Parameters<typeof assertOrgAccess>[0];

    expect(() => assertOrgAccess(request, 'org-1')).toThrow('Request is not authenticated');
  });
});
