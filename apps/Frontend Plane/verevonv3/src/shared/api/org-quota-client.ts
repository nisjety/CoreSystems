import { requestJson } from './http'

/**
 * One org quota row, mirroring org-core's `org.Quota`.
 *
 * `limit` is the settable ceiling. `value` is accumulated usage owned by
 * whatever meters it — raising a limit never resets it, which is why this
 * client has no way to write `value`.
 */
export interface OrgQuota {
  org_id: string
  key: string
  value: number
  limit: number
  reset_period: string
  last_reset_at?: string | null
  updated_at?: string
}

/**
 * The quota keys the Model Plane actually enforces, mirroring model-gateway's
 * `org_quota.rs` constants. The gateway rejects any other key rather than
 * storing a ceiling nothing reads.
 *
 * Cost is stored in MICRO-dollars because `org_quotas.quota_limit` is BIGINT —
 * the key names its unit so the conversion cannot be forgotten at a call site.
 */
export const QUOTA_KEY_MAX_COST_PER_RUN_USD_MICROS = 'max_cost_per_run_usd_micros'
export const QUOTA_KEY_MAX_TOKENS_PER_RUN = 'max_tokens_per_run'

export type QuotaResetPeriod = 'daily' | 'monthly' | 'none'

const MICROS_PER_USD = 1_000_000

/** Micro-dollars to USD, for display. */
export function microsToUsd(micros: number): number {
  return micros / MICROS_PER_USD
}

/**
 * USD to micro-dollars, for storage. Rounded rather than truncated so a
 * displayed value round-trips: entering the value shown must not silently
 * shave a fraction of a cent off the ceiling each time it is saved.
 */
export function usdToMicros(usd: number): number {
  return Math.round(usd * MICROS_PER_USD)
}

export function listOrgQuotas(orgId: string): Promise<{ organization_id: string; quotas: OrgQuota[] }> {
  return requestJson<{ organization_id: string; quotas: OrgQuota[] }>(
    `/api/v1/orgs/${encodeURIComponent(orgId)}/quotas`,
  )
}

/**
 * Set one quota's ceiling. Zero is a legitimate limit meaning "no allowance",
 * so it must be sent explicitly rather than treated as unset.
 */
export function setOrgQuota(
  orgId: string,
  key: string,
  body: { limit: number; reset_period?: QuotaResetPeriod },
): Promise<{ organization_id: string; quota: OrgQuota }> {
  return requestJson<{ organization_id: string; quota: OrgQuota }>(
    `/api/v1/orgs/${encodeURIComponent(orgId)}/quotas/${encodeURIComponent(key)}`,
    {
      method: 'PUT',
      body: JSON.stringify(body),
    },
  )
}
