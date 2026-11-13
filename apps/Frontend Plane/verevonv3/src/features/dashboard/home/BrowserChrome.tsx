import {
  AlertCircle,
  ArrowLeft,
  ArrowRight,
  Bot,
  Camera,
  Code2,
  Copy,
  Download,
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
  Search,
  Send,
  ShieldCheck,
  Sparkles,
  Square,
  SquareMousePointer,
  Star,
  Terminal,
  TextCursorInput,
  Timer,
  Trash2,
  X,
} from '@/shared/icons'
import {
  createEffect,
  createMemo,
  createSignal,
  createUniqueId,
  For,
  Match,
  onCleanup,
  Show,
  Switch,
  untrack,
} from 'solid-js'
import type { JSX } from '@solidjs/web'
import type {
  BrowserAction,
  BrowserActionSuggestionResponse,
  BrowserDevtoolsEvent,
  BrowserObservation,
  BrowserOwnerTimelineItem,
  BrowserProfileRestoreProbe,
  BrowserProfileSummary,
  BrowserSession,
  BrowserTab,
  CreatableBrowserProfileScope,
} from '@/shared/api/browser-client'
import {
  browserErrorMessage,
  controlBrowserTab,
  getBrowserSession,
  stopBrowserAttach,
  streamBrowserEvents,
} from '@/shared/api/browser-client'
import { ApiError } from '@/shared/api/http'
import { getSession } from '@/shared/session/session-store'
import { browserActionTarget, type BrowserActionTarget } from './browser-action-target'
import type { ApprovalDecision } from '@/shared/api/orchestration-client'
import { ERROR_COPY } from '@/shared/i18n/errors'
import {
  browserTabMenuActionSchema,
  browserTabMenuItems,
  browserTabMenuOutsideAction,
  type BrowserTabMenuItem,
} from '@/shared/actions/browser-tab-menu'
import { isBrowserLoopRunning, type BrowserLoopState } from './browser-loop'
import { BrowserBlockedNotice } from './BrowserBlockedNotice'
import { blockedStateFor } from './browser-blocked'
import {
  browserApprovalStatusLabel,
  browserRiskLabel,
} from '@/shared/approvals/approval-copy'
import { useI18n } from '@/shared/i18n'
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
  type BrowserNetworkEntry,
  type BrowserReplayViewEvent,
  type BrowserSessionViewModel,
  type BrowserStepRationale,
  type BrowserTimelineViewEntry,
} from './browser-session'
import { BrowserTimelineDetailPanel, compactBrowserUrl, consoleTone, networkTone } from './BrowserTimelineDetail'
import { OmniboxField } from '@/features/browser/omnibox/OmniboxField'
import { BrowserNavigationError } from '@/features/browser/omnibox/BrowserNavigationError'
import type { BrowserNavigationFailure } from '@/features/browser/session/browser-navigation-error'
import { BrowserSearchResults } from '@/features/browser/omnibox/BrowserSearchResults'
import { findInDomNodes, stepFindIndex } from '@/features/browser/find/page-find'
import { currentOmniboxHistoryKey, recordOmniboxVisit } from '@/features/browser/omnibox/omnibox-history'
import {
  isBookmarked,
  toggleBrowserBookmark,
  useBrowserBookmarks,
} from '@/features/browser/bookmarks/browser-bookmarks'
import {
  browserOmniboxSearchEnabled,
  resolveOmniboxIntent,
  type OmniboxIntent,
} from '@/features/browser/omnibox/omnibox-resolver'
import { hostnameOf } from './knowledge-preview'
import { ConsoleFilterBar, emptyConsoleFilter, filterConsoleEntries, type ConsoleFilter } from '@/features/browser/evidence/ConsoleFilterBar'
import { EgressReceiptsPanel } from '@/features/browser/evidence/EgressReceiptsPanel'
import { ProcedurePanel } from '@/features/browser/evidence/ProcedurePanel'
import { BrowserReplayScrubber } from '@/features/browser/replay/BrowserReplayScrubber'
import {
  replayFallbackFrame,
  replayScrubberAvailable,
  replayScrubberEnabled,
  replayScrubSteps,
} from '@/features/browser/replay/browser-replay-scrubber'
import {
  browserActionBriefEnabled,
  browserActionPill,
  browserActionTitle,
  factsFromTimelineEntry,
} from '@/features/browser/replay/browser-action-brief'
import { BrowserControlStrip } from '@/features/browser/dock/BrowserControlStrip'
import { useAssistantBrowser } from '@/features/core/assistant-agent/assistant-browser-context'
import { assistantBrowserTarget } from '@/features/core/assistant-agent/assistant-browser-agent'
import { BrowserApprovalDecisionCard } from '@/features/browser/approval/BrowserApprovalDecisionCard'
import {
  APPROVAL_REDIRECT_ENABLED,
  browserApprovalToCardModel,
  browserGateOnOtpCard,
  browserGateUsesDecisionCard,
} from '@/features/browser/approval/approval-model'
import {
  sessionHasHumanBrowsing,
  startSessionKeepAlive,
  type KeepAliveOutcome,
} from '@/features/browser/session/session-keepalive'

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

/**
 * Quarry's compare-and-set refusal on a tab's epoch (Q2-WS8.I, 409). The gateway passes Quarry's envelope through,
 * which carries it as code CONFLICT with this message, so the message is read as well as a typed code.
 */
function isStaleTabEpoch(reason: unknown): boolean {
  return reason instanceof ApiError
    && reason.status === 409
    && (reason.code === 'stale_tab_epoch' || reason.message === 'stale_tab_epoch')
}

/** Copy for a tab change or a Stop agent refused for a reason that has no row of its own in errors.ts. */
const TAB_CHANGE_FALLBACK = {
  no: 'Kunne ikke endre hvem som styrer fanen. Prøv igjen.',
  en: 'Could not change who controls the tab. Try again.',
}
const STOP_AGENT_FALLBACK = {
  no: 'Kunne ikke stoppe agenten. Prøv igjen.',
  en: 'Could not stop the agent. Try again.',
}

type TabControlAction = 'take_over' | 'give_back'
type DevtoolsTab = 'console' | 'dom' | 'network' | 'vision' | 'egress'
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

/** The durable Quarry activity log is deliberately compact. It is an audit
 * record, not a replay: no page body, screenshot, or DevTools payload is
 * inferred from it. */
function ownerTimelineEventLabel(event: BrowserOwnerTimelineItem): string {
  if (event.kind === 'action') return event.action?.replaceAll('_', ' ') || 'Nettleserhandling'
  if (event.kind === 'control') return event.mode === 'human_takeover' ? 'Menneske tok over' : 'Agentkontroll aktiv'
  if (event.kind === 'tab') {
    if (event.operation === 'opened') return 'Ny fane'
    if (event.operation === 'selected') return 'Fane valgt'
    if (event.operation === 'closed') return 'Fane lukket'
    return 'Fanehendelse'
  }
  if (event.kind === 'devtools') return 'DevTools-sammendrag'
  return event.state === 'closed' ? 'Økt lukket' : 'Økt startet'
}

/** Exported for its test: the error line of an owner-timeline row is end-user copy. */
export function ownerTimelineEventMeta(event: BrowserOwnerTimelineItem): string {
  const parts = [
    event.outcome,
    event.errorCode ? (ERROR_COPY[event.errorCode]?.no ?? 'Feil') : null,
    event.initiatedBy ?? null,
    event.tabId ?? event.activeTabId ?? null,
    event.eventCount !== undefined ? `${event.eventCount} hendelser` : null,
  ].filter(Boolean)
  return parts.length > 0 ? parts.join(' · ') : new Date(event.occurredAt).toLocaleString('nb-NO')
}

/**
 * The Network rows of a live session: the rows the projection carries, then one row per response
 * the live DevTools stream reported, with the method of its request. Kept out of the component's
 * closure on purpose: with the loop inside it, the checker typed the captured `devtoolsEvents` and
 * `session` as `never` after the loop (TS2349 at both reads).
 */
function liveNetworkRows(sessionRows: BrowserNetworkEntry[], devtools: BrowserDevtoolsEvent[]): BrowserNetworkEntry[] {
  const requestMethods = new Map<string, string>()
  for (const event of devtools) {
    if (event.name === 'Network.requestWillBeSent' && event.url && event.method) {
      requestMethods.set(event.url, event.method)
    }
  }
  const responses: BrowserNetworkEntry[] = devtools
    .filter((event) => event.name === 'Network.responseReceived' && event.url)
    .map((event) => ({
      content_type: null,
      method: event.url ? requestMethods.get(event.url) ?? 'GET' : 'GET',
      status: event.status ?? 0,
      url: event.url ?? '',
    }))
  return [...sessionRows, ...responses].slice(-120)
}

/** DOM-nodeliste delt mellom sidevisningens fallback og DevTools-panelet. */
/** Interactive kinds for selective observation (slice 8). */
const interactiveElementKinds = new Set(['button', 'link', 'input', 'checkbox', 'radio', 'select', 'textarea', 'menuitem'])

