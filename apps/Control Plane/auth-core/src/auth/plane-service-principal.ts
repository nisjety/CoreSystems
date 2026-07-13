import { createHash, timingSafeEqual } from 'crypto';

type ServicePrincipalConfig = {
  credential: string;
  audiences: readonly string[];
  orgIds: readonly string[];
  allowAnyOrg?: boolean;
  scopes: readonly string[];
};

type ServicePrincipalRegistry = Record<string, ServicePrincipalConfig>;

export type PlaneServicePrincipalRequest = {
  serviceId: string;
  credential: string;
  audience: string;
  orgId: string;
  requestedScopes: readonly string[];
  reason: string;
};

export type AuthorizedPlaneServicePrincipal = {
  serviceId: string;
  subject: string;
  orgId: string;
  scopes: readonly string[];
  reason: string;
};

export class ServicePrincipalConfigurationError extends Error {}
export class ServicePrincipalAuthorizationError extends Error {}

function nonEmptyStrings(value: unknown): value is readonly string[] {
  return (
    Array.isArray(value) &&
    value.length > 0 &&
    value.every((item) => typeof item === 'string' && item.trim().length > 0)
  );
}

function parseRegistry(raw: string): ServicePrincipalRegistry {
  if (!raw.trim()) {
    throw new ServicePrincipalConfigurationError(
      'PLANE_SERVICE_PRINCIPALS_JSON is not configured',
    );
  }

  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    throw new ServicePrincipalConfigurationError(
      'PLANE_SERVICE_PRINCIPALS_JSON is malformed',
    );
  }

  if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) {
    throw new ServicePrincipalConfigurationError(
      'PLANE_SERVICE_PRINCIPALS_JSON must be an object',
    );
  }

  for (const [serviceId, candidate] of Object.entries(parsed)) {
    if (
      !/^[a-z0-9][a-z0-9._-]{1,127}$/i.test(serviceId) ||
      !candidate ||
      typeof candidate !== 'object' ||
      Array.isArray(candidate)
    ) {
      throw new ServicePrincipalConfigurationError(
        'PLANE_SERVICE_PRINCIPALS_JSON contains an invalid principal',
      );
    }
    const config = candidate as Partial<ServicePrincipalConfig>;
    if (
      typeof config.credential !== 'string' ||
      config.credential.length < 16 ||
      !nonEmptyStrings(config.audiences) ||
      (config.allowAnyOrg !== true && !nonEmptyStrings(config.orgIds)) ||
      (config.allowAnyOrg !== undefined &&
        typeof config.allowAnyOrg !== 'boolean') ||
      !Array.isArray(config.orgIds) ||
      !nonEmptyStrings(config.scopes)
    ) {
      throw new ServicePrincipalConfigurationError(
        'PLANE_SERVICE_PRINCIPALS_JSON contains an invalid principal policy',
      );
    }
  }

  return parsed as ServicePrincipalRegistry;
}

function secureEqual(expected: string, received: string): boolean {
  const expectedDigest = createHash('sha256').update(expected).digest();
  const receivedDigest = createHash('sha256').update(received).digest();
  return timingSafeEqual(expectedDigest, receivedDigest);
}

function normalizedUnique(values: readonly string[]): string[] {
  return [...new Set(values.map((value) => value.trim()).filter(Boolean))];
}

/**
 * Resolves a service identity only from a deployment-owned allowlist. Caller
 * input may narrow tenant/scope, but can never widen the configured bounds.
 */
export function authorizePlaneServicePrincipal(
  rawRegistry: string,
  request: PlaneServicePrincipalRequest,
): AuthorizedPlaneServicePrincipal {
  const registry = parseRegistry(rawRegistry);
  const serviceId = request.serviceId.trim();
  const orgId = request.orgId.trim();
  const reason = request.reason.trim();
  const requestedScopes = normalizedUnique(request.requestedScopes);
  const principal = registry[serviceId];

  if (
    !principal ||
    !request.credential ||
    !secureEqual(principal.credential, request.credential) ||
    !principal.audiences.includes(request.audience) ||
    !orgId ||
    (principal.allowAnyOrg !== true && !principal.orgIds.includes(orgId)) ||
    requestedScopes.length === 0 ||
    requestedScopes.some((scope) => !principal.scopes.includes(scope)) ||
    reason.length < 3 ||
    reason.length > 500
  ) {
    throw new ServicePrincipalAuthorizationError(
      'Service principal request is not authorized',
    );
  }

  return {
    serviceId,
    subject: `service:${serviceId}`,
    orgId,
    scopes: requestedScopes,
    reason,
  };
}
