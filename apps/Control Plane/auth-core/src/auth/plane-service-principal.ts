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

/**
 * A principal's retention posture for one audience, as the deployment-owned
 * registry states it. Exactly one of:
 *
 *   `zdr`        every token is zero-retention. The default for an audience the
 *                registry says nothing about.
 *   `persistent` every token is persistent.
 *   `tenant`     the token's `zdr` claim is the ORG's own ZDR setting, read from
 *                org-core when the token is minted (never a caller's choice),
 *                and the token lives at most 60 seconds. For background workers
 *                that act on behalf of whichever org the work belongs to.
 *
 * This module only authorizes: it performs no I/O, so it cannot read an org.
 * For `tenant` it therefore reports the safe value (`zdr: true`) and leaves the
 * resolving to the token controllers, which must call
 * `resolveTenantServiceRetentionPosture` (interactive-retention-policy.ts)
 * before signing. A registry carrying `tenant` is read only by an Auth Core
 * that has this code: an older image rejects the value and refuses every
 * principal's token, so the image is deployed before any row is switched.
 */
export type PrincipalRetentionPolicy = 'zdr' | 'persistent' | 'tenant';

/**
 * How a principal's token organization was bound, for one mint:
 *
 *   `fixed`      the organization is one of the row's own `orgIds` (including a
 *                pseudo-organization such as `global`). Nothing to look up.
 *   `directory`  the row says `tenancy: 'directory'` (any organization org-core's
 *                directory holds). This module performs no I/O, so it only
 *                reports that the organization still has to be verified; the
 *                token controllers do that, after the caller is authorized and
 *                before anything is signed (`assertServiceOrganizationBinding`
 *                in interactive-retention-policy.ts).
 *   `any`        the legacy `allowAnyOrg: true` form: nothing is checked. The
 *                production loader refuses it; the audit event records it so a
 *                development stack's unchecked mints can be counted.
 */
export type PrincipalOrgBinding = 'fixed' | 'directory' | 'any';

type ServicePrincipalConfig = {
  credential: string;
  audiences: readonly string[];
  /**
   * Organizations this principal may act for without a lookup. Always present
   * (it may be empty); for a `tenancy: 'directory'` row it adds the
   * pseudo-organizations no directory holds.
   */
  orgIds: readonly string[];
  /** Development and tests only: the production loader refuses `true`. */
  allowAnyOrg?: boolean;
  /**
   * Production-valid form for a workload that acts for whichever organization
   * the work belongs to: any organization org-core's directory holds, verified
   * by Auth Core at every mint. The only accepted value is `directory`.
   */
  tenancy?: 'directory';
  scopes: readonly string[];
  scopesByAudience?: Readonly<Record<string, readonly string[]>>;
  retentionByAudience?: Readonly<Record<string, PrincipalRetentionPolicy>>;
  /**
   * Delegated-identity token exchange (auth-core §1, T-4). Keyed by the
   * TARGET audience being exchanged into (must be a member of `audiences`);
   * each value is the set of SUBJECT-token audiences this principal may
   * present as proof of an acting user for that target. Absent by default —
   * no principal gains token-exchange capability until a deployment
   * explicitly adds this field. The endpoint additionally hard-restricts
   * which subject audiences it will ever cryptographically accept
   * (`verifyPlaneSubjectToken`'s own allow-list); this map can only narrow
   * that further per-principal, never widen it.
   */
  tokenExchange?: Readonly<Record<string, readonly string[]>>;
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
  /** The registry's posture for the requested audience (`zdr` when it names none). */
  retention: PrincipalRetentionPolicy;
  /** How the requested organization was bound; `directory` is verified by the caller of this module. */
  orgBinding: PrincipalOrgBinding;
  /**
   * The `zdr` claim the registry alone supports. For `tenant` it is `true`, the
   * safe value: it stands only if a caller signs without having resolved the
   * org's posture, and can never be a non-zero-retention token by omission.
   * A caller that sees `retention === 'tenant'` must resolve the org instead.
   */
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
        // The unchecked "any organization" form is refused outright, also on a
        // row that carries `tenancy: 'directory'` beside it: a production row
        // names its organizations (`orgIds`) or the directory, never "anyone".
        throw new ServicePrincipalConfigurationError(
          'Production service principals require fixed organization allowlists or directory tenancy, not allowAnyOrg',
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
        (posture === 'zdr' || posture === 'persistent' || posture === 'tenant'),
    ) &&
    [...audiences].every((audience) => audience in config.retentionByAudience!)
  );
}

/**
 * A row names an organization source: a fixed list, the directory, or (legacy,
 * development) any organization. The tenancy value, when present, must be the
 * one production-valid form; anything else is a typo that would otherwise be
 * ignored and leave the row bound by whatever else it says.
 */
function validOrgSource(config: Partial<ServicePrincipalConfig>): boolean {
  if (config.tenancy !== undefined && config.tenancy !== 'directory') {
    return false;
  }
  return (
    config.allowAnyOrg === true ||
    config.tenancy === 'directory' ||
    nonEmptyStrings(config.orgIds)
  );
}

/**
 * How `orgId` is bound for this principal, or undefined when the principal may
 * not act for it. A fixed entry wins over the directory, and the directory over
 * the unchecked legacy form, so a row that carries more than one is held to the
 * strictest that applies to the organization at hand.
 */
