/**
 * Control-Plane-owned retention posture for interactive user tokens.
 *
 * Zero Data Retention (ZDR) is an OPT-IN, PAID, plan-gated add-on — never the
 * standard. A brand-new org is on normal retention (`zdr: false`). An org gets
 * `zdr: true` only when BOTH of the following hold in org-core, the sole
 * authority for organization state:
 *
 *   1. Stored intent — the org's admin turned ZDR on through the settings
 *      endpoint (`organizations.metadata.interactiveRetention.zdr`, written by
 *      org-core's `SetInteractiveRetention`).
 *   2. Plan entitlement — the org's CURRENT plan may use ZDR
 *      (`org-core/internal/org/types.go#zeroDataRetentionPlans`).
 *
 * Both are computed by org-core and returned by one narrow, service-only
 * endpoint (`GET /internal/orgs/:orgId/retention-posture` ->
 * `{ zdrIntent, entitled }`, scope `org:retention:read:any`). Auth Core does not
 * re-implement the plan allowlist and does not read the organization record.
 * `zdr = zdrIntent && entitled`; an org whose plan lapsed keeps its stored
 * intent but is not zero-retention (and a warning says so).
 *
 * Authentication: org-core applies service authentication to every route, so
 * the request carries `x-service-id: auth-core` and `x-service-token`
 * (`ORG_CORE_SERVICE_TOKEN`), the same credential Auth Core's other org-core
 * calls use. The previous version sent no headers, received 401 on every call,
 * swallowed it as an "outage" and resolved every org to `zdr: false`: a ZDR
 * org's setting never reached a token. The wire contract is pinned by a
 * golden file replayed through org-core's real middleware
 * (interactive-retention-policy.org-core-contract.spec.ts and org-core's
 * retention_posture_contract_test.go).
 *
 * What "cannot read the posture" means. A boolean claim cannot express
 * "unknown", and a wrong `false` is the dangerous direction: direct claim
 * readers (documents-api, session-core memory) persist a ZDR org's content when
 * the claim says `false`. So a failed lookup is never turned into a posture:
 *
 *   - FRESH: a successful lookup is cached for 45 s.
 *   - STALE-IF-ERROR: if org-core cannot answer, the last successful lookup for
 *     that org is served for a bounded time, measured from that lookup (a stale
 *     answer never extends it): 24 h when the org was ZDR, 15 min when it was
 *     not. A known ZDR org is never downgraded by an outage. The non-ZDR
 *     window is short because a stale `false` is the one that can leak (the org
 *     may have turned ZDR on since); 15 min matches the longest interactive
 *     token lifetime, so it at most doubles the staleness the cache already
 *     allows.
 *   - NO RESULT: with nothing usable, the resolver throws
 *     `RetentionPostureUnavailableError` (HTTP 503, `retention_posture_unavailable`)
 *     and no token is issued. The caller retries; nothing is persisted under a
 *     guessed posture.
 *   - FAILURES ARE NEVER CACHED: the next mint asks org-core again, so a
 *     recovery is seen immediately. (Concurrent mints for one org share a single
 *     lookup.)
 *
 * The availability trade-off is deliberate and narrow: an interactive token
 * cannot be minted for an org that has not been resolved in this process within
 * the window above while org-core is unreachable (a cold start during an
 * org-core outage, or an org-core outage longer than 15 min for non-ZDR orgs).
 * Sign-in itself (Better Auth) does not depend on org-core; the plane and Model
 * Plane token endpoints do, for orgs outside the window. Orgs seen recently
 * keep working through an outage.
 *
 * Service tokens for background workers (`tenant` retention). A principal whose
 * registry row says `tenant` for an audience (embedding-engine, graph-index)
 * acts for whichever org the work belongs to, so its token's `zdr` claim is that
 * org's posture, read through this same resolver (same cache, same stale rule,
 * same 503) by `resolveTenantServiceRetentionPosture`, and not a property of the
 * principal. The token controllers call it only after the caller is authorized,
 * so an unauthenticated caller cannot make Auth Core query org-core. Such tokens
 * live at most 60 seconds (`TENANT_PRINCIPAL_TOKEN_TTL_SECONDS`).
 *
 * Staleness, disclosed. After an org turns ZDR on, a tenant worker can still
 * hold a `zdr: false` token for up to the 45 s cache plus the 60 s lifetime
 * (about 105 s). While org-core is unreachable it is the stale rule above that
 * applies to these tokens too: a remembered posture is served for up to 15 min
 * (not ZDR) or 24 h (ZDR), so the window for a stale `false` is that 15 min plus
 * 60 s. Background work is retried by its callers, so refusing a stale
 * non-ZDR posture for tenant tokens (503 instead) costs only delay; it is not
 * done here because it would make this resolver answer two ways for one org.
 *
 * Directory binding (`tenancy: 'directory'`, the production principal model).
 * A workload that acts for whichever organization the work belongs to cannot
 * have a fixed `orgIds` list in a production registry (a new organization would
 * need a registry change and a secret rollout before its first document could be
 * embedded), and `allowAnyOrg` is refused there. Such a row is bound to org-core's
 * directory instead: `assertServiceOrganizationBinding` confirms, at every mint
 * and before anything is signed, that org-core holds an active record of the
 * organization the token is for. The answer comes from the same read, cache and
 * stale rule as the posture above (one org-core round trip serves both for a
 * `tenant` lane), and the failure semantics are the same: org-core unreadable
 * with nothing usable remembered is a 503 and no token
 * (`organization_directory_unavailable`), an organization org-core does not hold
 * is a 403. What it does and does not buy: no token for an organization that does
 * not exist (a fabricated or mistyped id, a deleted organization); it does NOT
 * stop a leaked credential from minting for a real organization. That boundary
 * belongs to the second proof of the delegated-identity design (token exchange
 * from a verified subject, T-4/T-5), which a principal that has one for its work
 * should use instead of `/internal-token`.
 *
 * Process-local, like the cache it replaces: Auth Core runs one instance per
 * container. A restart drops the last-known postures (see the report's
 * follow-up on persisting them).
 */
