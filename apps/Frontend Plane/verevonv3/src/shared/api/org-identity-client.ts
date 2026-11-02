import { requestJson } from './http'

/**
 * Organization identity client (WS05-S6): DNS-proven company domains.
 *
 * org-core owns the proof; the gateway takes the organization from the session
 * path, refuses anyone but a live owner or admin (typed 403 `identity_admin_required`)
 * and answers 404 `feature_disabled` while `ORG_IDENTITY_ADMIN_API` is off.
 * Human sessions only: none of these operations is model-eligible.
 */

export type OrgDomainStatus = 'pending' | 'verified'

export interface OrgDomainChallenge {
  type: 'dns_txt'
  /** The DNS name to publish the TXT record at. */
  name: string
  /** The TXT record value. A public challenge, not a secret. */
  value: string
  expires_at: string
}

export interface OrgDomain {
  domain: string
  status: OrgDomainStatus
  verification_method?: string
  verified_at?: string
  auto_invite_enabled: boolean
  last_check_at?: string
  last_check_reason?: string
  challenge?: OrgDomainChallenge
}

/** The closed set of reasons a verification attempt did not prove the domain. */
export type OrgDomainCheckReason = 'txt_not_found' | 'txt_mismatch' | 'dns_unavailable' | 'challenge_expired'

export interface OrgDomainVerifyResult {
  domain: string
  status: OrgDomainStatus
  checked_at: string
  reason?: OrgDomainCheckReason
}

function domainsPath(orgId: string): string {
  return `/api/v1/orgs/${encodeURIComponent(orgId)}/domains`
}

export async function listOrgDomains(orgId: string): Promise<OrgDomain[]> {
  const data = await requestJson<{ domains?: OrgDomain[] }>(domainsPath(orgId))
  return Array.isArray(data?.domains) ? data.domains : []
}

export async function addOrgDomain(orgId: string, domain: string): Promise<OrgDomain> {
  return requestJson<OrgDomain>(domainsPath(orgId), {
    method: 'POST',
    body: JSON.stringify({ domain }),
  })
}

export async function verifyOrgDomain(orgId: string, domain: string): Promise<OrgDomainVerifyResult> {
  return requestJson<OrgDomainVerifyResult>(`${domainsPath(orgId)}/${encodeURIComponent(domain)}/verify`, {
    method: 'POST',
  })
}

export async function revokeOrgDomain(orgId: string, domain: string): Promise<void> {
  await requestJson<unknown>(`${domainsPath(orgId)}/${encodeURIComponent(domain)}`, { method: 'DELETE' })
}
