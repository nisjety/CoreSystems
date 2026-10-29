/**
 * Chat-side "watch the agent work" model.
 *
 * An agentic / plan-mode chat turn gets a durable orchestration run id on the
 * `connected` SSE event (`chat-client.ts` → `ChatConnectedEvent.runId`). That
 * run's own lifecycle streams over the already-existing
 * `GET /api/v1/runs/:run_id/events` endpoint, consumed through
 * `run-console-client.ts` (`streamRunEvents`) — the same client the Agent Run
 * Console uses. This module owns the *pure* reduction of those events into
 * something the chat panel can render:
 *
 *   • per-browser-step evidence (action + observation + screenshot reference)
 *   • a compact activity feed for non-browser run events
 *   • the `BrowserSessionViewModel` that lets the chat panel REUSE the existing
 *     `BrowserChrome` component instead of forking a second browser panel
 *
 * Screenshots are never inlined on the wire — `browser_observation_received`
 * carries a `screenshot_ref` artifact id only, which the BFF serves from
 * `GET /api/v1/browser/sessions/:session_id/artifacts/:artifact_id`
 * (`apps/gateway/src/domains/browser.rs`). `runArtifactUrl` builds exactly that
 * URL, keyed on the run id.
 *
 * Everything here is a pure function over immutable state so the honesty rules
 * (below) are unit-testable without a DOM or a live run.
 */

import {
  type BrowserActionDispatchedEvent,
  type BrowserObservationReceivedEvent,
} from '@/shared/api/run-console-client'
import {
  gatewayBaseUrl,
} from '@/shared/api/config'
import {
  readClientValue,
  writeClientValue,
} from '@/shared/session/client-storage'
import type {
  BrowserSessionViewModel,
  BrowserTimelineViewEntry,
} from '@/features/dashboard/home/browser-session'

/** Persisted collapse state for the chat split view's live panel. */
export const CHAT_RUN_PANEL_COLLAPSED_KEY = 'verevon.chat.runPanel.collapsed.v1'

/**
 * Why a given browser step has no visible screenshot. The three "no image"
 * outcomes are deliberately distinct — conflating them is what produces a
 * spinner that never resolves or a broken `<img>`:
 *
 * - `pending`     — the action was dispatched, its observation has NOT arrived,
 *                   and the run stream is still open. A shot may still come.
 * - `withheld`    — Zero Data Retention turn. Quarry's capture is ZDR-gated
 *                   server-side, so `screenshot_ref` is empty BY DESIGN and no
 *                   image will ever exist. Show the metadata instead.
 * - `unavailable` — the observation arrived carrying no `screenshot_ref` (or
 *                   the run ended before one did). Never coming either, but for
 *                   a different reason than ZDR.
 * - `failed`      — a reference exists but the artifact could not be fetched
 *                   (expired lease, ownership gate, oversized artifact). We
 *                   know a shot existed; we just cannot show it.
 */
export type ChatRunScreenshotState = 'ready' | 'pending' | 'withheld' | 'unavailable' | 'failed'

/** One browser action + its observation, as the run stream reveals them. */
export type ChatRunBrowserStep = {
  /** 1-based arrival order — matches `BrowserTimelineEntry.step`. */
  step: number
  actionId: string
  actionType?: string
  /** The model's rationale for this action, when the planner was an LLM. */
  reason?: string
  /** Navigation target from the action, or the settled page url from the observation. */
  url?: string
  pageTitle?: string
  /** Observation status slug: success|failed|timeout|blocked. */
  status?: string
  /** True once the matching observation arrived (with or without a screenshot). */
  observed: boolean
  screenshotRef?: string
  /** BFF artifact URL for `screenshotRef`; absent when the ref is unusable. */
  screenshotUrl?: string
  /** Set when the browser failed to load `screenshotUrl`. */
  screenshotFailed?: boolean
  domSnapshotRef?: string
  at: string
}

export type ChatRunActivityKind = 'step' | 'plan' | 'approval' | 'pause' | 'resume' | 'subagent'

export type ChatRunActivityEntry = {
  id: string
  kind: ChatRunActivityKind
  title: string
  detail: string
  status?: string
  at: string
}

export type ChatRunWatchState = {
  runId: string
  /** Whether the run-event stream is still open (`onDone`/`onError` clear it). */
  live: boolean
  /** Zero Data Retention turn — decides `withheld` vs `unavailable`. */
  zdr: boolean
  error: string | null
  steps: ChatRunBrowserStep[]
  activity: ChatRunActivityEntry[]
}

const MAX_ACTIVITY_ENTRIES = 60
const MAX_BROWSER_STEPS = 48
const CHAT_RUN_VIEWPORT = { height: 800, width: 1280 }

export function emptyChatRunWatch(runId: string, zdr: boolean): ChatRunWatchState {
  return { activity: [], error: null, live: true, runId, steps: [], zdr }
}