import {
  ForbiddenException,
  Logger,
  ServiceUnavailableException,
} from '@nestjs/common';

import { requiredScopedServiceToken } from './control-service-credentials';
import type { AuthorizedPlaneServicePrincipal } from './plane-service-principal';

export type InteractiveRetentionPosture = Readonly<{
  zdr: boolean;
  authority: 'interactive-org-retention-policy';
}>;

const DEFAULT_POSTURE: InteractiveRetentionPosture = Object.freeze({
  zdr: false,
  authority: 'interactive-org-retention-policy',
});

const ZDR_POSTURE: InteractiveRetentionPosture = Object.freeze({
  zdr: true,
  authority: 'interactive-org-retention-policy',
});

const ORGANIZATION_ID = /^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$/;

/** How long a successful lookup is served without asking org-core again. */
export const RETENTION_POSTURE_FRESH_TTL_MS = 45_000;
/** How long a last-known ZDR posture survives an org-core outage. */
export const RETENTION_STALE_ZDR_MAX_AGE_MS = 24 * 60 * 60 * 1000;
/** How long a last-known non-ZDR posture survives an org-core outage. */
export const RETENTION_STALE_NON_ZDR_MAX_AGE_MS = 15 * 60 * 1000;
/** Bounds a single org-core round trip so a stalled dependency can't stall token issuance. */
const LOOKUP_TIMEOUT_MS = 2_000;
/** Same message at most once per this long, so an outage does not flood the log. */
const WARN_INTERVAL_MS = 60_000;
/** Upper bound on remembered orgs; far above any real tenant count. */
const MAX_REMEMBERED_ORGS = 10_000;

const SERVICE_ID = 'auth-core';
/** org-core's 404 body for an organization it has no active record of. */
const ORG_NOT_FOUND_ERROR = 'organization not found';
const logger = new Logger('InteractiveRetentionPolicy');

export type RetentionPostureUnavailableReason =
  | 'invalid_organization_id'
  | 'org_core_credential_missing'
  | 'org_core_credential_rejected'
  | 'org_core_unreachable'
  | 'org_core_error_status'
  | 'org_core_malformed_response';

/**
 * The org's retention posture could not be verified and nothing usable is
 * remembered, so no token may be issued. Extends the Nest 503 so the token
 * controllers answer it without handling it: the body is exactly
 * `{ statusCode: 503, error: 'retention_posture_unavailable', message }`. The
 * internal `reason` stays on the error and in the log, not in the response.
 */
export class RetentionPostureUnavailableError extends ServiceUnavailableException {
  constructor(readonly reason: RetentionPostureUnavailableReason) {
    super({
      statusCode: 503,
      error: 'retention_posture_unavailable',
      message:
        'The organization retention posture could not be verified. Retry shortly.',
    });
  }
}

