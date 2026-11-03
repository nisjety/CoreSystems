/**
 * The org web policy a `quarry` or `browser-broker` token carries (WS7.I).
 *
 * org-core owns the policy (WS7.H, ADR-1 outcome C). Auth Core reads the
 * EFFECTIVE document (the stored one folded with the plan's entitlements,
 * tightening only) from one narrow, service-only route,
 * `GET /internal/orgs/:orgId/web-policy` (scope `org:web-policy:read:any`, held
 * by Auth Core alone), and signs it into the token as the `web_policy` claim:
 *
 *   {"v":1,"rev":"<12 hex or \"0\">","caps":{...},"robots_max":...,
 *    "proxy_max":...,"domains":{"allow":[...],"deny":[...]}}
 *
 * Only the entries the document states are carried (org-core omits the
 * baseline ones); `{"v":1,"rev":"0"}` means "no policy". The claim is always
 * present on a user token for those audiences, so an ABSENT claim on a user
 * token identifies an old issuer, never "no policy". A service token carries it
 * only when the principal's registry row says
 * `webPolicyByAudience[audience] = 'tenant'` (`execution-core` and
 * `model-gateway` on `quarry`).
 *
 * `zdr` is untouched: it stays the only ZDR carrier, and nothing here reads or
 * changes a retention posture. This resolver is independent of
 * `interactive-retention-policy.ts` (a `zdr` registry row may still carry the
 * org's web policy).
 *
 * Cache and failure semantics (same shape as the retention resolver):
 *   - FRESH: a successful read is served for 45 s without asking again.
 *   - STALE-IF-ERROR: when org-core cannot answer, the last successful read for
 *     that org is served for 15 min measured from that read, with a warning.
 *   - NO RESULT: with nothing usable remembered, `WebPolicyUnavailableError`
 *     (HTTP 503, `web_policy_unavailable`) and no token.
 *   - FAILURES ARE NEVER CACHED: the next mint asks org-core again. Concurrent
 *     mints for one org share one read.
 *   - A 404 from the route is an ERROR, never an empty policy (W2-B3): a wrong
 *     `ORG_SERVICE_URL`, an org-core without the route and an unknown org all
 *     look alike from here, and none of them is "this org has no policy"
 *     (org-core answers that with 200 and `rev:"0"`).
 *
 * Size: a claim above 2 KiB (its compact JSON) fails closed with
 * `WebPolicyTooLargeError` (503, `web_policy_too_large`) and no token. org-core
 * caps the stored document below that, so this is a guard, not a code path.
 *
 * Flag: `WEB_POLICY_CLAIM_ENABLED` (default off until org-core WS7.H is
 * deployed). Off, every token for those audiences that may carry the claim
 * carries `{"v":1,"rev":"0"}` and org-core is never asked.
 *
 * Process-local, like the retention cache: one Auth Core per container.
 */
import { Logger, ServiceUnavailableException } from '@nestjs/common';

import { requiredScopedServiceToken } from './control-service-credentials';

/** The claim's only version. A consumer refuses any other. */
export const WEB_POLICY_CLAIM_VERSION = 1;

/** The audiences whose tokens carry the claim, and the only ones. */
export const WEB_POLICY_AUDIENCES: readonly string[] = Object.freeze([
  'quarry',
  'browser-broker',
]);

/** How long a successful read is served without asking org-core again. */
export const WEB_POLICY_FRESH_TTL_MS = 45_000;
/** How long a last-known policy survives an org-core outage, from that read. */
export const WEB_POLICY_STALE_MAX_AGE_MS = 15 * 60 * 1000;
/** Largest claim (compact JSON, UTF-8 bytes) a token may carry. */
export const WEB_POLICY_CLAIM_MAX_BYTES = 2048;
/** Bounds one org-core round trip; org-core's own handler timeout is 3 s. */
const LOOKUP_TIMEOUT_MS = 3_000;
/** Same message at most once per this long, so an outage does not flood the log. */
const WARN_INTERVAL_MS = 60_000;
/** Upper bound on remembered orgs; far above any real tenant count. */
const MAX_REMEMBERED_ORGS = 10_000;
const SERVICE_ID = 'auth-core';
const ORGANIZATION_ID = /^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$/;
/** org-core's digest: the first 12 hex of sha256, or "0" with no stored policy. */
const REV = /^(?:0|[0-9a-f]{12})$/;

const logger = new Logger('WebPolicy');

export type WebPolicyDomains = {
  allow?: readonly string[];
  deny?: readonly string[];
};

/** The `web_policy` claim exactly as it is signed. */
export type WebPolicyClaim = Readonly<{
  v: typeof WEB_POLICY_CLAIM_VERSION;
  rev: string;
  caps?: Readonly<Record<string, string>>;
  robots_max?: string;
  proxy_max?: string;
  domains?: Readonly<WebPolicyDomains>;
}>;

