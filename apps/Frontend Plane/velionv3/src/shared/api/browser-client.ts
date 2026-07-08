import { requestJson } from './http'

export type BrowserAction =
  | { type: 'navigate'; url: string }
  | { type: 'click'; selector: string }
  | { type: 'click_point'; x: number; y: number }
  | { type: 'type'; selector: string; text: string }
  | { type: 'press'; key: string }
  | { type: 'scroll'; target: string }
  | { type: 'mouse_wheel'; x: number; y: number; delta_x: number; delta_y: number }
  | { type: 'select'; selector: string; value: string }
  | { type: 'wait'; ms: number }
  | { type: 'wait_for'; selector: string; timeout_ms: number }
  | { type: 'screenshot'; full_page: boolean }
  | { type: 'back' }
  | { type: 'forward' }
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
  visual_observation_artifact_id?: string | null
}

export type BrowserDevtoolsEvent = {
  category: 'console' | 'lifecycle' | 'network' | string
  level?: string | null
  method?: string | null
  name: string
  payload?: unknown
  sequence: number
  status?: number | null
  tabId?: string | null
  text?: string | null
  timestampMs: number
  url?: string | null
}

export type BrowserDevtoolsState = {
  eventCount?: number
  events: BrowserDevtoolsEvent[]
  lastSequence?: number | null
}

export type BrowserProfileScope = 'ephemeral' | 'user_private' | 'org_shared' | 'run_scoped'
export type BrowserRenderMode = 'chromium' | 'dom_snapshot' | 'readability_fallback'
export type BrowserSessionStatus = 'live' | 'degraded' | 'closed'
export type BrowserControlMode = 'agent_control' | 'human_takeover'
export type BrowserActionActor = 'agent' | 'human'

export type BrowserProfileListResponse = {
  profiles: string[]
}

export type BrowserProfileRestoreProbe = {
  cookies_count: number
  has_user_agent: boolean
  has_viewport: boolean
  indexed_db_count: number
  local_storage_count: number
  locale?: string | null
  profile_id: string
  restorable: boolean
  session_storage_count: number
  timezone?: string | null
  url: string
}

export type BrowserFrame = {
  artifactId?: string | null
  kind: 'screenshot'
  mediaType?: string | null
  url?: string | null
}

export type BrowserVisualEvidence = {
  observationArtifactId?: string | null
  observationUrl?: string | null
}

export type BrowserTimelineEntry = {
  consoleSummary?: Array<{ level: string; text: string }> | null
  domInteractiveCount?: number | null
  domNodeCount?: number | null
  networkSummary?: Array<{ content_type?: string | null; method: string; status: number; url: string }> | null
  observedAt?: string | null
  policyDenials?: string[] | null
  screenshotArtifactId?: string | null
  screenshotUrl?: string | null
  step: number
  title?: string | null
  url?: string | null
  visualObservationArtifactId?: string | null
  visualObservationUrl?: string | null
}

export type BrowserReplayEvent = {
  action?: unknown
  actionType?: string | null
  activeTab?: BrowserTab | null
  actor?: BrowserActionActor | 'system' | string | null
  consoleCount?: number | null
  controlMode?: BrowserControlMode | string | null
  domInteractiveCount?: number | null
  domNodeCount?: number | null
  id: string
  dataBase64Length?: number | null
  eventCount?: number | null
  kind: 'observation' | 'control' | 'tab' | string
  lastSequence?: number | null
  mimeType?: string | null
  networkCount?: number | null
  observedAt?: string | null
  operation?: 'new' | 'select' | 'close' | string | null
  persisted?: boolean | null
  policyDenialCount?: number | null
  screenshotArtifactId?: string | null
  screenshotUrl?: string | null
  sequence?: number | null
  step?: number | null
  tabId?: string | null
  transport?: string | null
  transient?: boolean | null
  timestampMs?: number | null
  title?: string | null
  url?: string | null
  visualObservationArtifactId?: string | null
  visualObservationUrl?: string | null
  zdr?: boolean
}

export type BrowserReplay = {
  eventCount?: number
  events: BrowserReplayEvent[]
}

export type BrowserTab = {
  active: boolean
  tabId: string
  title?: string | null
  url?: string | null
}

export type BrowserSession = {
  capabilities: string[]
  frame?: BrowserFrame | null
  id: string
  leaseId?: string | null
  liveFrameStreamUrl?: string | null
  liveFrameUrl?: string | null
  liveFrameWsUrl?: string | null
  devtoolsUrl?: string | null
  devtools?: BrowserDevtoolsState | null
  profile: {
    id?: string | null
    scope: BrowserProfileScope
    storage: 'isolated' | 'persistent'
  }
  control?: {
    mode: BrowserControlMode
  } | null
  renderMode: BrowserRenderMode
  status: BrowserSessionStatus
  tabs?: BrowserTab[]
  tabsUrl?: string | null
  timeline?: BrowserTimelineEntry[]
  replay?: BrowserReplay | null
  title: string
  url: string
  visual?: BrowserVisualEvidence | null
  viewport: {
    height: number
    width: number
  }
  /** Zero Data Retention marker as reported by the gateway for this run. */
  zdr?: boolean
}

export type BrowserSessionResponse = {
  observation?: BrowserObservation | null
  session: BrowserSession
}

export type BrowserTabsResponse = {
  session?: BrowserSession
  tab?: BrowserTab | null
  tabs: BrowserTab[]
}

export type BrowserDevtoolsResponse = {
  events: BrowserDevtoolsEvent[]
  session?: BrowserSession
  zdr?: boolean
}

