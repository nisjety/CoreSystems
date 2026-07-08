import type {
  BrowserControlMode,
  BrowserDevtoolsEvent,
  BrowserObservation,
  BrowserReplayEvent,
  BrowserSession,
  BrowserTab,
  BrowserTabsResponse,
  BrowserTimelineEntry,
  BrowserSessionResponse,
} from '@/shared/api/browser-client'
import type {
  BrowserActionApprovalRequiredEvent,
  BrowserActionDecidedEvent,
} from '@/shared/api/run-console-client'
import { gatewayBaseUrl } from '@/shared/api/config'
import { hostnameOf, type ScrapePreview } from './knowledge-preview'

export type BrowserSurfaceMode = BrowserSession['renderMode']

export type BrowserDomNode = {
  id: string
  kind: string
  selector?: string
  text: string
}

export type BrowserConsoleEntry = {
  level: string
  text: string
}

export type BrowserNetworkEntry = {
  content_type?: string | null
  method: string
  status: number
  url: string
}

export type BrowserTimelineViewEntry = BrowserTimelineEntry & {
  screenshotUrl?: string | null
  visualObservationUrl?: string | null
}

export type BrowserReplayViewEvent = BrowserReplayEvent & {
  screenshotUrl?: string | null
  visualObservationUrl?: string | null
}

export type BrowserSessionViewModel = {
  capabilities: string[]
  commentAnchors: Array<{
    id: string
    label: string
    x: number
    y: number
  }>
  consoleEntries: BrowserConsoleEntry[]
  domNodes: BrowserDomNode[]
  degradedReason?: string | null
  frameArtifactId?: string | null
  frameMediaType?: string | null
  frameUrl?: string | null
  host: string
  controlMode: BrowserControlMode
  devtoolsEvents: BrowserDevtoolsEvent[]
  devtoolsUrl?: string | null
  liveFrameStreamUrl?: string | null
  liveFrameUrl?: string | null
  liveFrameWsUrl?: string | null
  networkEntries: BrowserNetworkEntry[]
  nodeCount?: number | null
  observation?: BrowserObservation | null
  policyDenials: string[]
  profileId: string | null
  profileLabel: string
  profileScope: BrowserSession['profile']['scope']
  profileStorage: BrowserSession['profile']['storage']
  renderMode: BrowserSurfaceMode
  replayEvents: BrowserReplayViewEvent[]
  screenshotArtifactId?: string | null
  sessionId?: string
  sourceLabel: string
  status: BrowserSession['status']
  tabs: BrowserTab[]
  tabsUrl?: string | null
  timeline: BrowserTimelineViewEntry[]
  title: string
  url: string
  visualObservationArtifactId?: string | null
  visualObservationUrl?: string | null
  viewport: {
    height: number
    width: number
  }
  zdr: boolean
}

const fallbackViewport = { width: 1280, height: 800 }
const maxBrowserTimelineEntries = 32
const maxBrowserReplayEvents = 96

