import { describeApiError, type ApiErrorDescription } from '@/shared/i18n/errors'
import type {
  BrowserRegionAttachment,
  RegionAttachRequest,
  RegionImageMimeType,
} from '@/features/browser/page-context/region-attach'
import { ApiError, requestJson } from './http'

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
  /** Saves the page as a PDF artifact (E5.1, behind VITE_BROWSER_PDF). The gateway refuses it for zero-retention sessions. */
  | { type: 'pdf' }

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
  /** The artifact of the page's PDF capture (a `pdf` action, E5.1); absent unless the page was saved as a PDF. */
  pdf_artifact_id?: string | null
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
 * Who started a browser run, as Quarry's trace label `runOrigin` (WS10.S4). A label for the run's trace,
 * never authority: no UI treats `user` as a person's authority, and no UI reads it yet.
 */
export type BrowserRunOrigin = 'user' | 'agent' | 'schedule' | 'webhook' | 'api'
/** The five run-origin labels: the closed set `browserRunOriginFromWire` accepts. */
const BROWSER_RUN_ORIGINS: readonly BrowserRunOrigin[] = ['user', 'agent', 'schedule', 'webhook', 'api']

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
  /** True when the edge serves clear-data (F-t2-068). Absent or false keeps the clear control hidden. */
  clear_data_enabled?: boolean
}

/**
 * A scope a caller may explicitly create/rescope a profile to. Creating
 * or rescoping to `ephemeral` is rejected server-side — ephemeral
 * browsing simply attaches no profile at all.
 */
export type CreatableBrowserProfileScope = Exclude<BrowserProfileScope, 'ephemeral'>

/** F-t2-068: what a clear of one profile's browsing data removes, by kind. Counts only. */
export type BrowserProfileBrowsingDataCounts = {
  cookies: number
  local_storage_entries: number
  session_storage_entries: number
  indexed_db_rows: number
  origins: number
}

/**
 * F-t2-068: the answer to a clear. A dry run changes nothing; `cleared` is true only when this
 * call removed data (false on a dry run, and when nothing was there).
 */
export type BrowserProfileClearResult = {
  profile_id: string
  dry_run: boolean
  cleared: boolean
  counts: BrowserProfileBrowsingDataCounts
}

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
  /** The page's PDF capture for this step (E5.1), when the step was a `pdf` action. */
  pdfArtifactId?: string | null
  pdfUrl?: string | null
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
  /** The page's PDF capture for this event (E5.1), when the event was a `pdf` action. */
  pdfArtifactId?: string | null
  pdfUrl?: string | null
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
  /**
   * What the attached agent marked the tab for (Q2-WS8.I): `deliverable` (a result the person keeps)
   * or `handoff` (for the person to carry on). A marked tab stays open when its attachment ends; an
   * unmarked one is closed. Absent for a tab without a mark.
   */
  mark?: 'deliverable' | 'handoff'
  /** The attachment this agent tab belongs to (Q2-WS8.I), in its wire shape. Absent for a tab outside an attachment. */
  attachId?: string
}

/**
 * The agent attached to a browser session (Q2-WS8.I) as the gateway allow-lists it: the attachment's
 * `state` (`attached`, or the word it ended in: `released`, `aborted`, `revoked`, `idle_released` or
 * `session_ended`) and the tab it works in. The attach grant id is never part of it.
 */
export type BrowserSessionAttach = {
  state: string
  tabId?: string
  tabEpoch?: number
}

/**
 * The run's step budget (Q2-WS9.H) as Quarry's owner projection counts it: the steps taken (`used`) and the
 * most the run may take (`max`). Counts only. Present only when the run's step cap binds a person's steps (an
 * agent-class run); a person's own session is never capped, so it has no budget.
 */
export type BrowserStepBudget = {
  used: number
  max: number
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
  /** The agent attached to this session (Q2-WS8.I); null when none is attached. */
  attach?: BrowserSessionAttach | null
  /** The run's step budget (Q2-WS9.H), rebuilt strictly; absent or null when no step cap binds the person's steps. Counts only. */
  stepBudget?: BrowserStepBudget | null
  /** Who started the run (Quarry's `runOrigin`, WS10.S4): one of the five labels, or absent. A label, never authority. */
  runOrigin?: BrowserRunOrigin
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
  /** Version 2 only: the element a semantic step acts on, by its locator alone. */
  target?: { locator: Record<string, unknown> }
  /** Version 2 only: the name of the slot this step types into. A name, never a value. */
  slot?: string
  /** Version 2 only: the postcondition Quarry verified for this step; every condition in `all` held. */
  expect?: { all: Array<Record<string, unknown>> }
}