/** What a token carries when there is no policy, or the flag is off. */
export const BASELINE_WEB_POLICY_CLAIM: WebPolicyClaim = Object.freeze({
  v: WEB_POLICY_CLAIM_VERSION,
  rev: '0',
});

export type WebPolicyUnavailableReason =
  | 'invalid_organization_id'
  | 'org_core_credential_missing'
  | 'org_core_credential_rejected'
  | 'org_core_unreachable'
  | 'org_core_not_found'
  | 'org_core_error_status'
  | 'org_core_malformed_response';

/**
 * The org's web policy could not be read and nothing usable is remembered, so
 * no token may be issued. Body: `{statusCode:503, error:'web_policy_unavailable',
 * message}`; the internal reason stays on the error and in the log.
 */
export class WebPolicyUnavailableError extends ServiceUnavailableException {
  constructor(readonly reason: WebPolicyUnavailableReason) {
    super({
      statusCode: 503,
      error: 'web_policy_unavailable',
      message:
        'The organization web policy could not be verified. Retry shortly.',
    });
  }
}

/** The claim would exceed 2 KiB. Fails closed: no token. */
export class WebPolicyTooLargeError extends ServiceUnavailableException {
  constructor(readonly bytes: number) {
    super({
      statusCode: 503,
      error: 'web_policy_too_large',
      message: 'The organization web policy is too large to be carried.',
    });
  }
}

/** Is the claim switched on? Read per mint so an operator flip needs no rebuild. */
export function webPolicyClaimEnabled(
  environment: Record<string, string | undefined> = process.env,
): boolean {
  const raw = environment.WEB_POLICY_CLAIM_ENABLED?.trim().toLowerCase();
  return raw === '1' || raw === 'true';
}

export function isWebPolicyAudience(audience: string): boolean {
  return WEB_POLICY_AUDIENCES.includes(audience);
}

/** Compact UTF-8 size of the claim as the signer serializes it. */
export function webPolicyClaimBytes(claim: WebPolicyClaim): number {
  return Buffer.byteLength(JSON.stringify(claim), 'utf8');
}

/** @throws WebPolicyTooLargeError above `WEB_POLICY_CLAIM_MAX_BYTES`. */
export function assertWebPolicyClaimSize(claim: WebPolicyClaim): void {
  const bytes = webPolicyClaimBytes(claim);
  if (bytes > WEB_POLICY_CLAIM_MAX_BYTES) {
    logger.error(
      `web_policy_too_large: claim is ${bytes} bytes (cap ${WEB_POLICY_CLAIM_MAX_BYTES}); no token issued`,
    );
    throw new WebPolicyTooLargeError(bytes);
  }
}

class WebPolicyLookupFailure extends Error {
  constructor(
    readonly reason: WebPolicyUnavailableReason,
    readonly detail?: string,
  ) {
    super(reason);
  }
}

type Entry = Readonly<{
  claim: WebPolicyClaim;
  /** When org-core last answered; never moved by a stale serve. */
  resolvedAt: number;
}>;

const cache = new Map<string, Entry>();
const inFlight = new Map<string, Promise<WebPolicyClaim>>();
const lastWarnedAt = new Map<string, number>();

function orgServiceBaseUrl(): string {
  // The same variables and default as the retention resolver.
  return (
    process.env.ORG_SERVICE_URL ||
    process.env.ORG_CORE_URL ||
    'http://org-core:8080'
  ).replace(/\/$/, '');
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === 'object' && !Array.isArray(value);
}

function isStringList(value: unknown): value is string[] {
  return (
    Array.isArray(value) && value.every((item) => typeof item === 'string')
  );
}

function throttled(key: string, now: number): boolean {
  const last = lastWarnedAt.get(key);
  if (last !== undefined && now - last < WARN_INTERVAL_MS) return true;
  if (lastWarnedAt.size >= MAX_REMEMBERED_ORGS * 2) lastWarnedAt.clear();
  lastWarnedAt.set(key, now);
  return false;
}

/** An org id that is safe to put in a log line, whatever it contains. */
function loggableOrgId(organizationId: string): string {
  return ORGANIZATION_ID.test(organizationId)
    ? organizationId
    : `<invalid id, ${organizationId.length} chars>`;
}

/** Name and system code of a transport error, never its message (it can carry the URL). */
function describeFetchError(error: unknown): string {
  if (!isRecord(error) && !(error instanceof Error)) return 'unknown';
  const name = (error as { name?: unknown }).name;
  const cause = (error as { cause?: unknown }).cause;
  const code =
    isRecord(cause) && typeof cause.code === 'string' ? cause.code : undefined;
  const label = typeof name === 'string' ? name : 'unknown';
  return code ? `${label}/${code}` : label;
}