export function browserSessionFromPreview(preview: ScrapePreview): BrowserSessionViewModel {
  const mode = preview.browserSession?.session.renderMode ?? 'readability_fallback'
  const observation = preview.browserSession?.observation ?? null
  const session = preview.browserSession?.session
  const frameUrl = resolveBrowserArtifactUrl(session?.frame?.url)
  const liveFrameStreamUrl = resolveBrowserArtifactUrl(session?.liveFrameStreamUrl)
  const liveFrameUrl = resolveBrowserArtifactUrl(session?.liveFrameUrl)
  const liveFrameWsUrl = resolveBrowserWsUrl(session?.liveFrameWsUrl)
  const devtoolsUrl = resolveBrowserArtifactUrl(session?.devtoolsUrl)
  const timeline = normalizeTimeline(session?.timeline)
  const replayEvents = normalizeReplayEvents(session?.replay?.events)
  const tabs = normalizeBrowserTabs(session?.tabs, session, preview)
  const visualObservationArtifactId =
    session?.visual?.observationArtifactId ?? observation?.visual_observation_artifact_id ?? null
  const visualObservationUrl =
    resolveBrowserArtifactUrl(session?.visual?.observationUrl)
    ?? resolveVisualObservationUrl(session?.id, visualObservationArtifactId)

  return {
    capabilities: session?.capabilities ?? ['annotate', 'dom_select', 'knowledge_ingest'],
    commentAnchors: [
      {
        id: 'comment-1',
        label: '1',
        x: 0.07,
        y: 0.18,
      },
    ],
    consoleEntries: observation?.console_summary ?? [],
    domNodes: observation?.dom_summary
      ? domNodesFromObservation(observation)
      : domNodesFromPreview(preview),
    degradedReason: preview.browserSessionError ?? null,
    frameArtifactId: session?.frame?.artifactId ?? observation?.screenshot_artifact_id ?? null,
    frameMediaType: session?.frame?.mediaType ?? null,
    frameUrl,
    host: hostnameOf(session?.url ?? preview.url),
    controlMode: session?.control?.mode ?? 'agent_control',
    devtoolsEvents: session?.devtools?.events ?? [],
    devtoolsUrl,
    liveFrameStreamUrl,
    liveFrameUrl,
    liveFrameWsUrl,
    networkEntries: observation?.network_summary ?? [],
    nodeCount: observation?.dom_summary?.node_count ?? null,
    observation,
    policyDenials: observation?.policy_denials ?? [],
    profileId: session?.profile.id ?? null,
    profileLabel: profileLabel(session),
    profileScope: session?.profile.scope ?? 'run_scoped',
    profileStorage: session?.profile.storage ?? 'isolated',
    renderMode: mode,
    replayEvents,
    screenshotArtifactId: observation?.screenshot_artifact_id ?? null,
    sessionId: session?.id,
    sourceLabel: frameUrl ? 'Chromium frame' : sourceLabel(mode),
    status: session?.status ?? 'degraded',
    tabs,
    tabsUrl: resolveBrowserArtifactUrl(session?.tabsUrl),
    timeline,
    title: observation?.title || session?.title || preview.title,
    url: observation?.url || session?.url || preview.url,
    visualObservationArtifactId,
    visualObservationUrl,
    viewport: session?.viewport ?? fallbackViewport,
    zdr: session?.zdr ?? false,
  }
}

/**
 * Whether the session's captured visual evidence is ephemeral. Only ZDR runs
 * make evidence ephemeral — for regular sessions Quarry retains artifacts per
 * its normal retention even when the browser profile is isolated (profile and
 * cookie non-persistence is surfaced separately via `profileStorage`). The UI
 * renders an explicit marker for ZDR sessions so they never imply persistence.
 */
export function evidenceIsEphemeral(model: Pick<BrowserSessionViewModel, 'zdr'>): boolean {
  return model.zdr
}

export function attachBrowserSession(
  preview: ScrapePreview,
  browserSession: BrowserSessionResponse | null,
): ScrapePreview {
  if (!browserSession) return preview
  const previous = preview.browserSession?.session
  const session = browserSession.session
  const mergedSession: BrowserSessionResponse['session'] = {
    ...session,
    capabilities: session.capabilities.length > 0
      ? session.capabilities
      : previous?.capabilities ?? session.capabilities,
    leaseId: session.leaseId ?? previous?.leaseId ?? null,
    liveFrameStreamUrl: session.liveFrameStreamUrl ?? previous?.liveFrameStreamUrl ?? null,
    liveFrameUrl: session.liveFrameUrl ?? previous?.liveFrameUrl ?? null,
    liveFrameWsUrl: session.liveFrameWsUrl ?? previous?.liveFrameWsUrl ?? null,
    devtoolsUrl: session.devtoolsUrl ?? previous?.devtoolsUrl ?? null,
    devtools: session.devtools?.events?.length
      ? session.devtools
      : previous?.devtools ?? { events: [], eventCount: 0, lastSequence: null },
    tabs: session.tabs?.length
      ? session.tabs
      : previous?.tabs ?? [],
    tabsUrl: session.tabsUrl ?? previous?.tabsUrl ?? null,
    control: session.control ?? previous?.control ?? { mode: 'agent_control' },
    profile: {
      id: session.profile.id ?? previous?.profile.id ?? null,
      scope: session.profile.id ? session.profile.scope : previous?.profile.scope ?? session.profile.scope,
      storage: session.profile.id ? session.profile.storage : previous?.profile.storage ?? session.profile.storage,
    },
    visual: session.visual ?? previous?.visual ?? null,
    timeline: session.timeline?.length
      ? session.timeline
      : previous?.timeline ?? [],
    replay: session.replay?.events?.length
      ? session.replay
      : previous?.replay ?? { events: [], eventCount: 0 },
    viewport: session.viewport ?? previous?.viewport,
    zdr: session.zdr ?? previous?.zdr ?? false,
  }
  const mergedBrowserSession: BrowserSessionResponse = {
    ...browserSession,
    session: mergedSession,
  }

  return {
    ...preview,
    browserSession: mergedBrowserSession,
    browserSessionError: null,
    title: browserSession.observation?.title || mergedSession.title || preview.title,
    url: browserSession.observation?.url || mergedSession.url || preview.url,
  }
}