class PostureLookupFailure extends Error {
  constructor(
    readonly reason: RetentionPostureUnavailableReason,
    readonly detail?: string,
  ) {
    super(reason);
  }
}

type PostureEntry = Readonly<{
  zdrIntent: boolean;
  entitled: boolean;
  /** When org-core last answered successfully; never moved by a stale serve. */
  resolvedAt: number;
}>;

type LookupResult =
  | Readonly<{ kind: 'found'; zdrIntent: boolean; entitled: boolean }>
  | Readonly<{ kind: 'not_found' }>;

const postureCache = new Map<string, PostureEntry>();
const lookupsInFlight = new Map<string, Promise<LookupResult>>();
const lastWarnedAt = new Map<string, number>();

function orgServiceBaseUrl(): string {
  // Same variables as Auth Core's other org-core client
  // (`orgCoreConfig` in organization-events.plugin.ts), with the in-cluster
  // default this module has always had.
  return (
    process.env.ORG_SERVICE_URL ||
    process.env.ORG_CORE_URL ||
    'http://org-core:8080'
  ).replace(/\/$/, '');
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === 'object' && !Array.isArray(value);
}

function isZdr(entry: Pick<PostureEntry, 'zdrIntent' | 'entitled'>): boolean {
  return entry.zdrIntent && entry.entitled;
}

function toPosture(
  entry: Pick<PostureEntry, 'zdrIntent' | 'entitled'>,
): InteractiveRetentionPosture {
  return isZdr(entry) ? ZDR_POSTURE : DEFAULT_POSTURE;
}

function staleWindowMs(entry: PostureEntry): number {
  return isZdr(entry)
    ? RETENTION_STALE_ZDR_MAX_AGE_MS
    : RETENTION_STALE_NON_ZDR_MAX_AGE_MS;
}

function usableStale(
  entry: PostureEntry | undefined,
  now: number,
): PostureEntry | undefined {
  return entry && now - entry.resolvedAt < staleWindowMs(entry)
    ? entry
    : undefined;
}

function throttled(key: string, now: number): boolean {
  const last = lastWarnedAt.get(key);
  if (last !== undefined && now - last < WARN_INTERVAL_MS) return true;
  // Bounded like the posture memory: past the cap, forget and start over.
  if (lastWarnedAt.size >= MAX_REMEMBERED_ORGS * 2) lastWarnedAt.clear();
  lastWarnedAt.set(key, now);
  return false;
}

function warnThrottled(key: string, message: string, now: number): void {
  if (!throttled(key, now)) logger.warn(message);
}

function errorThrottled(key: string, message: string, now: number): void {
  if (!throttled(key, now)) logger.error(message);
}

/** An org id that is safe to put in a log line, whatever it contains. */
function loggableOrgId(organizationId: string): string {
  return ORGANIZATION_ID.test(organizationId)
    ? organizationId
    : `<invalid id, ${organizationId.length} chars>`;
}

/**
 * The error's name and, when the transport reports one, its system code
 * (`TypeError/ECONNREFUSED`, `TimeoutError`), so an outage log says which kind
 * of outage it was. Never the message: a fetch error message can carry the URL.
 */
function describeFetchError(error: unknown): string {
  if (!isRecord(error) && !(error instanceof Error)) return 'unknown';
  const name = (error as { name?: unknown }).name;
  const cause = (error as { cause?: unknown }).cause;
  const code =
    isRecord(cause) && typeof cause.code === 'string' ? cause.code : undefined;
  const label = typeof name === 'string' ? name : 'unknown';
  return code ? `${label}/${code}` : label;
}

/**
 * One authenticated read of org-core's posture endpoint. Throws a
 * `PostureLookupFailure` for everything that is not an answer; never returns a
 * posture it did not read.
 */
