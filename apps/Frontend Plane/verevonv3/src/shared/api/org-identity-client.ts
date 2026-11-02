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

/**
 * SSO policy and providers (WS05-S7). org-core owns the policy; the gateway
 * applies the same owner/admin and `feature_disabled` rules as the domain routes.
 * The provider client secret is write-only: it is sent once and never returned.
 */

export type SsoPolicyMode = 'off' | 'report' | 'enforce'

export interface SsoProvider {
  provider_id: string
  domain: string
  issuer: string
  type: string
  domain_verified: boolean
}

/** Why `enforce` cannot be switched on yet. */
export type SsoEnforceReason =
  | 'no_verified_domain'
  | 'no_provider'
  | 'sso_not_proven'
  | 'break_glass_invalid'
  | 'break_glass_requires_2fa'
  | 'identity_provider_unavailable'

export interface SsoPolicy {
  mode: SsoPolicyMode
  revision: number
  domains: string[]
  break_glass_user_ids: string[]
  providers: SsoProvider[]
  can_enforce: { ok: boolean; reasons: SsoEnforceReason[] }
}

export interface SsoPolicyUpdate {
  mode: SsoPolicyMode
  breakGlassUserIds: string[]
  expectedRevision: number
}

export interface SsoProviderInput {
  providerId: string
  domain: string
  issuer: string
  clientId: string
  clientSecret: string
}

function ssoPath(orgId: string, tail: string): string {
  return `/api/v1/orgs/${encodeURIComponent(orgId)}/sso/${tail}`
}

export async function getSsoPolicy(orgId: string): Promise<SsoPolicy> {
  return requestJson<SsoPolicy>(ssoPath(orgId, 'policy'))
}

export async function updateSsoPolicy(orgId: string, update: SsoPolicyUpdate): Promise<SsoPolicy> {
  return requestJson<SsoPolicy>(ssoPath(orgId, 'policy'), {
    method: 'PUT',
    body: JSON.stringify({
      mode: update.mode,
      break_glass_user_ids: update.breakGlassUserIds,
      expected_revision: update.expectedRevision,
    }),
  })
}

export async function addSsoProvider(orgId: string, input: SsoProviderInput): Promise<SsoProvider> {
  return requestJson<SsoProvider>(ssoPath(orgId, 'providers'), {
    method: 'POST',
    body: JSON.stringify({
      provider_id: input.providerId,
      domain: input.domain,
      type: 'oidc',
      issuer: input.issuer,
      client_id: input.clientId,
      client_secret: input.clientSecret,
    }),
  })
}

export async function removeSsoProvider(orgId: string, providerId: string): Promise<void> {
  await requestJson<unknown>(ssoPath(orgId, `providers/${encodeURIComponent(providerId)}`), { method: 'DELETE' })
}