/**
 * Same validity rule the BFF enforces (`is_valid_artifact_id` in
 * `browser.rs`): an `art_`-prefixed segment of 3–128 url-safe characters.
 * Checking it client-side means a malformed reference degrades to the honest
 * "could not fetch" state instead of firing a request the gateway 400s.
 */
export function isFetchableArtifactRef(ref?: string | null): boolean {
  const value = ref?.trim() ?? ''
  if (!value.startsWith('art_') || value.length <= 4) return false
  return value.length <= 128 && /^[A-Za-z0-9_-]+$/.test(value)
}

/**
 * BFF artifact URL for a run's evidence artifact. The gateway keys browser
 * artifacts on the run id it registered ownership under, so the same run id the
 * chat `connected` event carried is the path segment here.
 */
export function runArtifactUrl(runId: string, ref?: string | null): string | undefined {
  if (!runId.trim() || !isFetchableArtifactRef(ref)) return undefined
  return `${gatewayBaseUrl()}/api/v1/browser/sessions/${encodeURIComponent(runId)}`
    + `/artifacts/${encodeURIComponent((ref ?? '').trim())}`
}

function withStep(
  state: ChatRunWatchState,
  actionId: string,
  update: (step: ChatRunBrowserStep) => ChatRunBrowserStep,
  create: (step: number) => ChatRunBrowserStep,
): ChatRunWatchState {
  const index = state.steps.findIndex((step) => step.actionId === actionId)
  if (index >= 0) {
    return {
      ...state,
      steps: state.steps.map((step, stepIndex) => (stepIndex === index ? update(step) : step)),
    }
  }
  const steps = [...state.steps, create(state.steps.length + 1)]
  return { ...state, steps: steps.slice(-MAX_BROWSER_STEPS) }
}

export function applyBrowserAction(
  state: ChatRunWatchState,
  event: BrowserActionDispatchedEvent,
): ChatRunWatchState {
  const actionId = event.actionId ?? `act-${state.steps.length + 1}`
  const at = event.at ?? new Date().toISOString()
  return withStep(
    state,
    actionId,
    (step) => ({
      ...step,
      actionType: event.actionType ?? step.actionType,
      at,
      reason: event.reason ?? step.reason,
      url: event.url ?? step.url,
    }),
    (step) => ({
      actionId,
      actionType: event.actionType,
      at,
      observed: false,
      reason: event.reason,
      step,
      url: event.url,
    }),
  )
}

export function applyBrowserObservation(
  state: ChatRunWatchState,
  event: BrowserObservationReceivedEvent,
): ChatRunWatchState {
  const actionId = event.actionId ?? `act-${state.steps.length + 1}`
  const at = event.at ?? new Date().toISOString()
  const screenshotUrl = runArtifactUrl(state.runId, event.screenshotRef)
  return withStep(
    state,
    actionId,
    (step) => ({
      ...step,
      at,
      domSnapshotRef: event.domSnapshotRef ?? step.domSnapshotRef,
      observed: true,
      pageTitle: event.pageTitle ?? step.pageTitle,
      screenshotFailed: false,
      screenshotRef: event.screenshotRef ?? step.screenshotRef,
      screenshotUrl: screenshotUrl ?? step.screenshotUrl,
      status: event.status ?? step.status,
      url: event.pageUrl ?? step.url,
    }),
    (step) => ({
      actionId,
      at,
      domSnapshotRef: event.domSnapshotRef,
      observed: true,
      pageTitle: event.pageTitle,
      screenshotRef: event.screenshotRef,
      screenshotUrl,
      status: event.status,
      step,
      url: event.pageUrl,
    }),
  )
}

/** A screenshot URL that failed to load — recorded so we never retry a broken `<img>`. */
export function markScreenshotFailed(state: ChatRunWatchState, actionId: string): ChatRunWatchState {
  return {
    ...state,
    steps: state.steps.map((step) => (
      step.actionId === actionId ? { ...step, screenshotFailed: true } : step
    )),
  }
}

export function appendActivity(
  state: ChatRunWatchState,
  entry: ChatRunActivityEntry,
): ChatRunWatchState {
  const index = state.activity.findIndex((item) => item.id === entry.id)
  if (index >= 0) {
    return {
      ...state,
      activity: state.activity.map((item, itemIndex) => (
        itemIndex === index ? { ...item, ...entry } : item
      )),
    }
  }
  return { ...state, activity: [...state.activity, entry].slice(-MAX_ACTIVITY_ENTRIES) }
}

export function closeChatRunWatch(state: ChatRunWatchState, error?: string): ChatRunWatchState {
  return { ...state, error: error ?? state.error, live: false }
}