async function fetchPosture(organizationId: string): Promise<LookupResult> {
  let token: string;
  try {
    token = requiredScopedServiceToken('ORG_CORE_SERVICE_TOKEN');
  } catch {
    errorThrottled(
      'credential_missing',
      'org_core_credential_missing: ORG_CORE_SERVICE_TOKEN is absent or a placeholder; no retention posture can be read',
      Date.now(),
    );
    throw new PostureLookupFailure('org_core_credential_missing');
  }

  let response: Response;
  try {
    response = await fetch(
      `${orgServiceBaseUrl()}/internal/orgs/${encodeURIComponent(organizationId)}/retention-posture`,
      {
        method: 'GET',
        headers: {
          accept: 'application/json',
          'x-service-id': SERVICE_ID,
          'x-service-token': token,
        },
        signal: AbortSignal.timeout(LOOKUP_TIMEOUT_MS),
      },
    );
  } catch (error) {
    throw new PostureLookupFailure(
      'org_core_unreachable',
      describeFetchError(error),
    );
  }

  if (response.status === 404) {
    // Only org-core's own "no such organization" answer is a definitive "no
    // record". Any other 404 (a wrong ORG_SERVICE_URL, a proxy, an org-core
    // build without the route) is a failure, never a posture.
    let notFoundBody: unknown;
    try {
      notFoundBody = await response.json();
    } catch {
      notFoundBody = undefined;
    }
    if (isRecord(notFoundBody) && notFoundBody.error === ORG_NOT_FOUND_ERROR) {
      return { kind: 'not_found' };
    }
    throw new PostureLookupFailure('org_core_error_status', '404');
  }
  if (response.status === 401 || response.status === 403) {
    // Not an outage: Auth Core's credential or scope is wrong, and every org
    // will fail the same way until a human fixes it. Say so loudly, once a
    // minute, whether or not a stale posture hides it from callers.
    errorThrottled(
      'credential_rejected',
      `org_core_credential_rejected: org-core answered ${response.status} to Auth Core's retention posture read; check ORG_CORE_SERVICE_TOKEN and the auth-core org:retention:read:any grant`,
      Date.now(),
    );
    throw new PostureLookupFailure(
      'org_core_credential_rejected',
      String(response.status),
    );
  }
  if (!response.ok) {
    throw new PostureLookupFailure(
      'org_core_error_status',
      String(response.status),
    );
  }

  let body: unknown;
  try {
    body = await response.json();
  } catch {
    throw new PostureLookupFailure('org_core_malformed_response');
  }
  if (
    !isRecord(body) ||
    typeof body.zdrIntent !== 'boolean' ||
    typeof body.entitled !== 'boolean'
  ) {
    throw new PostureLookupFailure('org_core_malformed_response');
  }
  return { kind: 'found', zdrIntent: body.zdrIntent, entitled: body.entitled };
}

/** Concurrent mints for one org share a single org-core read. */
function lookupOnce(organizationId: string): Promise<LookupResult> {
  const running = lookupsInFlight.get(organizationId);
  if (running) return running;
  const started = fetchPosture(organizationId).finally(() => {
    lookupsInFlight.delete(organizationId);
  });
  lookupsInFlight.set(organizationId, started);
  return started;
}

function remember(organizationId: string, entry: PostureEntry): void {
  // Delete first so a refreshed org moves to the newest slot of the Map's
  // insertion order, which the eviction below relies on.
  postureCache.delete(organizationId);
  postureCache.set(organizationId, entry);
  if (postureCache.size <= MAX_REMEMBERED_ORGS) return;
  for (const [orgId, remembered] of postureCache) {
    if (postureCache.size <= MAX_REMEMBERED_ORGS) break;
    if (!usableStale(remembered, entry.resolvedAt)) postureCache.delete(orgId);
  }
  for (const orgId of postureCache.keys()) {
    if (postureCache.size <= MAX_REMEMBERED_ORGS) break;
    postureCache.delete(orgId);
  }
}

function unavailable(
  organizationId: string,
  reason: RetentionPostureUnavailableReason,
  detail?: string,
): RetentionPostureUnavailableError {
  logger.error(
    `retention_posture_unavailable: no token will be issued for org=${loggableOrgId(organizationId)} reason=${reason}${detail ? ` detail=${detail}` : ''}`,
  );
  return new RetentionPostureUnavailableError(reason);
}

/**
 * Resolve the effective interactive-token retention posture for an
 * organization: `zdr: true` only when org-core reports both the stored intent
 * and the plan entitlement. See the module comment for the cache, stale and
 * failure semantics.
 *
 * @throws RetentionPostureUnavailableError (HTTP 503) when the posture cannot
 * be read and no usable last-known posture exists. Callers must let it
 * propagate: it means "do not issue a token", never "issue a non-ZDR one".
 */
