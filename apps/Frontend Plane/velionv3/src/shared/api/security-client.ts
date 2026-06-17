import { requestJson } from './http'

export type SecurityDataClass = 'public' | 'organization' | 'customer'
export type UrlReputationProvider = 'google_web_risk' | 'local_feeds' | 'policy_cache'
export type UrlInvestigationProvider = 'urlscan_io'
export type UrlInvestigationVisibility = 'private' | 'unlisted' | 'public'
export type UrlThreatType =
  | 'malware'
  | 'social_engineering'
  | 'unwanted_software'
  | 'potentially_harmful_application'
  | 'unknown'
export type UrlVerdict = 'safe' | 'suspicious' | 'malicious' | 'unknown'

export type UrlReputationCheckRequest = {
  url: string
  allowExternalLookup: true
  dataClass?: SecurityDataClass
  provider?: Extract<UrlReputationProvider, 'google_web_risk'>
  purpose?: 'ingestion_guard' | 'manual_review' | 'model_tool'
  reason?: string
}

export type UrlReputationMatch = {
  provider: UrlReputationProvider
  threatType: UrlThreatType
  expiresAt?: string
}

export type UrlReputationPolicy = {
  externalLookupUsed: boolean
  nextAction: 'allow' | 'warn' | 'block' | 'require_approval'
}

export type UrlReputationCheckResult = {
  id: string
  checkedAt: string
  verdict: UrlVerdict
  provider?: UrlReputationProvider
  matches: UrlReputationMatch[]
  policy?: UrlReputationPolicy
}

export type UrlInvestigationRequest = {
  url: string
  allowExternalSubmission: true
  dataClass?: SecurityDataClass
  provider?: UrlInvestigationProvider
  reason?: string
  tags?: string[]
  visibility?: UrlInvestigationVisibility
}

export type UrlInvestigationResult = {
  id: string
  provider: UrlInvestigationProvider
  status: 'queued' | 'submitted' | 'running' | 'completed' | 'failed' | 'blocked_by_policy'
  visibility: UrlInvestigationVisibility
  resultUrl?: string
  submittedAt?: string
  verdict?: UrlVerdict
}

function orgHeaders(orgId: string): Record<string, string> {
  return { 'x-velion-org-id': orgId }
}

export function checkUrlReputation(
  orgId: string,
  body: UrlReputationCheckRequest,
  signal?: AbortSignal,
): Promise<UrlReputationCheckResult> {
  return requestJson<UrlReputationCheckResult>('/api/v1/security/url-reputation-checks', {
    method: 'POST',
    body: JSON.stringify(body),
    headers: orgHeaders(orgId),
    signal,
  })
}

export function investigateUrl(
  orgId: string,
  body: UrlInvestigationRequest,
  signal?: AbortSignal,
): Promise<UrlInvestigationResult> {
  return requestJson<UrlInvestigationResult>('/api/v1/security/url-investigations', {
    method: 'POST',
    body: JSON.stringify(body),
    headers: orgHeaders(orgId),
    signal,
  })
}
