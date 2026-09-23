import { createHash, timingSafeEqual } from 'crypto';
import {
  closeSync,
  constants as fsConstants,
  fstatSync,
  lstatSync,
  openSync,
  readSync,
  type Stats,
} from 'node:fs';
import { isAbsolute, normalize } from 'node:path';

type ServicePrincipalConfig = {
  credential: string;
  audiences: readonly string[];
  orgIds: readonly string[];
  allowAnyOrg?: boolean;
  scopes: readonly string[];
  scopesByAudience?: Readonly<Record<string, readonly string[]>>;
  retentionByAudience?: Readonly<Record<string, 'zdr' | 'persistent'>>;
};

type ServicePrincipalRegistry = Record<string, ServicePrincipalConfig>;

type ServicePrincipalEnvironment = Readonly<{
  NODE_ENV?: string;
  PLANE_SERVICE_PRINCIPALS_FILE?: string;
  PLANE_SERVICE_PRINCIPALS_JSON?: string;
}>;

const maximumRegistryBytes = 1024 * 1024;

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
  zdr: boolean;
};

export class ServicePrincipalConfigurationError extends Error {}
export class ServicePrincipalAuthorizationError extends Error {}

function privateRegistryFile(stats: Stats): boolean {
  if (!stats.isFile()) {
    return false;
  }
  if (process.platform === 'win32') {
    // Node synthesizes mode/uid from the read-only attribute on Windows, so
    // POSIX owner/permission hardening cannot be expressed or enforced there.
    // Deployment targets are Linux; on Windows the file is protected by the
    // user-profile ACL instead. The isFile/symlink/size checks above and
    // below still apply on every platform.
    return true;
  }
  const currentUserId =
    typeof process.getuid === 'function' ? process.getuid() : stats.uid;
  return (
    stats.uid === currentUserId &&
    (stats.mode & 0o400) === 0o400 &&
    (stats.mode & 0o077) === 0
  );
}

function readPrivateRegistryFile(file: string): string {
  if (
    !file ||
    file !== file.trim() ||
    !isAbsolute(file) ||
    normalize(file) !== file
  ) {
    throw new ServicePrincipalConfigurationError(
      'PLANE_SERVICE_PRINCIPALS_FILE must be a normalized absolute path',
    );
  }

  let descriptor: number | undefined;
  try {
    const pathStats = lstatSync(file);
    if (pathStats.isSymbolicLink() || !privateRegistryFile(pathStats)) {
      throw new ServicePrincipalConfigurationError(
        'PLANE_SERVICE_PRINCIPALS_FILE must be a private regular file',
      );
    }
    if (pathStats.size > maximumRegistryBytes) {
      throw new ServicePrincipalConfigurationError(
        'PLANE_SERVICE_PRINCIPALS_FILE is too large',
      );
    }

    descriptor = openSync(file, fsConstants.O_RDONLY | fsConstants.O_NOFOLLOW);
    const openedStats = fstatSync(descriptor);
    if (
      openedStats.dev !== pathStats.dev ||
      openedStats.ino !== pathStats.ino ||
      !privateRegistryFile(openedStats)
    ) {
      throw new ServicePrincipalConfigurationError(
        'PLANE_SERVICE_PRINCIPALS_FILE changed while opening',
      );
    }

    const contents = Buffer.alloc(maximumRegistryBytes + 1);
    let offset = 0;
    while (offset < contents.length) {
      const bytesRead = readSync(
        descriptor,
        contents,
        offset,
        contents.length - offset,
        null,
      );
      if (bytesRead === 0) break;
      offset += bytesRead;
    }
    if (offset > maximumRegistryBytes) {
      throw new ServicePrincipalConfigurationError(
        'PLANE_SERVICE_PRINCIPALS_FILE is too large',
      );
    }
    return contents.subarray(0, offset).toString('utf8');
  } catch (error) {
    if (error instanceof ServicePrincipalConfigurationError) throw error;
    throw new ServicePrincipalConfigurationError(
      'PLANE_SERVICE_PRINCIPALS_FILE could not be read safely',
    );
  } finally {
    if (descriptor !== undefined) closeSync(descriptor);
  }
}

/**
 * Resolves the deployment-owned registry from exactly one source. Production
 * Compose mounts a private file; direct JSON remains available for tests and
 * the local development stack only.
 */
export function loadPlaneServicePrincipalRegistry(
  environment: ServicePrincipalEnvironment = process.env as ServicePrincipalEnvironment,
): string {
  const rawRegistry = environment.PLANE_SERVICE_PRINCIPALS_JSON ?? '';
  const registryFile = environment.PLANE_SERVICE_PRINCIPALS_FILE ?? '';
  const hasRawRegistry = rawRegistry.trim().length > 0;
  const hasRegistryFile = registryFile.trim().length > 0;
  const runtime = environment.NODE_ENV?.trim().toLowerCase() ?? '';

  if (hasRawRegistry && hasRegistryFile) {
    throw new ServicePrincipalConfigurationError(
      'PLANE_SERVICE_PRINCIPALS_JSON and PLANE_SERVICE_PRINCIPALS_FILE are both configured',
    );
  }
  if (hasRegistryFile) {
    const fileRegistry = readPrivateRegistryFile(registryFile);
    if (runtime !== 'development' && runtime !== 'test') {
      const parsed = parseRegistry(fileRegistry);
      if (
        Object.values(parsed).some(
          (principal) => principal.allowAnyOrg === true,
        )
      ) {
        throw new ServicePrincipalConfigurationError(
          'Production service principals require fixed organization allowlists',
        );
      }
    }
    return fileRegistry;
  }
  if (hasRawRegistry && runtime !== 'development' && runtime !== 'test') {
    throw new ServicePrincipalConfigurationError(
      'PLANE_SERVICE_PRINCIPALS_FILE is required outside development',
    );
  }
  return rawRegistry;
}

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

function validAudienceRetentionMap(
  config: Partial<ServicePrincipalConfig>,
): boolean {
  if (config.retentionByAudience === undefined) {
    return true;
  }
  if (
    !config.retentionByAudience ||
    typeof config.retentionByAudience !== 'object' ||
    Array.isArray(config.retentionByAudience) ||
    !nonEmptyStrings(config.audiences)
  ) {
    return false;
  }

  const entries = Object.entries(config.retentionByAudience);
  const audiences = new Set(config.audiences);
  return (
    entries.length === audiences.size &&
    entries.every(
      ([audience, posture]) =>
        audiences.has(audience) &&
        (posture === 'zdr' || posture === 'persistent'),
    ) &&
    [...audiences].every((audience) => audience in config.retentionByAudience!)
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
      Object.prototype.hasOwnProperty.call(config, 'allowPersistentData') ||
      !Array.isArray(config.orgIds) ||
      !nonEmptyStrings(config.scopes) ||
      !validAudienceScopeMap(config) ||
      !validAudienceRetentionMap(config)
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
  const allowedScopes = principal?.scopesByAudience
    ? principal.scopesByAudience[request.audience]
    : principal?.scopes;
  const retentionPosture =
    principal?.retentionByAudience?.[request.audience] ?? 'zdr';

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
    zdr: retentionPosture === 'zdr',
  };
}
