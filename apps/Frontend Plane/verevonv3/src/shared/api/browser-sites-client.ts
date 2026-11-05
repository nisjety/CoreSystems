import { requestJson } from './http'

export type BrowserSiteLevel = 'none' | 'read' | 'act'
export type BrowserSiteMode = 'open' | 'allowlist'

export interface BrowserSiteChoice {
  host: string
  level: BrowserSiteLevel
  expires_at: string
  updated_at: string
}

export interface BrowserSiteExplanation {
  organization_id: string
  user_id: string
  revision: number
  mode: BrowserSiteMode
  decision: {
    host: string
    effective: 'deny' | 'ask' | 'read' | 'act'
    cap: BrowserSiteLevel
    source: 'org_block' | 'org_cap' | 'user_pref' | 'default'
  }
  org_rule: string
  user_level: string
  user_expires_at: string | null
}

export function browserSitesPath(orgId: string): string {
  return `/api/v1/orgs/${encodeURIComponent(orgId)}/browser-sites`
}

export function explainBrowserSite(orgId: string, host: string, signal?: AbortSignal): Promise<BrowserSiteExplanation> {
  return requestJson<BrowserSiteExplanation>(`${browserSitesPath(orgId)}/explain?host=${encodeURIComponent(host)}`, { signal })
}

export function setMyBrowserSiteChoice(
  orgId: string, host: string, expectedRevision: number, level: BrowserSiteLevel, expiresAt?: string, signal?: AbortSignal,
): Promise<BrowserSiteChoice> {
  return requestJson<BrowserSiteChoice>(`${browserSitesPath(orgId)}/me/${encodeURIComponent(host)}`, {
    method: 'PUT',
    body: JSON.stringify({ expected_revision: expectedRevision, level, expires_at: expiresAt }),
    signal,
  })
}
