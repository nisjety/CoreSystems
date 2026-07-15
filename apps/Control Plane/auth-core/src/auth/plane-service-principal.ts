import { createHash, timingSafeEqual } from 'crypto';

type ServicePrincipalConfig = {
  credential: string;
  audiences: readonly string[];
  orgIds: readonly string[];
  allowAnyOrg?: boolean;
  allowPersistentData?: boolean;
  scopes: readonly string[];
  scopesByAudience?: Readonly<Record<string, readonly string[]>>;
};

type ServicePrincipalRegistry = Record<string, ServicePrincipalConfig>;

export type PlaneServicePrincipalRequest = {
  serviceId: string;
  credential: string;
  audience: string;
  orgId: string;
  requestedScopes: readonly string[];
  reason: string;
  zdr?: boolean;
};

export type AuthorizedPlaneServicePrincipal = {
  serviceId: string;
  subject: string;
  orgId: string;
  scopes: readonly string[];
  reason: string;
  zdr: boolean;
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

function validAudienceScopeMap(
  config: Partial<ServicePrincipalConfig>,
): boolean {
  if (config.scopesByAudience === undefined) {
    // Legacy single/union-scope registries remain valid until migrated.
    return true;
  }
  if (
    !config.scopesByAudience ||
    typeof config.scopesByAudience !== 'object' ||
    Array.isArray(config.scopesByAudience) ||
    !nonEmptyStrings(config.audiences) ||
    !nonEmptyStrings(config.scopes)
  ) {
    return false;
  }

  const entries = Object.entries(config.scopesByAudience);
  const audiences = new Set(config.audiences);
  if (
    entries.length !== audiences.size ||
    entries.some(
      ([audience, scopes]) =>
        !audiences.has(audience) ||
        !nonEmptyStrings(scopes) ||
        scopes.some((scope) => !config.scopes!.includes(scope)),
    ) ||
    [...audiences].some((audience) => !(audience in config.scopesByAudience!))
  ) {
    return false;
  }

  const mappedScopes = new Set(entries.flatMap(([, scopes]) => scopes));
  return (
    mappedScopes.size === new Set(config.scopes).size &&
    config.scopes.every((scope) => mappedScopes.has(scope))
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
      (config.allowPersistentData !== undefined &&
        typeof config.allowPersistentData !== 'boolean') ||
      (config.allowPersistentData === true &&
        !config.audiences?.includes('data-plane')) ||
      !Array.isArray(config.orgIds) ||
      !nonEmptyStrings(config.scopes) ||
      !validAudienceScopeMap(config)
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
  const requestedZdr = request.zdr ?? true;
  const principal = registry[serviceId];
  const allowedScopes = principal?.scopesByAudience
    ? principal.scopesByAudience[request.audience]
    : principal?.scopes;

  if (
    !principal ||
    !request.credential ||
    !secureEqual(principal.credential, request.credential) ||
    !principal.audiences.includes(request.audience) ||
    !orgId ||
    (principal.allowAnyOrg !== true && !principal.orgIds.includes(orgId)) ||
    requestedScopes.length === 0 ||
    !allowedScopes ||
    requestedScopes.some((scope) => !allowedScopes.includes(scope)) ||
    typeof requestedZdr !== 'boolean' ||
    (requestedZdr === false && principal.allowPersistentData !== true) ||
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
    zdr: requestedZdr,
  };
}
