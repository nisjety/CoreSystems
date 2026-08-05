import {
  AlertCircle,
  ArrowLeft,
  ArrowRight,
  Bot,
  Camera,
  Code2,
  Copy,
  ExternalLink,
  Hand,
  Eye,
  History,
  Keyboard,
  Loader2,
  LockKeyhole,
  MoreVertical,
  MousePointer2,
  Navigation,
  Network,
  PanelRight,
  Pause,
  Pencil,
  Play,
  Plus,
  RefreshCw,
  Send,
  ShieldCheck,
  Sparkles,
  Square,
  SquareMousePointer,
  Terminal,
  TextCursorInput,
  Timer,
  Trash2,
  X,
} from 'lucide-solid'
import {
  createEffect,
  createSignal,
  createUniqueId,
  For,
  Match,
  onCleanup,
  Show,
  Switch,
  untrack,
  type JSX,
} from 'solid-js'
import type {
  BrowserAction,
  BrowserActionSuggestionResponse,
  BrowserDevtoolsEvent,
  BrowserObservation,
  BrowserProfileRestoreProbe,
  BrowserProfileSummary,
  BrowserSession,
  BrowserTab,
  CreatableBrowserProfileScope,
} from '@/shared/api/browser-client'
import type { ApprovalDecision } from '@/shared/api/orchestration-client'
import { isBrowserLoopRunning, type BrowserLoopState } from './browser-loop'
import {
  closeBrowserChromePopover,
  evidenceIsEphemeral,
  initialBrowserChromeState,
  pendingBrowserApproval,
  rationaleForStep,
  timelineDetail,
  toggleBrowserChromePanel,
  toggleBrowserChromePopover,
  type BrowserApprovalEntry,
  type BrowserChromePanel,
  type BrowserChromePopover,
  type BrowserReplayViewEvent,
  type BrowserSessionViewModel,
  type BrowserStepRationale,
  type BrowserTimelineViewEntry,
} from './browser-session'
import { BrowserTimelineDetailPanel, compactBrowserUrl, consoleTone, networkTone } from './BrowserTimelineDetail'
import { browserOmniboxTarget } from './browser-omnibox'
import { hostnameOf } from './knowledge-preview'

export const SESSION_STATUS_LABELS: Record<BrowserSessionViewModel['status'], string> = {
  closed: 'Lukket',
  degraded: 'Degradert',
  live: 'Live',
}

export const RENDER_MODE_LABELS: Record<BrowserSessionViewModel['renderMode'], string> = {
  chromium: 'Chromium',
  dom_snapshot: 'DOM-snapshot',
  readability_fallback: 'Readability',
}

export const PROFILE_SCOPE_LABELS: Record<BrowserSessionViewModel['profileScope'], string> = {
  ephemeral: 'Efemer',
  org_shared: 'Org-delt',
  run_scoped: 'Kjøringsscopet',
  user_private: 'Privat',
}

/**
 * Phase 3 continuation — everything the profile popover needs to run full
 * CRUD (create/rename/delete/list) against the org's real, named+scoped
 * `ProfileStore` rows, independent of whichever profile the *current*
 * session happens to be attached to. Data is plain values (not signals) —
 * the caller (`KnowledgeComposer`) owns the state and re-renders this
 * component reactively the same way every other prop here already works.
 */
export type BrowserProfileManagerProps = {
  creating?: boolean
  deleteArmed?: boolean
  error?: string | null
  loading?: boolean
  mutating?: boolean
  newProfileName: string
  newProfileScope: CreatableBrowserProfileScope
  onArmDelete: () => void
  onCancelDelete: () => void
  onCancelRename: () => void
  onConfirmDelete: () => void
  onConfirmRename: () => void
  onCreateProfile: () => void
  onNewProfileNameChange: (value: string) => void
  onNewProfileScopeChange: (value: CreatableBrowserProfileScope) => void
  onProbeProfile: () => void
  onRefreshProfiles: () => void
  onRenameProfileNameChange: (value: string) => void
  onSelectProfile: (profileId: string) => void
  onStartRename: () => void
  profiles: BrowserProfileSummary[]
  renameProfileName: string
  renaming?: boolean
  selectedProfileId: string | null
  /** True while the visible session itself is ZDR — creating/attaching a
   * persistent profile is disabled in that case (client-side mirror of the
   * server's `reject_zdr_persistent_profile` guard, never a substitute for
   * it: the gateway still rejects the request independently). */
  zdrActive?: boolean
}

const CREATABLE_PROFILE_SCOPES: CreatableBrowserProfileScope[] = ['user_private', 'org_shared', 'run_scoped']

const LOOP_STATUS_LABELS: Record<BrowserLoopState['status'], string> = {
  acting: 'Utfører',
  awaiting_approval: 'Venter på godkjenning',
  done: 'Ferdig',
  idle: 'Inaktiv',
  paused: 'Pauset',
  stopped: 'Stoppet',
  suggesting: 'Foreslår',
}

/** Phase 5 HITL gate: Bokmål label per risk category self-reported by the
 * planner or derived by execution-core's deterministic backstop classifier.
 * Falls back to a generic label for any category not in this list, since the
 * category is a free-form string from the wire, not a closed union here. */
const BROWSER_APPROVAL_RISK_LABELS: Record<string, string> = {
  checkout: 'Utsjekk/betaling',
  cross_domain_navigation: 'Navigering til nytt domene',
  destructive: 'Ødeleggende handling',
  login: 'Innlogging',
  persistent_cookie_use: 'Bruker vedvarende profil/informasjonskapsler',
  posting_form: 'Skjemainnsending',
}

function browserApprovalRiskLabel(riskCategory: string): string {
  return BROWSER_APPROVAL_RISK_LABELS[riskCategory] ?? 'Risikofylt handling'
}

const BROWSER_APPROVAL_STATUS_LABELS: Record<BrowserApprovalEntry['status'], string> = {
  denied: 'Avslått',
  granted: 'Godkjent',
  pending: 'Venter på godkjenning',
  timed_out: 'Tidsavbrutt',
}

const BROWSER_WHEEL_THROTTLE_MS = 280
const LIVE_FRAME_REFRESH_MS = 700
const MAX_BROWSER_WHEEL_DELTA = 1200

function compactArtifactId(value: string): string {
  if (value.length <= 22) return value
  return `${value.slice(0, 9)}...${value.slice(-8)}`
}

function clampWheelDelta(value: number): number {
  if (!Number.isFinite(value)) return 0
  return Math.max(-MAX_BROWSER_WHEEL_DELTA, Math.min(MAX_BROWSER_WHEEL_DELTA, value))
}

function browserKeyFromKeyboardEvent(event: KeyboardEvent): string | null {
  if (event.key === ' ') return 'Space'
  if (event.key === 'Dead' || event.key === 'Unidentified') return null
  return event.key
}

type DevtoolsTab = 'console' | 'dom' | 'network' | 'vision'
type BrowserWheelAction = Extract<BrowserAction, { type: 'mouse_wheel' }>
type LiveFrameStreamPayload = {
  dataBase64?: string
  mimeType?: string
  sequence?: number
  zdr?: boolean
}
type BrowserWsServerMessage = LiveFrameStreamPayload & {
  control?: { mode?: BrowserSessionViewModel['controlMode'] }
  events?: BrowserDevtoolsEvent[]
  message?: string
  observation?: BrowserObservation
  session?: BrowserSession
  type?: 'control' | 'devtools' | 'done' | 'error' | 'frame' | 'observation' | 'pong'
}

/** Kort fanetekst for et snapshot-steg: tittel, ellers vertsnavn, ellers stegnummer. */
function snapshotTabLabel(entry: BrowserTimelineViewEntry): string {
  if (entry.title) return entry.title
  if (entry.url) return hostnameOf(entry.url)
  return `Steg ${entry.step}`
}

function replayEventLabel(event: BrowserReplayViewEvent): string {
  if (event.kind === 'control') {
    return event.controlMode === 'human_takeover' ? 'Menneske tok over' : 'Agentkontroll aktiv'
  }
  if (event.kind === 'tab') {
    if (event.operation === 'new') return 'Ny fane'
    if (event.operation === 'select') return 'Fane valgt'
    if (event.operation === 'close') return 'Fane lukket'
    return 'Fanehendelse'
  }
  if (event.kind === 'frame') return 'Live frame'
  if (event.kind === 'devtools') return 'DevTools'
  if (event.actionType) return event.actionType.replaceAll('_', ' ')
  return event.title || 'Observasjon'
}

function replayEventMeta(event: BrowserReplayViewEvent): string {
  if (event.kind === 'frame') {
    return [
      event.transport || 'websocket',
      event.sequence !== undefined && event.sequence !== null ? `#${event.sequence}` : null,
      event.mimeType ?? null,
      event.persisted === false ? 'flyktig' : null,
    ].filter(Boolean).join(' · ')
  }
  if (event.kind === 'devtools') {
    return [
      event.eventCount !== undefined && event.eventCount !== null ? `${event.eventCount} hendelser` : null,
      event.lastSequence !== undefined && event.lastSequence !== null ? `seq ${event.lastSequence}` : null,
    ].filter(Boolean).join(' · ') || 'DevTools'
  }
  const parts = [
    event.actor,
    event.step !== undefined && event.step !== null ? `#${event.step}` : null,
    event.url ? compactBrowserUrl(event.url) : null,
  ].filter(Boolean)
  return parts.length > 0 ? parts.join(' · ') : 'Ingen detalj'
}

/** DOM-nodeliste delt mellom sidevisningens fallback og DevTools-panelet. */
function DomNodeList(props: { nodes: BrowserSessionViewModel['domNodes'] }) {
  return (
    <div class="knowledge-browser-live-dom">
      <For
        each={props.nodes}
        fallback={<p class="knowledge-browser-empty">Ingen DOM-noder returnert ennå.</p>}
      >
        {(node) => (
          <div class="knowledge-browser-live-dom__node">
            <span>{node.kind}</span>
            <p>{node.text}</p>
            <Show when={node.selector}>
              {(selector) => <code>{selector()}</code>}
            </Show>
          </div>
        )}
      </For>
    </div>
  )
}