/** An open input of a version 2 procedure, by name and kind only. A replay supplies its value. */
export type BrowserProcedureSlotView = {
  name: string
  kind: 'text' | 'secret'
}

export type BrowserProcedureView = {
  procedureId: string
  version: number
  sourceReceiptIds: string[]
  steps: BrowserProcedureStepView[]
  /** Version 2 only: the open inputs of the procedure. Absent for version 1. */
  slots?: BrowserProcedureSlotView[]
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
  const runOrigin = browserRunOriginFromWire(raw.runOrigin)
  // The owner GET forwards Quarry's raw projection, so the budget is read from its `step` and `maxSteps`.
  const stepBudget = browserStepBudgetFromWire({ used: raw.step, max: raw.maxSteps })
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
      attach: browserSessionAttachFromWire(raw.attach),
      ...(runOrigin ? { runOrigin } : {}),
      ...(stepBudget ? { stepBudget } : {}),
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

/** The wire shape of a tab or attachment id: 3 to 128 letters, digits, `_` or `-` (the gateway's own rule). */
const BROWSER_WIRE_ID_RE = /^[A-Za-z0-9_-]{3,128}$/

/** Whether a value has the wire shape of a tab or attachment id (Q2-WS8.I). */
export function isBrowserWireId(value: unknown): value is string {
  return typeof value === 'string' && BROWSER_WIRE_ID_RE.test(value)
}

/** The attach states the gateway passes through (Q2-WS8.I). Any other word means no attachment. */
const BROWSER_ATTACH_STATES: readonly string[] = [
  'attached',
  'released',
  'aborted',
  'revoked',
  'idle_released',
  'session_ended',
]

/**
 * The session's attachment rebuilt field by field (Q2-WS8.I). A state outside the closed set means no
 * attachment; `tabId` and `tabEpoch` are kept only in their wire shape. Every other key is dropped, so
 * the attach grant id is never copied.
 */
export function browserSessionAttachFromWire(value: unknown): BrowserSessionAttach | null {
  const wire = recordOf(value)
  const state = wire?.state
  if (typeof state !== 'string' || !BROWSER_ATTACH_STATES.includes(state)) return null
  const tabId = wire?.tabId
  const tabEpoch = wire?.tabEpoch
  return {
    state,
    ...(isBrowserWireId(tabId) ? { tabId } : {}),
    ...(typeof tabEpoch === 'number' && Number.isSafeInteger(tabEpoch) && tabEpoch >= 0 ? { tabEpoch } : {}),
  }
}

/**
 * The step budget rebuilt strictly (Q2-WS9.H): two non-negative safe integers. A `used` above `max` is
 * clamped to `max`. Anything else is `null`, so no budget is shown and no count is guessed.
 */
export function browserStepBudgetFromWire(value: unknown): BrowserStepBudget | null {
  const wire = recordOf(value)
  const used = wire?.used
  const max = wire?.max
  if (typeof used !== 'number' || typeof max !== 'number') return null
  if (!Number.isSafeInteger(used) || !Number.isSafeInteger(max) || used < 0 || max < 0) return null
  return { used: Math.min(used, max), max }
}

/**
 * A run origin rebuilt strictly (WS10.S4): exactly one of the five labels, case-sensitive and untrimmed.
 * Anything else (absent, `null`, another word) is `undefined`, so no origin is ever guessed.
 */
export function browserRunOriginFromWire(value: unknown): BrowserRunOrigin | undefined {
  return BROWSER_RUN_ORIGINS.find((origin) => origin === value)
}

/** The answer to one tab's control change (Q2-WS8.I): the tab, who holds it now, and its new epoch. */
export type BrowserTabControlResult = {
  tabId: string
  holder: 'human' | 'agent'
  tabEpoch: number | null
}

/**
 * Hands one tab of a browser session to the attached agent (`holder: 'agent'`, Give back) or takes it
 * back (`holder: 'human'`, Take over): `POST /api/v1/browser/sessions/{id}/tabs/{tabId}/control`
 * (Q2-WS8.I). With `expectedEpoch` the change is a compare-and-set on the tab's epoch: a tab that changed
 * hands since is refused and nothing moves. Only the person's own token may change who holds a tab (403
 * `browser_tab_control_forbidden`).
 */
export async function controlBrowserTab(
  orgId: string,
  sessionId: string,
  tabId: string,
  holder: 'human' | 'agent',
  expectedEpoch?: number,
  signal?: AbortSignal,
): Promise<BrowserTabControlResult> {
  const body: { holder: 'human' | 'agent'; expectedEpoch?: number } = { holder }
  if (typeof expectedEpoch === 'number') body.expectedEpoch = expectedEpoch
  return requestJson<BrowserTabControlResult>(
    `/api/v1/browser/sessions/${encodeURIComponent(sessionId)}/tabs/${encodeURIComponent(tabId)}/control`,
    {
      method: 'POST',
      body: JSON.stringify(body),
      headers: orgHeaders(orgId),
      signal,
    },
  )
}

/**
 * Stops the browser agent attached to a session (Stop agent, Q2-WS8.I):
 * `POST /api/v1/browser/sessions/{id}/attach/stop`. The gateway revokes the agent's access and ends its
 * attachment; the answer says only that the stop went through.
 */
export async function stopBrowserAttach(
  orgId: string,
  sessionId: string,
  signal?: AbortSignal,
): Promise<{ stopped: boolean }> {
  return requestJson<{ stopped: boolean }>(
    `/api/v1/browser/sessions/${encodeURIComponent(sessionId)}/attach/stop`,
    {
      method: 'POST',
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

/** A slot a saved recipe asks for: its name, and its kind (a typed text, or a secret a person supplies). */
export type BrowserRecipeSlot = { name: string; kind: 'text' | 'secret' }

/**
 * One saved recipe in the list (Q4-E4.2-4a, `GET /api/v1/browser/recipes`): its metadata and its newest
 * version number. The list carries no body and no `slots`: the panel reads them per recipe with
 * {@link getBrowserRecipe} (QF-29) before it can start a run.
 */
export type BrowserSavedRecipe = {
  recipeId: string
  name: string
  description?: string
  sourceRunId?: string
  latestVersion: number
  createdAt?: string
  updatedAt?: string
  slots?: BrowserRecipeSlot[]
}

/** One page of the saved recipes: `nextAfterId` is set while older recipes remain. */
export type BrowserSavedRecipesResponse = {
  recipes: BrowserSavedRecipe[]
  nextAfterId?: string
}

/** The caller's saved recipes, one page at a time (Q4-E4.2-4a). Mounted only with `BROWSER_RECIPES` on. */
export async function listBrowserRecipes(
  orgId: string,
  options?: { limit?: number; afterId?: string },
  signal?: AbortSignal,
): Promise<BrowserSavedRecipesResponse> {
  const params = new URLSearchParams()
  if (typeof options?.limit === 'number') params.set('limit', String(options.limit))
  if (options?.afterId) params.set('afterId', options.afterId)
  const query = params.toString()
  return requestJson<BrowserSavedRecipesResponse>(`/api/v1/browser/recipes${query ? `?${query}` : ''}`, {
    method: 'GET',
    headers: orgHeaders(orgId),
    signal,
  })
}

/**
 * One saved recipe's runnable shape (QF-29, `GET /api/v1/browser/recipes/{recipeId}`): the version read
 * and that version's slot names and kinds. It never carries a step body, a selector or a URL.
 */
export type BrowserRecipeDetail = {
  recipeId: string
  name?: string
  version: number
  slots: BrowserRecipeSlot[]
}

/** One saved recipe's slots, for its newest version or for `version` (QF-29). Mounted only with `BROWSER_RECIPES` on. */
export async function getBrowserRecipe(
  orgId: string,
  recipeId: string,
  options?: { version?: number },
  signal?: AbortSignal,
): Promise<BrowserRecipeDetail> {
  const query = typeof options?.version === 'number' ? `?version=${encodeURIComponent(String(options.version))}` : ''
  return requestJson<BrowserRecipeDetail>(`/api/v1/browser/recipes/${encodeURIComponent(recipeId)}${query}`, {
    method: 'GET',
    headers: orgHeaders(orgId),
    signal,
  })
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

/**
 * F-t2-068: remove a stored profile's cookies, web storage and IndexedDB. `dryRun` defaults to
 * true on the server as well: a call that does not pass `dryRun: false` only counts. The edge
 * answers 409 `browser_profile_in_use` while a browser session holds the profile.
 */
export async function clearBrowserProfileData(
  orgId: string,
  profileId: string,
  options: { dryRun: boolean },
  signal?: AbortSignal,
): Promise<BrowserProfileClearResult> {
  return requestJson<BrowserProfileClearResult>(
    `/api/v1/browser/profiles/${encodeURIComponent(profileId)}/clear-data`,
    {
      method: 'POST',
      body: JSON.stringify({ dryRun: options.dryRun }),
      headers: orgHeaders(orgId),
      signal,
    },
  )
}

/** One `@tab` mention as the composer sends it (E3.3-c): the token as typed and the tab id it spells (null for the bare `@tab`). */
export type BrowserTabMentionInput = {
  token?: string
  tabId: string | null
}

/**
 * One tab the gateway resolved for a turn (E3.3-c). It carries no page content and no grant:
 * `trustClass` is always `browser-scraped` and `grant` is always `none`. `label`, `title` and
 * `target` are page-derived text and are never trusted.
 */
export type BrowserTabContextRef = {
  kind: 'browser-tab'
  sessionId: string
  tabId: string
  label: string
  labelSource: 'page' | 'url' | 'tab-id'
  title: string | null
  /** Origin and path only. Null when the tab has no http(s) URL. */
  target: string | null
  trustClass: 'browser-scraped'
  grant: 'none'
}

/** Most mentions the gateway resolves in one call. */
export const BROWSER_TAB_MENTIONS_MAX = 8

const BROWSER_TAB_REF_LABEL_SOURCES: readonly string[] = ['page', 'url', 'tab-id']

function browserTabRefFromWire(value: unknown, sessionId: string): BrowserTabContextRef | null {
  if (!value || typeof value !== 'object') return null
  const ref = value as Record<string, unknown>
  if (ref.kind !== 'browser-tab' || ref.trustClass !== 'browser-scraped' || ref.grant !== 'none') return null
  if (ref.sessionId !== sessionId) return null
  if (typeof ref.tabId !== 'string' || !ref.tabId) return null
  if (typeof ref.label !== 'string' || typeof ref.labelSource !== 'string') return null
  if (!BROWSER_TAB_REF_LABEL_SOURCES.includes(ref.labelSource)) return null
  return {
    kind: 'browser-tab',
    sessionId,
    tabId: ref.tabId,
    label: ref.label,
    labelSource: ref.labelSource as BrowserTabContextRef['labelSource'],
    title: typeof ref.title === 'string' ? ref.title : null,
    target: typeof ref.target === 'string' ? ref.target : null,
    trustClass: 'browser-scraped',
    grant: 'none',
  }
}

/**
 * Has the gateway re-resolve a turn's `@tab` mentions against Quarry's live tabs (E3.3-c;
 * `POST /api/v1/browser/sessions/{id}/context/tab-mentions`, mounted only with
 * `BROWSER_TAB_MENTIONS`). Resolves to the refs, each one a context reference without page
 * content or a grant. The call is all-or-nothing: if any mention does not resolve to exactly one
 * live tab, the session is zero-retention, the flag is off or the caller does not own the session,
 * it rejects with the typed {@link ApiError} (`browser_tab_mention_unresolved`,
 * `browser_tab_context_refused_zdr`, 400/404/429 ...) and nothing may be attached or sent.
 * An answer that is not a non-empty list of well-formed refs for this session also rejects.
 */
export async function resolveBrowserTabMentions(
  sessionId: string,
  mentions: readonly BrowserTabMentionInput[],
  signal?: AbortSignal,
): Promise<BrowserTabContextRef[]> {
  const value = await requestJson<{ refs?: unknown }>(
    `/api/v1/browser/sessions/${encodeURIComponent(sessionId)}/context/tab-mentions`,
    {
      method: 'POST',
      body: JSON.stringify({
        mentions: mentions.map((mention) => ({
          ...(mention.token === undefined ? {} : { token: mention.token }),
          tabId: mention.tabId,
        })),
      }),
      signal,
    },
  )
  const wire = Array.isArray(value?.refs) ? value.refs : []
  const refs = wire.map((item) => browserTabRefFromWire(item, sessionId))
  if (refs.length === 0 || refs.length > BROWSER_TAB_MENTIONS_MAX || refs.some((ref) => ref === null)) {
    throw new ApiError('browser_tab_mention_response_invalid', 502, 'browser_tab_mention_response_invalid')
  }
  return refs as BrowserTabContextRef[]
}

/** The image types the gateway accepts for a region (E3.3-d). */
const REGION_IMAGE_MIME_TYPES: readonly string[] = ['image/png', 'image/jpeg']

/**
 * The region attach token as the gateway issues it (QF-8): 16-512 characters of
 * `[A-Za-z0-9_.-]` (`<expires_unix>.<base64url HMAC>`). Anything else is no token.
 */
const REGION_ATTACH_TOKEN_RE = /^[A-Za-z0-9_.-]{16,512}$/

function regionAttachTokenOf(value: unknown): string | null {
  return typeof value === 'string' && REGION_ATTACH_TOKEN_RE.test(value) ? value : null
}

function recordOf(value: unknown): Record<string, unknown> | null {
  return value && typeof value === 'object' && !Array.isArray(value) ? (value as Record<string, unknown>) : null
}

function finiteNumberOf(record: Record<string, unknown> | null, key: string): number | null {
  const value = record?.[key]
  return typeof value === 'number' && Number.isFinite(value) ? value : null
}

/**
 * The region attachment as the gateway answered it, rebuilt field by field (as
 * {@link resolveBrowserTabMentions} rebuilds its refs). It is refused unless it has the
 * browser-scraped, grant-none, ephemeral shape for this session, so a malformed answer never
 * reaches the composer.
 */
function browserRegionAttachmentFromWire(value: unknown, sessionId: string): BrowserRegionAttachment | null {
  const wire = recordOf(value)
  if (!wire || wire.kind !== 'browser-region' || wire.trustClass !== 'browser-scraped' || wire.grant !== 'none') return null
  if (wire.retention !== 'ephemeral' || wire.sessionId !== sessionId) return null
  const tabId = wire.tabId
  const label = wire.label
  if (typeof tabId !== 'string' || !tabId || typeof label !== 'string') return null
  const region = recordOf(wire.region)
  const viewport = recordOf(wire.viewport)
  const image = recordOf(wire.image)
  const x = finiteNumberOf(region, 'x')
  const y = finiteNumberOf(region, 'y')
  const width = finiteNumberOf(region, 'width')
  const height = finiteNumberOf(region, 'height')
  const viewportWidth = finiteNumberOf(viewport, 'width')
  const viewportHeight = finiteNumberOf(viewport, 'height')
  const imageWidth = finiteNumberOf(image, 'width')
  const imageHeight = finiteNumberOf(image, 'height')
  const byteLength = finiteNumberOf(image, 'byteLength')
  if (x === null || y === null || width === null || height === null) return null
  if (viewportWidth === null || viewportHeight === null) return null
  if (imageWidth === null || imageHeight === null || byteLength === null) return null
  const mimeType = image?.mimeType
  const dataBase64 = image?.dataBase64
  if (typeof mimeType !== 'string' || !REGION_IMAGE_MIME_TYPES.includes(mimeType)) return null
  if (typeof dataBase64 !== 'string' || !dataBase64) return null
  const selection = recordOf(wire.selection)
  const selectionText = selection?.text
  const selectionLength = finiteNumberOf(selection, 'length')
  return {
    kind: 'browser-region',
    trustClass: 'browser-scraped',
    grant: 'none',
    retention: 'ephemeral',
    sessionId,
    tabId,
    label,
    title: typeof wire.title === 'string' ? wire.title : null,
    target: typeof wire.target === 'string' ? wire.target : null,
    region: { x, y, width, height },
    viewport: { width: viewportWidth, height: viewportHeight },
    image: {
      mimeType: mimeType as RegionImageMimeType,
      dataBase64,
      width: imageWidth,
      height: imageHeight,
      byteLength,
    },
    selection: typeof selectionText === 'string' && selectionLength !== null
      ? { text: selectionText, length: selectionLength }
      : null,
    attachToken: regionAttachTokenOf(wire.attachToken),
  }
}

/**
 * Attaches a region of the live frame (E3.3-d): `POST /api/v1/browser/sessions/{id}/attachments/region`,
 * mounted only with `BROWSER_REGION_ATTACH` (flag off: 404). The gateway re-resolves the tab and the
 * region itself and answers an ephemeral `browser-scraped` attachment that grants nothing and stores
 * nothing. A zero-data-retention session is refused with 409 `browser_attachment_refused_zdr`; that
 * refusal names no feature, so the rejected {@link ApiError} carries a null `feature`. An answer without
 * the expected shape for this session rejects as `browser_region_attach_response_invalid`, and nothing
 * is attached.
 */
export async function attachBrowserRegion(
  sessionId: string,
  request: RegionAttachRequest,
  signal?: AbortSignal,
): Promise<BrowserRegionAttachment> {
  const value = await requestJson<{ attachment?: unknown }>(
    `/api/v1/browser/sessions/${encodeURIComponent(sessionId)}/attachments/region`,
    { method: 'POST', body: JSON.stringify(request), signal },
  )
  const attachment = browserRegionAttachmentFromWire(value?.attachment, sessionId)
  if (!attachment) {
    throw new ApiError('browser_region_attach_response_invalid', 502, 'browser_region_attach_response_invalid')
  }
  return attachment
}

/**
 * One admitted download of a browser session, as the gateway lists it (Q3-E3.4-7b). It carries no URL and no
 * file name. `handoffState` is Quarry's hand-off state: `handed_off` once the owner holds the file's text.
 */
export type BrowserDownloadListEntry = {
  artifactId: string
  mimeType: string
  sizeBytes: number
  sha256: string
  admittedType: 'pdf' | 'txt' | 'md' | 'csv'
  handoffState: 'not_handed_off' | 'handed_off'
}

const DOWNLOAD_ADMITTED_TYPES: readonly string[] = ['pdf', 'txt', 'md', 'csv']
const DOWNLOAD_HANDOFF_STATES: readonly string[] = ['not_handed_off', 'handed_off']

/** One list entry rebuilt field by field: an entry without the expected shape is dropped. */
function browserDownloadEntryFromWire(value: unknown): BrowserDownloadListEntry | null {
  const wire = recordOf(value)
  const sizeBytes = finiteNumberOf(wire, 'sizeBytes')
  if (!wire || sizeBytes === null || sizeBytes < 0) return null
  const { artifactId, mimeType, sha256, admittedType, handoffState } = wire
  if (typeof artifactId !== 'string' || !artifactId) return null
  if (typeof mimeType !== 'string' || typeof sha256 !== 'string' || !/^[0-9a-f]{64}$/.test(sha256)) return null
  if (typeof admittedType !== 'string' || !DOWNLOAD_ADMITTED_TYPES.includes(admittedType)) return null
  if (typeof handoffState !== 'string' || !DOWNLOAD_HANDOFF_STATES.includes(handoffState)) return null
  return {
    artifactId,
    mimeType,
    sizeBytes,
    sha256,
    admittedType: admittedType as BrowserDownloadListEntry['admittedType'],
    handoffState: handoffState as BrowserDownloadListEntry['handoffState'],
  }
}

/**
 * The admitted downloads of a browser session: `GET /api/v1/browser/sessions/{id}/downloads` (Q3-E3.4-7b).
 * The gateway answers an empty list for a zero-retention session and when nothing was downloaded. A 404
 * `browser_downloads_unavailable` (the BFF switch is off) and a 403 also resolve to an empty list, so the
 * drawer stays hidden. Any other failure rejects.
 */
export async function listBrowserDownloads(
  sessionId: string,
  signal?: AbortSignal,
): Promise<BrowserDownloadListEntry[]> {
  let value: unknown
  try {
    value = await requestJson<unknown>(`/api/v1/browser/sessions/${encodeURIComponent(sessionId)}/downloads`, {
      method: 'GET',
      signal,
    })
  } catch (error) {
    if (error instanceof ApiError && (error.code === 'browser_downloads_unavailable' || error.status === 403)) return []
    throw error
  }
  if (!Array.isArray(value)) return []
  return value
    .map(browserDownloadEntryFromWire)
    .filter((entry): entry is BrowserDownloadListEntry => entry !== null)
}