function orgBindingFor(
  principal: ServicePrincipalConfig,
  orgId: string,
): PrincipalOrgBinding | undefined {
  if (principal.orgIds.includes(orgId)) return 'fixed';
  if (principal.tenancy === 'directory') return 'directory';
  if (principal.allowAnyOrg === true) return 'any';
  return undefined;
}

function validTokenExchangeMap(config: Partial<ServicePrincipalConfig>): boolean {
  if (config.tokenExchange === undefined) {
    // Absent by default — no principal gains token-exchange capability
    // until a deployment explicitly opts it in.
    return true;
  }
  if (
    !config.tokenExchange ||
    typeof config.tokenExchange !== 'object' ||
    Array.isArray(config.tokenExchange) ||
    !nonEmptyStrings(config.audiences)
  ) {
    return false;
  }

  const audiences = new Set(config.audiences);
  const entries = Object.entries(config.tokenExchange);
  return entries.every(
    ([targetAudience, subjectAudiences]) =>
      audiences.has(targetAudience) && nonEmptyStrings(subjectAudiences),
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
      !validOrgSource(config) ||
      (config.allowAnyOrg !== undefined &&
        typeof config.allowAnyOrg !== 'boolean') ||
      Object.prototype.hasOwnProperty.call(config, 'allowPersistentData') ||
      !Array.isArray(config.orgIds) ||
      !nonEmptyStrings(config.scopes) ||
      !validAudienceScopeMap(config) ||
      !validAudienceRetentionMap(config) ||
      !validTokenExchangeMap(config)
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
  const retentionPosture: PrincipalRetentionPolicy =
    principal?.retentionByAudience?.[request.audience] ?? 'zdr';
  const orgBinding =
    principal && orgId ? orgBindingFor(principal, orgId) : undefined;

  if (
    !principal ||
    !request.credential ||
    !secureEqual(principal.credential, request.credential) ||
    !principal.audiences.includes(request.audience) ||
    !orgId ||
    !orgBinding ||
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
    retention: retentionPosture,
    orgBinding,
    // Only an explicit `persistent` is non-zero-retention here. `tenant` is
    // read as zero-retention until a controller resolves the org; the old
    // `retentionPosture === 'zdr'` would have read it as persistent.
    zdr: retentionPosture !== 'persistent',
  };
}

export type PlaneServiceTokenExchangeRequest = {
  serviceId: string;
  credential: string;
  /** TARGET audience being exchanged into, e.g. "ingestion". */
  audience: string;
  orgId: string;
  requestedScopes: readonly string[];
  reason: string;
  /**
   * The already-cryptographically-verified subject token's own `aud` claim
   * (from `ConvexTokenService.verifyPlaneSubjectToken`'s result) — never a
   * caller-supplied value.
   */
  subjectAudience: string;
};

/**
 * Result of a successful token-exchange authorization. Deliberately carries
 * NO `zdr`/retention field at all — unlike `AuthorizedPlaneServicePrincipal`,
 * whose `zdr` reflects the registry's own default. The exchanged token's
 * `zdr` must come from the verified SUBJECT token instead (T-5); omitting
 * the field here removes the temptation/possibility of a caller
 * accidentally reusing the registry default for this path.
 */
export type AuthorizedPlaneServiceTokenExchange = {
  serviceId: string;
  subject: string;
  orgId: string;
  scopes: readonly string[];
  reason: string;
};

/**
 * Authorizes an RFC 8693-style token exchange: a registered service
 * principal presenting an already-verified subject token as proof of an
 * acting user, requesting a new, narrower token for `request.audience`.
 * Distinct from `authorizePlaneServicePrincipal` (used by `/internal-token`)
 * per T-5 — this is the one place `tokenExchange` is checked, and its
 * result never carries a `zdr`/retention default the caller could
 * mistakenly reuse in place of the subject token's own.
 */
export function authorizePlaneServiceTokenExchange(
  rawRegistry: string,
  request: PlaneServiceTokenExchangeRequest,
): AuthorizedPlaneServiceTokenExchange {
  const registry = parseRegistry(rawRegistry);
  const serviceId = request.serviceId.trim();
  const orgId = request.orgId.trim();
  const reason = request.reason.trim();
  const requestedScopes = normalizedUnique(request.requestedScopes);
  const principal = registry[serviceId];
  const allowedScopes = principal?.scopesByAudience
    ? principal.scopesByAudience[request.audience]
    : principal?.scopes;
  const allowedSubjectAudiences = principal?.tokenExchange?.[request.audience];

  if (
    !principal ||
    !request.credential ||
    !secureEqual(principal.credential, request.credential) ||
    !principal.audiences.includes(request.audience) ||
    !allowedSubjectAudiences ||
    !allowedSubjectAudiences.includes(request.subjectAudience) ||
    !orgId ||
    // The organization of an exchange is proven by the verified subject token
    // (the controller refuses a request whose orgId differs from it), which is
    // stronger evidence than a directory lookup, so a `directory` row needs no
    // lookup here. A fixed-list row still may not exchange outside its list.
    !orgBindingFor(principal, orgId) ||
    requestedScopes.length === 0 ||
    !allowedScopes ||
    requestedScopes.some((scope) => !allowedScopes.includes(scope)) ||
    reason.length < 3 ||
    reason.length > 500
  ) {
    throw new ServicePrincipalAuthorizationError(
      'Service principal token exchange is not authorized',
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