function DomNodeList(props: { nodes: BrowserSessionViewModel['domNodes'] }) {
  const [interactiveOnly, setInteractiveOnly] = createSignal(false)
  const nodes = () => {
    const filterOn = import.meta.env.VITE_BROWSER_ELEMENT_FILTER === 'true' && interactiveOnly()
    return filterOn ? props.nodes.filter((node) => interactiveElementKinds.has(node.kind)) : props.nodes
  }
  return (
    <div class="knowledge-browser-live-dom">
      <Show when={import.meta.env.VITE_BROWSER_ELEMENT_FILTER === 'true'}>
        <label class="knowledge-browser-live-dom__filter">
          <input type="checkbox" checked={interactiveOnly()} onChange={(event) => setInteractiveOnly(event.currentTarget.checked)} />
          Kun interaktive (valgobservasjon)
        </label>
      </Show>
      <For
        each={nodes()}
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
  const i18n = useI18n()
  const approval = () => props.approval
  const isPending = () => approval().status === 'pending'
  return (
    <div
      class={['verevon-run-approval', {
        [`verevon-run-approval--${approval().status}`]: !isPending(),
      }]}
    >
      <div class="verevon-run-approval__head">
        <span
          class={['verevon-run-approval__badge', {
            'verevon-run-approval__badge--granted': approval().status === 'granted',
            'verevon-run-approval__badge--denied': approval().status === 'denied' || approval().status === 'timed_out',
          }]}
        >
          <Show when={isPending()} fallback={approval().status === 'granted' ? <ShieldCheck class="size-3" /> : <AlertCircle class="size-3" />}>
            <Pause class="size-3" />
          </Show>
          {browserApprovalStatusLabel(approval().status, i18n)}
        </span>
        <span class="verevon-run-approval__kind">{browserRiskLabel(approval().riskCategory, i18n)}</span>
      </div>
      <p class="verevon-run-approval__detail">
        {approval().reason || i18n.tr('Handlingen krever godkjenning før den utføres.', 'The action needs approval before it is carried out.')}
      </p>
      <Show when={approval().url || approval().selector}>
        <p class="verevon-run-approval__meta">
          <Show when={approval().actionType}>{approval().actionType} · </Show>
          <Show when={approval().url}>{compactBrowserUrl(approval().url)}</Show>
          <Show when={approval().selector}> · {approval().selector}</Show>
        </p>
      </Show>
      <Show when={approval().decidedBy}>
        <p class="verevon-run-approval__meta">{i18n.tr('Avgjort av', 'Decided by')} {approval().decidedBy}</p>
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
            {i18n.tr('Godkjenn', 'Approve')}
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
            {i18n.tr('Avslå', 'Reject')}
          </button>
        </div>
      </Show>
    </div>
  )
}

/**
 * E3.2-b swap: a pending browser-action gate on the shared keyboard-first card
 * (W3-B1-06), mounted only with VITE_BROWSER_APPROVAL_CARD. The parent still
 * owns the decision: the card's `decide` hands the gate's key to `onDecide`,
 * so the in-flight keys and the refresh stay where they were. The redirect box
 * stays off (APPROVAL_REDIRECT_ENABLED) until execution-core reads redirect
 * text. Renders nothing for a gate that is no longer pending or has no approval id
 * yet. A one-time-code fill (E3.1-i) gets the OTP variant here while
 * VITE_BROWSER_OTP_APPROVAL_CARD is on, and no generic button is offered for it.
 */
function BrowserGateDecisionCard(props: {
  approval: BrowserApprovalEntry
  onDecide?: (approvalKey: string, decision: ApprovalDecision) => void
  /** "I will type it myself" on a one-time-code fill (E3.1-i). Omitted when the takeover cannot start. */
  onTakeOver?: () => void
}) {
  const model = () => browserApprovalToCardModel(props.approval)
  return (
    <Show when={model()}>
      {(card) => (
        <BrowserApprovalDecisionCard
          approval={card()}
          allowRedirect={APPROVAL_REDIRECT_ENABLED}
          decide={(request) => {
            props.onDecide?.(props.approval.key, request.decision)
            return Promise.resolve()
          }}
          onTakeOver={props.onTakeOver}
        />
      )}
    </Show>
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
  /** Opt in only for an owner-authorized, single-tab session. The event is presentation, not control. */
  showActionTargets?: boolean
  /** A person opened this browser directly for browsing. */
  personalBrowsing?: boolean
  allowAssistantContinuation?: boolean
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
  /** A navigation that did not load (WS9.B): shown as an error card with Retry. */
  browserNavigationError?: BrowserNavigationFailure | null
  onBrowserNavigationDismiss?: () => void
  onBrowserNavigationRetry?: (url: string) => void
  onBrowserAction?: (action: BrowserAction) => void
  onBrowserAutoRun?: (goal: string) => Promise<BrowserActionSuggestionResponse | null>
  onBrowserControlMode?: (mode: BrowserSessionViewModel['controlMode'], expectedEpoch?: number | null) => void
  /** WS8.D: one keep-alive call for an open human session. Absent means no timer runs. */
  onKeepAlive?: () => Promise<KeepAliveOutcome>
  onBrowserLoopPause?: () => void
  onBrowserLoopResume?: () => void
  onBrowserLoopStop?: () => void
  onBrowserNewTab?: () => Promise<void> | void
  /** Omnibox "Ask Verevon": hands the typed text to chat. Omit it and the row is not offered. */
  onAskVerevon?: (text: string) => void
  /** Omnibox search (WS9.A). Defaults to the build flag VITE_BROWSER_OMNIBOX_SEARCH. */
  searchEnabled?: boolean
  onBrowserSelectTab?: (tabId: string) => Promise<void> | void
  /** Tab context menu (WS9 tab strip): close the named tab. Omit it and Close is not offered. */
  onBrowserCloseTab?: (tabId: string) => Promise<void> | void
  onBrowserSuggestAction?: (goal: string) => Promise<BrowserActionSuggestionResponse | null>
  onBrowserSocketObservation?: (observation: BrowserObservation, session?: BrowserSession) => void
  /** "Try another source" in the blocked notice: leave the site the observation says blocks automation. */
  onBrowserTryOtherSource?: () => void
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
  const [searchQuery, setSearchQuery] = createSignal<string | null>(null)
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
  const [consoleFilter, setConsoleFilter] = createSignal<ConsoleFilter>(emptyConsoleFilter())
  const [liveFrameWsConnected, setLiveFrameWsConnected] = createSignal(false)
  const [liveFrameWsFailed, setLiveFrameWsFailed] = createSignal(false)
  const [liveFrameSseFailed, setLiveFrameSseFailed] = createSignal(false)
  const [wsActionPending, setWsActionPending] = createSignal(false)
  const [liveFrameTick, setLiveFrameTick] = createSignal(0)
  const [actionTarget, setActionTarget] = createSignal<BrowserActionTarget | null>(null)
  const [frameAspectMatches, setFrameAspectMatches] = createSignal(false)
  // Q2-WS8.I per-tab control and Stop agent: one change at a time, its refusal, and the take-over notice's tab.
  const [tabActionPending, setTabActionPending] = createSignal(false)
  const [tabActionError, setTabActionError] = createSignal<string | null>(null)
  const [takeOverTabId, setTakeOverTabId] = createSignal<string | null>(null)
  // Tab context menu (WS9 tab strip, behind VITE_BROWSER_TAB_MENU=true).
  const [tabMenu, setTabMenu] = createSignal<{ tabId: string } | null>(null)
  const tabMenuEnabled = () => import.meta.env.VITE_BROWSER_TAB_MENU === 'true'
  let tabMenuRef: HTMLDivElement | undefined
  // The tab button that opened the menu. Focus goes back to it when the menu closes (see closeTabMenu).
  let tabMenuOpener: HTMLElement | null = null
  // Bookmarks (F-t2-068 local slice, behind VITE_BROWSER_BOOKMARKS=true).
  const bookmarks = useBrowserBookmarks()
  const bookmarksEnabled = () => import.meta.env.VITE_BROWSER_BOOKMARKS === 'true'
  // Page zoom (F-t2-069 presentation slice, behind VITE_BROWSER_ZOOM=true).
  // Scales the live view only; no browser action and no stored state.
  const zoomEnabled = () => import.meta.env.VITE_BROWSER_ZOOM === 'true'
  const [frameZoom, setFrameZoom] = createSignal(100)
  const zoomFrame = (delta: number) => setFrameZoom((value) => Math.min(150, Math.max(50, value + delta)))
  // Page find (F-t2-069 presentation slice, behind VITE_BROWSER_FIND=true).
  // Client-side search over already-loaded DOM text only: no server call,
  // no persistence, no ZDR effect. Matches are listed (kind + text); the
  // frame itself is an image and is never highlighted in-image.
  const findEnabled = () => import.meta.env.VITE_BROWSER_FIND === 'true'
  const [findOpen, setFindOpen] = createSignal(false)
  const [findQuery, setFindQuery] = createSignal('')
  const [findIndex, setFindIndex] = createSignal(0)
  const findMatches = () => (findEnabled() && findOpen() ? findInDomNodes(session().domNodes, findQuery()) : [])
  const currentFindMatch = () => findMatches()[findIndex()] ?? null
  const stepFind = (direction: 1 | -1) =>
    setFindIndex((current) => stepFindIndex(current, findMatches().length, direction))
  let findInputRef: HTMLInputElement | undefined
  // Browser keyboard commands (F-t2-070, behind VITE_BROWSER_KEYMAP=true).
  const keymapEnabled = () => import.meta.env.VITE_BROWSER_KEYMAP === 'true'
  // Save the page as a PDF (F-t2-069 step 1, E5.1, behind VITE_BROWSER_PDF=true).
  const pdfEnabled = () => import.meta.env.VITE_BROWSER_PDF === 'true'
  const tabMenuTab = () => {
    const menu = tabMenu()
    return menu ? session().tabs.find((tab) => tab.tabId === menu.tabId) ?? null : null
  }
  const tabOutsideUrl = (tab: BrowserTab): string | null => {
    if (!tab.url) return null
    try {
      const url = new URL(tab.url)
      return url.protocol === 'https:' || url.protocol === 'http:'
        ? url.href
        : null
    } catch {
      return null
    }
  }
  // Closes the tab menu. With restoreFocus, focus goes back to the tab button that opened it, if it is still in
  // the page. Escape asks for it only while focus was in the menu, so a menu closed from elsewhere never takes focus.
  const closeTabMenu = (options: { restoreFocus?: boolean } = {}) => {
    const opener = tabMenuOpener
    tabMenuOpener = null
    setTabMenu(null)
    if (options.restoreFocus && opener?.isConnected) opener.focus()
  }
  const currentPageUrl = () => session().url ?? ''
  const toggleBookmark = () => {
    if (!bookmarksEnabled()) return
    const url = tabOutsideUrl({ tabId: '', active: true, url: currentPageUrl() })
    if (!url) return
    toggleBrowserBookmark({ url, label: session().title ?? '' })
  }
  let addressInputRef: HTMLInputElement | undefined
  let pendingObservedAddress: string | null = null
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

  const i18n = useI18n()
  const assistantBrowser = useAssistantBrowser()
  const session = () => props.session
  const observedAddress = () => props.browserNavigationError?.reason === 'http_response'
    ? props.browserNavigationError.url
    : session().url
  const isLive = () => session().renderMode === 'chromium'
  // Evidence panels (WS9.D) are shown only when the gateway advertises capability 'evidence'.
  const hasEvidence = () => session().capabilities.includes('evidence')
  // WS8.D: while a person's session is live and the tab visible, its grant is renewed on a timer.
  // Keyed on the session id (a memo), so a projection refresh does not restart the timer.
  const keepAliveSessionId = createMemo(() => {
    const current = session()
    return props.onKeepAlive && sessionHasHumanBrowsing(current.capabilities) ? (current.sessionId ?? null) : null
  })
  // Solid 2 takes (compute, apply): compute tracks the session id and the handler; apply starts the
  // timer and returns its stop function as the cleanup.
  createEffect(
    () => {
      const sessionId = keepAliveSessionId()
      const keepAlive = props.onKeepAlive
      return sessionId && keepAlive ? { sessionId, keepAlive } : null
    },
    (target) => {
      if (!target) return
      const { sessionId, keepAlive } = target
      return startSessionKeepAlive({
        isActive: () =>
          document.visibilityState === 'visible' && session().sessionId === sessionId && session().status === 'live',
        tick: keepAlive,
      })
    },
  )
  const visibleConsoleEntries = () =>
    hasEvidence() ? filterConsoleEntries(liveConsoleEntries(), consoleFilter()) : liveConsoleEntries()
  const controlsDisabled = () => !isLive() || props.browserBusy || wsActionPending() || !session().sessionId
  // Keyboard commands (behind the flag): Ctrl/Cmd+T new tab, Ctrl/Cmd+L focus
  // the address field, Ctrl/Cmd+W close the current tab when there are several.
  // Typing in fields is never intercepted.
  createEffect(() => keymapEnabled(), (enabled) => {
    if (!enabled) return
    const onKeyDown = (event: KeyboardEvent) => {
      if (!(event.ctrlKey || event.metaKey) || event.altKey || event.shiftKey) return
      const target = event.target as HTMLElement | null
      const tag = target?.tagName?.toLowerCase()
      if (tag === 'input' || tag === 'textarea' || tag === 'select' || target?.isContentEditable) return
      const key = event.key.toLowerCase()
      if (key === 't') {
        event.preventDefault()
        void prepareNewTabNavigation()
      } else if (key === 'l') {
        event.preventDefault()
        addressInputRef?.focus()
        addressInputRef?.select()
      } else if (key === 'w' && session().tabs.length > 1) {
        event.preventDefault()
        const active = session().tabs.find((tab) => tab.active)
        if (active && props.onBrowserCloseTab) void props.onBrowserCloseTab(active.tabId)
      }
    }
    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  })
  // Page find (behind VITE_BROWSER_FIND): Ctrl/Cmd+F toggles the find bar and
  // focuses it. Works even while typing in fields (unlike the keymap), and
  // Esc closes it. Prevents the browser's own find UI while the bar is open.
  createEffect(() => findEnabled(), (enabled) => {
    if (!enabled) return
    const onKeyDown = (event: KeyboardEvent) => {
      const modifier = event.ctrlKey || event.metaKey
      if (modifier && !event.altKey && event.key.toLowerCase() === 'f') {
        event.preventDefault()
        setFindOpen(true)
        window.requestAnimationFrame(() => findInputRef?.focus())
        return
      }
      if (event.key === 'Escape' && findOpen()) {
        setFindOpen(false)
        setFindQuery('')
        setFindIndex(0)
      }
    }
    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  })
  const loopStatus = () => props.browserLoop?.status ?? 'idle'
  const loopRunning = () => isBrowserLoopRunning(loopStatus())
  // Phase 5 HITL gate: the single most recent still-pending browser-action
  // approval, if any — the one the AI bubble surfaces over the live page.
  const pendingApproval = () => pendingBrowserApproval(props.browserApprovals ?? [])
  const decidingBrowserApprovalKeys = () => props.decidingBrowserApprovalKeys ?? []
  const humanTakeoverActive = () => session().controlMode === 'human_takeover'
  const continuationTab = createMemo(() => {
    const { sessionId, tabs, zdr } = session()
    if (!props.allowAssistantContinuation || !assistantBrowser.prepareContinuation ||
        !sessionId || !isLive() || !humanTakeoverActive() || zdr || blockedStateFor(session().observation?.challenge)) return null
    const [tab, second] = tabs.filter((item) => item.active)
    if (!tab?.url || second || tab.holder !== 'human' || tab.tabEpoch === undefined) return null
    const url = assistantBrowserTarget(tab.url)
    return url ? { sessionId, tabId: tab.tabId, origin: url.origin, tabEpoch: tab.tabEpoch } : null
  })
  const liveBrowserTabs = () => session().tabs.length > 0
    ? session().tabs
    : [{
        active: true,
        tabId: 'tab-1',
        title: session().title || session().host,
        url: session().url,
      }]
  const timelineTabs = () => session().timeline.filter((entry) => entry.step > 0).slice(-8)
  // The one-line brief for a timeline step (F-t3-050 R2), behind VITE_BROWSER_ACTION_BRIEF. Its facts
  // come from the step's entry, the rationale recorded for the step and the approval on its action. It
  // is null while the switch is off, so the timeline keeps its own labels.
  const briefTr = (noText: string, enText: string) => i18n.tr(noText, enText)
  const timelineBrief = (entry: BrowserTimelineViewEntry) => {
    if (!browserActionBriefEnabled()) return null
    const action = entry.action ?? null
    const approval = action
      ? ((props.browserApprovals ?? []).find((item) => item.actionId === action.id) ?? null)
      : null
    const facts = factsFromTimelineEntry(entry, rationaleForStep(props.browserRationales ?? [], entry.step), approval)
    return { pill: browserActionPill(facts, briefTr), title: browserActionTitle(facts, briefTr) }
  }
  // Replay scrubber (F-t3-050 R1, behind VITE_BROWSER_REPLAY_SCRUBBER=true). It covers every recorded
  // step, and while one is on screen the frame never falls back to a later frame.
  const replaySteps = () => replayScrubSteps(session().timeline)
  const replayScrubbing = () => replayScrubberEnabled() && selectedTimelineStep() !== null
  const replayEvents = () => session().replayEvents.slice(-12)
  const ownerTimeline = () => session().ownerTimeline?.slice(-24) ?? []
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
      replayFallbackFrame(replayScrubbing(), session().frameUrl),
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
  const liveNetworkEntries = () => liveNetworkRows(session().networkEntries, devtoolsEvents())

  const togglePanel = (panel: BrowserChromePanel) => setChrome((state) => toggleBrowserChromePanel(state, panel))
  const togglePopover = (popover: BrowserChromePopover) =>
    setChrome((state) => toggleBrowserChromePopover(state, popover))
  const closePopover = () => setChrome(closeBrowserChromePopover)

  const browserControlUnavailable = () => !isLive() || !session().sessionId
  const humanOnlyBrowsing = () => session().capabilities.includes('human_browsing')
  const cannotGiveAgentControl = () => humanOnlyBrowsing() && humanTakeoverActive()
  const browserControlModeDisabled = () => browserControlUnavailable() || props.browserBusy || !props.onBrowserControlMode || cannotGiveAgentControl()
  const toggleBrowserControlMode = () => {
    if (browserControlModeDisabled()) return
    const nextMode = humanTakeoverActive() ? 'agent_control' : 'human_takeover'
    if (sendBrowserWsControl(nextMode)) return
    props.onBrowserControlMode?.(nextMode, session().controlEpoch ?? null)
  }
  /**
   * "I will type it myself" on a one-time-code approval (E3.1-i): the chrome's own takeover,
   * offered only while it can start, so it never hands an active takeover back to the agent.
   * It sends no decision; the approval stays pending.
   */
  const approvalTakeOver = () =>
    browserControlModeDisabled() || humanTakeoverActive() ? undefined : toggleBrowserControlMode
  // Q2-WS8.I: the owner's per-tab control and Stop agent. The org is the active one in the session store, as the
  // evidence panels read it. The calls are the REST routes: the action registry has no gateway executor for them.
  const currentOrgId = () => getSession().activeOrg?.id ?? ''
  const activeTabId = () => session().tabs.find((tab) => tab.active)?.tabId ?? null
  const attachLive = () => session().attach?.state === 'attached'
  // The per-tab control of an agent-opened tab, from its holder. Absent without the tab_control capability.
  const tabControlFor = (tab: BrowserTab): TabControlAction | null => {
    if (tab.openedBy !== 'agent' || !session().capabilities.includes('tab_control')) return null
    if (tab.holder === 'agent') return 'take_over'
    return tab.holder === 'human' && Boolean(tab.attachId) && attachLive() ? 'give_back' : null
  }
  const tabControlCopy = (action: TabControlAction) => action === 'take_over'
    ? { text: i18n.tr('Ta over', 'Take over'), hint: i18n.tr('Ta over fanen fra agenten', 'Take over this tab from the agent') }
    : { text: i18n.tr('Gi tilbake', 'Give back'), hint: i18n.tr('Gi fanen tilbake til agenten', 'Give this tab back to the agent') }
  const tabMarkLabel = (mark: 'deliverable' | 'handoff') =>
    mark === 'deliverable' ? i18n.tr('Levert', 'Delivered') : i18n.tr('Overlevert', 'Handed off')
  // Typing into a tab an agent holds is the person's take-over (implicit in Quarry, ADR-3); the notice says so.
  const noteTakeOverFromKeystroke = () => {
    const tab = session().tabs.find((item) => item.active)
    if (tab?.holder === 'agent' && session().capabilities.includes('tab_control')) setTakeOverTabId(tab.tabId)
  }
  const takeOverNoticeVisible = () => takeOverTabId() !== null && takeOverTabId() === activeTabId()
  // Stop agent (Q2-WS8.I): an entry in the tab menu while a live attached agent exists.
  const stopAgentMenuEntry = () => attachLive() && session().capabilities.includes('attach')
    ? { label: i18n.tr('Stopp agenten', 'Stop agent'), disabled: controlsDisabled() || tabActionPending() }
    : null
  // The projection Quarry reports now goes to the host through its socket-observation channel, which applies the
  // session and keeps its capabilities. A host without that channel takes the change on its next update.
  const refreshBrowserProjection = async (orgId: string, sessionId: string) => {
    const onObservation = props.onBrowserSocketObservation
    if (!onObservation) return
    const response = await getBrowserSession(orgId, sessionId)
    const observation = response.observation ?? session().observation
    if (observation) onObservation(observation, response.session)
  }
  // One change at a time. A refused change says why (errors.ts); a stale tab epoch also refetches the projection, so
  // the chip shows who holds the tab now. A change that went through keeps its success when the refresh fails.
  const runTabChange = async (
    change: (orgId: string, sessionId: string) => Promise<unknown>,
    fallback: { no: string; en: string },
  ) => {
    const sessionId = session().sessionId
    const orgId = currentOrgId()
    if (!sessionId || !orgId || tabActionPending()) return
    setTabActionPending(true)
    setTabActionError(null)
    let changed = false
    try {
      await change(orgId, sessionId)
      changed = true
      setTakeOverTabId(null)
      await refreshBrowserProjection(orgId, sessionId)
    } catch (reason) {
      if (!changed) {
        const stale = isStaleTabEpoch(reason)
        if (stale) await refreshBrowserProjection(orgId, sessionId).catch(() => undefined)
        setTabActionError(browserErrorMessage(reason, i18n.tr, stale ? ERROR_COPY.stale_tab_epoch : fallback))
      } else {
        setTabActionError(browserErrorMessage(reason, i18n.tr))
      }
    } finally {
      setTabActionPending(false)
    }
  }
  const changeTabHolder = (tabId: string, holder: 'human' | 'agent') => {
    const tab = session().tabs.find((item) => item.tabId === tabId)
    if (!tab) return Promise.resolve()
    // Compare-and-set on the tab epoch the person saw: a tab that changed hands since is refused (409).
    return runTabChange((orgId, sessionId) => controlBrowserTab(orgId, sessionId, tabId, holder, tab.tabEpoch), TAB_CHANGE_FALLBACK)
  }
  const stopAgent = () => attachLive()
    ? runTabChange((orgId, sessionId) => stopBrowserAttach(orgId, sessionId), STOP_AGENT_FALLBACK)
    : Promise.resolve()
  const runAction = (action: BrowserAction) => {
    if (controlsDisabled()) return
    // Typing (press, type) into an agent-held tab is the person's take-over: the notice shows before the key goes.
    if (action.type === 'press' || action.type === 'type') noteTakeOverFromKeystroke()
    if (!(action.type === 'navigate' && props.browserNavigationError && props.onBrowserAction) && sendBrowserWsAction(action)) return
    props.onBrowserAction?.(action)
  }
  /**
   * Reload from the chrome (WS9.B). A soft reload after a failed navigation retries
   * that URL; a hard reload is the Quarry `reload` action and needs the capability.
   */
  const reloadFromChrome = (ignoreCache: boolean) => {
    const failure = props.browserNavigationError
    if (!ignoreCache && failure && props.onBrowserNavigationRetry) {
      props.onBrowserNavigationRetry(failure.url)
      return
    }
    if (ignoreCache) runAction({ type: 'reload', ignore_cache: true })
    else runAction({ type: 'navigate', url: session().url })
  }
  /** F5 and Ctrl/Cmd+R reload inside the chrome; Ctrl/Cmd+Shift+R is a hard reload. */
  const onChromeReloadKey = (event: KeyboardEvent) => {
    const modifier = event.ctrlKey || event.metaKey
    const isReload = event.key === 'F5' || (modifier && event.key.toLowerCase() === 'r')
    if (!isReload) return
    event.preventDefault()
    reloadFromChrome(modifier && event.shiftKey && session().capabilities.includes('hard_reload'))
  }
  /** Mouse buttons 4 and 5 on the page frame go back and forward (WS9.B). */
  const onChromePageAuxClick = (event: MouseEvent) => {
    if (event.button === 3) {
      event.preventDefault()
      runAction({ type: 'back' })
    } else if (event.button === 4) {
      event.preventDefault()
      runAction({ type: 'forward' })
    }
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
  const omniboxSearchEnabled = () => props.searchEnabled ?? browserOmniboxSearchEnabled()
  // Runs what the omnibox resolved: a navigation, a Verevon search shown in the
  // frame stage, or a hand-off to chat. A noop (empty field, search off) does nothing.
  const submitOmnibox = (intent: OmniboxIntent) => {
    if (intent.kind === 'navigate') {
      setSearchQuery(null)
      setAddressInput(intent.url)
      runAction({ type: 'navigate', url: intent.url })
      return
    }
    if (intent.kind === 'search') {
      setSearchQuery(intent.query)
      return
    }
    if (intent.kind === 'ask') props.onAskVerevon?.(intent.text)
  }
  const navigateFromAddress = () =>
    submitOmnibox(resolveOmniboxIntent(addressInput(), { searchEnabled: omniboxSearchEnabled(), fallbackUrl: observedAddress() }))
  // A result click keeps the results open until the navigation settles: a refusal outside the
  // run's domains is explained next to the results (D2-046), any other outcome closes them.
  const [pendingResult, setPendingResult] = createSignal<{ url: string; fromUrl: string } | null>(null)
  const openSearchResult = (url: string) => {
    if (controlsDisabled()) return
    setPendingResult({ url, fromUrl: session().url })
    setAddressInput(url)
    runAction({ type: 'navigate', url })
  }
  createEffect(
    () => ({ pending: pendingResult(), failure: props.browserNavigationError, url: session().url }),
    ({ pending, failure, url }) => {
      if (!pending) return
      if (failure && failure.url === pending.url) {
        setPendingResult(null)
        if (failure.reason !== 'outside_scope') setSearchQuery(null)
        return
      }
      if (url !== pending.fromUrl) {
        setPendingResult(null)
        setSearchQuery(null)
      }
    },
  )
  const outsideScopeFailure = () => {
    const failure = props.browserNavigationError
    return failure && failure.reason === 'outside_scope' ? failure : null
  }
  const limitedToHost = () => (outsideScopeFailure() ? session().host || hostnameOf(session().url) || null : null)
  const refusedHost = () => {
    const failure = outsideScopeFailure()
    return failure ? hostnameOf(failure.url) || null : null
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

  createEffect(
    () => ({ url: observedAddress(), lastObserved: lastObservedUrl(), failed: props.browserNavigationError?.reason === 'http_response' }),
    ({ url, lastObserved, failed }) => {
      if (url && lastObserved !== url) {
        // Live page updates must not overwrite a person's unfinished address.
        if (document.activeElement === addressInputRef) pendingObservedAddress = url
        else { pendingObservedAddress = null; setAddressInput(url) }
        setLastObservedUrl(url)
        if (!failed) recordOmniboxVisit({ url, title: session().title }, { zdr: session().zdr })
      }
    },
  )

  createEffect(
    () => ({ sessionId: session().sessionId ?? '', devtoolsEvents: session().devtoolsEvents }),
    ({ sessionId, devtoolsEvents }) => {
      if (sessionId === lastDevtoolsSessionId) return
      lastDevtoolsSessionId = sessionId
      setDevtoolsEvents(devtoolsEvents)
    },
  )

  // Phase 5 HITL gate: a pending browser-action approval is the highest-
  // priority surface in the whole chrome — a blocked run needs a decision
  // now. Auto-open the evidence drawer the moment a NEW one appears (once
  // per approval key, so a user who deliberately closes the drawer again
  // isn't fought on every re-render), mirroring how AgentRunConsole's
  // approval deck scrolls itself into view.
  createEffect(
    () => pendingApproval(),
    (approval) => {
      if (!approval || approval.key === lastAutoOpenedApprovalKey) return
      lastAutoOpenedApprovalKey = approval.key
      setChrome((state) => (state.evidenceOpen ? state : { ...state, evidenceOpen: true, popover: null }))
    },
  )

  createEffect(
    () => session().devtoolsEvents,
    (cachedEvents) => {
      if (cachedEvents.length === 0) return
      setDevtoolsEvents((current) => {
        const bySequence = new Map<number, BrowserDevtoolsEvent>()
        for (const event of current) bySequence.set(event.sequence, event)
        for (const event of cachedEvents) bySequence.set(event.sequence, event)
        return [...bySequence.values()]
          .sort((a, b) => a.sequence - b.sequence)
          .slice(-512)
      })
    },
  )

  createEffect(
    () => `${session().liveFrameWsUrl ?? ''}|${session().liveFrameStreamUrl ?? ''}`,
    (transportKey) => {
      if (transportKey === lastLiveFrameTransportKey) return
      lastLiveFrameTransportKey = transportKey
      setLiveFrameWsFailed(false)
      setLiveFrameSseFailed(false)
    },
  )

  createEffect(
    () => ({ selected: selectedTimelineStep(), timeline: session().timeline }),
    ({ selected, timeline }) => {
      if (selected === null) return
      if (!timeline.some((entry) => entry.step === selected)) {
        setSelectedTimelineStep(null)
      }
    },
  )

  createEffect(
    () => ({ tab: devtoolsTab(), hasEvidence: hasEvidence() }),
    ({ tab, hasEvidence: available }) => {
      if (tab === 'egress' && !available) setDevtoolsTab('dom')
    },
  )

  createEffect(
    () => ({ tab: devtoolsTab(), hasVisual: Boolean(session().visualObservationArtifactId) }),
    ({ tab, hasVisual }) => {
      if (tab === 'vision' && !hasVisual) {
        setDevtoolsTab('dom')
      }
    },
  )

  createEffect(
    () => controlsDisabled(),
    (disabled) => {
      if (disabled || !queuedWheelAction) return
      clearQueuedWheelTimer()
      flushQueuedWheelAction()
    },
  )

  createEffect(
    () => pollingLiveFrameAvailable(),
    (available) => {
      if (!available) {
        clearLiveFrameTimer()
        return
      }
      setLiveFrameTick((tick) => tick + 1)
    },
  )

  createEffect(
    () => liveFrameWsSrc(),
    (src) => {
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

      return closeLiveFrameSocket
    },
  )

  createEffect(
    () => liveFrameStreamSrc(),
    (src) => {
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
      return closeLiveFrameSource
    },
  )

  // Action target display is opt-in until the event carries a tab identity.
  createEffect(
    () => props.showActionTargets && isLive() && selectedTimelineStep() === null && session().tabs.length === 1
      ? session().sessionId ?? null : null,
    (sessionId) => {
      setActionTarget(null)
      if (!sessionId) return
      const subscribedAt = Date.now()
      let timeout: number | undefined
      const clearTarget = () => {
        if (timeout !== undefined) window.clearTimeout(timeout)
        timeout = undefined
        setActionTarget(null)
      }
      const stream = streamBrowserEvents(sessionId, {}, {
        onEvent: (name, event) => {
          clearTarget()
          if (name !== 'action.started') return
          const target = browserActionTarget(event, subscribedAt, Date.now())
          if (!target) return
          setActionTarget(target)
          timeout = window.setTimeout(clearTarget, 2000)
        },
        onReplayGap: clearTarget,
        onError: clearTarget,
        onDone: clearTarget,
      })
      return () => { clearTarget(); stream.close() }
    },
  )

  createEffect(
    () => `${session().url}|${session().tabs.map((tab) => `${tab.tabId}:${tab.url}`).join('|')}`,
    () => { setActionTarget(null) },
  )

  onCleanup(clearQueuedWheelTimer)
  onCleanup(clearLiveFrameTimer)
  onCleanup(closeLiveFrameSource)
  onCleanup(closeLiveFrameSocket)

  createEffect(
    () => chrome().popover,
    (popover) => {
      if (!popover) return
      const onKeyDown = (event: KeyboardEvent) => {
        if (event.key === 'Escape') closePopover()
      }
      document.addEventListener('keydown', onKeyDown)
      return () => document.removeEventListener('keydown', onKeyDown)
    },
  )

  // The tab menu's items for the open tab, rebuilt from the session on each change. The list is rendered
  // by position (see the For below), so the item that has focus keeps it when the session updates.
  const tabMenuItems = (): BrowserTabMenuItem[] => {
    const tab = tabMenuTab()
    if (!tab) return []
    return browserTabMenuItems({
      tab,
      tabCount: session().tabs.length,
      busy: Boolean(props.browserBusy),
      controlsDisabled: controlsDisabled(),
      hasCloseHandler: Boolean(props.onBrowserCloseTab),
      outsideUrl: tabOutsideUrl(tab),
      stopAgent: stopAgentMenuEntry(),
    })
  }
  // Runs a chosen tab-menu item. The action is parsed through the contract before any handler sees it, and
  // each handler is the one the chrome already used for the same operation. Returns false when nothing ran.
  const runTabMenuItem = (item: BrowserTabMenuItem): boolean => {
    if (item.disabled) return false
    const parsed = browserTabMenuActionSchema.safeParse(item.action)
    if (!parsed.success) return false
    const action = parsed.data
    if (action.type === 'browser.tab.open_outside') {
      // The link's own navigation opens the page. The menu closes on a later turn, so the link is still in
      // place when the browser follows it.
      window.setTimeout(() => closeTabMenu({ restoreFocus: true }), 0)
      return true
    }
    closeTabMenu({ restoreFocus: true })
    if (action.type === 'browser.tab.select') {
      setSelectedTimelineStep(null)
      void props.onBrowserSelectTab?.(action.tabId)
    } else if (action.type === 'browser.tab.new') {
      void prepareNewTabNavigation()
    } else if (action.type === 'browser.tab.close') {
      void props.onBrowserCloseTab?.(action.tabId)
    } else if (action.type === 'browser.attach.stop') {
      void stopAgent()
    }
    return true
  }
  // Tab context menu dismissal and focus (behind VITE_BROWSER_TAB_MENU=true). Focus stays on the tab button
  // that opened the menu, so a key handler on the menu itself never saw Escape. While the menu is open the
  // document listens instead: Escape closes it, a pointer down outside closes it, Tab leaves it, and the
  // arrow keys move between its enabled items. The first enabled item takes focus when the menu opens.
  createEffect(
    () => tabMenu(),
    (menu) => {
      if (!menu) return
      const enabledItems = () => Array.from(
        tabMenuRef?.querySelectorAll<HTMLElement>('[role="menuitem"]:not(:disabled)') ?? [],
      )
      const onPointerDown = (event: PointerEvent) => {
        if (event.target instanceof Node && tabMenuRef?.contains(event.target)) return
        // The pointer's own mousedown then moves focus to whatever it pressed, so this only sets the default.
        closeTabMenu({ restoreFocus: true })
      }
      const onKeyDown = (event: KeyboardEvent) => {
        if (event.key === 'Escape') {
          event.preventDefault()
          event.stopPropagation()
          const inMenu = tabMenuRef?.contains(document.activeElement) ?? false
          closeTabMenu({ restoreFocus: inMenu })
          return
        }
        // Tab and the arrow keys act only while focus is in the menu, so typing elsewhere is never intercepted.
        if (!tabMenuRef?.contains(document.activeElement)) return
        if (event.key === 'Tab') {
          closeTabMenu({ restoreFocus: true })
          return
        }
        if (event.key !== 'ArrowDown' && event.key !== 'ArrowUp') return
        const items = enabledItems()
        if (items.length === 0) return
        event.preventDefault()
        const current = items.indexOf(document.activeElement as HTMLElement)
        const step = event.key === 'ArrowDown' ? 1 : -1
        const next = current < 0
          ? (step > 0 ? 0 : items.length - 1)
          : (current + step + items.length) % items.length
        items[next]?.focus()
      }
      document.addEventListener('pointerdown', onPointerDown, true)
      document.addEventListener('keydown', onKeyDown)
      queueMicrotask(() => {
        if (tabMenu()) enabledItems()[0]?.focus()
      })
      return () => {
        document.removeEventListener('pointerdown', onPointerDown, true)
        document.removeEventListener('keydown', onKeyDown)
      }
    },
  )

  // The take-over notice and a tab error belong to the tab they were raised on.
  createEffect(
    () => activeTabId(),
    () => {
      setTakeOverTabId(null)
      setTabActionError(null)
    },
  )

  return (
    <div
      class={['knowledge-browser-chrome', { 'knowledge-browser-chrome--compact': Boolean(props.compact), 'knowledge-browser-chrome--personal': Boolean(props.personalBrowsing) }]}
      onKeyDown={onChromeReloadKey}
    >
      <div class="knowledge-browser-frame__topbar knowledge-browser-frame__topbar--minimal">
        <div class="knowledge-browser-frame__window">
          <Show when={!props.personalBrowsing}>
            <span class="knowledge-browser-traffic" aria-hidden="true">
              <i />
              <i />
              <i />
            </span>
          </Show>
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
                  <>
                  <button
                    type="button"
                    role="tab"
                    disabled={props.browserBusy}
                    aria-selected={selectedTimelineStep() === null && tab.active ? 'true' : 'false'}
                    aria-haspopup="menu"
                    aria-expanded={tabMenu()?.tabId === tab.tabId ? 'true' : 'false'}
                    class={['knowledge-browser-tab', { 'knowledge-browser-tab--active': selectedTimelineStep() === null && tab.active }]}
                    title={`Live · ${liveTabLabel(tab)}`}
                    onClick={() => {
                      setSelectedTimelineStep(null)
                      if (!tab.active) void props.onBrowserSelectTab?.(tab.tabId)
                    }}
                    onContextMenu={(event) => {
                      if (!tabMenuEnabled()) return
                      event.preventDefault()
                      tabMenuOpener = event.currentTarget
                      setTabMenu({ tabId: tab.tabId })
                    }}
                  >
                    <Show when={tab.openedBy === 'agent'}>
                      <span class="knowledge-browser-tab__agent" role="img" aria-label="Åpnet av agenten" title="Åpnet av agenten">
                        <Bot class="size-3" aria-hidden="true" />
                      </span>
                    </Show>
                    <span class="knowledge-browser-tab__dot" data-status={session().status} aria-hidden="true" />
                    <Show when={!props.personalBrowsing}><span class="knowledge-browser-tab__mode">Live</span></Show>
                    <strong class="knowledge-browser-tab__label">{liveTabLabel(tab)}</strong>
                    <Show when={tab.mark}>
                      {(mark) => (
                        <span class="knowledge-browser-tab__mode" data-mark={mark()} title={tabMarkLabel(mark())}>
                          {tabMarkLabel(mark())}
                        </span>
                      )}
                    </Show>
                  </button>
                  <Show when={tab.openedBy === 'agent' && !tab.active}>
                    <button
                      type="button"
                      class="knowledge-browser-tab__reveal"
                      aria-label="Vis agentens fane"
                      title="Vis agentens fane"
                      disabled={props.browserBusy}
                      onClick={() => {
                        setSelectedTimelineStep(null)
                        void props.onBrowserSelectTab?.(tab.tabId)
                      }}
                    >
                      Vis
                    </button>
                  </Show>
                  <Show when={tabControlFor(tab)}>
                    {(action) => (
                      <button
                        type="button"
                        class="knowledge-browser-tab__reveal"
                        aria-label={tabControlCopy(action()).hint}
                        title={tabControlCopy(action()).hint}
                        disabled={controlsDisabled() || tabActionPending()}
                        onClick={() => void changeTabHolder(tab.tabId, action() === 'take_over' ? 'human' : 'agent')}
                      >
                        {tabControlCopy(action()).text}
                      </button>
                    )}
                  </Show>
                  </>
                )}
              </For>
              <For each={timelineTabs()}>
                {(entry) => (
                  <button
                    type="button"
                    role="tab"
                    aria-selected={selectedTimelineStep() === entry.step ? 'true' : 'false'}
                    class={['knowledge-browser-tab', { 'knowledge-browser-tab--active': selectedTimelineStep() === entry.step }]}
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
          <Show when={tabMenuEnabled() && tabMenuTab()}>
            {(tab) => (
              <div
                ref={(element) => { tabMenuRef = element }}
                class="knowledge-browser-tab-menu"
                role="menu"
                aria-label={`Fanealternativ for ${liveTabLabel(tab())}`}
              >
                <For each={tabMenuItems()} keyed={false}>
                  {(item) => (
                    <Show
                      when={browserTabMenuOutsideAction(item().action)}
                      fallback={(
                        <button
                          type="button"
                          role="menuitem"
                          disabled={item().disabled}
                          onClick={() => { runTabMenuItem(item()) }}
                        >
                          {item().label}
                        </button>
                      )}
                    >
                      {(outside) => (
                        <a
                          role="menuitem"
                          href={outside().url}
                          target="_blank"
                          rel="noopener noreferrer"
                          onClick={(event) => { if (!runTabMenuItem(item())) event.preventDefault() }}
                        >
                          {item().label}
                        </a>
                      )}
                    </Show>
                  )}
                </For>
              </div>
            )}
          </Show>
        </div>
        {props.frameControls}
      </div>
      <Show when={bookmarksEnabled() && bookmarks().length > 0}>
        <div class="knowledge-browser-bookmarks" aria-label="Bokmerker">
          <For each={bookmarks()}>
            {(bookmark) => (
              <button
                type="button"
                class="knowledge-browser-bookmarks__item"
                title={bookmark.url}
                disabled={controlsDisabled()}
                onClick={() => runAction({ type: 'navigate', url: bookmark.url })}
              >
                <Star class="size-3" aria-hidden="true" /> {bookmark.label}
              </button>
            )}
          </For>
        </div>
      </Show>

      <Show when={props.browserNavigationError}>
        <BrowserNavigationError
          failure={props.browserNavigationError as BrowserNavigationFailure}
          busy={props.browserBusy}
          onRetry={(url) => props.onBrowserNavigationRetry?.(url)}
          onDismiss={() => props.onBrowserNavigationDismiss?.()}
        />
      </Show>
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
            onClick={() => reloadFromChrome(false)}
          >
            <RefreshCw class="size-3.5" />
          </button>
          <Show when={!props.personalBrowsing && session().capabilities.includes('hard_reload')}>
            <button
              type="button"
              aria-label="Last nettlesersiden på nytt uten hurtigbuffer"
              title="Last på nytt uten hurtigbuffer"
              disabled={controlsDisabled()}
              onClick={() => reloadFromChrome(true)}
            >
              <RefreshCw class="size-3.5" />
            </button>
          </Show>
          <Show when={bookmarksEnabled()}>
            <button
              type="button"
              aria-label={isBookmarked(currentPageUrl()) ? 'Fjern bokmerke' : 'Legg til bokmerke'}
              aria-pressed={isBookmarked(currentPageUrl()) ? 'true' : 'false'}
              title={isBookmarked(currentPageUrl()) ? 'Fjern bokmerke' : 'Legg til bokmerke'}
              disabled={controlsDisabled() || !currentPageUrl()}
              onClick={toggleBookmark}
            >
              <Star class="size-3.5" />
            </button>
          </Show>
          <Show when={zoomEnabled()}>
            <button type="button" aria-label="Zoom ut" title="Zoom ut" disabled={controlsDisabled() || frameZoom() <= 50} onClick={() => zoomFrame(-10)}>−</button>
            <span aria-label="Zoomnivå" aria-live="polite">{frameZoom()}%</span>
            <button type="button" aria-label="Zoom inn" title="Zoom inn" disabled={controlsDisabled() || frameZoom() >= 150} onClick={() => zoomFrame(10)}>+</button>
            <button type="button" aria-label="Nullstill zoom" title="Nullstill zoom" disabled={controlsDisabled() || frameZoom() === 100} onClick={() => setFrameZoom(100)}>100%</button>
          </Show>
          <Show when={findEnabled()}>
            <button
              type="button"
              aria-label="Finn på siden"
              aria-pressed={findOpen() ? 'true' : 'false'}
              title="Finn på siden (Ctrl+F)"
              disabled={!isLive()}
              onClick={() => {
                setFindOpen(!findOpen())
                if (!findOpen()) window.requestAnimationFrame(() => findInputRef?.focus())
              }}
            >
              <Search class="size-3.5" />
            </button>
          </Show>
        </div>

        <div class="knowledge-browser-chrome__address" onFocusOut={(event) => {
          if (event.target === addressInputRef && pendingObservedAddress) {
            setAddressInput(pendingObservedAddress)
            pendingObservedAddress = null
          }
        }}>
          <button
            type="button"
            class="knowledge-browser-chrome__lock"
            aria-label="Profil, cookies og personvern for økten"
            aria-haspopup="dialog"
            aria-expanded={chrome().popover === 'profile' ? 'true' : 'false'}
            aria-controls={profilePopoverId}
            title="Profil, cookies og personvern"
            onClick={() => togglePopover('profile')}
          >
            <LockKeyhole class="size-3.5" />
          </button>
          <OmniboxField
            ref={(element) => { addressInputRef = element }}
            value={addressInput()}
            disabled={controlsDisabled()}
            label="Nettleseradresse"
            searchEnabled={omniboxSearchEnabled()}
            askEnabled={props.onAskVerevon !== undefined}
            fallbackUrl={observedAddress()}
            historyKey={currentOmniboxHistoryKey}
            zdr={() => session().zdr}
            onValueChange={setAddressInput}
            onIntent={submitOmnibox}
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
          <Show when={!props.personalBrowsing || !isLive()}><span class="knowledge-browser-mode-badge" title={`Gjengivelsesmodus: ${session().renderMode}`}>
            {RENDER_MODE_LABELS[session().renderMode]}
          </span></Show>
          <Show when={!props.personalBrowsing && !cannotGiveAgentControl()}><button
            type="button"
            class={['knowledge-browser-control-chip', { 'knowledge-browser-control-chip--human': humanTakeoverActive() }]}
            aria-pressed={humanTakeoverActive() ? 'true' : 'false'}
            aria-label={humanTakeoverActive() ? props.personalBrowsing && !loopRunning() ? 'Gi nettleserkontroll til AI-agenten' : 'Gi nettleserkontroll tilbake til AI-agenten' : 'Ta over nettleserkontrollen manuelt'}
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
          </button></Show>
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
          <Show when={pdfEnabled() && !evidenceIsEphemeral(session())}>
            <button
              type="button"
              class="knowledge-browser-toolbtn"
              aria-label="Lagre siden som PDF"
              title="Lagre som PDF"
              disabled={controlsDisabled()}
              onClick={() => runAction({ type: 'pdf' })}
            >
              <Download class="size-3.5" />
            </button>
          </Show>
          <Show when={!humanOnlyBrowsing() && (!props.personalBrowsing || loopRunning())}>
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
              class={[`knowledge-browser-loop-chip knowledge-browser-loop-chip--${loopStatus()}`, { 'knowledge-browser-loop-chip--error': Boolean(props.browserLoop?.error) }]}
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
            class={['knowledge-browser-toolbtn', { 'knowledge-browser-toolbtn--on': chrome().actionsOpen }]}
            aria-label="Vis eller skjul manuelle nettleserhandlinger"
            aria-expanded={chrome().actionsOpen ? 'true' : 'false'}
            aria-controls={actionbarId}
            title="Manuelle handlinger"
            disabled={!isLive()}
            onClick={() => togglePanel('actions')}
          >
            <SquareMousePointer class="size-3.5" />
          </button>
          <button
            type="button"
            class={['knowledge-browser-toolbtn', { 'knowledge-browser-toolbtn--on': chrome().devtoolsOpen }]}
            aria-label="Vis eller skjul DevTools-panelet"
            aria-expanded={chrome().devtoolsOpen ? 'true' : 'false'}
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
            class={['knowledge-browser-toolbtn', { 'knowledge-browser-toolbtn--on': chrome().evidenceOpen }]}
            aria-label="Vis eller skjul tidslinje og evidens"
            aria-expanded={chrome().evidenceOpen ? 'true' : 'false'}
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
          </Show>
          <button
            type="button"
            class="knowledge-browser-toolbtn"
            aria-label="Flere valg"
            aria-haspopup="menu"
            aria-expanded={chrome().popover === 'overflow' ? 'true' : 'false'}
            aria-controls={overflowPopoverId}
            title="Flere valg"
            onClick={() => togglePopover('overflow')}
          >
            <MoreVertical class="size-3.5" />
          </button>
        </div>

        <Show when={findEnabled() && findOpen()}>
          <div class="knowledge-browser-find" role="search" aria-label="Finn på siden">
            <input
              ref={(element) => { findInputRef = element }}
              type="search"
              value={findQuery()}
              aria-label="Finn på siden"
              placeholder="Finn på siden…"
              onInput={(event) => {
                setFindQuery(event.currentTarget.value)
                setFindIndex(0)
              }}
              onKeyDown={(event) => {
                if (event.key === 'Enter') {
                  event.preventDefault()
                  stepFind(event.shiftKey ? -1 : 1)
                }
                if (event.key === 'Escape') {
                  event.stopPropagation()
                  setFindOpen(false)
                  setFindQuery('')
                  setFindIndex(0)
                }
              }}
            />
            <span aria-live="polite">
              {findQuery().trim()
                ? findMatches().length > 0
                  ? `${Math.min(findIndex() + 1, findMatches().length)}/${findMatches().length}`
                  : 'Ingen treff'
                : ''}
            </span>
            <button
              type="button"
              aria-label="Forrige treff"
              title="Forrige treff (Shift+Enter)"
              disabled={findMatches().length === 0}
              onClick={() => stepFind(-1)}
            >
              ▲
            </button>
            <button
              type="button"
              aria-label="Neste treff"
              title="Neste treff (Enter)"
              disabled={findMatches().length === 0}
              onClick={() => stepFind(1)}
            >
              ▼
            </button>
            <button
              type="button"
              aria-label="Lukk søk"
              title="Lukk (Esc)"
              onClick={() => {
                setFindOpen(false)
                setFindQuery('')
                setFindIndex(0)
              }}
            >
              <X class="size-3.5" />
            </button>
          </div>
          <Show when={currentFindMatch()}>
            {(match) => (
              <p class="knowledge-browser-find__current" aria-live="polite">
                <span>{match().kind}</span> — {match().text}
              </p>
            )}
          </Show>
        </Show>

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
                  class={{ 'knowledge-browser-popover__value--persistent': session().profileStorage === 'persistent' }}
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
                          <li class={{ 'knowledge-browser-profile-list__item--active': item.profile_id === manager().selectedProfileId }}>
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
                <Show when={props.personalBrowsing}>
                  <Show when={session().capabilities.includes('hard_reload')}>
                    <button type="button" role="menuitem" disabled={controlsDisabled()} onClick={() => { closePopover(); reloadFromChrome(true) }}>
                      <RefreshCw class="size-3.5" aria-hidden="true" /> Last på nytt uten hurtigbuffer
                    </button>
                  </Show>
                  <button type="button" role="menuitem" disabled={!isLive()} onClick={() => { closePopover(); togglePanel('actions') }}>
                    <SquareMousePointer class="size-3.5" aria-hidden="true" /> Manuelle handlinger
                  </button>
                  <button type="button" role="menuitem" disabled={!isLive()} onClick={() => { closePopover(); togglePanel('devtools') }}>
                    <PanelRight class="size-3.5" aria-hidden="true" /> Sideverktøy
                  </button>
                  <button type="button" role="menuitem" disabled={!isLive()} onClick={() => { closePopover(); togglePanel('evidence') }}>
                    <History class="size-3.5" aria-hidden="true" /> Tidslinje og evidens
                  </button>
                  <Show when={!humanOnlyBrowsing()}>
                    <button type="button" role="menuitem" disabled={modelActionDisabled()} onClick={() => { closePopover(); void suggestAction() }}>
                      <Sparkles class="size-3.5" aria-hidden="true" /> Foreslå nettlesersteg
                    </button>
                    <button type="button" role="menuitem" disabled={modelLoopDisabled()} onClick={() => { closePopover(); void runModelLoop() }}>
                      <Play class="size-3.5" aria-hidden="true" /> Start AI-loop
                    </button>
                  </Show>
                </Show>
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

      <Show when={isLive()}>
        <BrowserControlStrip
          controlMode={session().controlMode}
          controlEpoch={session().controlEpoch ?? null}
          agentWorking={loopRunning()}
          personalBrowsing={props.personalBrowsing}
          busy={props.browserBusy === true}
          disabled={continuationTab() && selectedTimelineStep() === null ? controlsDisabled() : browserControlModeDisabled()}
          zdr={session().zdr}
          onContinueHere={continuationTab() && selectedTimelineStep() === null
            ? () => {
                const tab = continuationTab()
                if (tab && !assistantBrowser.taskBusy?.()) assistantBrowser.prepareContinuation?.(tab)
              }
            : undefined}
          continueDisabled={assistantBrowser.taskBusy?.()}
          onChangeMode={cannotGiveAgentControl() ? undefined : (mode, expectedEpoch) => props.onBrowserControlMode?.(mode, expectedEpoch)}
        />
      </Show>
      <Show when={isLive() && takeOverNoticeVisible()}>
        <p class="knowledge-browser-takeover-notice" role="status" aria-live="polite">
          <Hand class="size-3" aria-hidden="true" />
          <span>{i18n.tr('Du tar over', 'You are taking over')}</span>
        </p>
      </Show>
      <Show when={isLive() && tabActionError()}>
        {(message) => (
          <p class="knowledge-browser-takeover-notice knowledge-browser-takeover-notice--error" role="alert">
            {message()}
          </p>
        )}
      </Show>
      <Show when={isLive()}>
        <BrowserBlockedNotice
          challenge={session().observation?.challenge}
          openOutsideUrl={props.personalBrowsing ? assistantBrowserTarget(observedAddress())?.href : undefined}
          humanTakeoverActive={humanTakeoverActive()}
          takeoverDisabled={browserControlModeDisabled()}
          tryAnotherSourceDisabled={props.browserBusy === true || !props.onBrowserTryOtherSource}
          onTakeOver={toggleBrowserControlMode}
          onTryAnotherSource={() => props.onBrowserTryOtherSource?.()}
        />
      </Show>

      <div
        class={['knowledge-browser-chrome__body', { 'knowledge-browser-chrome__body--devtools': isLive() && chrome().devtoolsOpen }]}
      >
        <Show when={isLive()} fallback={props.children}>
          <div class="knowledge-browser-chrome__page" aria-label="Gjengitt nettleserside" onAuxClick={onChromePageAuxClick}>
            <Show when={searchQuery()}>
              {(query) => (
                <BrowserSearchResults
                  query={query()}
                  zdr={session().zdr}
                  limitedToHost={limitedToHost()}
                  refusedHost={refusedHost()}
                  onOpen={openSearchResult}
                  onClose={() => setSearchQuery(null)}
                />
              )}
            </Show>
            <Show when={pendingApproval()}>
              {(approval) => (
                <Show
                  when={browserGateUsesDecisionCard(approval().actionType) && browserApprovalToCardModel(approval())}
                  fallback={
                <div
                  class="knowledge-browser-ai-bubble knowledge-browser-ai-bubble--approval"
                  role="alert"
                  aria-label={i18n.tr('Venter på godkjenning', 'Waiting for approval')}
                >
                  <AlertCircle class="size-3.5" aria-hidden="true" />
                  <div class="knowledge-browser-ai-bubble__body">
                    <span class="knowledge-browser-ai-bubble__kind">
                      {browserRiskLabel(approval().riskCategory, i18n)}
                    </span>
                    <p>{approval().reason || i18n.tr('Handlingen krever godkjenning før den utføres.', 'The action needs approval before it is carried out.')}</p>
                    <Show when={approval().url || approval().selector}>
                      <p class="knowledge-browser-ai-bubble__meta">
                        <Show when={approval().actionType}>{approval().actionType} · </Show>
                        <Show when={approval().url}>{compactBrowserUrl(approval().url)}</Show>
                        <Show when={approval().selector}> · {approval().selector}</Show>
                      </p>
                    </Show>
                    {/* A one-time-code fill waits for the OTP card (E3.1-i): no generic buttons, even without an id. */}
                    <Show when={!browserGateOnOtpCard(approval().actionType)}>
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
                          {i18n.tr('Godkjenn', 'Approve')}
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
                          {i18n.tr('Avslå', 'Reject')}
                        </button>
                      </div>
                    </Show>
                  </div>
                </div>
                  }
                >
                  {/* E3.2-b swap: the shared card, inline over the live page (D9). */}
                  <div
                    class="knowledge-browser-ai-bubble knowledge-browser-ai-bubble--approval"
                    aria-label={i18n.tr('Venter på godkjenning', 'Waiting for approval')}
                  >
                    <BrowserGateDecisionCard
                      approval={approval()}
                      onDecide={props.onDecideBrowserApproval}
                      onTakeOver={approvalTakeOver()}
                    />
                  </div>
                </Show>
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
            <Show when={import.meta.env.VITE_BROWSER_NEWTAB === 'true' && !frameUrl() && !searchQuery() && session().tabs.length === 0}>
              <form class="knowledge-browser-newtab" onSubmit={(event) => { event.preventDefault(); const value = addressInput().trim(); if (value) runAction({ type: 'navigate', url: value }) }}>
                <label>Ny fane</label>
                <input aria-label="Søk eller skriv nettadresse" placeholder="Søk eller skriv nettadresse" value={addressInput()} onInput={(event) => setAddressInput(event.currentTarget.value)} disabled={controlsDisabled()} type="text" inputmode="url" />
                <button type="submit" disabled={controlsDisabled() || !addressInput().trim()}>Åpne</button>
                <p>Start her: ingen åpen fane.</p>
              </form>
            </Show>
            <Show
              when={searchQuery() ? null : frameUrl()}
              keyed
              fallback={
                searchQuery() ? null : replayScrubbing() ? (
                  <p class="knowledge-browser-empty">{i18n.tr('Ingen skjermbilde for dette steget.', 'No screenshot for this step.')}</p>
                ) : <div class="knowledge-browser-chrome__domlist">
                  <DomNodeList nodes={session().domNodes} />
                </div>
              }
            >              {(src: string) => (
                <div
                  class="knowledge-browser-screenshot knowledge-browser-screenshot--interactive"
                  style={{ position: 'relative', transform: zoomEnabled() ? `scale(${frameZoom() / 100})` : undefined, 'transform-origin': 'top center' }}
                  aria-label="Interaktiv Chromium-side. Klikk, scroll eller fokuser for tastatur."
                  role="application"
                  tabindex={0}
                  onClick={runViewportClick}
                  onKeyDown={runViewportKey}
                  onWheel={runViewportWheel}
                >
                  <img
                    src={src}
                    alt={`Gjengitt nettleserside for ${session().title}`}
                    decoding="async"
                    onLoad={(event) => {
                      const img = event.currentTarget
                      const viewport = session().viewport
                      setFrameAspectMatches(Boolean(img.naturalHeight && viewport.height &&
                        Math.abs(img.naturalWidth / img.naturalHeight - viewport.width / viewport.height) < 0.01))
                      handleFrameLoad(src)
                    }}
                    onError={() => { setFrameAspectMatches(false); handleFrameError(src) }}
                  />
                  <Show when={frameAspectMatches() && (src === streamFrameSrc() || src === liveFrameSrc()) && actionTarget()}>
                    {(target) => (
                      <svg aria-hidden="true" viewBox={`0 0 ${session().viewport.width} ${session().viewport.height}`}
                        preserveAspectRatio="xMidYMin meet"
                        style={{ position: 'absolute', inset: '0', width: '100%', height: '100%', 'pointer-events': 'none' }}>
                        <rect x={target().x} y={target().y} width={target().width} height={target().height}
                          fill="rgba(238,122,80,.12)" stroke="#cf613f" stroke-width="3" />
                        <path d={`M ${target().x} ${target().y} l 4 23 5 -7 8 8 4 -4 -8 -8 8 -4 z`}
                          fill="#cf613f" stroke="white" stroke-width="2" />
                      </svg>
                    )}
                  </Show>
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
                  aria-selected={devtoolsTab() === 'dom' ? 'true' : 'false'}
                  class={['knowledge-browser-devtools__tab', { 'knowledge-browser-devtools__tab--active': devtoolsTab() === 'dom' }]}
                  onClick={() => setDevtoolsTab('dom')}
                >
                  <Code2 class="size-3.5" /> DOM <em>{session().nodeCount ?? session().domNodes.length}</em>
                </button>
                <button
                  type="button"
                  role="tab"
                  aria-selected={devtoolsTab() === 'console' ? 'true' : 'false'}
                  class={['knowledge-browser-devtools__tab', { 'knowledge-browser-devtools__tab--active': devtoolsTab() === 'console' }]}
                  onClick={() => setDevtoolsTab('console')}
                >
                  <Terminal class="size-3.5" /> Console <em>{liveConsoleEntries().length}</em>
                </button>
                <button
                  type="button"
                  role="tab"
                  aria-selected={devtoolsTab() === 'network' ? 'true' : 'false'}
                  class={['knowledge-browser-devtools__tab', { 'knowledge-browser-devtools__tab--active': devtoolsTab() === 'network' }]}
                  onClick={() => setDevtoolsTab('network')}
                >
                  <Network class="size-3.5" /> Network <em>{liveNetworkEntries().length}</em>
                </button>
                <Show when={hasEvidence()}>
                  <button
                    type="button"
                    role="tab"
                    aria-selected={devtoolsTab() === 'egress' ? 'true' : 'false'}
                    class={['knowledge-browser-devtools__tab', { 'knowledge-browser-devtools__tab--active': devtoolsTab() === 'egress' }]}
                    onClick={() => setDevtoolsTab('egress')}
                  >
                    <Send class="size-3.5" /> Egress
                  </button>
                </Show>
                <Show when={session().visualObservationArtifactId}>
                  <button
                    type="button"
                    role="tab"
                    aria-selected={devtoolsTab() === 'vision' ? 'true' : 'false'}
                    class={['knowledge-browser-devtools__tab', { 'knowledge-browser-devtools__tab--active': devtoolsTab() === 'vision' }]}
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
                    <Show when={hasEvidence()}>
                      <ConsoleFilterBar
                        value={consoleFilter()}
                        total={liveConsoleEntries().length}
                        shown={visibleConsoleEntries().length}
                        onChange={setConsoleFilter}
                      />
                    </Show>
                    <div class="knowledge-browser-console-list">
                      <For
                        each={visibleConsoleEntries()}
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
                  <Match when={devtoolsTab() === 'egress'}>
                    <EgressReceiptsPanel sessionId={session().sessionId ?? ''} zdr={session().zdr} />
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
            <strong>{session().timeline.length} steg · {ownerTimeline().length} eierhendelser</strong>
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
                    <Show
                      when={browserGateUsesDecisionCard(approval.actionType) && approval.status === 'pending'}
                      fallback={
                        <BrowserApprovalCard
                          approval={approval}
                          deciding={decidingBrowserApprovalKeys().includes(approval.key)}
                          onDecide={props.onDecideBrowserApproval}
                        />
                      }
                    >
                      <BrowserGateDecisionCard
                        approval={approval}
                        onDecide={props.onDecideBrowserApproval}
                        onTakeOver={approvalTakeOver()}
                      />
                    </Show>
                  )}
                </For>
              </div>
            </div>
          </Show>
          <Show when={hasEvidence()}>
            <ProcedurePanel sessionId={session().sessionId ?? ''} zdr={session().zdr} />
          </Show>
          <Show when={session().ownerTimelineError}>
            {(message) => (
              <p class="knowledge-browser-evidence__hint" role="alert">
                Varig aktivitet kunne ikke lastes: {message()}
              </p>
            )}
          </Show>
          <Show
            when={session().timeline.length > 0 || ownerTimeline().length > 0}
            fallback={<p class="knowledge-browser-evidence__hint">Ingen varig nettleseraktivitet er registrert ennå.</p>}
          >
            <Show when={ownerTimeline().length > 0}>
              <div class="knowledge-browser-replay" aria-label="Varig nettleseraktivitet">
                <header>
                  <span>Aktivitet</span>
                  <strong>{ownerTimeline().length}</strong>
                </header>
                <For each={ownerTimeline()}>
                  {(event) => (
                    <div class={`knowledge-browser-replay__event knowledge-browser-replay__event--${event.kind}`}>
                      <span class="knowledge-browser-replay__kind">{event.kind}</span>
                      <div>
                        <strong>{ownerTimelineEventLabel(event)}</strong>
                        <p>{ownerTimelineEventMeta(event)}</p>
                      </div>
                    </div>
                  )}
                </For>
              </div>
            </Show>
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
            <Show when={replayScrubberEnabled() && replayScrubberAvailable(replaySteps())}>
              <BrowserReplayScrubber
                steps={replaySteps()}
                selectedStep={selectedTimelineStep()}
                onSeek={(step) => setSelectedTimelineStep(step)}
              />
            </Show>
            <div class="knowledge-browser-timeline" aria-label="Nettleserhistorikk">
              <button
                type="button"
                class={{ 'knowledge-browser-timeline__step--active': selectedTimelineStep() === null }}
                onClick={() => setSelectedTimelineStep(null)}
              >
                <span>live</span>
                <strong>{session().title}</strong>
              </button>
              <For each={timelineTabs()}>
                {(entry) => (
                  <button
                    type="button"
                    class={{ 'knowledge-browser-timeline__step--active': selectedTimelineStep() === entry.step }}
                    title="Vis stegdetaljer med evidens"
                    onClick={() => setSelectedTimelineStep((current) => (current === entry.step ? null : entry.step))}
                  >
                    <span>#{entry.step}</span>
                    <Show
                      when={timelineBrief(entry)}
                      fallback={<strong>{entry.title || compactBrowserUrl(entry.url || session().url)}</strong>}
                    >
                      {(brief) => <strong>{brief().title}</strong>}
                    </Show>
                    <Show when={rationaleForStep(props.browserRationales ?? [], entry.step) || entry.visualObservationArtifactId || timelineBrief(entry)}>
                      <span class="knowledge-browser-timeline__markers">
                        <Show when={rationaleForStep(props.browserRationales ?? [], entry.step)}>
                          <Sparkles class="size-3" aria-label="AI-foreslått steg" />
                        </Show>
                        <Show when={entry.visualObservationArtifactId}>
                          <Eye class="size-3" aria-label="Visuell observasjon tilgjengelig" />
                        </Show>
                        <Show when={timelineBrief(entry)}>
                          {(brief) => (
                            <span class={`verevon-run-timeline__pill verevon-run-timeline__pill--${brief().pill.tone}`}>
                              {brief().pill.label}
                            </span>
                          )}
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