export async function resolveInteractiveRetentionPosture(
  organizationId: string,
): Promise<InteractiveRetentionPosture> {
  if (!ORGANIZATION_ID.test(organizationId)) {
    // Not a lookup failure, and not an org that can be assumed non-ZDR either:
    // an id this resolver cannot even ask about has an unknown posture.
    throw unavailable(organizationId, 'invalid_organization_id');
  }

  const known = postureCache.get(organizationId);
  if (known && Date.now() - known.resolvedAt < RETENTION_POSTURE_FRESH_TTL_MS) {
    return toPosture(known);
  }

  let result: LookupResult;
  try {
    result = await lookupOnce(organizationId);
  } catch (error) {
    const failure =
      error instanceof PostureLookupFailure
        ? error
        : new PostureLookupFailure('org_core_unreachable');
    const now = Date.now();
    const stale = usableStale(known, now);
    if (stale) {
      warnThrottled(
        `stale:${organizationId}`,
        `serving_stale_posture: org-core could not answer (${failure.reason}); serving org=${organizationId} zdr=${isZdr(stale)} last confirmed ${Math.round((now - stale.resolvedAt) / 1000)}s ago`,
        now,
      );
      return toPosture(stale);
    }
    throw unavailable(organizationId, failure.reason, failure.detail);
  }

  const now = Date.now();
  if (result.kind === 'not_found') {
    // org-core has no active record, so no ZDR intent can exist for it (the
    // intent lives on that record). Do not remember the answer: the record can
    // appear moments later (Auth projects new orgs asynchronously). But a
    // 404 for an org we knew as ZDR is an anomaly, not a downgrade.
    const stale = usableStale(known, now);
    if (stale && isZdr(stale)) {
      warnThrottled(
        `stale:${organizationId}`,
        `serving_stale_posture: org-core answered 404 for a known ZDR org=${organizationId}; keeping zdr=true`,
        now,
      );
      return ZDR_POSTURE;
    }
    postureCache.delete(organizationId);
    return DEFAULT_POSTURE;
  }

  const entry: PostureEntry = {
    zdrIntent: result.zdrIntent,
    entitled: result.entitled,
    resolvedAt: now,
  };
  remember(organizationId, entry);
  if (entry.zdrIntent && !entry.entitled) {
    warnThrottled(
      `plan:${organizationId}`,
      `plan_not_entitled: org=${organizationId} has ZDR switched on but its plan no longer includes it; interactive tokens are issued with zdr=false`,
      now,
    );
  }
  return toPosture(entry);
}

/**
 * What a service token carries when its principal's registry row is `tenant`:
 * the org's posture, stamped with the service authority (the only one a service
 * token's signer honours) and `source: 'tenant-policy'` (the signer caps the
 * lifetime at 60 s, and the audit event records where `zdr` came from).
 */
export type TenantServiceRetentionPosture = Readonly<{
  zdr: boolean;
  authority: 'service-principal-policy';
  source: 'tenant-policy';
}>;

/**
 * Resolve the posture for a `tenant` service principal's mint: the posture of
 * the org the token is for, through `resolveInteractiveRetentionPosture`, so a
 * background worker and an interactive user of one org always see one answer.
 *
 * Call it after the principal is authorized and before the token is signed, and
 * let the error propagate: nothing may be signed under a guessed posture.
 *
 * @throws RetentionPostureUnavailableError (HTTP 503, `retention_posture_unavailable`)
 * when the org's posture cannot be read and no usable last-known posture exists.
 * No token is issued; the worker's caller retries (the Data Plane marks the
 * work pending).
 * @throws Error when the principal's retention is not `tenant`. That is a bug in
 * the caller, not a runtime condition: a `zdr` or `persistent` principal's
 * posture is its registry row, and letting the org override it would change
 * what those rows mean.
 */
export async function resolveTenantServiceRetentionPosture(
  principal: Pick<
    AuthorizedPlaneServicePrincipal,
    'retention' | 'orgId' | 'serviceId'
  >,
): Promise<TenantServiceRetentionPosture> {
  if (principal.retention !== 'tenant') {
    throw new Error(
      `service principal ${principal.serviceId} does not have the tenant retention posture`,
    );
  }
  try {
    const posture = await resolveInteractiveRetentionPosture(principal.orgId);
    return {
      zdr: posture.zdr,
      authority: 'service-principal-policy',
      source: 'tenant-policy',
    };
  } catch (error) {
    if (error instanceof RetentionPostureUnavailableError) {
      // The resolver's own log names the org and the reason; this adds who is
      // being refused, which is what an operator needs to see which worker is
      // stalled.
      warnThrottled(
        `tenant:${principal.serviceId}:${principal.orgId}`,
        `tenant_posture_unavailable: no token issued to service=${principal.serviceId} for org=${loggableOrgId(principal.orgId)} (${error.reason})`,
        Date.now(),
      );
    }
    throw error;
  }
}