export function attachBrowserTabs(
  preview: ScrapePreview,
  tabsResponse: BrowserTabsResponse,
): ScrapePreview {
  const previous = preview.browserSession
  if (!previous) return preview
  const session = tabsResponse.session
    ? {
        ...previous.session,
        ...tabsResponse.session,
        tabs: tabsResponse.tabs.length > 0
          ? tabsResponse.tabs
          : tabsResponse.session.tabs ?? previous.session.tabs ?? [],
        timeline: tabsResponse.session.timeline ?? previous.session.timeline ?? [],
        replay: tabsResponse.session.replay ?? previous.session.replay ?? { events: [], eventCount: 0 },
        devtools: tabsResponse.session.devtools ?? previous.session.devtools ?? { events: [], eventCount: 0, lastSequence: null },
        devtoolsUrl: tabsResponse.session.devtoolsUrl ?? previous.session.devtoolsUrl ?? null,
        visual: tabsResponse.session.visual ?? previous.session.visual ?? null,
      }
    : {
        ...previous.session,
        tabs: tabsResponse.tabs,
      }

  return {
    ...preview,
    browserSession: {
      ...previous,
      session,
    },
  }
}

export function attachBrowserObservation(
  preview: ScrapePreview,
  observation: BrowserObservation,
  options?: { controlMode?: BrowserControlMode },
): ScrapePreview {
  const previous = preview.browserSession
  if (!previous) return preview

  const session = previous.session
  const screenshotArtifactId = observation.screenshot_artifact_id ?? null
  const screenshotUrl = browserArtifactUrl(session.id, screenshotArtifactId)
  const visualObservationArtifactId = observation.visual_observation_artifact_id ?? null
  const visualObservationUrl = browserArtifactUrl(session.id, visualObservationArtifactId)
  const timelineEntry: BrowserTimelineEntry = {
    consoleSummary: observation.console_summary ?? [],
    domInteractiveCount: observation.dom_summary?.interactive_elements?.length ?? null,
    domNodeCount: observation.dom_summary?.node_count ?? null,
    networkSummary: observation.network_summary ?? [],
    observedAt: observation.observed_at ?? null,
    policyDenials: observation.policy_denials ?? [],
    screenshotArtifactId,
    screenshotUrl,
    step: observation.step,
    title: observation.title ?? null,
    url: observation.url,
    visualObservationArtifactId,
    visualObservationUrl,
  }
  const timeline = [
    ...(session.timeline ?? []).filter((entry) => entry.step !== observation.step),
    timelineEntry,
  ].slice(-maxBrowserTimelineEntries)
  const replayEvent = replayEventFromObservation(
    session.id,
    observation,
    options?.controlMode ?? session.control?.mode ?? 'human_takeover',
    session.zdr ?? false,
  )
  const replayEvents = [
    ...(session.replay?.events ?? []).filter((entry) => entry.id !== replayEvent.id),
    replayEvent,
  ].slice(-maxBrowserReplayEvents)

  return attachBrowserSession(preview, {
    observation,
    session: {
      ...session,
      control: { mode: options?.controlMode ?? session.control?.mode ?? 'human_takeover' },
      frame: screenshotArtifactId
        ? {
            artifactId: screenshotArtifactId,
            kind: 'screenshot',
            mediaType: 'image/png',
            url: screenshotUrl,
          }
        : session.frame ?? null,
      timeline,
      replay: {
        events: replayEvents,
        eventCount: replayEvents.length,
      },
      title: observation.title || session.title,
      url: observation.url || session.url,
      visual: visualObservationArtifactId
        ? {
            observationArtifactId: visualObservationArtifactId,
            observationUrl: visualObservationUrl,
          }
        : session.visual ?? null,
    },
  })
}