type MutableClaim = {
  v: typeof WEB_POLICY_CLAIM_VERSION;
  rev: string;
  caps?: Record<string, string>;
  robots_max?: string;
  proxy_max?: string;
  domains?: WebPolicyDomains;
};

/**
 * Turn org-core's answer (`{data:{revision, rev, policy, entitlements}}`) into
 * the claim. Only the four document fields a version 1 claim defines are
 * carried; capability keys are sorted so one policy always signs to the same
 * bytes; empty containers are dropped. Anything malformed is a failure, never
 * a partial policy.
 *
 * @throws Error (internal lookup failure) when the answer is malformed.
 */
export function webPolicyClaimFromOrgCore(body: unknown): WebPolicyClaim {
  if (!isRecord(body) || !isRecord(body.data)) {
    throw new WebPolicyLookupFailure('org_core_malformed_response', 'envelope');
  }
  const { rev, policy } = body.data;
  if (typeof rev !== 'string' || !REV.test(rev)) {
    throw new WebPolicyLookupFailure('org_core_malformed_response', 'rev');
  }
  if (policy !== undefined && policy !== null && !isRecord(policy)) {
    throw new WebPolicyLookupFailure('org_core_malformed_response', 'policy');
  }
  const doc: Record<string, unknown> = isRecord(policy) ? policy : {};
  const claim: MutableClaim = { v: WEB_POLICY_CLAIM_VERSION, rev };

  if (doc.caps !== undefined && doc.caps !== null) {
    const rawCaps = doc.caps;
    if (!isRecord(rawCaps)) {
      throw new WebPolicyLookupFailure('org_core_malformed_response', 'caps');
    }
    const caps: Record<string, string> = {};
    for (const key of Object.keys(rawCaps).sort()) {
      const mode = rawCaps[key];
      if (typeof mode !== 'string') {
        throw new WebPolicyLookupFailure('org_core_malformed_response', 'caps');
      }
      caps[key] = mode;
    }
    if (Object.keys(caps).length > 0) claim.caps = caps;
  }
  for (const field of ['robots_max', 'proxy_max'] as const) {
    const value = doc[field];
    if (value === undefined || value === null || value === '') continue;
    if (typeof value !== 'string') {
      throw new WebPolicyLookupFailure('org_core_malformed_response', field);
    }
    claim[field] = value;
  }
  if (doc.domains !== undefined && doc.domains !== null) {
    const rawDomains = doc.domains;
    if (!isRecord(rawDomains)) {
      throw new WebPolicyLookupFailure(
        'org_core_malformed_response',
        'domains',
      );
    }
    const domains: { allow?: string[]; deny?: string[] } = {};
    for (const side of ['allow', 'deny'] as const) {
      const list = rawDomains[side];
      if (list === undefined || list === null) continue;
      if (!isStringList(list)) {
        throw new WebPolicyLookupFailure('org_core_malformed_response', side);
      }
      if (list.length > 0) domains[side] = [...list];
    }
    if (domains.allow || domains.deny) claim.domains = domains;
  }
  return Object.freeze(claim);
}