export type BrowserSuggestedAction = {
  action?: BrowserAction | null
  confidence?: number | null
  done?: boolean
  reason?: string | null
}

export type BrowserActionSuggestionResponse = {
  id?: string
  model_used?: string
  object?: string
  suggestion: BrowserSuggestedAction
  usage?: {
    input_tokens?: number
    output_tokens?: number
  }
  visual_summary?: string | null
}

export type CreateBrowserSessionRequest = {
  persistentProfile?: boolean
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
  options?: { actor?: BrowserActionActor },
  signal?: AbortSignal,
): Promise<BrowserSessionResponse> {
  return requestJson<BrowserSessionResponse>(`/api/v1/browser/sessions/${encodeURIComponent(sessionId)}/actions`, {
    method: 'POST',
    body: JSON.stringify({ action, actor: options?.actor ?? 'human' }),
    headers: orgHeaders(orgId),
    signal,
  })
}

export async function setBrowserControlMode(
  orgId: string,
  sessionId: string,
  mode: BrowserControlMode,
  signal?: AbortSignal,
): Promise<BrowserSessionResponse> {
  return requestJson<BrowserSessionResponse>(`/api/v1/browser/sessions/${encodeURIComponent(sessionId)}/control`, {
    method: 'POST',
    body: JSON.stringify({ mode }),
    headers: orgHeaders(orgId),
    signal,
  })
}

export async function listBrowserTabs(
  orgId: string,
  sessionId: string,
  signal?: AbortSignal,
): Promise<BrowserTabsResponse> {
  return requestJson<BrowserTabsResponse>(`/api/v1/browser/sessions/${encodeURIComponent(sessionId)}/tabs`, {
    method: 'GET',
    headers: orgHeaders(orgId),
    signal,
  })
}

export async function createBrowserTab(
  orgId: string,
  sessionId: string,
  body?: { url?: string | null },
  signal?: AbortSignal,
): Promise<BrowserTabsResponse> {
  return requestJson<BrowserTabsResponse>(`/api/v1/browser/sessions/${encodeURIComponent(sessionId)}/tabs`, {
    method: 'POST',
    body: JSON.stringify(body ?? {}),
    headers: orgHeaders(orgId),
    signal,
  })
}

export async function selectBrowserTab(
  orgId: string,
  sessionId: string,
  tabId: string,
  signal?: AbortSignal,
): Promise<BrowserTabsResponse> {
  return requestJson<BrowserTabsResponse>(
    `/api/v1/browser/sessions/${encodeURIComponent(sessionId)}/tabs/${encodeURIComponent(tabId)}/select`,
    {
      method: 'POST',
      headers: orgHeaders(orgId),
      signal,
    },
  )
}

export async function closeBrowserTab(
  orgId: string,
  sessionId: string,
  tabId: string,
  signal?: AbortSignal,
): Promise<BrowserTabsResponse> {
  return requestJson<BrowserTabsResponse>(
    `/api/v1/browser/sessions/${encodeURIComponent(sessionId)}/tabs/${encodeURIComponent(tabId)}`,
    {
      method: 'DELETE',
      headers: orgHeaders(orgId),
      signal,
    },
  )
}

export async function getBrowserDevtoolsEvents(
  orgId: string,
  sessionId: string,
  options?: { afterSequence?: number; limit?: number },
  signal?: AbortSignal,
): Promise<BrowserDevtoolsResponse> {
  const params = new URLSearchParams()
  if (typeof options?.afterSequence === 'number') params.set('afterSequence', String(options.afterSequence))
  if (typeof options?.limit === 'number') params.set('limit', String(options.limit))
  const query = params.toString()
  return requestJson<BrowserDevtoolsResponse>(
    `/api/v1/browser/sessions/${encodeURIComponent(sessionId)}/devtools${query ? `?${query}` : ''}`,
    {
      method: 'GET',
      headers: orgHeaders(orgId),
      signal,
    },
  )
}

export async function suggestBrowserAction(
  orgId: string,
  sessionId: string,
  body: { goal?: string; includeScreenshot?: boolean },
  signal?: AbortSignal,
): Promise<BrowserActionSuggestionResponse> {
  return requestJson<BrowserActionSuggestionResponse>(
    `/api/v1/browser/sessions/${encodeURIComponent(sessionId)}/suggestions`,
    {
      method: 'POST',
      body: JSON.stringify(body),
      headers: orgHeaders(orgId),
      signal,
    },
  )
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

export async function listBrowserProfiles(
  orgId: string,
  signal?: AbortSignal,
): Promise<BrowserProfileListResponse> {
  return requestJson<BrowserProfileListResponse>('/api/v1/browser/profiles', {
    headers: orgHeaders(orgId),
    signal,
  })
}

export async function probeBrowserProfile(
  orgId: string,
  profileId: string,
  url: string,
  signal?: AbortSignal,
): Promise<BrowserProfileRestoreProbe> {
  return requestJson<BrowserProfileRestoreProbe>(
    `/api/v1/browser/profiles/${encodeURIComponent(profileId)}/restore-probe`,
    {
      method: 'POST',
      body: JSON.stringify({ url }),
      headers: orgHeaders(orgId),
      signal,
    },
  )
}

export async function deleteBrowserProfile(
  orgId: string,
  profileId: string,
  signal?: AbortSignal,
): Promise<{ deleted: boolean }> {
  return requestJson<{ deleted: boolean }>(
    `/api/v1/browser/profiles/${encodeURIComponent(profileId)}`,
    {
      method: 'DELETE',
      headers: orgHeaders(orgId),
      signal,
    },
  )
}
