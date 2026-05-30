/**
 * Fetches organization plan, quotas, and entitlements from org-core.
 *
 * Used by entitlement guards to enforce plan-level authorization on
 * connect-session and connection routes.
 */
import { HttpError } from '../http/http-error';

// ─── Types ────────────────────────────────────────────────────────────────────

export type PlanTier = 'free' | 'starter' | 'pro' | 'enterprise';

export interface QuotaInfo {
  key: string;
  value: number;
  limit: number;
  resetPeriod: string;
}

export interface OrgPlan {
  plan: PlanTier;
  quotas: Record<string, QuotaInfo>;
  entitlements: Record<string, boolean>;
}

export interface OrgClient {
  getOrgPlan(orgId: string, userId: string): Promise<OrgPlan>;
}

// ─── org-core response schema ─────────────────────────────────────────────────

interface OrgCoreOrgResponse {
  id: string;
  name: string;
  plan: string;
  status: string;
  quotas?: Array<{
    key: string;
    value: number;
    limit: number;
    reset_period: string;
  }>;
  entitlements?: Array<{
    key: string;
    enabled: boolean;
  }>;
  [key: string]: unknown;
}

// ─── Implementation ───────────────────────────────────────────────────────────

export class HttpOrgClient implements OrgClient {
  constructor(
    private readonly orgCoreBaseUrl: string,
    private readonly internalApiKey: string,
    private readonly timeoutMs: number = 5_000
  ) {}

  async getOrgPlan(orgId: string, userId: string): Promise<OrgPlan> {
    const url = `${this.orgCoreBaseUrl.replace(/\/$/, '')}/org/${encodeURIComponent(orgId)}`;

    let response: Response;

    try {
      response = await fetch(url, {
        method: 'GET',
        headers: {
          'x-user-id': userId,
          'x-internal-api-key': this.internalApiKey
        },
        signal: AbortSignal.timeout(this.timeoutMs)
      });
    } catch (cause) {
      throw new HttpError(
        503,
        'org_core_unreachable',
        'Unable to reach org-core for plan lookup',
        { cause: cause instanceof Error ? cause.message : String(cause) }
      );
    }

    if (!response.ok) {
      if (response.status === 404) {
        throw new HttpError(404, 'org_not_found', `Organization ${orgId} not found in org-core`);
      }
      throw new HttpError(
        502,
        'org_core_error',
        `org-core returned HTTP ${response.status}`
      );
    }

    let body: OrgCoreOrgResponse;

    try {
      body = (await response.json()) as OrgCoreOrgResponse;
    } catch {
      throw new HttpError(502, 'org_core_bad_response', 'org-core returned non-JSON response');
    }

    const plan = normalizePlan(body.plan);

    const quotas: Record<string, QuotaInfo> = {};
    if (Array.isArray(body.quotas)) {
      for (const q of body.quotas) {
        quotas[q.key] = {
          key: q.key,
          value: q.value,
          limit: q.limit,
          resetPeriod: q.reset_period
        };
      }
    }

    const entitlements: Record<string, boolean> = {};
    if (Array.isArray(body.entitlements)) {
      for (const e of body.entitlements) {
        entitlements[e.key] = e.enabled;
      }
    }

    return { plan, quotas, entitlements };
  }
}

function normalizePlan(raw: string | undefined): PlanTier {
  const lower = (raw ?? '').toLowerCase().trim();
  if (lower === 'pro' || lower === 'enterprise' || lower === 'starter') {
    return lower;
  }
  return 'free';
}