/** One authenticated read. Throws a lookup failure for anything not an answer. */
async function fetchWebPolicy(organizationId: string): Promise<WebPolicyClaim> {
  let token: string;
  try {
    token = requiredScopedServiceToken('ORG_CORE_SERVICE_TOKEN');
  } catch {
    if (!throttled('credential_missing', Date.now())) {
      logger.error(
        'org_core_credential_missing: ORG_CORE_SERVICE_TOKEN is absent or a placeholder; no web policy can be read',
      );
    }
    throw new WebPolicyLookupFailure('org_core_credential_missing');
  }

  let response: Response;
  try {
    response = await fetch(
      `${orgServiceBaseUrl()}/internal/orgs/${encodeURIComponent(organizationId)}/web-policy`,
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
    throw new WebPolicyLookupFailure(
      'org_core_unreachable',
      describeFetchError(error),
    );
  }

  if (response.status === 404) {
    // Never "no policy": see the module comment (W2-B3).
    throw new WebPolicyLookupFailure('org_core_not_found', '404');
  }
  if (response.status === 401 || response.status === 403) {
    if (!throttled('credential_rejected', Date.now())) {
      logger.error(
        `org_core_credential_rejected: org-core answered ${response.status} to Auth Core's web policy read; check ORG_CORE_SERVICE_TOKEN and the auth-core org:web-policy:read:any grant`,
      );
    }
    throw new WebPolicyLookupFailure(
      'org_core_credential_rejected',
      String(response.status),
    );
  }
  if (!response.ok) {
    throw new WebPolicyLookupFailure(
      'org_core_error_status',
      String(response.status),
    );
  }
  let body: unknown;
  try {
    body = await response.json();
  } catch {
    throw new WebPolicyLookupFailure('org_core_malformed_response', 'json');
  }
  return webPolicyClaimFromOrgCore(body);
}

/** Concurrent mints for one org share a single org-core read. */
function lookupOnce(organizationId: string): Promise<WebPolicyClaim> {
  const running = inFlight.get(organizationId);
  if (running) return running;
  const started = fetchWebPolicy(organizationId).finally(() => {
    inFlight.delete(organizationId);
  });
  inFlight.set(organizationId, started);
  return started;
}

function remember(organizationId: string, entry: Entry): void {
  // Delete first so a refreshed org moves to the newest insertion slot.
  cache.delete(organizationId);
  cache.set(organizationId, entry);
  if (cache.size <= MAX_REMEMBERED_ORGS) return;
  for (const [orgId, remembered] of cache) {
    if (cache.size <= MAX_REMEMBERED_ORGS) break;
    if (
      entry.resolvedAt - remembered.resolvedAt >=
      WEB_POLICY_STALE_MAX_AGE_MS
    ) {
      cache.delete(orgId);
    }
  }
  for (const orgId of cache.keys()) {
    if (cache.size <= MAX_REMEMBERED_ORGS) break;
    cache.delete(orgId);
  }
}

function unavailable(
  organizationId: string,
  reason: WebPolicyUnavailableReason,
  detail?: string,
): WebPolicyUnavailableError {
  logger.error(
    `web_policy_unavailable: no token will be issued for org=${loggableOrgId(organizationId)} reason=${reason}${detail ? ` detail=${detail}` : ''}`,
  );
  return new WebPolicyUnavailableError(reason);
}

/**
 * The `web_policy` claim for an organization, read from org-core (see the
 * module comment for the cache, stale and failure rules). Ignores the flag: the
 * caller decides whether to ask (`webPolicyClaimFor`).
 *
 * @throws WebPolicyUnavailableError (503) when org-core cannot answer and no
 * usable last-known policy exists. Let it propagate: no token.
 * @throws WebPolicyTooLargeError (503) when the claim would exceed 2 KiB.
 */
export async function resolveWebPolicy(
  organizationId: string,
): Promise<WebPolicyClaim> {
  if (!ORGANIZATION_ID.test(organizationId)) {
    throw unavailable(organizationId, 'invalid_organization_id');
  }
  const known = cache.get(organizationId);
  if (known && Date.now() - known.resolvedAt < WEB_POLICY_FRESH_TTL_MS) {
    return known.claim;
  }

  let claim: WebPolicyClaim;
  try {
    claim = await lookupOnce(organizationId);
  } catch (error) {
    const failure =
      error instanceof WebPolicyLookupFailure
        ? error
        : new WebPolicyLookupFailure('org_core_unreachable');
    const now = Date.now();
    if (known && now - known.resolvedAt < WEB_POLICY_STALE_MAX_AGE_MS) {
      if (!throttled(`stale:${organizationId}`, now)) {
        logger.warn(
          `serving_stale_web_policy: org-core could not answer (${failure.reason}); serving org=${organizationId} rev=${known.claim.rev} last confirmed ${Math.round((now - known.resolvedAt) / 1000)}s ago`,
        );
      }
      return known.claim;
    }
    throw unavailable(organizationId, failure.reason, failure.detail);
  }

  // Checked before it is remembered: an oversized answer is never served,
  // fresh or stale, and the next mint asks again.
  assertWebPolicyClaimSize(claim);
  remember(organizationId, { claim, resolvedAt: Date.now() });
  return claim;
}

/**
 * What a token for `audience` carries, or `undefined` when it carries none.
 *
 *   - not `quarry` / `browser-broker`: none.
 *   - flag off: `{"v":1,"rev":"0"}`, org-core not asked.
 *   - flag on: the org's policy through `resolveWebPolicy` (errors propagate).
 *
 * The caller has already decided the principal may carry it (every user; a
 * service only for a `tenant` row) and has authorized the caller, so an
 * unauthenticated request never makes Auth Core query org-core.
 */
export async function webPolicyClaimFor(
  audience: string,
  organizationId: string,
): Promise<WebPolicyClaim | undefined> {
  if (!isWebPolicyAudience(audience)) return undefined;
  if (!webPolicyClaimEnabled()) return BASELINE_WEB_POLICY_CLAIM;
  return resolveWebPolicy(organizationId);
}

/** Test-only: clear the process-local state between cases. */
export function resetWebPolicyCacheForTests(): void {
  cache.clear();
  inFlight.clear();
  lastWarnedAt.clear();
}