export function screenshotStateFor(
  step: ChatRunBrowserStep,
  options: { live: boolean; zdr: boolean },
): ChatRunScreenshotState {
  if (step.screenshotFailed) return 'failed'
  if (step.screenshotUrl) return 'ready'
  // A reference we cannot turn into a URL is a fetch problem, not an absence.
  if (step.screenshotRef) return 'failed'
  if (options.zdr) return 'withheld'
  if (!step.observed) return options.live ? 'pending' : 'unavailable'
  return 'unavailable'
}

export const CHAT_RUN_SCREENSHOT_NOTES: Record<Exclude<ChatRunScreenshotState, 'ready'>, string> = {
  failed: 'Skjermbildet kunne ikke hentes.',
  pending: 'Venter på skjermbilde …',
  unavailable: 'Ingen skjermbilde for dette steget.',
  withheld: 'Midlertidig samtale – skjermbilder lagres ikke.',
}

export function hasBrowserFrames(state: ChatRunWatchState): boolean {
  return state.steps.some((step) => Boolean(step.screenshotUrl) && !step.screenshotFailed)
}

export function latestBrowserStep(state: ChatRunWatchState): ChatRunBrowserStep | null {
  return state.steps.length > 0 ? state.steps[state.steps.length - 1] ?? null : null
}

export function chatRunHostname(url?: string): string {
  const value = url?.trim()
  if (!value) return ''
  try {
    return new URL(value).hostname.replace(/^www\./, '')
  } catch {
    return value.replace(/^https?:\/\//i, '').split('/')[0] ?? ''
  }
}

function chatRunSessionStatus(state: ChatRunWatchState): BrowserSessionViewModel['status'] {
  if (state.error) return 'degraded'
  if (!state.live) return 'closed'
  const latest = latestBrowserStep(state)
  const status = latest?.status?.toLowerCase()
  if (status && status !== 'success') return 'degraded'
  return 'live'
}

/**
 * Adapts the reduced run state onto the view model the EXISTING `BrowserChrome`
 * component already renders (frame, tab strip, step timeline, ZDR chip,
 * devtools drawer). Live-frame transports are deliberately left null: a chat
 * turn's browser runs server-side inside execution-core, so there is no
 * gateway-registered live-frame lease to attach — the honest transport is the
 * per-step artifact frame Quarry captures on every step, which is exactly what
 * `BrowserChrome`'s `frameUrl`/`timeline` candidates render. Because every url
 * here is null/absent, none of `BrowserChrome`'s WebSocket → SSE → polling
 * fallback tiers ever arm.
 */
export function browserSessionFromChatRun(state: ChatRunWatchState): BrowserSessionViewModel {
  const latest = latestBrowserStep(state)
  const framed = [...state.steps].reverse().find((step) => step.screenshotUrl && !step.screenshotFailed)
  const timeline: BrowserTimelineViewEntry[] = state.steps.map((step) => ({
    observedAt: step.at,
    screenshotArtifactId: step.screenshotRef ?? null,
    screenshotUrl: step.screenshotUrl ?? null,
    step: step.step,
    title: step.pageTitle ?? step.actionType ?? null,
    url: step.url ?? null,
  }))

  return {
    capabilities: [],
    commentAnchors: [],
    consoleEntries: [],
    controlMode: 'agent_control',
    degradedReason: state.error,
    devtoolsEvents: [],
    devtoolsUrl: null,
    domNodes: [],
    frameArtifactId: framed?.screenshotRef ?? null,
    frameMediaType: 'image/png',
    frameUrl: framed?.screenshotUrl ?? null,
    host: chatRunHostname(latest?.url),
    liveFrameStreamUrl: null,
    liveFrameUrl: null,
    liveFrameWsUrl: null,
    networkEntries: [],
    nodeCount: null,
    observation: null,
    policyDenials: [],
    profileId: null,
    profileLabel: 'Agent-økt',
    profileScope: 'run_scoped',
    profileStorage: 'isolated',
    renderMode: 'chromium',
    replayEvents: [],
    screenshotArtifactId: framed?.screenshotRef ?? null,
    // Deliberately absent: no gateway-created browser session backs a chat run,
    // so every manual control in `BrowserChrome` stays disabled instead of
    // pretending the user can drive the agent's browser from here.
    sessionId: undefined,
    sourceLabel: 'Agent-nettleser',
    status: chatRunSessionStatus(state),
    tabs: [],
    tabsUrl: null,
    timeline,
    title: latest?.pageTitle ?? latest?.actionType ?? 'Agent-nettleser',
    url: latest?.url ?? '',
    viewport: CHAT_RUN_VIEWPORT,
    visualObservationArtifactId: null,
    visualObservationUrl: null,
    zdr: state.zdr,
  }
}

export function readChatRunPanelCollapsed(): boolean {
  return readClientValue(CHAT_RUN_PANEL_COLLAPSED_KEY) === '1'
}

export function writeChatRunPanelCollapsed(collapsed: boolean): void {
  writeClientValue(CHAT_RUN_PANEL_COLLAPSED_KEY, collapsed ? '1' : '0')
}
