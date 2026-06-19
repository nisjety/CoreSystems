import { requestJson } from './http'

export type BrowserAction =
  | { type: 'navigate'; url: string }
  | { type: 'click'; selector: string }
  | { type: 'type'; selector: string; text: string }
  | { type: 'press'; key: string }
  | { type: 'scroll'; target: string }
  | { type: 'select'; selector: string; value: string }
  | { type: 'wait'; ms: number }
  | { type: 'wait_for'; selector: string; timeout_ms: number }
  | { type: 'screenshot'; full_page: boolean }
  | { type: 'back' }
  | { type: 'get_content' }

export type BrowserInteractiveElement = {
  role?: string | null
  selector: string
  tag: string
  text?: string | null
}

export type BrowserDomSummary = {
  interactive_elements: BrowserInteractiveElement[]
  node_count: number
  text_snippet?: string | null
}

export type BrowserObservation = {
  console_summary?: Array<{ level: string; text: string }>
  dom_summary?: BrowserDomSummary | null
  network_summary?: Array<{ content_type?: string | null; method: string; status: number; url: string }>
  observed_at?: string
  policy_denials?: string[]
  run_id: string
  screenshot_artifact_id?: string | null
  step: number
  title?: string | null
  url: string
}

export type BrowserProfileScope = 'ephemeral' | 'user_private' | 'org_shared' | 'run_scoped'
export type BrowserRenderMode = 'chromium' | 'dom_snapshot' | 'readability_fallback'
export type BrowserSessionStatus = 'live' | 'degraded' | 'closed'

export type BrowserFrame = {
  artifactId?: string | null
  kind: 'screenshot'
  mediaType?: string | null
  url?: string | null
}

export type BrowserSession = {
  capabilities: string[]
  frame?: BrowserFrame | null
  id: string
  leaseId?: string | null
  profile: {
    id?: string | null
    scope: BrowserProfileScope
    storage: 'isolated' | 'persistent'
  }
  renderMode: BrowserRenderMode
  status: BrowserSessionStatus
  title: string
  url: string
  viewport: {
    height: number
    width: number
  }
}

export type BrowserSessionResponse = {
  observation?: BrowserObservation | null
  session: BrowserSession
}

export type CreateBrowserSessionRequest = {
  profileId?: string
  url: string
  viewport?: {
    height: number
    width: number
  }
}

function orgHeaders(orgId: string): Record<string, string> {
  return { 'x-velion-org-id': orgId }
}

export async function createBrowserSession(
  orgId: string,
  body: CreateBrowserSessionRequest,
  signal?: AbortSignal,
): Promise<BrowserSessionResponse> {
  return requestJson<BrowserSessionResponse>('/api/v1/browser/sessions', {
    method: 'POST',
    body: JSON.stringify(body),
    headers: orgHeaders(orgId),
    signal,
  })
}

export async function runBrowserAction(
  orgId: string,
  sessionId: string,
  action: BrowserAction,
  signal?: AbortSignal,
): Promise<BrowserSessionResponse> {
  return requestJson<BrowserSessionResponse>(`/api/v1/browser/sessions/${encodeURIComponent(sessionId)}/actions`, {
    method: 'POST',
    body: JSON.stringify({ action }),
    headers: orgHeaders(orgId),
    signal,
  })
}

export async function closeBrowserSession(
  orgId: string,
  sessionId: string,
  signal?: AbortSignal,
): Promise<{ closed: boolean }> {
  return requestJson<{ closed: boolean }>(`/api/v1/browser/sessions/${encodeURIComponent(sessionId)}`, {
    method: 'DELETE',
    headers: orgHeaders(orgId),
    signal,
  })
}