/**
 * Phase 5 HITL gate: one pending/decided browser-action approval, rendered
 * in the evidence drawer's "Godkjenninger" section. Reuses the SAME
 * `.verevon-run-approval*` classes as AgentRunConsole's approval deck (the
 * app's one existing approve/reject pattern) rather than inventing a new
 * visual language — only the state-modifier classes below (granted/denied/
 * timed_out) are new, so a decided browser-action approval reads as a
 * distinct, deliberate marker instead of a normal completed/failed timeline
 * step (a denied action never reaches the timeline at all — Quarry never
 * dispatches it, so there is no step to attach a marker to).
 */
function BrowserApprovalCard(props: {
  approval: BrowserApprovalEntry
  deciding: boolean
  onDecide?: (approvalKey: string, decision: ApprovalDecision) => void
}) {
  const approval = () => props.approval
  const isPending = () => approval().status === 'pending'
  return (
    <div
      class="verevon-run-approval"
      classList={{
        [`verevon-run-approval--${approval().status}`]: !isPending(),
      }}
    >
      <div class="verevon-run-approval__head">
        <span
          class="verevon-run-approval__badge"
          classList={{
            'verevon-run-approval__badge--granted': approval().status === 'granted',
            'verevon-run-approval__badge--denied': approval().status === 'denied' || approval().status === 'timed_out',
          }}
        >
          <Show when={isPending()} fallback={approval().status === 'granted' ? <ShieldCheck class="size-3" /> : <AlertCircle class="size-3" />}>
            <Pause class="size-3" />
          </Show>
          {BROWSER_APPROVAL_STATUS_LABELS[approval().status]}
        </span>
        <span class="verevon-run-approval__kind">{browserApprovalRiskLabel(approval().riskCategory)}</span>
      </div>
      <p class="verevon-run-approval__detail">
        {approval().reason || 'Handlingen krever godkjenning før den utføres.'}
      </p>
      <Show when={approval().url || approval().selector}>
        <p class="verevon-run-approval__meta">
          <Show when={approval().actionType}>{approval().actionType} · </Show>
          <Show when={approval().url}>{compactBrowserUrl(approval().url)}</Show>
          <Show when={approval().selector}> · {approval().selector}</Show>
        </p>
      </Show>
      <Show when={approval().decidedBy}>
        <p class="verevon-run-approval__meta">Avgjort av {approval().decidedBy}</p>
      </Show>
      <Show when={isPending()}>
        <div class="verevon-run-approval__actions">
          <button
            type="button"
            class="verevon-run-approval__approve"
            disabled={props.deciding}
            onClick={() => props.onDecide?.(approval().key, 'approve')}
          >
            <Show when={props.deciding}>
              <Loader2 class="size-3.5 verevon-run-spin" />
            </Show>
            Godkjenn
          </button>
          <button
            type="button"
            class="verevon-run-approval__reject"
            disabled={props.deciding}
            onClick={() => props.onDecide?.(approval().key, 'reject')}
          >
            <Show when={props.deciding}>
              <Loader2 class="size-3.5 verevon-run-spin" />
            </Show>
            Avslå
          </button>
        </div>
      </Show>
    </div>
  )
}

/**
 * Unified browser chrome for the knowledge-card browser tab: ONE tab strip
 * (live + snapshot frames), ONE toolbar (nav, padlock+address, status, tools),
 * and content that dominates. Everything that used to be a permanent band —
 * profile strip, manual action bar, observation inspector, timeline + step
 * detail — now lives behind toolbar toggles as a popover, a collapsible right
 * devtools panel, or a bottom evidence drawer. The only always-visible marker
 * is the ZDR chip: the plan's honesty constraint requires ephemeral sessions to
 * say so at all times.
 */