/** What org-core's directory says about an organization id. */
export type OrganizationDirectoryStatus = 'active' | 'unknown';

/**
 * org-core could not say whether the organization exists and nothing usable is
 * remembered, so no token may be issued. A 503 with its own error code: this is
 * the directory read failing, not a retention posture, though both ride the same
 * org-core route. The internal `reason` stays on the error and in the log.
 */
export class OrganizationDirectoryUnavailableError extends ServiceUnavailableException {
  constructor(readonly reason: RetentionPostureUnavailableReason) {
    super({
      statusCode: 503,
      error: 'organization_directory_unavailable',
      message: 'The organization could not be verified. Retry shortly.',
    });
  }
}

/**
 * Does org-core's directory hold an active record of this organization?
 *
 * Reads through the posture route, whose 404 body is org-core's own "no such
 * organization": one narrow grant (`org:retention:read:any`), no widening of
 * Auth Core's org-core credential, and one round trip serves the existence
 * question and the posture. A positive answer is cached like the posture
 * (fresh 45 s, stale-if-error 15 min or 24 h); a negative one is never cached,
 * because the record can appear moments later (Auth projects new organizations
 * asynchronously), and it never touches the posture cache, so a 404 here cannot
 * weaken the "a known ZDR organization is not downgraded" rule above.
 *
 * @throws OrganizationDirectoryUnavailableError (HTTP 503) when org-core cannot
 * answer and no usable last-known record exists. Callers must let it propagate:
 * it means "do not issue a token", never "the organization is fine".
 */
export async function resolveOrganizationDirectoryStatus(
  organizationId: string,
): Promise<OrganizationDirectoryStatus> {
  // An id the lookup cannot even ask about is not an organization.
  if (!ORGANIZATION_ID.test(organizationId)) return 'unknown';

  const known = postureCache.get(organizationId);
  if (known && Date.now() - known.resolvedAt < RETENTION_POSTURE_FRESH_TTL_MS) {
    return 'active';
  }

  let result: LookupResult;
  try {
    result = await lookupOnce(organizationId);
  } catch (error) {
    const failure =
      error instanceof PostureLookupFailure
        ? error
        : new PostureLookupFailure('org_core_unreachable');
    const now = Date.now();
    const stale = usableStale(known, now);
    if (stale) {
      warnThrottled(
        `stale-directory:${organizationId}`,
        `serving_stale_directory: org-core could not answer (${failure.reason}); org=${organizationId} was confirmed active ${Math.round((now - stale.resolvedAt) / 1000)}s ago`,
        now,
      );
      return 'active';
    }
    logger.error(
      `organization_directory_unavailable: no token will be issued for org=${loggableOrgId(organizationId)} reason=${failure.reason}${failure.detail ? ` detail=${failure.detail}` : ''}`,
    );
    throw new OrganizationDirectoryUnavailableError(failure.reason);
  }

  if (result.kind === 'not_found') return 'unknown';
  remember(organizationId, {
    zdrIntent: result.zdrIntent,
    entitled: result.entitled,
    resolvedAt: Date.now(),
  });
  return 'active';
}

/**
 * Enforce a principal's organization binding before a token is signed. A fixed
 * or legacy-unchecked binding needs nothing; a `directory` binding needs org-core
 * to vouch for the organization. The token controllers call this right after the
 * caller is authorized, next to the tenant posture, and must let both errors
 * propagate (403 for an organization the directory does not hold, 503 when it
 * cannot be read). The caller is told only "not authorized": which organizations
 * exist is not something an error body should teach.
 */
export async function assertServiceOrganizationBinding(
  principal: Pick<
    AuthorizedPlaneServicePrincipal,
    'orgBinding' | 'orgId' | 'serviceId'
  >,
): Promise<void> {
  if (principal.orgBinding !== 'directory') return;
  const status = await resolveOrganizationDirectoryStatus(principal.orgId);
  if (status === 'active') return;
  warnThrottled(
    `directory:${principal.serviceId}:${principal.orgId}`,
    `organization_not_in_directory: refused service=${principal.serviceId} for org=${loggableOrgId(principal.orgId)}`,
    Date.now(),
  );
  throw new ForbiddenException('Service principal is not authorized');
}

/** Test-only: clear the process-local posture state between test cases. */
export function resetInteractiveRetentionCacheForTests(): void {
  postureCache.clear();
  lookupsInFlight.clear();
  lastWarnedAt.clear();
}
