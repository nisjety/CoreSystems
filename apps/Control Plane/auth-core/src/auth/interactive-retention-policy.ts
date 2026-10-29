/**
 * Control-Plane-owned retention posture for interactive user tokens.
 *
 * Zero Data Retention (ZDR) is an OPT-IN, PAID, plan-gated add-on — never the
 * standard. The default for every organization, including brand-new orgs and
 * any org whose retention intent cannot be resolved, is normal retention
 * (`zdr: false`). An org gets `zdr: true` only when BOTH of the following are
 * independently verified against org-core, the sole authority for
 * organization state:
 *
 *   1. Stored intent — the org explicitly toggled ZDR on via the self-serve
 *      settings endpoint. Persisted at
 *      `organizations.metadata.interactiveRetention.zdr` by org-core's
 *      `SetInteractiveRetention` (`org-core/internal/org/repository.go`) and
 *      already returned by the existing `GET /api/v1/organizations/:id`
 *      response (`Organization.Metadata`).
 *   2. Plan entitlement — the org's current plan is entitled to ZDR. Rather
 *      than reimplementing org-core's plan allowlist here (which would drift
 *      out of sync with `org-core/internal/org/types.go`'s
 *      `zeroDataRetentionPlans`), this reuses org-core's own server-computed
 *      `feature.zero_data_retention` entitlement from
 *      `GET /api/v1/organizations/:id/entitlements`
 *      (`org-core/internal/org/service_enhanced.go#PlanAllowsZeroDataRetention`).
 *
 * This is defense-in-depth: org-core already refuses to persist a `zdr: true`
 * intent for a non-qualifying plan (`SetInteractiveRetention` returns
 * `ErrPlanUpgradeRequired`), but a single layer is never trusted alone for a
 * security-sensitive default in this project, so Auth Core independently
 * re-verifies the plan entitlement rather than trusting the stored intent.
 *
 * Fail-closed contract: any missing/absent metadata, invalid organization id,
 * org-core call failure, timeout, or non-2xx response resolves to
 * `zdr: false` (normal retention). This resolver never throws — an org-core
 * outage must never block interactive token issuance (login), and it must
 * never fail OPEN to `zdr: true` either.
 *
 * A short-TTL process-local cache avoids hammering org-core on every
 * token-mint (which happens on every login/refresh for every interactive
 * request path).
 */

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

/** Simple process-local TTL cache — single-instance-per-container service, no distributed cache needed. */
const CACHE_TTL_MS = 45_000;
/** Bounds a single org-core round trip so a stalled dependency can't stall token issuance. */
const LOOKUP_TIMEOUT_MS = 2_000;

type CacheEntry = Readonly<{
  posture: InteractiveRetentionPosture;
  expiresAt: number;
}>;

const postureCache = new Map<string, CacheEntry>();

function orgServiceBaseUrl(): string {
  // Same env var + default as the existing synchronous org-core client
  // pattern in `organizations.controller.ts`.
  return (process.env.ORG_SERVICE_URL ?? 'http://org-core:8080').replace(
    /\/$/,
    '',
  );
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === 'object' && !Array.isArray(value);
}

/**
 * `Array.isArray` narrows to `any[]` in TS's lib types, which would make a
 * downstream `.find()` return `any` and trip `no-unsafe-assignment`. Route
 * through an explicit `unknown[]` assertion so callers get a clean,
 * still-unchecked-per-element array type.
 */
function asUnknownArray(value: unknown): unknown[] | undefined {
  return Array.isArray(value) ? (value as unknown[]) : undefined;
}

async function fetchOrgCoreJson(path: string): Promise<unknown> {
  const response = await fetch(`${orgServiceBaseUrl()}${path}`, {
    signal: AbortSignal.timeout(LOOKUP_TIMEOUT_MS),
  });
  if (!response.ok) {
    throw new Error(`org-core responded ${response.status} for ${path}`);
  }
  return response.json();
}

/**
 * Read the org's stored interactive-retention intent straight from the
 * existing organization record — no new org-core endpoint required.
 */
async function storedZdrIntent(organizationId: string): Promise<boolean> {
  const body = await fetchOrgCoreJson(
    `/api/v1/organizations/${encodeURIComponent(organizationId)}`,
  );
  if (!isRecord(body) || !isRecord(body.metadata)) {
    return false;
  }
  const interactiveRetention = body.metadata.interactiveRetention;
  return isRecord(interactiveRetention) && interactiveRetention.zdr === true;
}

/**
 * Independently re-verify the plan entitlement via org-core's own
 * server-computed boolean instead of reimplementing the plan allowlist here.
 */
async function planEntitlesZdr(organizationId: string): Promise<boolean> {
  const body = await fetchOrgCoreJson(
    `/api/v1/organizations/${encodeURIComponent(organizationId)}/entitlements`,
  );
  const entitlements = isRecord(body)
    ? asUnknownArray(body.entitlements)
    : undefined;
  if (!entitlements) {
    return false;
  }
  const entry = entitlements.find(
    (candidate) =>
      isRecord(candidate) && candidate.key === 'feature.zero_data_retention',
  );
  return isRecord(entry) && entry.enabled === true;
}

/**
 * Resolve the effective interactive-token retention posture for an
 * organization. `zdr: true` only when the stored intent AND the
 * independently-verified plan entitlement both hold; every other case
 * (missing data, invalid id, org-core error/timeout/non-2xx) fails closed to
 * `zdr: false`. Never throws.
 */
export async function resolveInteractiveRetentionPosture(
  organizationId: string,
): Promise<InteractiveRetentionPosture> {
  if (!ORGANIZATION_ID.test(organizationId)) {
    return DEFAULT_POSTURE;
  }

  const now = Date.now();
  const cached = postureCache.get(organizationId);
  if (cached && cached.expiresAt > now) {
    return cached.posture;
  }

  let posture = DEFAULT_POSTURE;
  try {
    const [intent, entitled] = await Promise.all([
      storedZdrIntent(organizationId),
      planEntitlesZdr(organizationId),
    ]);
    posture = intent && entitled ? ZDR_POSTURE : DEFAULT_POSTURE;
  } catch {
    // org-core unreachable/timeout/non-2xx/malformed body — fail closed.
    posture = DEFAULT_POSTURE;
  }

  postureCache.set(organizationId, { posture, expiresAt: now + CACHE_TTL_MS });
  return posture;
}

/** Test-only: clear the process-local posture cache between test cases. */
export function resetInteractiveRetentionCacheForTests(): void {
  postureCache.clear();
}