function domNodesFromObservation(observation: BrowserObservation): BrowserDomNode[] {
  const nodes = observation.dom_summary?.interactive_elements ?? []
  return nodes
    .map((element, index) => ({
      id: `${element.tag}-${index}`,
      kind: element.role || element.tag,
      selector: element.selector,
      text: (element.text || element.selector || element.tag).trim(),
    }))
    .filter((node) => node.text.length > 0)
    .slice(0, 40)
}

function domNodesFromPreview(preview: ScrapePreview): BrowserDomNode[] {
  return preview.blocks.map((block, index) => ({
    id: `block-${index}`,
    kind: block.heading ? 'heading' : 'paragraph',
    text: block.text,
  }))
}

function profileLabel(session?: BrowserSession): string {
  if (!session) return 'Isolert fallback'
  if (session.profile.id) return `${session.profile.id} · ${session.profile.storage}`
  return session.profile.storage === 'persistent' ? 'Persist profile' : 'Ephemeral profile'
}

function sourceLabel(mode: BrowserSurfaceMode): string {
  if (mode === 'chromium') return 'Chromium observation'
  if (mode === 'dom_snapshot') return 'DOM snapshot'
  return 'Readability fallback'
}

function resolveBrowserArtifactUrl(url?: string | null): string | null {
  const value = url?.trim()
  if (!value) return null
  if (value.startsWith('/')) return `${gatewayBaseUrl()}${value}`
  if (/^https?:\/\//i.test(value)) return value
  return null
}

function resolveBrowserWsUrl(url?: string | null): string | null {
  const value = url?.trim()
  if (!value) return null
  if (/^wss?:\/\//i.test(value)) return value
  const absolute = value.startsWith('/')
    ? gatewayBaseUrl()
      ? `${gatewayBaseUrl()}${value}`
      : `${globalThis.location.protocol === 'https:' ? 'wss' : 'ws'}://${globalThis.location.host}${value}`
    : value
  if (/^wss?:\/\//i.test(absolute)) return absolute
  if (/^http:\/\//i.test(absolute)) return absolute.replace(/^http:\/\//i, 'ws://')
  if (/^https:\/\//i.test(absolute)) return absolute.replace(/^https:\/\//i, 'wss://')
  return null
}

function resolveVisualObservationUrl(sessionId?: string, artifactId?: string | null): string | null {
  const normalizedSessionId = sessionId?.trim()
  const normalizedArtifactId = artifactId?.trim()
  if (!normalizedSessionId || !normalizedArtifactId) return null
  return resolveBrowserArtifactUrl(
    `/api/v1/browser/sessions/${encodeURIComponent(normalizedSessionId)}/artifacts/${encodeURIComponent(normalizedArtifactId)}`,
  )
}

function normalizeBrowserTabs(
  tabs: BrowserTab[] | null | undefined,
  session: BrowserSession | undefined,
  preview: ScrapePreview,
): BrowserTab[] {
  const normalized = (tabs ?? []).reduce<BrowserTab[]>((items, tab) => {
    const tabId = tab.tabId?.trim()
    if (!tabId) return items
    return [
      ...items,
      {
        active: Boolean(tab.active),
        tabId,
        title: tab.title?.trim() || null,
        url: tab.url?.trim() || null,
      },
    ]
  }, [])
  if (normalized.length > 0) {
    return normalized.some((tab) => tab.active)
      ? normalized
      : normalized.map((tab, index) => ({ ...tab, active: index === 0 }))
  }
  if (!session?.id) return []
  return [{
    active: true,
    tabId: 'tab-1',
    title: session.title || preview.title,
    url: session.url || preview.url,
  }]
}

function normalizeTimeline(entries?: BrowserTimelineEntry[] | null): BrowserTimelineViewEntry[] {
  return (entries ?? []).map((entry) => ({
    ...entry,
    screenshotUrl: resolveBrowserArtifactUrl(entry.screenshotUrl),
    visualObservationUrl: resolveBrowserArtifactUrl(entry.visualObservationUrl),
  }))
}

function normalizeReplayEvents(entries?: BrowserReplayEvent[] | null): BrowserReplayViewEvent[] {
  return (entries ?? []).map((entry) => ({
    ...entry,
    screenshotUrl: resolveBrowserArtifactUrl(entry.screenshotUrl),
    visualObservationUrl: resolveBrowserArtifactUrl(entry.visualObservationUrl),
  }))
}

function replayEventFromObservation(
  sessionId: string,
  observation: BrowserObservation,
  controlMode: BrowserControlMode,
  zdr: boolean,
): BrowserReplayEvent {
  const screenshotArtifactId = observation.screenshot_artifact_id ?? null
  const visualObservationArtifactId = observation.visual_observation_artifact_id ?? null
  const timestampMs = observation.observed_at
    ? Date.parse(observation.observed_at)
    : Number.NaN
  return {
    actor: 'human',
    consoleCount: observation.console_summary?.length ?? 0,
    controlMode,
    domInteractiveCount: observation.dom_summary?.interactive_elements?.length ?? null,
    domNodeCount: observation.dom_summary?.node_count ?? null,
    id: `${sessionId}:observation:${observation.step}`,
    kind: 'observation',
    networkCount: observation.network_summary?.length ?? 0,
    observedAt: observation.observed_at ?? null,
    policyDenialCount: observation.policy_denials?.length ?? 0,
    screenshotArtifactId,
    screenshotUrl: browserArtifactUrl(sessionId, screenshotArtifactId),
    step: observation.step,
    timestampMs: Number.isFinite(timestampMs) ? timestampMs : observation.step,
    title: observation.title ?? null,
    url: observation.url,
    visualObservationArtifactId,
    visualObservationUrl: browserArtifactUrl(sessionId, visualObservationArtifactId),
    zdr,
  }
}

/** Gateway artifact URL for a session-scoped artifact id, or null when either id is unusable. */
export function browserArtifactUrl(sessionId?: string | null, artifactId?: string | null): string | null {
  return resolveVisualObservationUrl(sessionId ?? undefined, artifactId)
}

// --- Timeline detail selection ---------------------------------------------

export type BrowserTimelineDetail = {
  entry: BrowserTimelineViewEntry
  previous: BrowserTimelineViewEntry | null
}

/** The timeline entry for a step plus the preceding entry (for before/after evidence). */
export function timelineDetail(
  timeline: BrowserTimelineViewEntry[],
  step: number | null,
): BrowserTimelineDetail | null {
  if (step === null) return null
  const index = timeline.findIndex((entry) => entry.step === step)
  if (index < 0) return null
  const entry = timeline[index]
  if (!entry) return null
  return { entry, previous: index > 0 ? timeline[index - 1] ?? null : null }
}

export type BrowserTimelineDelta = {
  domNodeDelta: number | null
  titleChanged: boolean
  urlChanged: boolean
}

/**
 * Deterministic delta between two observed timeline entries. Values are
 * computed only from fields the gateway returned; anything unavailable is null.
 */
export function describeTimelineDelta(detail: BrowserTimelineDetail): BrowserTimelineDelta {
  const { entry, previous } = detail
  const currentNodes = entry.domNodeCount ?? null
  const previousNodes = previous?.domNodeCount ?? null
  return {
    domNodeDelta: currentNodes !== null && previousNodes !== null ? currentNodes - previousNodes : null,
    titleChanged: Boolean(previous) && (previous?.title ?? '') !== (entry.title ?? ''),
    urlChanged: Boolean(previous) && (previous?.url ?? '') !== (entry.url ?? ''),
  }
}

// --- Artifact descriptors ----------------------------------------------------

export type BrowserArtifactMediaKind = 'image' | 'json'

export type BrowserArtifactDescriptor = {
  artifactId: string
  kind: 'screenshot' | 'visual_observation'
  label: string
  mediaKind: BrowserArtifactMediaKind
  url: string
}

/** The artifacts the gateway returned for one timeline entry, typed for the viewer. */
export function artifactsForTimelineEntry(entry: BrowserTimelineViewEntry): BrowserArtifactDescriptor[] {
  const artifacts: BrowserArtifactDescriptor[] = []
  if (entry.screenshotArtifactId && entry.screenshotUrl) {
    artifacts.push({
      artifactId: entry.screenshotArtifactId,
      kind: 'screenshot',
      label: 'screenshot.png',
      mediaKind: 'image',
      url: entry.screenshotUrl,
    })
  }
  if (entry.visualObservationArtifactId && entry.visualObservationUrl) {
    artifacts.push({
      artifactId: entry.visualObservationArtifactId,
      kind: 'visual_observation',
      label: 'visual_observation.json',
      mediaKind: 'json',
      url: entry.visualObservationUrl,
    })
  }
  return artifacts
}

// --- Browser chrome open-state -------------------------------------------------

export type BrowserChromePanel = 'actions' | 'devtools' | 'evidence'
export type BrowserChromePopover = 'overflow' | 'profile'

/**
 * Open-state for the unified browser chrome: collapsible panels (manual action
 * bar, right devtools panel, bottom evidence drawer) plus at most one anchored
 * popover (profile/security from the padlock, or the overflow menu).
 */
export type BrowserChromeState = {
  actionsOpen: boolean
  devtoolsOpen: boolean
  evidenceOpen: boolean
  popover: BrowserChromePopover | null
}

export const initialBrowserChromeState: BrowserChromeState = {
  actionsOpen: false,
  devtoolsOpen: false,
  evidenceOpen: false,
  popover: null,
}

/**
 * Immutably toggle one collapsible chrome panel. Panels are independent of one
 * another, but any open popover closes: a popover is transient chrome and never
 * survives a layout-changing interaction.
 */
export function toggleBrowserChromePanel(
  state: BrowserChromeState,
  panel: BrowserChromePanel,
): BrowserChromeState {
  switch (panel) {
    case 'actions':
      return { ...state, actionsOpen: !state.actionsOpen, popover: null }
    case 'devtools':
      return { ...state, devtoolsOpen: !state.devtoolsOpen, popover: null }
    case 'evidence':
      return { ...state, evidenceOpen: !state.evidenceOpen, popover: null }
  }
}

/** Immutably toggle a popover; popovers are mutually exclusive. */
export function toggleBrowserChromePopover(
  state: BrowserChromeState,
  popover: BrowserChromePopover,
): BrowserChromeState {
  return { ...state, popover: state.popover === popover ? null : popover }
}

/** Close any open popover; returns the same state object when nothing is open. */
export function closeBrowserChromePopover(state: BrowserChromeState): BrowserChromeState {
  return state.popover === null ? state : { ...state, popover: null }
}

// --- Model rationale (AI-suggested steps) -------------------------------------

export type BrowserStepRationale = {
  actionType: string | null
  confidence: number | null
  done: boolean
  goal: string
  modelUsed: string | null
  reason: string | null
  /** Step number of the observation produced by executing the suggested action. */
  step: number
}

export function rationaleForStep(
  rationales: BrowserStepRationale[],
  step: number | null,
): BrowserStepRationale | null {
  if (step === null) return null
  return rationales.find((rationale) => rationale.step === step) ?? null
}

/** Immutably record a rationale for a step, replacing any previous record for that step. */
export function withStepRationale(
  rationales: BrowserStepRationale[],
  rationale: BrowserStepRationale,
): BrowserStepRationale[] {
  return [...rationales.filter((existing) => existing.step !== rationale.step), rationale]
}

// --- Phase 5: HITL browser-action approvals -----------------------------------
//
// These are NOT part of the Quarry-derived timeline above: a HITL gate fires
// on the Model Plane orchestration-event stream *before* an action dispatches
// (or once at run start for the persistent-cookie-use run-level gate), so a
// denied action never produces a Quarry observation/step to attach to. This
// keeps approvals as their own ordered list, rendered as a dedicated section
// of the evidence drawer — distinct from a normal completed/failed timeline
// step, per the plan's own authority split (Quarry never judges policy; only
// Model Plane orchestration events carry this).

export type BrowserApprovalStatus = 'pending' | 'granted' | 'denied' | 'timed_out'

export type BrowserApprovalEntry = {
  /** Correlates a `required` event to its later `decided` event. See
   * `browserApprovalEntryKey` — not necessarily the durable approval id
   * (which is unknown until `create_approval` resolves server-side). */
  key: string
  approvalId: string
  runId: string
  planId: string
  actionId: string
  actionType: string
  url: string
  selector: string
  reason: string
  /** login|checkout|posting_form|destructive|cross_domain_navigation|persistent_cookie_use */
  riskCategory: string
  status: BrowserApprovalStatus
  decidedBy: string | null
  requestedAt: string
  decidedAt: string | null
}

const maxBrowserApprovalEntries = 16

/**
 * Correlation key for a HITL gate. `actionId` is empty for a run-level gate
 * (persistent_cookie_use, gated once before the first action) — the plan's
 * own `cookie_use_gate_done` flag guarantees at most one such gate is ever
 * pending per run, so a fixed per-run key is safe and unambiguous.
 */
function browserApprovalEntryKey(runId: string, actionId: string): string {
  return actionId ? actionId : `${runId}:run-level`
}

function normalizeApprovalDecision(decision?: string): BrowserApprovalStatus {
  if (decision === 'granted') return 'granted'
  if (decision === 'timed_out') return 'timed_out'
  return 'denied'
}

/** Record a newly-required approval, replacing any stale entry with the same key. */
export function withBrowserApprovalRequested(
  entries: BrowserApprovalEntry[],
  event: BrowserActionApprovalRequiredEvent,
): BrowserApprovalEntry[] {
  const runId = event.runId ?? ''
  const actionId = event.actionId ?? ''
  const key = browserApprovalEntryKey(runId, actionId)
  const entry: BrowserApprovalEntry = {
    key,
    approvalId: event.approvalId ?? '',
    runId,
    planId: event.planId ?? '',
    actionId,
    actionType: event.actionType ?? '',
    url: event.url ?? '',
    selector: event.selector ?? '',
    reason: event.reason ?? '',
    riskCategory: event.riskCategory ?? '',
    status: 'pending',
    decidedBy: null,
    requestedAt: event.at ?? new Date().toISOString(),
    decidedAt: null,
  }
  return [...entries.filter((existing) => existing.key !== key), entry].slice(
    -maxBrowserApprovalEntries,
  )
}

/** Apply a decision to the matching pending entry (a no-op if it was never recorded). */
export function withBrowserApprovalDecided(
  entries: BrowserApprovalEntry[],
  event: BrowserActionDecidedEvent,
): BrowserApprovalEntry[] {
  const key = browserApprovalEntryKey(event.runId ?? '', event.actionId ?? '')
  return entries.map((entry) =>
    entry.key === key
      ? {
          ...entry,
          approvalId: event.approvalId || entry.approvalId,
          status: normalizeApprovalDecision(event.decision),
          decidedBy: event.decidedBy ?? null,
          decidedAt: event.at ?? new Date().toISOString(),
        }
      : entry,
  )
}

/** The single most recent still-pending approval, if any — the one the AI bubble surfaces. */
export function pendingBrowserApproval(entries: BrowserApprovalEntry[]): BrowserApprovalEntry | null {
  for (let index = entries.length - 1; index >= 0; index -= 1) {
    const entry = entries[index]
    if (entry?.status === 'pending') return entry
  }
  return null
}
