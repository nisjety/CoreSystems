import { describeApiError, type ApiErrorDescription } from '@/shared/i18n/errors'
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
  /** Human-only (WS9.B): the gateway refuses it for agent actors. */
  | { type: 'reload'; ignore_cache?: boolean }

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

/**
 * Quarry's `ChallengeSignal` as it arrives on an observation (snake_case: the gateway
 * passes the observation through). Only `strength === 'strong'` is a verdict that a bot
 * wall stands in front of the page; `weak` and `non_signal` (a login form, a cookie
 * banner, a paywall, a rate limit, a bare status) are ordinary access states. A signal
 * from before the taxonomy has no `strength`. `evidence` is deliberately not typed:
 * it is never shown.
 */
export type BrowserChallengeStrength = 'strong' | 'weak' | 'non_signal'

export type BrowserChallenge = {
  /** Why a strong signal is strong: `captcha_widget`, `waf_interstitial`, `challenge_loop` or `unexpected_bot_error`. */
  block_reason?: string | null
  confidence?: number
  kind: string
  /** `available` while one wait or reload is still allowed, `exhausted` once it is spent. */
  recovery?: string
  requires_escalation?: boolean
  strength?: BrowserChallengeStrength | string
}

export type BrowserObservation = {
  challenge?: BrowserChallenge | null
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

/**
 * Phase 3 continuation: one row of `GET /api/v1/browser/profiles` — a
 * real name + scope, not just a bare id string. Field names stay
 * snake_case (unlike most gateway responses in this file) because this
 * is a raw Quarry-edge pass-through, matching
 * `BrowserProfileRestoreProbe` below.
 */
export type BrowserProfileSummary = {
  profile_id: string
  name?: string | null
  scope: BrowserProfileScope
}

export type BrowserProfileListResponse = {
  profiles: BrowserProfileSummary[]
}

/**
 * A scope a caller may explicitly create/rescope a profile to. Creating
 * or rescoping to `ephemeral` is rejected server-side — ephemeral
 * browsing simply attaches no profile at all.
 */
export type CreatableBrowserProfileScope = Exclude<BrowserProfileScope, 'ephemeral'>

export type BrowserProfileRestoreProbe = {
  cookies_count: number
  has_user_agent: boolean
  has_viewport: boolean
  indexed_db_count: number
  local_storage_count: number
  locale?: string | null
  name?: string | null
  profile_id: string
  restorable: boolean
  scope?: BrowserProfileScope
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
  /** Which takeover the owner is in; counts real control changes. */
  controlEpoch?: number | null
  /** Who drove the action: set by Quarry on timeline items. */
  initiatedBy?: 'human' | 'agent' | 'system' | string | null
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

/** Compact owner audit data from Quarry. This excludes observation bodies,
 * screenshots, raw DevTools payloads, credentials, and browser storage. */
export type BrowserOwnerTimelineItem = {
  id: string
  occurredAt: string
  kind: 'action' | 'control' | 'tab' | 'devtools' | 'lifecycle'
  action?: BrowserAction['type'] | string
  outcome?: 'completed' | 'failed' | 'skipped'
  errorCode?: string | null
  mode?: BrowserControlMode
  initiatedBy?: 'human' | 'agent' | 'system'
  operation?: 'opened' | 'selected' | 'closed'
  tabId?: string | null
  activeTabId?: string | null
  eventCount?: number
  lastSequence?: number
  state?: 'started' | 'closed'
}

export type BrowserOwnerTimelineResponse = {
  items: BrowserOwnerTimelineItem[]
  nextCursor?: string | null
}

export type BrowserTab = {
  active: boolean
  tabId: string
  title?: string | null
  url?: string | null
  /** Who opened the tab (WS8.F). Absent until Quarry reports it; the agent marker needs it. */
  openedBy?: 'human' | 'agent' | null
  tabEpoch?: number
  holder?: 'human' | 'agent'
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
    /** Quarry's control epoch (WS9.I): the compare-and-set token for Take over and Give back. */
    epoch?: number | null
  } | null
  renderMode: BrowserRenderMode
  status: BrowserSessionStatus
  tabs?: BrowserTab[]
  tabsUrl?: string | null
  /** Client-hydrated compact activity from the actor-scoped Quarry timeline.
   * This is intentionally not supplied by the browser-session projection,
   * because it is a separately paginated owner resource. */
  ownerTimeline?: BrowserOwnerTimelineItem[]
  /** Client-visible failure state for the separately fetched owner timeline.
   * This prevents a failed canonical read from being rendered as empty history. */
  ownerTimelineError?: string | null
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
  /** Initial document navigation failed, but the personal browser is still usable. */
  navigationError?: {
    code: 'browser_initial_navigation_failed'
    httpStatus?: number
    redirectCount?: number
  }
  observation?: BrowserObservation | null
  resolvedStartUrl?: string
  session: BrowserSession
}

export type BrowserTabsResponse = {
  session?: BrowserSession
  tab?: BrowserTab | null
  tabs: BrowserTab[]
}

export type BrowserDevtoolsResponse = {
  events: BrowserDevtoolsEvent[]
  /** Resume cursor: the last event the read examined (WS9.C). */
  nextSequence?: number
  session?: BrowserSession
  zdr?: boolean
}

/** A receipt as the browser may see it (WS9.C). Observations and typed text are never sent. */
export type BrowserReceiptView = {
  receiptId: string
  step: number
  startedAt?: string
  finishedAt?: string
  actionType: string
  actionLabel: string
  url?: string
  outcome: { status: string; latencyMs?: number; errorCode?: string }
  costMicroUsd?: number
  actionOutcome?: string
  correctionOf?: string
  initiatedBy?: string
  controlEpoch?: number
}

export type BrowserReceiptsResponse = {
  receipts: BrowserReceiptView[]
  truncated: boolean
  zdr: boolean
}

/** A redacted egress decision (WS9.C). The URL keeps only origin and path. */
export type BrowserEgressReceiptView = {
  sequence: number
  tabId?: string
  method: string
  url: string
  decision: 'allow' | 'block'
  policy: string
  timestampMs: number
}

export type BrowserEgressReceiptsResponse = {
  receipts: BrowserEgressReceiptView[]
  counts: { allow: number; block: number }
  nextSequence: number
  zdr: boolean
}

export type BrowserProcedureStepView = {
  step: number
  /** The action; a typed `text` is replaced by `{ redacted: true, length }`. */
  action: Record<string, unknown>
  expectedOutcome: string
}

export type BrowserProcedureView = {
  procedureId: string
  version: number
  sourceReceiptIds: string[]
  steps: BrowserProcedureStepView[]
}

export type BrowserProcedureQuality = {
  stepCount: number
  verifiedStepCount: number
  promotionAllowed: boolean
  reasons?: string[]
}

export type BrowserProcedureResponse = {
  procedure: BrowserProcedureView
  quality: BrowserProcedureQuality
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
  purpose?: 'personal_browsing' | 'assistant_task'
  persistentProfile?: boolean
  profileId?: string
  /**
   * Phase 3 continuation: explicit scope for the profile this session
   * attaches. Optional — when omitted the gateway infers scope from
   * `profileId`/`persistentProfile` exactly as before.
   */
  scope?: BrowserProfileScope
  url: string
  viewport?: {
    height: number
    width: number
  }
  /**
   * Zero Data Retention mode for this session. Enforced server-side: the
   * gateway rejects (400 `zdr_persistent_profile_forbidden`) any request
   * that sets `zdr: true` together with `profileId` or `persistentProfile`.
   */
  zdr?: boolean
}

/**
 * A failed browser call as the person should see it and as the caller should
 * react to it. The gateway's typed envelope (`{ error: { code, message,
 * details } }`) survives the client as an `ApiError` (`code`, `status`,
 * `retryAfterSeconds`, `resumable`, `blockReason`); this turns it into copy
 * by `code` and tells the cases apart by `kind`:
 *
 * - 401 `unauthenticated`: sign in again (the transport already signals session expiry).
 * - 403 `forbidden`: not allowed; ask an administrator.
 * - 409 `USER_TOOK_OVER` / `browser_human_takeover_active` -> `takeover`: a person
 *   holds the browser, the agent step is not retried.
 * - 403 `browser_site_blocked` / `site_blocked` -> `blocked`: take over or use another source.
 * - 429 `rate_limited`: wait `retryAfterSeconds` (from `Retry-After`), then retry.
 * - 410 / a reaped or lost session -> `session_lost`: start a new session.
 * - 5xx `server`: a backing service failed; retry later.
 *
 * An unknown code is never shown: it falls back to `fallback`, then to a
 * generic message. The raw `message` of the response is never used.
 */
export function describeBrowserError(
  err: unknown,
  tr: (noText: string, enText: string) => string,
  fallback?: { no: string; en: string },
): ApiErrorDescription {
  return describeApiError(err, tr, fallback)
}

/** The text of {@link describeBrowserError}, for a toast or an inline notice. */
export function browserErrorMessage(
  err: unknown,
  tr: (noText: string, enText: string) => string,
  fallback?: { no: string; en: string },
): string {
  return describeBrowserError(err, tr, fallback).message
}

function orgHeaders(orgId: string): Record<string, string> {
  return { 'x-verevon-org-id': orgId }
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

/**
 * Takes over or hands back browser control. With `expectedEpoch` the change is a
 * compare-and-set (WS9.I): when the run has moved on the gateway answers 409
 * `browser_control_epoch_stale` and nothing changes.
 */
export async function setBrowserControlMode(
  orgId: string,
  sessionId: string,
  mode: BrowserControlMode,
  options?: { expectedEpoch?: number | null; signal?: AbortSignal },
): Promise<BrowserSessionResponse> {
  const body: { mode: BrowserControlMode; expectedEpoch?: number } = { mode }
  if (typeof options?.expectedEpoch === 'number') body.expectedEpoch = options.expectedEpoch
  return requestJson<BrowserSessionResponse>(`/api/v1/browser/sessions/${encodeURIComponent(sessionId)}/control`, {
    method: 'POST',
    body: JSON.stringify(body),
    headers: orgHeaders(orgId),
    signal: options?.signal,
  })
}

/** Extends a person's browser session grant by one broker interval (WS8.D). Returns only the new expiry. */
export async function keepAliveBrowserSession(
  orgId: string,
  sessionId: string,
  signal?: AbortSignal,
): Promise<{ expiresAt: string | null }> {
  return requestJson<{ expiresAt: string | null }>(
    `/api/v1/browser/sessions/${encodeURIComponent(sessionId)}/keepalive`,
    { method: 'POST', headers: orgHeaders(orgId), signal },
  )
}

/** Reads one browser session projection (WS9.I: the refetch after a stale control epoch). */
export async function getBrowserSession(
  orgId: string,
  sessionId: string,
  signal?: AbortSignal,
): Promise<BrowserSessionResponse> {
  // Unlike create/control/select, the owner GET deliberately returns Quarry's
  // raw camelCase projection. Validate and adapt it at the API boundary so
  // stale-control recovery and browser handoff consume the same shape.
  const value = await requestJson<unknown>(`/api/v1/browser/sessions/${encodeURIComponent(sessionId)}`, {
    method: 'GET',
    headers: orgHeaders(orgId),
    signal,
  })
  return browserSessionFromOwnerProjection(value, sessionId)
}

export function browserSessionFromOwnerProjection(value: unknown, expectedSessionId: string): BrowserSessionResponse {
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error('Invalid browser owner projection')
  const raw = value as Record<string, unknown>
  if (raw.runId !== expectedSessionId || typeof raw.live !== 'boolean' ||
    (raw.controlMode !== 'agent_control' && raw.controlMode !== 'human_takeover') ||
    (!Number.isSafeInteger(raw.controlEpoch) || (raw.controlEpoch as number) < 0)) {
    throw new Error('Invalid browser owner projection')
  }
  const viewport = raw.viewport && typeof raw.viewport === 'object' ? raw.viewport as Record<string, unknown> : null
  if (!viewport || typeof viewport.width !== 'number' || !Number.isFinite(viewport.width) || viewport.width <= 0 ||
    typeof viewport.height !== 'number' || !Number.isFinite(viewport.height) || viewport.height <= 0 || typeof raw.zdr !== 'boolean') {
    throw new Error('Invalid browser owner projection')
  }
  const width = viewport.width, height = viewport.height
  const scopes: BrowserProfileScope[] = ['ephemeral', 'org_shared', 'run_scoped', 'user_private']
  if (typeof raw.profileScope !== 'string' || !scopes.includes(raw.profileScope as BrowserProfileScope) ||
    (raw.profileStorage !== 'ephemeral' && raw.profileStorage !== 'persistent')) throw new Error('Invalid browser owner projection')
  const scope = raw.profileScope as BrowserProfileScope
  const tabs = Array.isArray(raw.tabs) ? raw.tabs.filter((item): item is BrowserTab => Boolean(item && typeof item === 'object' && typeof item.tabId === 'string' && typeof item.active === 'boolean')) : []
  const url = typeof raw.currentUrl === 'string' ? raw.currentUrl : ''
  return {
    session: {
      id: expectedSessionId,
      status: raw.live ? 'live' : raw.status === 'closed' ? 'closed' : 'degraded',
      renderMode: 'chromium',
      control: { mode: raw.controlMode, epoch: typeof raw.controlEpoch === 'number' ? raw.controlEpoch : null },
      profile: { id: typeof raw.profileId === 'string' ? raw.profileId : null, scope, storage: raw.profileStorage === 'persistent' ? 'persistent' : 'isolated' },
      viewport: { width, height },
      title: tabs.find(tab => tab.active)?.title ?? url,
      url, tabs,
      leaseId: typeof raw.leaseId === 'string' ? raw.leaseId : null,
      // The raw projection is not the BFF capability envelope. Keep this empty;
      // a normalized mutation response supplies the authoritative capabilities.
      capabilities: [],
      zdr: raw.zdr,
    },
    observation: raw.lastObservation && typeof raw.lastObservation === 'object' ? raw.lastObservation as BrowserObservation : null,
  }
}

/** Read the actor-scoped, append-only browser timeline from its Quarry owner.
 * `nextCursor` is passed back unchanged to continue a stable page. */
export async function getBrowserOwnerTimeline(
  orgId: string,
  sessionId: string,
  options?: { cursor?: string; limit?: number },
  signal?: AbortSignal,
): Promise<BrowserOwnerTimelineResponse> {
  const params = new URLSearchParams()
  if (options?.cursor) params.set('cursor', options.cursor)
  if (typeof options?.limit === 'number') params.set('limit', String(options.limit))
  const query = params.toString()
  return requestJson<BrowserOwnerTimelineResponse>(
    `/api/v1/browser/sessions/${encodeURIComponent(sessionId)}/timeline${query ? `?${query}` : ''}`,
    {
      method: 'GET',
      headers: orgHeaders(orgId),
      signal,
    },
  )
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

/** Filters for the devtools read (WS9.C). Levels are normalised by the server. */
export type BrowserDevtoolsFilterOptions = {
  afterSequence?: number
  limit?: number
  levels?: string[]
  filter?: string
  category?: 'console' | 'network' | 'lifecycle'
  tabId?: string
}

export async function getBrowserDevtoolsEvents(
  orgId: string,
  sessionId: string,
  options?: BrowserDevtoolsFilterOptions,
  signal?: AbortSignal,
): Promise<BrowserDevtoolsResponse> {
  const params = new URLSearchParams()
  if (typeof options?.afterSequence === 'number') params.set('afterSequence', String(options.afterSequence))
  if (typeof options?.limit === 'number') params.set('limit', String(options.limit))
  if (options?.levels && options.levels.length > 0) params.set('levels', options.levels.join(','))
  if (options?.filter) params.set('filter', options.filter)
  if (options?.category) params.set('category', options.category)
  if (options?.tabId) params.set('tabId', options.tabId)
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

/** One event of a run's trace as the events stream sends it (WS10.S3). `seq` is per run. */
export type BrowserTraceEvent = {
  event_id: string
  run_id?: string | null
  job_id?: string | null
  /** The envelope's snake_case type (`action_started`); the SSE name is in `onEvent`. */
  type: string
  ts: string
  seq: number
  payload: Record<string, unknown>
  idempotency_key: string
}

/** The resume point is older than the ring: read the durable timeline for the missed part. */
export type BrowserReplayGap = {
  requested: number
  oldest_retained: number
  fallback: string
}

export type BrowserEventsStreamOptions = {
  /** Replay the retained events after this seq before the live tail. */
  fromSeq?: number
  /** Stop after this many user-facing events. */
  maxEvents?: number
}

export type BrowserEventsStreamHandlers = {
  /** One user-facing event in seq order; `name` is the SSE event name (e.g. `action.started`). */
  onEvent: (name: string, event: BrowserTraceEvent) => void
  onReplayGap?: (gap: BrowserReplayGap) => void
  /** The stream is finished: `run_closed` or `max_events`. */
  onDone?: (reason: string) => void
  /** A connection problem. EventSource reconnects on its own with `Last-Event-ID`. */
  onError?: () => void
}

const BROWSER_EVENT_NAMES = [
  'agent.started',
  'action.started',
  'action.completed',
  'action.failed',
  'observation.ready',
  'agent.completed',
  'agent.expired',
  'agent.failed',
] as const

function parseSseJson<T>(raw: string): T | null {
  try {
    return JSON.parse(raw) as T
  } catch {
    return null
  }
}

/**
 * Open the typed event stream of a browser session's run (WS10.S3). Cookie-authenticated
 * like the live-frame stream, since EventSource cannot send the org header. The browser
 * reconnects with `Last-Event-ID`, which the gateway forwards, so a reconnect continues
 * after the last seq it saw. Returns a handle that closes the stream.
 */
export function streamBrowserEvents(
  sessionId: string,
  options: BrowserEventsStreamOptions,
  handlers: BrowserEventsStreamHandlers,
): { close: () => void } {
  const params = new URLSearchParams()
  if (typeof options.fromSeq === 'number') params.set('from_seq', String(options.fromSeq))
  if (typeof options.maxEvents === 'number') params.set('max_events', String(options.maxEvents))
  const query = params.toString()
  const source = new EventSource(
    `/api/v1/browser/sessions/${encodeURIComponent(sessionId)}/events/stream${query ? `?${query}` : ''}`,
  )
  for (const name of BROWSER_EVENT_NAMES) {
    source.addEventListener(name, (message) => {
      const event = parseSseJson<BrowserTraceEvent>((message as MessageEvent<string>).data)
      if (event) handlers.onEvent(name, event)
    })
  }
  source.addEventListener('replay_gap', (message) => {
    const gap = parseSseJson<BrowserReplayGap>((message as MessageEvent<string>).data)
    if (gap) handlers.onReplayGap?.(gap)
  })
  source.addEventListener('done', (message) => {
    handlers.onDone?.((message as MessageEvent<string>).data)
    source.close()
  })
  source.onerror = () => handlers.onError?.()
  return { close: () => source.close() }
}

/** The receipts of a session, newest `limit` only (WS9.C). Refused for ZDR sessions. */
export async function listBrowserReceipts(
  orgId: string,
  sessionId: string,
  options?: { limit?: number },
  signal?: AbortSignal,
): Promise<BrowserReceiptsResponse> {
  const params = new URLSearchParams()
  if (typeof options?.limit === 'number') params.set('limit', String(options.limit))
  const query = params.toString()
  return requestJson<BrowserReceiptsResponse>(
    `/api/v1/browser/sessions/${encodeURIComponent(sessionId)}/receipts${query ? `?${query}` : ''}`,
    {
      method: 'GET',
      headers: orgHeaders(orgId),
      signal,
    },
  )
}

/** Redacted live egress decisions after a cursor (WS9.C). Allowed for ZDR sessions. */
export async function listBrowserEgressReceipts(
  orgId: string,
  sessionId: string,
  options?: { afterSequence?: number; limit?: number },
  signal?: AbortSignal,
): Promise<BrowserEgressReceiptsResponse> {
  const params = new URLSearchParams()
  if (typeof options?.afterSequence === 'number') params.set('afterSequence', String(options.afterSequence))
  if (typeof options?.limit === 'number') params.set('limit', String(options.limit))
  const query = params.toString()
  return requestJson<BrowserEgressReceiptsResponse>(
    `/api/v1/browser/sessions/${encodeURIComponent(sessionId)}/egress-receipts${query ? `?${query}` : ''}`,
    {
      method: 'GET',
      headers: orgHeaders(orgId),
      signal,
    },
  )
}

/** Compile and promotion-check a replay procedure from the session's receipts (WS9.C). */
export async function compileBrowserProcedure(
  orgId: string,
  sessionId: string,
  body?: { procedureId?: string },
  signal?: AbortSignal,
): Promise<BrowserProcedureResponse> {
  return requestJson<BrowserProcedureResponse>(
    `/api/v1/browser/sessions/${encodeURIComponent(sessionId)}/procedure`,
    {
      method: 'POST',
      body: JSON.stringify(body ?? {}),
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

/**
 * Phase 3 continuation: explicitly create a named, scoped profile (as
 * opposed to attaching one implicitly via `persistentProfile`/`profileId`
 * at session-create time). Returns the created profile's summary so the
 * caller can select it immediately.
 */
export async function createBrowserProfile(
  orgId: string,
  body: { name?: string; scope: CreatableBrowserProfileScope },
  signal?: AbortSignal,
): Promise<BrowserProfileSummary> {
  return requestJson<BrowserProfileSummary>('/api/v1/browser/profiles', {
    method: 'POST',
    body: JSON.stringify(body),
    headers: orgHeaders(orgId),
    signal,
  })
}

/**
 * Phase 3 continuation: rename and/or rescope an existing profile. At
 * least one of `name`/`scope` must be provided.
 */
export async function renameBrowserProfile(
  orgId: string,
  profileId: string,
  body: { name?: string; scope?: CreatableBrowserProfileScope },
  signal?: AbortSignal,
): Promise<BrowserProfileSummary> {
  return requestJson<BrowserProfileSummary>(
    `/api/v1/browser/profiles/${encodeURIComponent(profileId)}`,
    {
      method: 'PATCH',
      body: JSON.stringify(body),
      headers: orgHeaders(orgId),
      signal,
    },
  )
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