export function BrowserChrome(props: {
  /** Phase 5 HITL gate: pending/decided browser-action approvals for the
   * current durable AI run, oldest first. Surfaced in the AI bubble (the
   * single most recent still-pending one) and listed in full in the evidence
   * drawer's "Godkjenninger" section. */
  browserApprovals?: BrowserApprovalEntry[]
  browserBusy?: boolean
  browserLoop?: BrowserLoopState
  /** Latest streamed rationale from a durable server-side AI run (Phase 2):
   * shown live in the AI bubble as `browser_action_dispatched` events arrive,
   * kept separate from the per-tab-step `browserRationales` map. */
  browserLoopRationale?: BrowserStepRationale | null
  browserRationales?: BrowserStepRationale[]
  /** Fallback body (non-live render modes); shown instead of the live page. */
  children?: JSX.Element
  /**
   * Density variant for narrow columns (the chat split view's live panel).
   * Purely additive: it only adds a modifier class the stylesheet uses to
   * tighten paddings and hide the chrome that needs horizontal room. Omitting
   * it — as the Knowledge surface does — keeps the full-width layout byte-for
   * byte unchanged.
   */
  compact?: boolean
  /** Approval *keys* (see `BrowserApprovalEntry.key`) with an in-flight
   * decide call — disables that card's Approve/Reject buttons. */
  decidingBrowserApprovalKeys?: string[]
  frameControls?: JSX.Element
  onBrowserAction?: (action: BrowserAction) => void
  onBrowserAutoRun?: (goal: string) => Promise<BrowserActionSuggestionResponse | null>
  onBrowserControlMode?: (mode: BrowserSessionViewModel['controlMode']) => void
  onBrowserLoopPause?: () => void
  onBrowserLoopResume?: () => void
  onBrowserLoopStop?: () => void
  onBrowserNewTab?: () => Promise<void> | void
  onBrowserSelectTab?: (tabId: string) => Promise<void> | void
  onBrowserSuggestAction?: (goal: string) => Promise<BrowserActionSuggestionResponse | null>
  onBrowserSocketObservation?: (observation: BrowserObservation, session?: BrowserSession) => void
  /** Phase 5 HITL gate: approve or reject a pending browser-action approval. */
  onDecideBrowserApproval?: (approvalKey: string, decision: ApprovalDecision) => void
  /** Phase 3 continuation: full profile CRUD surfaced in the popover. Omit
   * to fall back to the read-only current-session info the popover always
   * showed before this. */
  profileManager?: BrowserProfileManagerProps
  profileProbe?: BrowserProfileRestoreProbe | null
  session: BrowserSessionViewModel
}) {
  const uid = createUniqueId()
  const actionbarId = `browser-actionbar-${uid}`
  const devtoolsId = `browser-devtools-${uid}`
  const evidenceId = `browser-evidence-${uid}`
  const profilePopoverId = `browser-profile-${uid}`
  const overflowPopoverId = `browser-overflow-${uid}`

  const [chrome, setChrome] = createSignal(initialBrowserChromeState)
  const [devtoolsTab, setDevtoolsTab] = createSignal<DevtoolsTab>('dom')
  const [selectedTimelineStep, setSelectedTimelineStep] = createSignal<number | null>(null)
  const [brokenFrameUrl, setBrokenFrameUrl] = createSignal<string | null>(null)
  const [addressInput, setAddressInput] = createSignal('')
  const [lastObservedUrl, setLastObservedUrl] = createSignal('')
  const [selectorInput, setSelectorInput] = createSignal('')
  const [textInput, setTextInput] = createSignal('')
  const [keyInput, setKeyInput] = createSignal('Enter')
  const [goalInput, setGoalInput] = createSignal('Capture useful evidence from this page')
  const [lastSuggestion, setLastSuggestion] = createSignal<BrowserActionSuggestionResponse | null>(null)
  const [suggestionDismissed, setSuggestionDismissed] = createSignal(false)
  const [copyState, setCopyState] = createSignal<'copied' | 'failed' | 'idle'>('idle')
  const [streamFrameSrc, setStreamFrameSrc] = createSignal<string | null>(null)
  const [devtoolsEvents, setDevtoolsEvents] = createSignal<BrowserDevtoolsEvent[]>([])
  const [liveFrameWsConnected, setLiveFrameWsConnected] = createSignal(false)
  const [liveFrameWsFailed, setLiveFrameWsFailed] = createSignal(false)
  const [liveFrameSseFailed, setLiveFrameSseFailed] = createSignal(false)
  const [wsActionPending, setWsActionPending] = createSignal(false)
  const [liveFrameTick, setLiveFrameTick] = createSignal(0)
  let addressInputRef: HTMLInputElement | undefined
  let lastWheelDispatchMs = 0
  let queuedWheelAction: BrowserWheelAction | null = null
  let queuedWheelTimer: number | undefined
  let liveFrameTimer: number | undefined
  let liveFrameSource: EventSource | undefined
  let liveFrameSocket: WebSocket | undefined
  let liveFrameSocketGeneration = 0
  let lastLiveFrameTransportKey = ''
  let lastDevtoolsSessionId = ''
  let lastAutoOpenedApprovalKey = ''
  let wsActionTimer: number | undefined

  const session = () => props.session
  const isLive = () => session().renderMode === 'chromium'
  const controlsDisabled = () => !isLive() || props.browserBusy || wsActionPending() || !session().sessionId
  const loopStatus = () => props.browserLoop?.status ?? 'idle'
  const loopRunning = () => isBrowserLoopRunning(loopStatus())
  // Phase 5 HITL gate: the single most recent still-pending browser-action
  // approval, if any — the one the AI bubble surfaces over the live page.
  const pendingApproval = () => pendingBrowserApproval(props.browserApprovals ?? [])
  const decidingBrowserApprovalKeys = () => props.decidingBrowserApprovalKeys ?? []
  const humanTakeoverActive = () => session().controlMode === 'human_takeover'
  const liveBrowserTabs = () => session().tabs.length > 0
    ? session().tabs
    : [{
        active: true,
        tabId: 'tab-1',
        title: session().title || session().host,
        url: session().url,
      }]
  const timelineTabs = () => session().timeline.filter((entry) => entry.step > 0).slice(-8)
  const replayEvents = () => session().replayEvents.slice(-12)
  const selectedTimelineEntry = () => {
    const selected = selectedTimelineStep()
    if (selected === null) return null
    return session().timeline.find((entry) => entry.step === selected) ?? null
  }
  const selectedTimelineDetail = () => timelineDetail(session().timeline, selectedTimelineStep())
  const selectedRationale = () => rationaleForStep(props.browserRationales ?? [], selectedTimelineStep())
  const liveFrameWsAvailable = () =>
    isLive() && !props.browserBusy && selectedTimelineStep() === null && Boolean(session().liveFrameWsUrl)
      && !liveFrameWsFailed()
  const liveFrameStreamAvailable = () =>
    isLive() && !props.browserBusy && selectedTimelineStep() === null && Boolean(session().liveFrameStreamUrl)
      && (!session().liveFrameWsUrl || liveFrameWsFailed()) && !liveFrameSseFailed()
  const pollingLiveFrameAvailable = () =>
    isLive() && !props.browserBusy && selectedTimelineStep() === null && Boolean(session().liveFrameUrl)
      && (!session().liveFrameWsUrl || liveFrameWsFailed())
      && (!session().liveFrameStreamUrl || liveFrameSseFailed())
  const liveFrameWsSrc = () => {
    const url = liveFrameWsAvailable() ? session().liveFrameWsUrl : null
    if (!url) return null
    const separator = url.includes('?') ? '&' : '?'
    return `${url}${separator}maxWidth=${session().viewport.width}&maxHeight=${session().viewport.height}`
  }
  const liveFrameStreamSrc = () => {
    const url = liveFrameStreamAvailable() ? session().liveFrameStreamUrl : null
    if (!url) return null
    const separator = url.includes('?') ? '&' : '?'
    return `${url}${separator}maxWidth=${session().viewport.width}&maxHeight=${session().viewport.height}`
  }
  const liveFrameSrc = () => {
    const url = pollingLiveFrameAvailable() ? session().liveFrameUrl : null
    if (!url) return null
    const separator = url.includes('?') ? '&' : '?'
    return `${url}${separator}maxWidth=${session().viewport.width}&maxHeight=${session().viewport.height}&t=${liveFrameTick()}`
  }
  const frameUrl = () => {
    const candidates = [
      selectedTimelineEntry()?.screenshotUrl,
      selectedTimelineStep() === null ? streamFrameSrc() : null,
      liveFrameSrc(),
      session().frameUrl,
    ].filter((url): url is string => Boolean(url))
    return candidates.find((url) => brokenFrameUrl() !== url) ?? null
  }
  const selector = () => selectorInput().trim()
  const address = () => addressInput().trim()
  const textValue = () => textInput()
  const keyValue = () => keyInput().trim() || 'Enter'
  const selectorActionDisabled = () => controlsDisabled() || selector().length === 0
  const typeActionDisabled = () => selectorActionDisabled() || textValue().length === 0
  const modelLoopDisabled = () => controlsDisabled() || humanTakeoverActive() || !props.onBrowserAutoRun
  const modelActionDisabled = () => controlsDisabled() || humanTakeoverActive() || !props.onBrowserSuggestAction
  const probe = () => {
    const current = props.profileProbe
    return current && current.profile_id === session().profileId ? current : null
  }
  const statusTitle = () => {
    const base = `Øktstatus: ${SESSION_STATUS_LABELS[session().status]}`
    const reason = session().degradedReason
    const transport = `Visuell transport: ${frameTransportLabel()}`
    return reason ? `${base} — ${reason} · ${transport}` : `${base} · ${transport}`
  }
  const frameTransportLabel = () => {
    if (liveFrameWsConnected()) return 'WebSocket'
    if (liveFrameStreamAvailable()) return 'SSE'
    if (pollingLiveFrameAvailable()) return 'polling'
    return 'artifact'
  }
  const loopTitle = () => {
    const parts = [`AI-loop: ${LOOP_STATUS_LABELS[loopStatus()]}`]
    const goal = props.browserLoop?.goal
    const loopError = props.browserLoop?.error
    if (goal) parts.push(`Mål: ${goal}`)
    if (loopError) parts.push(`Feil: ${loopError}`)
    return parts.join(' · ')
  }
  const shotArtifactId = () => session().frameArtifactId ?? session().screenshotArtifactId ?? null
  const liveTabLabel = (tab: BrowserTab) =>
    tab.title || hostnameOf(tab.url ?? session().url) || session().host || 'Browser'
  const liveConsoleEntries = () => {
    const events = devtoolsEvents()
      .filter((event) => event.category === 'console' && event.text?.trim())
      .map((event) => ({
        level: event.level || 'info',
        text: event.text ?? '',
      }))
    return [...session().consoleEntries, ...events].slice(-80)
  }
  const liveNetworkEntries = () => {
    const requestMethods = new Map<string, string>()
    for (const event of devtoolsEvents()) {
      if (event.name === 'Network.requestWillBeSent' && event.url && event.method) {
        requestMethods.set(event.url, event.method)
      }
    }
    const events = devtoolsEvents()
      .filter((event) => event.name === 'Network.responseReceived' && event.url)
      .map((event) => ({
        content_type: null,
        method: event.url ? requestMethods.get(event.url) ?? 'GET' : 'GET',
        status: event.status ?? 0,
        url: event.url ?? '',
      }))
    return [...session().networkEntries, ...events].slice(-120)
  }

  const togglePanel = (panel: BrowserChromePanel) => setChrome((state) => toggleBrowserChromePanel(state, panel))
  const togglePopover = (popover: BrowserChromePopover) =>
    setChrome((state) => toggleBrowserChromePopover(state, popover))
  const closePopover = () => setChrome(closeBrowserChromePopover)

  const browserControlUnavailable = () => !isLive() || !session().sessionId
  const browserControlModeDisabled = () => browserControlUnavailable() || props.browserBusy || !props.onBrowserControlMode
  const toggleBrowserControlMode = () => {
    if (browserControlModeDisabled()) return
    const nextMode = humanTakeoverActive() ? 'agent_control' : 'human_takeover'
    if (sendBrowserWsControl(nextMode)) return
    props.onBrowserControlMode?.(nextMode)
  }
  const runAction = (action: BrowserAction) => {
    if (controlsDisabled()) return
    if (sendBrowserWsAction(action)) return
    props.onBrowserAction?.(action)
  }
  const clearQueuedWheelTimer = () => {
    if (queuedWheelTimer === undefined) return
    window.clearTimeout(queuedWheelTimer)
    queuedWheelTimer = undefined
  }
  const clearLiveFrameTimer = () => {
    if (liveFrameTimer === undefined) return
    window.clearTimeout(liveFrameTimer)
    liveFrameTimer = undefined
  }
  const clearWsActionTimer = () => {
    if (wsActionTimer === undefined) return
    window.clearTimeout(wsActionTimer)
    wsActionTimer = undefined
  }
  const closeLiveFrameSource = () => {
    liveFrameSource?.close()
    liveFrameSource = undefined
  }
  const closeLiveFrameSocket = () => {
    liveFrameSocketGeneration += 1
    liveFrameSocket?.close()
    liveFrameSocket = undefined
    setLiveFrameWsConnected(false)
    clearWsActionPending()
  }
  const beginWsActionPending = () => {
    clearWsActionTimer()
    setWsActionPending(true)
    wsActionTimer = window.setTimeout(() => {
      wsActionTimer = undefined
      setWsActionPending(false)
      setLiveFrameWsFailed(true)
    }, 20_000)
  }
  const clearWsActionPending = () => {
    clearWsActionTimer()
    setWsActionPending(false)
  }
  const frameDataUrl = (payload: LiveFrameStreamPayload) => {
    const data = payload.dataBase64?.trim()
    if (!data) return null
    const mimeType = payload.mimeType === 'image/png' ? 'image/png' : 'image/jpeg'
    return `data:${mimeType};base64,${data}`
  }
  const handleBrowserWsMessage = (payload: BrowserWsServerMessage) => {
    if (payload.type === 'frame') {
      const dataUrl = frameDataUrl(payload)
      if (dataUrl) setStreamFrameSrc(dataUrl)
      return
    }
    if (payload.type === 'observation' && payload.observation) {
      clearWsActionPending()
      props.onBrowserSocketObservation?.(payload.observation, payload.session)
      return
    }
    if (payload.type === 'control' && payload.observation && payload.session) {
      clearWsActionPending()
      props.onBrowserSocketObservation?.(payload.observation, payload.session)
      return
    }
    if (payload.type === 'devtools' && payload.events?.length) {
      setDevtoolsEvents((current) => {
        const bySequence = new Map<number, BrowserDevtoolsEvent>()
        for (const event of current) bySequence.set(event.sequence, event)
        for (const event of payload.events ?? []) bySequence.set(event.sequence, event)
        return [...bySequence.values()]
          .sort((a, b) => a.sequence - b.sequence)
          .slice(-512)
      })
      return
    }
    if (payload.type === 'error') {
      clearWsActionPending()
      setLiveFrameWsFailed(true)
    }
  }
  const sendBrowserWsAction = (action: BrowserAction) => {
    if (!props.onBrowserSocketObservation || wsActionPending()) return false
    const socket = liveFrameSocket
    if (!socket || socket.readyState !== WebSocket.OPEN) return false
    try {
      beginWsActionPending()
      socket.send(JSON.stringify({
        type: 'action',
        actor: 'human',
        action,
        instruction: 'Human browser takeover action from Verevon.',
      }))
      return true
    } catch {
      clearWsActionPending()
      setLiveFrameWsFailed(true)
      return false
    }
  }
  const sendBrowserWsControl = (mode: BrowserSessionViewModel['controlMode']) => {
    if (!props.onBrowserSocketObservation) return false
    const socket = liveFrameSocket
    if (!socket || socket.readyState !== WebSocket.OPEN) return false
    try {
      socket.send(JSON.stringify({
        type: 'control',
        actor: 'human',
        mode,
      }))
      return true
    } catch {
      setLiveFrameWsFailed(true)
      return false
    }
  }
  const isLiveFrameSource = (src: string) => {
    const url = session().liveFrameUrl
    return Boolean(url && src.startsWith(url))
  }
  const scheduleLiveFrameRefresh = () => {
    clearLiveFrameTimer()
    if (!pollingLiveFrameAvailable()) return
    liveFrameTimer = window.setTimeout(() => {
      liveFrameTimer = undefined
      if (untrack(pollingLiveFrameAvailable)) setLiveFrameTick((tick) => tick + 1)
    }, LIVE_FRAME_REFRESH_MS)
  }
  const handleFrameLoad = (src: string) => {
    if (isLiveFrameSource(src)) scheduleLiveFrameRefresh()
  }
  const handleFrameError = (src: string) => {
    setBrokenFrameUrl(src)
    if (isLiveFrameSource(src)) scheduleLiveFrameRefresh()
  }
  const flushQueuedWheelAction = () => {
    if (controlsDisabled() || !queuedWheelAction) return
    const action = queuedWheelAction
    queuedWheelAction = null
    lastWheelDispatchMs = Date.now()
    runAction(action)
  }
  const scheduleQueuedWheelFlush = () => {
    if (queuedWheelTimer !== undefined) return
    const elapsed = Date.now() - lastWheelDispatchMs
    const delay = Math.max(0, BROWSER_WHEEL_THROTTLE_MS - elapsed)
    queuedWheelTimer = window.setTimeout(() => {
      untrack(() => {
        queuedWheelTimer = undefined
        flushQueuedWheelAction()
        if (queuedWheelAction && !props.browserBusy) scheduleQueuedWheelFlush()
      })
    }, delay)
  }
  const queueWheelAction = (action: BrowserWheelAction) => {
    queuedWheelAction = queuedWheelAction
      ? {
          ...action,
          delta_x: clampWheelDelta(queuedWheelAction.delta_x + action.delta_x),
          delta_y: clampWheelDelta(queuedWheelAction.delta_y + action.delta_y),
        }
      : action
    if (!props.browserBusy) scheduleQueuedWheelFlush()
  }
  const viewportPoint = (event: MouseEvent | WheelEvent, element: HTMLElement) => {
    const target = element.querySelector('img') ?? element
    const rect = target.getBoundingClientRect()
    if (rect.width <= 0 || rect.height <= 0) return null
    const localX = event.clientX - rect.left
    const localY = event.clientY - rect.top
    if (localX < 0 || localY < 0 || localX > rect.width || localY > rect.height) return null
    return {
      x: (localX / rect.width) * session().viewport.width,
      y: (localY / rect.height) * session().viewport.height,
    }
  }
  const runViewportClick = (event: MouseEvent & { currentTarget: HTMLDivElement }) => {
    if (controlsDisabled() || event.button !== 0) return
    const point = viewportPoint(event, event.currentTarget)
    if (!point) return
    event.preventDefault()
    event.currentTarget.focus({ preventScroll: true })
    runAction({ type: 'click_point', x: point.x, y: point.y })
  }
  const runViewportWheel = (event: WheelEvent & { currentTarget: HTMLDivElement }) => {
    if (browserControlUnavailable()) return
    const point = viewportPoint(event, event.currentTarget)
    if (!point) return
    event.preventDefault()
    const scale = event.deltaMode === WheelEvent.DOM_DELTA_LINE
      ? 16
      : event.deltaMode === WheelEvent.DOM_DELTA_PAGE
        ? session().viewport.height
        : 1
    const action: BrowserWheelAction = {
      type: 'mouse_wheel',
      x: point.x,
      y: point.y,
      delta_x: clampWheelDelta(event.deltaX * scale),
      delta_y: clampWheelDelta(event.deltaY * scale),
    }

    const now = Date.now()
    if (props.browserBusy || now - lastWheelDispatchMs < BROWSER_WHEEL_THROTTLE_MS) {
      queueWheelAction(action)
      return
    }
    lastWheelDispatchMs = now
    runAction(action)
  }
  const runViewportKey = (event: KeyboardEvent & { currentTarget: HTMLDivElement }) => {
    if (controlsDisabled() || event.metaKey || event.ctrlKey || event.altKey) return
    const key = browserKeyFromKeyboardEvent(event)
    if (!key) return
    event.preventDefault()
    runAction({ type: 'press', key })
  }
  const navigateFromAddress = () => {
    const target = browserOmniboxTarget(address(), session().url)
    if (!target) return
    setAddressInput(target)
    runAction({ type: 'navigate', url: target })
  }
  const prepareNewTabNavigation = async () => {
    if (controlsDisabled()) return
    setSelectedTimelineStep(null)
    await props.onBrowserNewTab?.()
    setAddressInput('')
    window.requestAnimationFrame(() => addressInputRef?.focus())
  }
  const suggestAction = async () => {
    if (modelActionDisabled()) return
    const suggestion = await props.onBrowserSuggestAction?.(goalInput().trim())
    setLastSuggestion(suggestion ?? null)
    setSuggestionDismissed(false)
  }
  const runModelLoop = async () => {
    if (modelLoopDisabled()) return
    setSelectedTimelineStep(null)
    const suggestion = await props.onBrowserAutoRun?.(goalInput().trim())
    setLastSuggestion(suggestion ?? null)
    setSuggestionDismissed(false)
  }
  const copyProfileId = async () => {
    const profileId = session().profileId
    if (!profileId) return
    try {
      await navigator.clipboard.writeText(profileId)
      setCopyState('copied')
    } catch {
      setCopyState('failed')
    }
    window.setTimeout(() => setCopyState('idle'), 1800)
  }

  createEffect(() => {
    const url = session().url
    if (url && lastObservedUrl() !== url) {
      setAddressInput(url)
      setLastObservedUrl(url)
    }
  })

  createEffect(() => {
    const sessionId = session().sessionId ?? ''
    if (sessionId === lastDevtoolsSessionId) return
    lastDevtoolsSessionId = sessionId
    setDevtoolsEvents(session().devtoolsEvents)
  })

  // Phase 5 HITL gate: a pending browser-action approval is the highest-
  // priority surface in the whole chrome — a blocked run needs a decision
  // now. Auto-open the evidence drawer the moment a NEW one appears (once
  // per approval key, so a user who deliberately closes the drawer again
  // isn't fought on every re-render), mirroring how AgentRunConsole's
  // approval deck scrolls itself into view.
  createEffect(() => {
    const approval = pendingApproval()
    if (!approval || approval.key === lastAutoOpenedApprovalKey) return
    lastAutoOpenedApprovalKey = approval.key
    setChrome((state) => (state.evidenceOpen ? state : { ...state, evidenceOpen: true, popover: null }))
  })

  createEffect(() => {
    const cachedEvents = session().devtoolsEvents
    if (cachedEvents.length === 0) return
    setDevtoolsEvents((current) => {
      const bySequence = new Map<number, BrowserDevtoolsEvent>()
      for (const event of current) bySequence.set(event.sequence, event)
      for (const event of cachedEvents) bySequence.set(event.sequence, event)
      return [...bySequence.values()]
        .sort((a, b) => a.sequence - b.sequence)
        .slice(-512)
    })
  })

  createEffect(() => {
    const transportKey = `${session().liveFrameWsUrl ?? ''}|${session().liveFrameStreamUrl ?? ''}`
    if (transportKey === lastLiveFrameTransportKey) return
    lastLiveFrameTransportKey = transportKey
    setLiveFrameWsFailed(false)
    setLiveFrameSseFailed(false)
  })

  createEffect(() => {
    const selected = selectedTimelineStep()
    if (selected === null) return
    if (!session().timeline.some((entry) => entry.step === selected)) {
      setSelectedTimelineStep(null)
    }
  })

  createEffect(() => {
    if (devtoolsTab() === 'vision' && !session().visualObservationArtifactId) {
      setDevtoolsTab('dom')
    }
  })

  createEffect(() => {
    if (controlsDisabled() || !queuedWheelAction) return
    clearQueuedWheelTimer()
    flushQueuedWheelAction()
  })

  createEffect(() => {
    if (!pollingLiveFrameAvailable()) {
      clearLiveFrameTimer()
      return
    }
    setLiveFrameTick((tick) => tick + 1)
  })

  createEffect(() => {
    const src = liveFrameWsSrc()
    closeLiveFrameSocket()
    setStreamFrameSrc(null)
    if (!src) return

    const generation = liveFrameSocketGeneration
    const socket = new WebSocket(src)
    liveFrameSocket = socket
    setLiveFrameWsConnected(false)
    setLiveFrameWsFailed(false)
    setLiveFrameSseFailed(false)

    socket.onopen = () => setLiveFrameWsConnected(true)
    socket.onmessage = (event) => {
      try {
        handleBrowserWsMessage(JSON.parse(String(event.data)) as BrowserWsServerMessage)
      } catch {
        // Ignore malformed socket frames; the transport fallback remains available.
      }
    }
    socket.onerror = () => {
      if (generation !== liveFrameSocketGeneration) return
      setLiveFrameWsFailed(true)
      clearWsActionPending()
    }
    socket.onclose = () => {
      if (generation !== liveFrameSocketGeneration) return
      setLiveFrameWsConnected(false)
      clearWsActionPending()
      setLiveFrameWsFailed(true)
    }

    onCleanup(closeLiveFrameSocket)
  })

  createEffect(() => {
    const src = liveFrameStreamSrc()
    closeLiveFrameSource()
    setStreamFrameSrc(null)
    if (!src) return

    const source = new EventSource(src)
    liveFrameSource = source
    setLiveFrameSseFailed(false)
    source.addEventListener('frame', (event) => {
      try {
        const payload = JSON.parse((event as MessageEvent<string>).data) as LiveFrameStreamPayload
        const dataUrl = frameDataUrl(payload)
        if (dataUrl) setStreamFrameSrc(dataUrl)
      } catch {
        // Ignore malformed stream frames; the fallback artifact frame remains visible.
      }
    })
    source.addEventListener('done', () => closeLiveFrameSource())
    source.addEventListener('error', () => {
      setLiveFrameSseFailed(true)
      closeLiveFrameSource()
    })
    onCleanup(closeLiveFrameSource)
  })

  onCleanup(clearQueuedWheelTimer)
  onCleanup(clearLiveFrameTimer)
  onCleanup(closeLiveFrameSource)
  onCleanup(closeLiveFrameSocket)

  createEffect(() => {
    if (!chrome().popover) return
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') closePopover()
    }
    document.addEventListener('keydown', onKeyDown)
    onCleanup(() => document.removeEventListener('keydown', onKeyDown))
  })

  return (
    <div
      class="knowledge-browser-chrome"
      classList={{ 'knowledge-browser-chrome--compact': props.compact }}
    >
      <div class="knowledge-browser-frame__topbar knowledge-browser-frame__topbar--minimal">
        <div class="knowledge-browser-frame__window">
          <span class="knowledge-browser-traffic" aria-hidden="true">
            <i />
            <i />
            <i />
          </span>
          <Show
            when={isLive()}
            fallback={<strong>{session().title || session().host || 'Browser'}</strong>}
          >
            <div
              class="knowledge-browser-chrome__tabstrip knowledge-browser-chrome__tabstrip--frame"
              role="tablist"
              aria-label="Nettleserfaner"
            >
              <For each={liveBrowserTabs()}>
                {(tab) => (
                  <button
                    type="button"
                    role="tab"
                    aria-selected={selectedTimelineStep() === null && tab.active}
                    class="knowledge-browser-tab"
                    classList={{ 'knowledge-browser-tab--active': selectedTimelineStep() === null && tab.active }}
                    title={`Live · ${liveTabLabel(tab)}`}
                    onClick={() => {
                      setSelectedTimelineStep(null)
                      if (!tab.active) void props.onBrowserSelectTab?.(tab.tabId)
                    }}
                  >
                    <span class="knowledge-browser-tab__dot" data-status={session().status} aria-hidden="true" />
                    <span class="knowledge-browser-tab__mode">Live</span>
                    <strong class="knowledge-browser-tab__label">{liveTabLabel(tab)}</strong>
                  </button>
                )}
              </For>
              <For each={timelineTabs()}>
                {(entry) => (
                  <button
                    type="button"
                    role="tab"
                    aria-selected={selectedTimelineStep() === entry.step}
                    class="knowledge-browser-tab"
                    classList={{ 'knowledge-browser-tab--active': selectedTimelineStep() === entry.step }}
                    title={`Snapshot fra steg ${entry.step} — ${snapshotTabLabel(entry)}`}
                    onClick={() => setSelectedTimelineStep((current) => (current === entry.step ? null : entry.step))}
                  >
                    <span class="knowledge-browser-tab__dot knowledge-browser-tab__dot--snapshot" aria-hidden="true" />
                    <span class="knowledge-browser-tab__mode">#{entry.step}</span>
                    <strong class="knowledge-browser-tab__label">{snapshotTabLabel(entry)}</strong>
                    <Show when={rationaleForStep(props.browserRationales ?? [], entry.step) || entry.visualObservationArtifactId}>
                      <span class="knowledge-browser-tab__markers">
                        <Show when={rationaleForStep(props.browserRationales ?? [], entry.step)}>
                          <Sparkles class="size-3" aria-label="AI-foreslått steg" />
                        </Show>
                        <Show when={entry.visualObservationArtifactId}>
                          <Eye class="size-3" aria-label="Visuell observasjon tilgjengelig" />
                        </Show>
                      </span>
                    </Show>
                  </button>
                )}
              </For>
              <button
                type="button"
                class="knowledge-browser-tab-new"
                aria-label="Ny fane"
                title="Ny fane"
                disabled={controlsDisabled()}
                onClick={() => void prepareNewTabNavigation()}
              >
                <Plus class="size-3.5" />
              </button>
            </div>
          </Show>
        </div>
        {props.frameControls}
      </div>

      <div class="knowledge-browser-chrome__toolbar" aria-label="Nettleserkontroller">
        <div class="knowledge-browser-chrome__nav">
          <button
            type="button"
            aria-label="Gå tilbake i nettleserøkten"
            title="Gå tilbake"
            disabled={controlsDisabled()}
            onClick={() => runAction({ type: 'back' })}
          >
            <ArrowLeft class="size-3.5" />
          </button>
          <button
            type="button"
            aria-label="Gå fremover i nettleserøkten"
            title="Gå fremover"
            disabled={controlsDisabled()}
            onClick={() => runAction({ type: 'forward' })}
          >
            <ArrowRight class="size-3.5" />
          </button>
          <button
            type="button"
            aria-label="Last nettlesersiden på nytt"
            title="Last på nytt"
            disabled={controlsDisabled()}
            onClick={() => runAction({ type: 'navigate', url: session().url })}
          >
            <RefreshCw class="size-3.5" />
          </button>
        </div>

        <div class="knowledge-browser-chrome__address">
          <button
            type="button"
            class="knowledge-browser-chrome__lock"
            aria-label="Profil, cookies og personvern for økten"
            aria-haspopup="dialog"
            aria-expanded={chrome().popover === 'profile'}
            aria-controls={profilePopoverId}
            title="Profil, cookies og personvern"
            onClick={() => togglePopover('profile')}
          >
            <LockKeyhole class="size-3.5" />
          </button>
          <input
            ref={(element) => { addressInputRef = element }}
            value={addressInput()}
            disabled={controlsDisabled()}
            aria-label="Nettleseradresse"
            spellcheck={false}
            onInput={(event) => setAddressInput(event.currentTarget.value)}
            onKeyDown={(event) => {
              if (event.key === 'Enter') {
                event.preventDefault()
                navigateFromAddress()
              }
            }}
          />
          <button
            type="button"
            class="knowledge-browser-chrome__go"
            aria-label="Naviger til adresse"
            title="Naviger"
            disabled={controlsDisabled() || !address()}
            onClick={navigateFromAddress}
          >
            <Send class="size-3.5" />
          </button>
        </div>

        <div class="knowledge-browser-chrome__status">
          <span
            class={`knowledge-browser-status knowledge-browser-status--${session().status}`}
            title={statusTitle()}
          >
            {SESSION_STATUS_LABELS[session().status]}
          </span>
          <span class="knowledge-browser-mode-badge" title={`Gjengivelsesmodus: ${session().renderMode}`}>
            {RENDER_MODE_LABELS[session().renderMode]}
          </span>
          <button
            type="button"
            class="knowledge-browser-control-chip"
            classList={{ 'knowledge-browser-control-chip--human': humanTakeoverActive() }}
            aria-pressed={humanTakeoverActive()}
            aria-label={humanTakeoverActive() ? 'Gi nettleserkontroll tilbake til AI-agenten' : 'Ta over nettleserkontrollen manuelt'}
            title={humanTakeoverActive() ? 'Gi kontroll til AI-agenten' : 'Ta over nettleseren'}
            disabled={browserControlModeDisabled()}
            onClick={toggleBrowserControlMode}
          >
            <Show
              when={humanTakeoverActive()}
              fallback={<><Bot class="size-3" aria-hidden="true" /> AI</>}
            >
              <Hand class="size-3" aria-hidden="true" /> Manuell
            </Show>
          </button>
          <Show when={evidenceIsEphemeral(session())}>
            <span
              class="knowledge-browser-zdr-chip"
              title="Zero Data Retention — flyktig evidens; skjermbilder, cookies og profil lagres ikke"
            >
              <ShieldCheck class="size-3" aria-hidden="true" /> ZDR · flyktig
            </span>
          </Show>
        </div>

        <div class="knowledge-browser-chrome__tools">
          <button
            type="button"
            class="knowledge-browser-toolbtn"
            aria-label="Ta skjermbilde"
            title="Skjermbilde"
            disabled={controlsDisabled()}
            onClick={() => runAction({ type: 'screenshot', full_page: false })}
          >
            <Camera class="size-3.5" />
          </button>
          <i class="knowledge-browser-chrome__divider" aria-hidden="true" />
          <button
            type="button"
            class="knowledge-browser-toolbtn knowledge-browser-toolbtn--ai"
            aria-label="Kjør ett AI-foreslått nettlesersteg"
            title="AI-steg"
            disabled={modelActionDisabled()}
            onClick={() => void suggestAction()}
          >
            <Sparkles class="size-3.5" />
          </button>
          <button
            type="button"
            class="knowledge-browser-toolbtn knowledge-browser-toolbtn--ai"
            aria-label="Kjør flere AI-foreslåtte nettlesersteg"
            title="AI-loop"
            disabled={modelLoopDisabled()}
            onClick={() => void runModelLoop()}
          >
            <Play class="size-3.5" />
          </button>
          <Show when={loopRunning()}>
            <Show
              when={loopStatus() === 'paused'}
              fallback={
                <button
                  type="button"
                  class="knowledge-browser-toolbtn knowledge-browser-toolbtn--ai"
                  aria-label="Pause AI-loopen mellom steg"
                  title="Pause AI-loop"
                  onClick={() => props.onBrowserLoopPause?.()}
                >
                  <Pause class="size-3.5" />
                </button>
              }
            >
              <button
                type="button"
                class="knowledge-browser-toolbtn knowledge-browser-toolbtn--ai"
                aria-label="Fortsett AI-loopen"
                title="Fortsett AI-loop"
                onClick={() => props.onBrowserLoopResume?.()}
              >
                <Play class="size-3.5" />
              </button>
            </Show>
            <button
              type="button"
              class="knowledge-browser-toolbtn knowledge-browser-toolbtn--stop"
              aria-label="Stopp AI-loopen"
              title="Stopp AI-loop"
              onClick={() => props.onBrowserLoopStop?.()}
            >
              <Square class="size-3.5" />
            </button>
          </Show>
          <Show when={loopStatus() !== 'idle'}>
            <span
              class={`knowledge-browser-loop-chip knowledge-browser-loop-chip--${loopStatus()}`}
              classList={{ 'knowledge-browser-loop-chip--error': Boolean(props.browserLoop?.error) }}
              role="status"
              aria-label="AI-loopstatus"
              title={loopTitle()}
            >
              {LOOP_STATUS_LABELS[loopStatus()]}
              <Show when={(props.browserLoop?.step ?? 0) > 0}>
                <strong>{props.browserLoop?.step}</strong>
              </Show>
              <Show when={props.browserLoop?.error}>
                <AlertCircle class="size-3" aria-hidden="true" />
              </Show>
            </span>
          </Show>
          <i class="knowledge-browser-chrome__divider" aria-hidden="true" />
          <button
            type="button"
            class="knowledge-browser-toolbtn"
            classList={{ 'knowledge-browser-toolbtn--on': chrome().actionsOpen }}
            aria-label="Vis eller skjul manuelle nettleserhandlinger"
            aria-expanded={chrome().actionsOpen}
            aria-controls={actionbarId}
            title="Manuelle handlinger"
            disabled={!isLive()}
            onClick={() => togglePanel('actions')}
          >
            <SquareMousePointer class="size-3.5" />
          </button>
          <button
            type="button"
            class="knowledge-browser-toolbtn"
            classList={{ 'knowledge-browser-toolbtn--on': chrome().devtoolsOpen }}
            aria-label="Vis eller skjul DevTools-panelet"
            aria-expanded={chrome().devtoolsOpen}
            aria-controls={devtoolsId}
            title="DevTools — DOM, console, network"
            disabled={!isLive()}
            onClick={() => togglePanel('devtools')}
          >
            <PanelRight class="size-3.5" />
            <Show when={session().policyDenials.length > 0}>
              <span class="knowledge-browser-toolbtn__badge knowledge-browser-toolbtn__badge--warn">
                {session().policyDenials.length}
              </span>
            </Show>
          </button>
          <button
            type="button"
            class="knowledge-browser-toolbtn"
            classList={{ 'knowledge-browser-toolbtn--on': chrome().evidenceOpen }}
            aria-label="Vis eller skjul tidslinje og evidens"
            aria-expanded={chrome().evidenceOpen}
            aria-controls={evidenceId}
            title="Tidslinje og evidens"
            disabled={!isLive()}
            onClick={() => togglePanel('evidence')}
          >
            <History class="size-3.5" />
            <Show when={session().timeline.length > 0}>
              <span class="knowledge-browser-toolbtn__badge">{session().timeline.length}</span>
            </Show>
          </button>
          <button
            type="button"
            class="knowledge-browser-toolbtn"
            aria-label="Flere valg"
            aria-haspopup="menu"
            aria-expanded={chrome().popover === 'overflow'}
            aria-controls={overflowPopoverId}
            title="Flere valg"
            onClick={() => togglePopover('overflow')}
          >
            <MoreVertical class="size-3.5" />
          </button>
        </div>

        <Show when={chrome().popover === 'profile'}>
          <>
            <div class="knowledge-browser-popover-backdrop" onClick={closePopover} />
            <div
              id={profilePopoverId}
              class="knowledge-browser-popover knowledge-browser-popover--profile"
              role="dialog"
              aria-label="Nettleserprofil og personvern"
            >
              <header class="knowledge-browser-popover__head">
                <LockKeyhole class="size-3.5" aria-hidden="true" />
                <span>{PROFILE_SCOPE_LABELS[session().profileScope]} profil</span>
              </header>
              <Show when={session().profileId} fallback={
                <div class="knowledge-browser-popover__row">
                  <span>Profil</span>
                  <strong>{session().profileLabel}</strong>
                </div>
              }>
                {(profileId) => (
                  <div class="knowledge-browser-popover__row">
                    <span>Profil-ID</span>
                    <span class="knowledge-browser-popover__id">
                      <code>{profileId()}</code>
                      <button
                        type="button"
                        class="knowledge-browser-popover__copy"
                        aria-label="Kopier profil-ID"
                        title="Kopier profil-ID"
                        onClick={() => void copyProfileId()}
                      >
                        <Copy class="size-3" />
                      </button>
                      <Show when={copyState() !== 'idle'}>
                        <em>{copyState() === 'copied' ? 'Kopiert' : 'Kunne ikke kopiere'}</em>
                      </Show>
                    </span>
                  </div>
                )}
              </Show>
              <div class="knowledge-browser-popover__row">
                <span>Cookies</span>
                <strong
                  classList={{ 'knowledge-browser-popover__value--persistent': session().profileStorage === 'persistent' }}
                >
                  {session().profileStorage === 'persistent' ? 'Lagres i profilen' : 'Lagres ikke'}
                </strong>
              </div>
              <Show when={probe()}>
                {(currentProbe) => (
                  <div class="knowledge-browser-popover__row">
                    <span>Probe</span>
                    <strong>
                      {currentProbe().cookies_count} cookies · {currentProbe().restorable ? 'gjenopprettbar' : 'ikke gjenopprettbar'}
                    </strong>
                  </div>
                )}
              </Show>
              <div class="knowledge-browser-popover__row">
                <span>Gjengivelse</span>
                <strong>
                  {RENDER_MODE_LABELS[session().renderMode]} · {session().viewport.width} × {session().viewport.height}
                </strong>
              </div>
              <div class="knowledge-browser-popover__row">
                <span>Kilde</span>
                <strong>{session().sourceLabel}</strong>
              </div>
              <Show when={session().degradedReason}>
                {(reason) => (
                  <div class="knowledge-browser-popover__row knowledge-browser-popover__row--warning">
                    <span>Degradert</span>
                    <strong title={reason()}>{reason()}</strong>
                  </div>
                )}
              </Show>
              <Show when={evidenceIsEphemeral(session())}>
                <div class="knowledge-browser-popover__row">
                  <span>ZDR</span>
                  <span class="knowledge-browser-zdr-chip">
                    <ShieldCheck class="size-3" aria-hidden="true" /> Flyktig evidens — ingen lagring
                  </span>
                </div>
              </Show>

              <Show when={props.profileManager}>
                {(manager) => (
                  <div class="knowledge-browser-popover__profiles">
                    <header class="knowledge-browser-popover__subhead">
                      <span>Alle profiler</span>
                      <button
                        type="button"
                        aria-label="Oppdater profillisten"
                        title="Oppdater"
                        disabled={manager().loading}
                        onClick={() => manager().onRefreshProfiles()}
                      >
                        <Show when={!manager().loading} fallback={<Loader2 class="size-3 dashboard-xsearch-spin" />}>
                          <RefreshCw class="size-3" />
                        </Show>
                      </button>
                    </header>
                    <ul class="knowledge-browser-profile-list">
                      <For each={manager().profiles} fallback={<li class="knowledge-browser-empty">Ingen lagrede profiler ennå.</li>}>
                        {(item) => (
                          <li classList={{ 'knowledge-browser-profile-list__item--active': item.profile_id === manager().selectedProfileId }}>
                            <button
                              type="button"
                              class="knowledge-browser-profile-list__select"
                              title={`Velg ${item.name || item.profile_id} for neste økt`}
                              onClick={() => manager().onSelectProfile(item.profile_id)}
                            >
                              <strong>{item.name?.trim() || compactArtifactId(item.profile_id)}</strong>
                              <span class="knowledge-browser-profile-list__scope">{PROFILE_SCOPE_LABELS[item.scope]}</span>
                            </button>
                          </li>
                        )}
                      </For>
                    </ul>

                    <Show when={manager().selectedProfileId}>
                      <div class="knowledge-browser-popover__profile-actions">
                        <button
                          type="button"
                          aria-label="Sjekk gjenopprettbarhet for valgt profil"
                          title="Sjekk profil"
                          disabled={manager().mutating}
                          onClick={() => manager().onProbeProfile()}
                        >
                          <ShieldCheck class="size-3" /> Sjekk
                        </button>
                        <Show
                          when={manager().renaming}
                          fallback={
                            <button
                              type="button"
                              aria-label="Gi valgt profil nytt navn"
                              disabled={manager().mutating}
                              onClick={() => manager().onStartRename()}
                            >
                              <Pencil class="size-3" /> Nytt navn
                            </button>
                          }
                        >
                          <div class="knowledge-browser-profile-rename">
                            <input
                              value={manager().renameProfileName}
                              placeholder="Profilnavn"
                              aria-label="Nytt profilnavn"
                              onInput={(event) => manager().onRenameProfileNameChange(event.currentTarget.value)}
                            />
                            <button
                              type="button"
                              disabled={manager().mutating || manager().renameProfileName.trim().length === 0}
                              onClick={() => manager().onConfirmRename()}
                            >
                              Lagre
                            </button>
                            <button type="button" onClick={() => manager().onCancelRename()}>Avbryt</button>
                          </div>
                        </Show>
                        <Show
                          when={manager().deleteArmed}
                          fallback={
                            <button
                              type="button"
                              class="knowledge-browser-popover__delete"
                              aria-label="Slett valgt profil"
                              disabled={manager().mutating}
                              onClick={() => manager().onArmDelete()}
                            >
                              <Trash2 class="size-3" /> Slett
                            </button>
                          }
                        >
                          <div class="knowledge-browser-profile-delete-confirm">
                            <span>Slette permanent?</span>
                            <button
                              type="button"
                              class="knowledge-browser-popover__delete"
                              disabled={manager().mutating}
                              onClick={() => manager().onConfirmDelete()}
                            >
                              Bekreft
                            </button>
                            <button type="button" onClick={() => manager().onCancelDelete()}>Avbryt</button>
                          </div>
                        </Show>
                      </div>
                    </Show>

                    <div class="knowledge-browser-profile-create">
                      <input
                        value={manager().newProfileName}
                        placeholder="Navn på ny profil"
                        aria-label="Navn på ny profil"
                        disabled={manager().zdrActive || manager().creating}
                        onInput={(event) => manager().onNewProfileNameChange(event.currentTarget.value)}
                      />
                      <select
                        value={manager().newProfileScope}
                        aria-label="Omfang for ny profil"
                        disabled={manager().zdrActive || manager().creating}
                        onChange={(event) => manager().onNewProfileScopeChange(event.currentTarget.value as CreatableBrowserProfileScope)}
                      >
                        <For each={CREATABLE_PROFILE_SCOPES}>
                          {(scope) => <option value={scope}>{PROFILE_SCOPE_LABELS[scope]}</option>}
                        </For>
                      </select>
                      <button
                        type="button"
                        disabled={manager().zdrActive || manager().creating}
                        onClick={() => manager().onCreateProfile()}
                      >
                        <Show when={!manager().creating} fallback={<Loader2 class="size-3 dashboard-xsearch-spin" />}>
                          <Plus class="size-3" />
                        </Show>
                        Opprett
                      </button>
                    </div>
                    <Show when={manager().zdrActive}>
                      <p class="knowledge-browser-popover__profile-hint">
                        ZDR-økter kan ikke bruke en lagret profil — kun efemert.
                      </p>
                    </Show>
                    <Show when={manager().error}>
                      {(message) => <p class="knowledge-browser-popover__profile-error">{message()}</p>}
                    </Show>
                  </div>
                )}
              </Show>
            </div>
          </>
        </Show>

        <Show when={chrome().popover === 'overflow'}>
          <>
            <div class="knowledge-browser-popover-backdrop" onClick={closePopover} />
            <div
              id={overflowPopoverId}
              class="knowledge-browser-popover knowledge-browser-popover--overflow"
              role="menu"
              aria-label="Flere nettleservalg"
            >
              <div class="knowledge-browser-popover__menu">
                <a
                  href={session().url}
                  target="_blank"
                  rel="noopener noreferrer"
                  role="menuitem"
                  onClick={closePopover}
                >
                  <ExternalLink class="size-3.5" aria-hidden="true" /> Åpne siden i ny fane
                </a>
                <Show when={session().visualObservationUrl}>
                  {(url) => (
                    <a
                      href={url()}
                      target="_blank"
                      rel="noopener noreferrer"
                      role="menuitem"
                      onClick={closePopover}
                    >
                      <Eye class="size-3.5" aria-hidden="true" /> Åpne visual_observation.json
                    </a>
                  )}
                </Show>
              </div>
            </div>
          </>
        </Show>
      </div>

      <Show when={isLive() && chrome().actionsOpen}>
        <div id={actionbarId} class="knowledge-browser-actionbar" aria-label="Manuelle nettleserhandlinger">
          <label class="knowledge-browser-actionbar__field knowledge-browser-actionbar__field--selector">
            <MousePointer2 class="size-3.5" aria-hidden="true" />
            <input
              value={selectorInput()}
              disabled={controlsDisabled()}
              placeholder="CSS selector"
              spellcheck={false}
              onInput={(event) => setSelectorInput(event.currentTarget.value)}
            />
          </label>
          <label class="knowledge-browser-actionbar__field">
            <TextCursorInput class="size-3.5" aria-hidden="true" />
            <input
              value={textInput()}
              disabled={controlsDisabled()}
              placeholder="Text"
              onInput={(event) => setTextInput(event.currentTarget.value)}
            />
          </label>
          <label class="knowledge-browser-actionbar__field knowledge-browser-actionbar__field--key">
            <Keyboard class="size-3.5" aria-hidden="true" />
            <input
              value={keyInput()}
              disabled={controlsDisabled()}
              placeholder="Key"
              spellcheck={false}
              onInput={(event) => setKeyInput(event.currentTarget.value)}
              onKeyDown={(event) => {
                if (event.key === 'Enter') {
                  event.preventDefault()
                  runAction({ type: 'press', key: keyValue() })
                }
              }}
            />
          </label>
          <label class="knowledge-browser-actionbar__field knowledge-browser-actionbar__field--goal">
            <Sparkles class="size-3.5" aria-hidden="true" />
            <input
              value={goalInput()}
              disabled={controlsDisabled()}
              placeholder="AI goal"
              onInput={(event) => setGoalInput(event.currentTarget.value)}
              onKeyDown={(event) => {
                if (event.key === 'Enter') {
                  event.preventDefault()
                  void suggestAction()
                }
              }}
            />
          </label>
          <div class="knowledge-browser-actionbar__buttons">
            <button
              type="button"
              aria-label="Klikk valgt selector"
              title="Klikk"
              disabled={selectorActionDisabled()}
              onClick={() => runAction({ type: 'click', selector: selector() })}
            >
              <MousePointer2 class="size-3.5" />
            </button>
            <button
              type="button"
              aria-label="Skriv tekst i valgt selector"
              title="Skriv"
              disabled={typeActionDisabled()}
              onClick={() => runAction({ type: 'type', selector: selector(), text: textValue() })}
            >
              <TextCursorInput class="size-3.5" />
            </button>
            <button
              type="button"
              aria-label="Send tastetrykk"
              title="Tast"
              disabled={controlsDisabled()}
              onClick={() => runAction({ type: 'press', key: keyValue() })}
            >
              <Keyboard class="size-3.5" />
            </button>
            <button
              type="button"
              aria-label="Vent på valgt selector"
              title="Vent på selector"
              disabled={selectorActionDisabled()}
              onClick={() => runAction({ type: 'wait_for', selector: selector(), timeout_ms: 5000 })}
            >
              <Timer class="size-3.5" />
            </button>
            <button
              type="button"
              aria-label="Rull til valgt selector"
              title="Rull"
              disabled={selectorActionDisabled()}
              onClick={() => runAction({ type: 'scroll', target: selector() })}
            >
              <Navigation class="size-3.5" />
            </button>
          </div>
        </div>
      </Show>

      <div
        class="knowledge-browser-chrome__body"
        classList={{ 'knowledge-browser-chrome__body--devtools': isLive() && chrome().devtoolsOpen }}
      >
        <Show when={isLive()} fallback={props.children}>
          <div class="knowledge-browser-chrome__page" aria-label="Gjengitt nettleserside">
            <Show when={pendingApproval()}>
              {(approval) => (
                <div
                  class="knowledge-browser-ai-bubble knowledge-browser-ai-bubble--approval"
                  role="alert"
                  aria-label="Venter på godkjenning"
                >
                  <AlertCircle class="size-3.5" aria-hidden="true" />
                  <div class="knowledge-browser-ai-bubble__body">
                    <span class="knowledge-browser-ai-bubble__kind">
                      {browserApprovalRiskLabel(approval().riskCategory)}
                    </span>
                    <p>{approval().reason || 'Handlingen krever godkjenning før den utføres.'}</p>
                    <Show when={approval().url || approval().selector}>
                      <p class="knowledge-browser-ai-bubble__meta">
                        <Show when={approval().actionType}>{approval().actionType} · </Show>
                        <Show when={approval().url}>{compactBrowserUrl(approval().url)}</Show>
                        <Show when={approval().selector}> · {approval().selector}</Show>
                      </p>
                    </Show>
                    <div class="verevon-run-approval__actions">
                      <button
                        type="button"
                        class="verevon-run-approval__approve"
                        disabled={decidingBrowserApprovalKeys().includes(approval().key)}
                        onClick={() => props.onDecideBrowserApproval?.(approval().key, 'approve')}
                      >
                        <Show when={decidingBrowserApprovalKeys().includes(approval().key)}>
                          <Loader2 class="size-3.5 verevon-run-spin" />
                        </Show>
                        Godkjenn
                      </button>
                      <button
                        type="button"
                        class="verevon-run-approval__reject"
                        disabled={decidingBrowserApprovalKeys().includes(approval().key)}
                        onClick={() => props.onDecideBrowserApproval?.(approval().key, 'reject')}
                      >
                        <Show when={decidingBrowserApprovalKeys().includes(approval().key)}>
                          <Loader2 class="size-3.5 verevon-run-spin" />
                        </Show>
                        Avslå
                      </button>
                    </div>
                  </div>
                </div>
              )}
            </Show>
            <Show when={!pendingApproval() && !suggestionDismissed() && (props.browserLoopRationale?.reason ?? lastSuggestion()?.suggestion.reason)}>
              {(reason) => (
                <div class="knowledge-browser-ai-bubble" role="status" aria-label="Siste AI-forslag">
                  <Sparkles class="size-3.5" aria-hidden="true" />
                  <div class="knowledge-browser-ai-bubble__body">
                    <span class="knowledge-browser-ai-bubble__kind">
                      {props.browserLoopRationale
                        ? `${props.browserLoopRationale.actionType ?? 'observe'} · steg ${props.browserLoopRationale.step}`
                        : lastSuggestion()?.suggestion.done ? 'done' : lastSuggestion()?.suggestion.action?.type ?? 'no-action'}
                    </span>
                    <p>{reason()}</p>
                  </div>
                  <button
                    type="button"
                    aria-label="Lukk AI-forslaget"
                    onClick={() => setSuggestionDismissed(true)}
                  >
                    <X class="size-3" />
                  </button>
                </div>
              )}
            </Show>
            <For each={session().commentAnchors}>
              {(anchor) => (
                <span
                  class="knowledge-browser-comment-anchor"
                  style={{
                    left: `${anchor.x * 100}%`,
                    top: `${anchor.y * 100}%`,
                  }}
                  aria-label={`Kommentar ${anchor.label}`}
                >
                  {anchor.label}
                </span>
              )}
            </For>
            <Show
              when={frameUrl()}
              keyed
              fallback={
                <div class="knowledge-browser-chrome__domlist">
                  <DomNodeList nodes={session().domNodes} />
                </div>
              }
            >
              {(src) => (
                <div
                  class="knowledge-browser-screenshot knowledge-browser-screenshot--interactive"
                  aria-label="Interaktiv Chromium-side. Klikk, scroll eller fokuser for tastatur."
                  role="application"
                  tabIndex={0}
                  onClick={runViewportClick}
                  onKeyDown={runViewportKey}
                  onWheel={runViewportWheel}
                >
                  <img
                    src={src}
                    alt={`Gjengitt nettleserside for ${session().title}`}
                    decoding="async"
                    onLoad={() => handleFrameLoad(src)}
                    onError={() => handleFrameError(src)}
                  />
                </div>
              )}
            </Show>
          </div>

          <Show when={chrome().devtoolsOpen}>
            <aside id={devtoolsId} class="knowledge-browser-devtools" aria-label="Nettleserinspektør">
              <header class="knowledge-browser-devtools__head">
                <span>DevTools</span>
                <span class="knowledge-browser-devtools__stats">
                  {session().domNodes.length}/{session().nodeCount ?? session().domNodes.length} DOM · {liveNetworkEntries().length} network
                </span>
                <Show when={shotArtifactId()}>
                  {(artifactId) => <code title={artifactId()}>shot {compactArtifactId(artifactId())}</code>}
                </Show>
                <button
                  type="button"
                  aria-label="Lukk DevTools-panelet"
                  onClick={() => togglePanel('devtools')}
                >
                  <X class="size-3.5" />
                </button>
              </header>
              <div class="knowledge-browser-devtools__tabs" role="tablist" aria-label="Observasjonspaneler">
                <button
                  type="button"
                  role="tab"
                  aria-selected={devtoolsTab() === 'dom'}
                  class="knowledge-browser-devtools__tab"
                  classList={{ 'knowledge-browser-devtools__tab--active': devtoolsTab() === 'dom' }}
                  onClick={() => setDevtoolsTab('dom')}
                >
                  <Code2 class="size-3.5" /> DOM <em>{session().nodeCount ?? session().domNodes.length}</em>
                </button>
                <button
                  type="button"
                  role="tab"
                  aria-selected={devtoolsTab() === 'console'}
                  class="knowledge-browser-devtools__tab"
                  classList={{ 'knowledge-browser-devtools__tab--active': devtoolsTab() === 'console' }}
                  onClick={() => setDevtoolsTab('console')}
                >
                  <Terminal class="size-3.5" /> Console <em>{liveConsoleEntries().length}</em>
                </button>
                <button
                  type="button"
                  role="tab"
                  aria-selected={devtoolsTab() === 'network'}
                  class="knowledge-browser-devtools__tab"
                  classList={{ 'knowledge-browser-devtools__tab--active': devtoolsTab() === 'network' }}
                  onClick={() => setDevtoolsTab('network')}
                >
                  <Network class="size-3.5" /> Network <em>{liveNetworkEntries().length}</em>
                </button>
                <Show when={session().visualObservationArtifactId}>
                  <button
                    type="button"
                    role="tab"
                    aria-selected={devtoolsTab() === 'vision'}
                    class="knowledge-browser-devtools__tab"
                    classList={{ 'knowledge-browser-devtools__tab--active': devtoolsTab() === 'vision' }}
                    onClick={() => setDevtoolsTab('vision')}
                  >
                    <Eye class="size-3.5" /> Vision
                  </button>
                </Show>
              </div>
              <div class="knowledge-browser-devtools__content">
                <Switch>
                  <Match when={devtoolsTab() === 'dom'}>
                    <p class="knowledge-browser-devtools__caption">
                      Interaktive noder · {session().domNodes.length} av {session().nodeCount ?? session().domNodes.length}
                    </p>
                    <DomNodeList nodes={session().domNodes} />
                  </Match>
                  <Match when={devtoolsTab() === 'console'}>
                    <div class="knowledge-browser-console-list">
                      <For
                        each={liveConsoleEntries()}
                        fallback={<p class="knowledge-browser-empty">Ingen console-hendelser.</p>}
                      >
                        {(entry) => (
                          <div class={`knowledge-browser-console-row knowledge-browser-console-row--${consoleTone(entry.level)}`}>
                            <span>{entry.level}</span>
                            <p>{entry.text}</p>
                          </div>
                        )}
                      </For>
                    </div>
                  </Match>
                  <Match when={devtoolsTab() === 'network'}>
                    <div class="knowledge-browser-network-list">
                      <For
                        each={liveNetworkEntries()}
                        fallback={<p class="knowledge-browser-empty">Ingen nettverkskall returnert ennå.</p>}
                      >
                        {(entry) => (
                          <div class={`knowledge-browser-network-row knowledge-browser-network-row--${networkTone(entry.status)}`}>
                            <span>{entry.method}</span>
                            <strong>{entry.status}</strong>
                            <p title={entry.url}>{compactBrowserUrl(entry.url)}</p>
                          </div>
                        )}
                      </For>
                    </div>
                  </Match>
                  <Match when={devtoolsTab() === 'vision'}>
                    <Show when={session().visualObservationArtifactId}>
                      {(artifactId) => (
                        <div class="knowledge-browser-vision-artifact">
                          <span>visual_observation.json</span>
                          <code>{compactArtifactId(artifactId())}</code>
                          <Show when={session().visualObservationUrl}>
                            {(url) => (
                              <a href={url()} target="_blank" rel="noopener noreferrer" aria-label="Åpne visuell observasjon">
                                <ExternalLink class="size-3.5" />
                              </a>
                            )}
                          </Show>
                        </div>
                      )}
                    </Show>
                  </Match>
                </Switch>
              </div>
              <Show when={session().policyDenials.length > 0}>
                <div class="knowledge-browser-devtools__policy">
                  <header>
                    <span><ShieldCheck class="size-3.5" /> Policy</span>
                    <strong>{session().policyDenials.length}</strong>
                  </header>
                  <For each={session().policyDenials.slice(0, 4)}>
                    {(denial) => (
                      <p class="knowledge-browser-policy-denial">
                        <AlertCircle class="size-3.5" /> {denial}
                      </p>
                    )}
                  </For>
                </div>
              </Show>
            </aside>
          </Show>
        </Show>
      </div>

      <Show when={isLive() && chrome().evidenceOpen}>
        <div id={evidenceId} class="knowledge-browser-evidence" role="region" aria-label="Tidslinje og evidens">
          <header class="knowledge-browser-evidence__head">
            <History class="size-3.5" aria-hidden="true" />
            <span>Tidslinje</span>
            <strong>{session().timeline.length} steg · {session().replayEvents.length} hendelser</strong>
            <button
              type="button"
              aria-label="Lukk tidslinjen"
              onClick={() => togglePanel('evidence')}
            >
              <X class="size-3.5" />
            </button>
          </header>
          <Show when={(props.browserApprovals ?? []).length > 0}>
            <div class="knowledge-browser-evidence__approvals" aria-label="Godkjenninger">
              <header class="knowledge-browser-evidence__approvals-head">
                <ShieldCheck class="size-3.5" aria-hidden="true" />
                <span>Godkjenninger</span>
                <strong>{(props.browserApprovals ?? []).length}</strong>
              </header>
              <div class="verevon-run-approvals" role="group" aria-label="Nettleserhandlinger som krever godkjenning">
                <For each={props.browserApprovals}>
                  {(approval) => (
                    <BrowserApprovalCard
                      approval={approval}
                      deciding={decidingBrowserApprovalKeys().includes(approval.key)}
                      onDecide={props.onDecideBrowserApproval}
                    />
                  )}
                </For>
              </div>
            </div>
          </Show>
          <Show
            when={session().timeline.length > 0}
            fallback={<p class="knowledge-browser-evidence__hint">Ingen steg registrert ennå — naviger eller kjør en handling for å bygge tidslinjen.</p>}
          >
            <Show when={session().replayEvents.length > 0}>
              <div class="knowledge-browser-replay" aria-label="Agent replay">
                <header>
                  <span>Replay</span>
                  <strong>{session().replayEvents.length}</strong>
                </header>
                <For each={replayEvents()}>
                  {(event) => (
                    <div class={`knowledge-browser-replay__event knowledge-browser-replay__event--${event.kind}`}>
                      <span class="knowledge-browser-replay__kind">{event.kind}</span>
                      <div>
                        <strong>{replayEventLabel(event)}</strong>
                        <p>{replayEventMeta(event)}</p>
                      </div>
                      <Show when={event.screenshotArtifactId || event.visualObservationArtifactId || event.tabId}>
                        <span class="knowledge-browser-replay__markers">
                          <Show when={event.screenshotArtifactId}>
                            <Camera class="size-3" aria-label="Screenshot" />
                          </Show>
                          <Show when={event.visualObservationArtifactId}>
                            <Eye class="size-3" aria-label="Visuell observasjon" />
                          </Show>
                          <Show when={event.tabId}>
                            <span>{event.tabId}</span>
                          </Show>
                        </span>
                      </Show>
                    </div>
                  )}
                </For>
              </div>
            </Show>
            <div class="knowledge-browser-timeline" aria-label="Nettleserhistorikk">
              <button
                type="button"
                classList={{ 'knowledge-browser-timeline__step--active': selectedTimelineStep() === null }}
                onClick={() => setSelectedTimelineStep(null)}
              >
                <span>live</span>
                <strong>{session().title}</strong>
              </button>
              <For each={timelineTabs()}>
                {(entry) => (
                  <button
                    type="button"
                    classList={{ 'knowledge-browser-timeline__step--active': selectedTimelineStep() === entry.step }}
                    title="Vis stegdetaljer med evidens"
                    onClick={() => setSelectedTimelineStep((current) => (current === entry.step ? null : entry.step))}
                  >
                    <span>#{entry.step}</span>
                    <strong>{entry.title || compactBrowserUrl(entry.url || session().url)}</strong>
                    <Show when={rationaleForStep(props.browserRationales ?? [], entry.step) || entry.visualObservationArtifactId}>
                      <span class="knowledge-browser-timeline__markers">
                        <Show when={rationaleForStep(props.browserRationales ?? [], entry.step)}>
                          <Sparkles class="size-3" aria-label="AI-foreslått steg" />
                        </Show>
                        <Show when={entry.visualObservationArtifactId}>
                          <Eye class="size-3" aria-label="Visuell observasjon tilgjengelig" />
                        </Show>
                      </span>
                    </Show>
                  </button>
                )}
              </For>
            </div>
            <Show
              when={selectedTimelineDetail()}
              fallback={<p class="knowledge-browser-evidence__hint">Velg et steg for å se før/etter-evidens, deltaer og artefakter.</p>}
            >
              {(detail) => (
                <BrowserTimelineDetailPanel
                  detail={detail()}
                  onClose={() => setSelectedTimelineStep(null)}
                  rationale={selectedRationale()}
                  sessionId={session().sessionId}
                />
              )}
            </Show>
          </Show>
        </div>
      </Show>
    </div>
  )
}
