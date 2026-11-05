import { changeBrowserControl, STALE_CONTROL_COPY } from '@/features/browser/dock/browser-control'
import { useAssistantBrowser, type NettBrowserOwner } from '@/features/core/assistant-agent/assistant-browser-context'
import {
  AlertCircle,
  ArrowRight,
  CheckCircle2,
  FileUp,
  Globe2,
  KeyRound,
  Link2,
  ListChecks,
  Loader2,
  Pencil,
  Plus,
  RefreshCw,
  ShieldCheck,
  ShoppingBag,
  Trash2,
} from '@/shared/icons'
import { createEffect, createMemo, createSignal, For, Match, onCleanup, Show, Switch, untrack } from 'solid-js'
import { createResource } from '@/shared/lib/create-resource-compat'
import { executeAction } from '@/shared/actions/action-client'
import { leftWithRunControlsDisabled, runControlsDisabledNotice } from './browser-blocked'
import { getAuthSession, getSessionContext } from '@/shared/api/auth-client'
import {
  closeBrowserSession,
  closeBrowserTab,
  createBrowserTab,
  createBrowserSession,
  createBrowserProfile,
  browserErrorMessage,
  deleteBrowserProfile,
  getBrowserOwnerTimeline,
  getBrowserSession,
  keepAliveBrowserSession,
  listBrowserProfiles,
  listBrowserTabs,
  probeBrowserProfile,
  renameBrowserProfile,
  runBrowserAction,
  selectBrowserTab,
  suggestBrowserAction,
  type BrowserAction,
  type BrowserActionSuggestionResponse,
  type BrowserControlMode,
  type BrowserObservation,
  type BrowserProfileRestoreProbe,
  type BrowserProfileSummary,
  type BrowserSession,
  type BrowserSessionResponse,
  type CreatableBrowserProfileScope,
} from '@/shared/api/browser-client'
import { createAiRunStartKeyring, startBrowserAiRun } from '@/shared/api/browser-run-client'
import { streamRunEvents } from '@/shared/api/run-console-client'
import { decideApproval, listApprovals, type ApprovalDecision } from '@/shared/api/orchestration-client'
import {
  buildCaptureRequest,
  buildCrawlScopeOptions,
  CRAWL_MAX_DEPTH,
  crawlSelectedPages,
  createDocument,
  discoverCrawlPages,
  extractProducts,
  getImportJob,
  importUpload,
  knowledgeScrapeOptionsEnabled,
  listCrawlJobs,
  scrapePreview,
  scrapeUrl,
  startCrawl,
  streamCrawlRunEvents,
  summarizeProducts,
  type CrawlDiscovery,
  type CrawlWorkflowEvent,
  type Product,
  type ProductExtraction,
} from '@/shared/api/knowledge-client'
import { resolveCrawlIngest } from '@/shared/api/settings-client'
import { cn } from '@/shared/lib/cn'
import { useI18n, translateApiError, LocalizedError } from '@/shared/i18n'
import { readClientJson, removeClientValue, writeClientJson } from '@/shared/session/client-storage'
import { CrawlPagePicker } from './CrawlPagePicker'
import {
  CrawlApprovalPrompt,
  type CrawlApprovalEstimate,
  type CrawlApprovalRequest,
} from '@/features/knowledge/components/CrawlApprovalPrompt'
import { DashboardPlanBadge } from './DashboardPlanBadge'
import { ProductPicker } from './ProductPicker'
import { KnowledgeMonitoring } from './KnowledgeMonitoring'
import { ScrapePreviewPanel } from './KnowledgeScrapePreview'
import { navigationFailureFromError, navigationHttpResponseFailure, type BrowserNavigationFailure } from '@/features/browser/session/browser-navigation-error'
import type { KeepAliveOutcome } from '@/features/browser/session/session-keepalive'
import { ApiError } from '@/shared/api/http'
import { getSession } from '@/shared/session/session-store'
import { PROFILE_SCOPE_LABELS, type BrowserProfileManagerProps } from './BrowserChrome'
import { createBrowserLoopController, type BrowserLoopState } from './browser-loop'
import {
  attachBrowserObservation,
  attachBrowserOwnerTimeline,
  attachBrowserSession,
  attachBrowserTabs,
  withBrowserOwnerTimelineError,
  withBrowserApprovalDecided,
  withBrowserApprovalRequested,
  withStepRationale,
  type BrowserApprovalEntry,
  type BrowserStepRationale,
} from './browser-session'
import {
  hostnameOf,
  normalizeUrl,
  toScrapePreview,
  type ScrapeBlock,
  type ScrapePreview,
} from './knowledge-preview'
import { robotsBlockedText } from './knowledge-robots'
import { webAddress } from './web-address'

type IngestMode = 'link' | 'crawl' | 'products' | 'monitor'
type IngestKind = 'link' | 'crawl' | 'upload'

type IngestJob = {
  agentMode?: boolean
  detail: string
  error?: string
  events?: IngestEvent[]
  eventStream?: string
  id?: string
  /** Whether this crawl/batch job asked quarry to persist pages into the
   * knowledge base (`ingest: true`). Undefined/false for a working-set-only
   * run. Drives {@link statusLabel} — a crawl's own completion is NEVER, by
   * itself, evidence that anything was durably indexed. */
  ingestRequested?: boolean
  /** Set true once a `store_record_failed` event is observed for this job —
   * the crawl succeeded but the Data Plane write for at least one page did
   * not (e.g. an org-identity or Data Plane outage). */
  ingestFailed?: boolean
  /** Set true once at least one `store_record_written` event is observed —
   * real proof a page was durably persisted, independent of quarry's own
   * scrape/crawl completion status. */
  ingestWritten?: boolean
  key: string
  kind: IngestKind
  label: string
  /** Pages fetched, surfaced from the crawl's run_completed event. */
  pages?: number
  /** Pages the crawl's robots.txt gate refused (WS14.S4), from the run_completed event. */
  pagesBlockedByRobots?: number
  progress?: number
  status: string
}

type ActiveIngestJob = IngestJob & { id: string }
type IngestEvent = {
  createdAt: string
  detail: string
  event?: string
  progress?: number
  status?: string
}
type BrowserSessionAttempt = {
  error: string | null
  errorCode?: string | null
  navigationFailure?: BrowserNavigationFailure
  session: BrowserSessionResponse | null
}
type PersistedBrowserPreview = {
  browserRationales: BrowserStepRationale[]
  preview: ScrapePreview
  savedAt: string
}

const TERMINAL = new Set(['completed', 'complete', 'failed', 'error', 'succeeded', 'cancelled'])
const POLL_INTERVAL_MS = 2500
const BROWSER_SESSION_TIMEOUT_MS = 30000
const BROWSER_AI_LOOP_MAX_STEPS = 5
const crawlJobsStoragePrefix = 'verevon.dashboard.crawl.jobs'
const browserPreviewStoragePrefix = 'verevon.dashboard.browser.preview'
const isolatedProfileChoice = 'isolated'
const newProfileChoice = 'new'

async function loadOrgContext() {
  const [session, ctx] = await Promise.all([getAuthSession(), getSessionContext()])
  return {
    orgId: ctx.orgId ?? ctx.orgs[0]?.id ?? '',
    userId: session?.user.id ?? ctx.userId ?? '',
  }
}

function isTerminal(status: string): boolean {
  return TERMINAL.has(status.trim().toLowerCase())
}

function hasActiveJobId(job: IngestJob): job is ActiveIngestJob {
  return Boolean(job.id) && !isTerminal(job.status)
}

export function KnowledgeComposer(props: {
  /** Reuse the existing browser surface in Nett without the extraction controls. */
  browserOnly?: boolean
  browserUrl?: string | null
  browserContextVisible?: boolean
  inlinePlanLabel?: string
  inlineTitle?: string
  onPreviewActiveChange?: (active: boolean) => void
  previewCollapsed?: boolean
}) {
  const i18n = useI18n()
  let pollTimer: number | undefined
  let tabRefreshTimers: number[] = []
  let tabRefreshGeneration = 0
  let latestAppliedTabRefresh = 0
  let hydratedJobsKey: string | null = null
  let hydratedBrowserPreviewKey: string | null = null
  let keySeq = 0
  let previewRequestVersion = 0
  const crawlStreams = new Map<string, AbortController>()
  const [ctx] = createResource(loadOrgContext)
  const orgId = createMemo(() => ctx()?.orgId ?? '')
  const crawlJobsStorageKey = createMemo(() => !props.browserOnly && orgId() ? `${crawlJobsStoragePrefix}.${orgId()}` : null)
  const browserPreviewStorageKey = createMemo(() => !props.browserOnly && orgId() ? `${browserPreviewStoragePrefix}.${orgId()}` : null)
  const [url, setUrl] = createSignal('')
  const [mode, setMode] = createSignal<IngestMode>(props.browserOnly ? 'link' : 'crawl')
  const [agentMode, setAgentMode] = createSignal(false)
  const [crawlMaxPages, setCrawlMaxPages] = createSignal(25)
  // W2-U65-51: crawl scope options and scrape captures. Behind VITE_KNOWLEDGE_SCRAPE_OPTIONS_ENABLED
  // (off by default); with the flag off the controls are hidden and nothing below is sent.
  const scrapeOptions = knowledgeScrapeOptionsEnabled()
  const [crawlMaxDepth, setCrawlMaxDepth] = createSignal<number | undefined>(undefined)
  const [crawlIncludeText, setCrawlIncludeText] = createSignal('')
  const [crawlExcludeText, setCrawlExcludeText] = createSignal('')
  const [crawlAllowSubdomains, setCrawlAllowSubdomains] = createSignal(false)
  // W3-E3.7.9b: a crawl over the approval threshold waits here for the person. Set when the gateway
  // answers a crawl start with a pending approval (W3-E3.7.9a, not built yet), so nothing sets it today;
  // the prompt itself also renders nothing unless VITE_KNOWLEDGE_CRAWL_APPROVAL_ENABLED is "true".
  const [crawlApproval, setCrawlApproval] = createSignal<{
    request: CrawlApprovalRequest
    estimate: CrawlApprovalEstimate
    urls: string[]
  } | null>(null)
  const [captureScreenshot, setCaptureScreenshot] = createSignal(false)
  const [capturePdf, setCapturePdf] = createSignal(false)
  const [submitting, setSubmitting] = createSignal(false)
  const [adding, setAdding] = createSignal(false)
  const [browserBusy, setBrowserBusy] = createSignal(false)
  // A navigation that did not load, kept for the error card and its Retry (WS9.B).
  const [browserNavigationError, setBrowserNavigationError] = createSignal<BrowserNavigationFailure | null>(null)
  const [browserProfiles, setBrowserProfiles] = createSignal<BrowserProfileSummary[]>([])
  const [browserProfilesLoading, setBrowserProfilesLoading] = createSignal(false)
  const [browserProfileChoice, setBrowserProfileChoice] = createSignal(isolatedProfileChoice)
  const [browserProfileProbe, setBrowserProfileProbe] = createSignal<BrowserProfileRestoreProbe | null>(null)
  const [browserProfileProbing, setBrowserProfileProbing] = createSignal(false)
  const [browserProfileError, setBrowserProfileError] = createSignal<string | null>(null)
  // Phase 3 continuation — explicit profile create/rename/delete state.
  const [newProfileName, setNewProfileName] = createSignal('')
  const [newProfileScope, setNewProfileScope] = createSignal<CreatableBrowserProfileScope>('user_private')
  const [browserProfileCreating, setBrowserProfileCreating] = createSignal(false)
  const [browserProfileMutating, setBrowserProfileMutating] = createSignal(false)
  const [browserProfileDeleteArmed, setBrowserProfileDeleteArmed] = createSignal(false)
  const [renamingProfile, setRenamingProfile] = createSignal(false)
  const [renameProfileName, setRenameProfileName] = createSignal('')
  // Client-side mirror of the gateway's `reject_zdr_persistent_profile`
  // guard: when checked, the profile picker is forced back to isolated and
  // disabled — this NEVER replaces the server-side check (`createBrowserSession`
  // below still omits every profile field whenever this is true), it only
  // stops the UI from ever building a request the server would reject anyway.
  const [zdrRequested, setZdrRequested] = createSignal(false)
  const [browserLoopState, setBrowserLoopState] = createSignal<BrowserLoopState>({
    error: null,
    goal: '',
    status: 'idle',
    step: 0,
  })
  const browserLoop = createBrowserLoopController(setBrowserLoopState)
  const [browserRationales, setBrowserRationales] = createSignal<BrowserStepRationale[]>([])
  // Latest streamed rationale from the durable server-side AI run (Phase 2).
  // Kept out of `browserRationales` on purpose: that map is keyed by the
  // visible tab session's observation steps, while an AI run executes in its
  // own Quarry session with its own step numbering.
  const [aiRunRationale, setAiRunRationale] = createSignal<BrowserStepRationale | null>(null)
  // Phase 5 HITL gate: pending/decided browser-action approvals for the
  // active durable AI run. Reset alongside `aiRunRationale` at the start of
  // every run — a stale approval from a previous run must never linger.
  const [browserApprovals, setBrowserApprovals] = createSignal<BrowserApprovalEntry[]>([])
  // Approval *keys* (not approval ids — the id is often still unknown, see
  // `handleDecideBrowserApproval`) with an in-flight decide call, so the
  // Approve/Reject buttons can show a busy state and avoid double-submits.
  const [decidingApprovalKeys, setDecidingApprovalKeys] = createSignal<string[]>([])
  let aiRunAbort: AbortController | null = null
  onCleanup(() => aiRunAbort?.abort())
  // One `Idempotency-Key` per logical AI-run start: kept across a retry of the
  // same start after an ambiguous failure, dropped on any definite answer.
  const aiRunStartKeys = createAiRunStartKeyring()
  // The run the last start created, with the session it was started on, so leaving a
  // blocked site can stop it (the server treats a run that already ended as nothing
  // left to stop). Only ever used for that same session, and dropped wherever the
  // preview goes away (Discard, leaving a blocked site, adding to the knowledge base).
  let lastAiRun: { sessionId: string; runId: string } | null = null
  const [preview, setPreview] = createSignal<ScrapePreview | null>(null)
  const assistantBrowser = useAssistantBrowser()
  const [nettBrowserOwner, setNettBrowserOwner] = createSignal<NettBrowserOwner | null>(null)
  const [assistantHandoff, setAssistantHandoff] = createSignal<'waiting' | 'unavailable' | null>(null)
  const currentOwner = () => {
    const current = getSession()
    return { orgId: current.activeOrg?.id, userId: current.user?.id }
  }
  const matchesCurrentOwner = (owner: { orgId?: string; userId?: string }) => {
    const current = currentOwner()
    return Boolean(owner.orgId && owner.userId && owner.orgId === current.orgId && owner.userId === current.userId)
  }
  if (props.browserOnly && assistantBrowser.registerNettBrowser) {
    onCleanup(assistantBrowser.registerNettBrowser(() => {
      const owner = nettBrowserOwner()
      const browser = preview()?.browserSession?.session
      return props.browserContextVisible && owner && matchesCurrentOwner(owner) && !browserBusy() && !assistantHandoff() &&
        browser?.id === owner.sessionId && browser.status === 'live' && browser.control?.mode === 'human_takeover'
        ? owner : null
    }))
    createEffect(currentOwner, (current) => {
      const owner = untrack(nettBrowserOwner)
      if (owner && (owner.orgId !== current.orgId || owner.userId !== current.userId)) setNettBrowserOwner(null)
    })
  }
  const [pendingSavedPreview, setPendingSavedPreview] = createSignal<PersistedBrowserPreview | null>(null)
  const [discovery, setDiscovery] = createSignal<CrawlDiscovery | null>(null)
  const [discovering, setDiscovering] = createSignal(false)
  const [productExtraction, setProductExtraction] = createSignal<ProductExtraction | null>(null)
  const [productSummary, setProductSummary] = createSignal<string | null>(null)
  const [summarizing, setSummarizing] = createSignal(false)
  const [formError, setFormError] = createSignal<string | null>(null)
  const [browserOpenRecovery, setBrowserOpenRecovery] = createSignal<{
    url: string
    kind: 'permission' | 'driver' | 'handoff'
  } | null>(null)
  const [browserOpening, setBrowserOpening] = createSignal(false)
  // A plain-language note that is not a failure: set when the site was left but the AI run
  // could not be stopped (run controls off). Cleared by the next link submit.
  const [leaveNotice, setLeaveNotice] = createSignal<string | null>(null)
  const [jobs, setJobs] = createSignal<IngestJob[]>([])
  let fileInputRef: HTMLInputElement | undefined
  let hydratedProfilesOrg: string | null = null

  // Tell the dashboard when a preview / page-picker / product-picker is on screen
  // so it can hide the info cards and surface the results/cards chevron toggle.
  createEffect(
    () => Boolean(preview() || discovery() || productExtraction()),
    (active) => { props.onPreviewActiveChange?.(active) },
  )

  const ready = createMemo(() => orgId().length > 0)
  const canSubmitUrl = createMemo(() => url().trim().length > 0 && !submitting() && !discovering() && ready() && (mode() !== 'crawl' || crawlMaxPages() > 0))
  const canDiscover = createMemo(() => url().trim().length > 0 && !discovering() && !submitting() && ready())
  const openFileDialog = () => fileInputRef?.click()
  const selectedBrowserProfileId = createMemo(() => {
    const choice = browserProfileChoice()
    return isProfileId(choice) ? choice : null
  })
  // Phase 3 continuation: the selected profile's real stored summary
  // (name + scope) so session-create can pass the genuine scope instead
  // of inferring it from a boolean.
  const selectedBrowserProfileSummary = createMemo<BrowserProfileSummary | null>(() => {
    const id = selectedBrowserProfileId()
    if (!id) return null
    return browserProfiles().find((profile) => profile.profile_id === id) ?? null
  })

  // ZDR client-side guard (mirrors, never replaces, the gateway's
  // `reject_zdr_persistent_profile`): as soon as ZDR is requested, force the
  // picker back to isolated so no persistent-profile UI state can leak into
  // the next `createBrowserSession` call.
  createEffect(
    () => ({ zdr: zdrRequested(), choice: browserProfileChoice() }),
    ({ zdr, choice }) => {
      if (!zdr) return
      if (choice === isolatedProfileChoice) return
      setBrowserProfileChoice(isolatedProfileChoice)
      setBrowserProfileProbe(null)
      setBrowserProfileDeleteArmed(false)
      setRenamingProfile(false)
    },
  )

  const updateJob = (key: string, patch: Partial<IngestJob>) => {
    setJobs((current) => current.map((job) => (job.key === key ? { ...job, ...patch } : job)))
  }

  const appendJobEvent = (key: string, event: CrawlWorkflowEvent) => {
    const next: IngestEvent = {
      createdAt: new Date().toISOString(),
      detail: event.detail,
      event: event.event,
      progress: event.progress,
      status: event.status,
    }
    setJobs((current) => current.map((job) => {
      if (job.key !== key) return job
      const events = [next, ...(job.events ?? [])].slice(0, 5)
      return {
        ...job,
        detail: event.detail || job.detail,
        events,
        ...(event.progress !== undefined ? { progress: event.progress } : {}),
        ...(event.status ? { status: event.status } : {}),
        ...(event.pagesVisited !== undefined ? { pages: event.pagesVisited } : {}),
        ...(event.pagesBlockedByRobots !== undefined ? { pagesBlockedByRobots: event.pagesBlockedByRobots } : {}),
        // Real Data Plane ingest proof/failure — independent of quarry's own
        // run status, so a crawl can't read "Indexed" off scrape completion
        // alone. Latched (never cleared) so one written page still counts
        // even if a later page on the same job fails, and vice versa.
        ...(event.event === 'store_record_written' ? { ingestWritten: true } : {}),
        ...(event.event === 'store_record_failed' ? { ingestFailed: true } : {}),
      }
    }))
  }

  const addJob = (job: Omit<IngestJob, 'key'>): string => {
    const key = `job-${(keySeq += 1)}`
    setJobs((current) => [{ ...job, key }, ...current].slice(0, 6))
    ensurePolling()
    return key
  }

  const ensurePolling = () => {
    if (pollTimer !== undefined) return
    pollTimer = window.setInterval(() => void pollActiveJobs(), POLL_INTERVAL_MS)
  }

  const stopPollingIfIdle = () => {
    const active = untrack(() => jobs().some(hasActiveJobId))
    if (!active && pollTimer !== undefined) {
      window.clearInterval(pollTimer)
      pollTimer = undefined
    }
  }

  const pollActiveJobs = async () => {
    const id = orgId()
    if (!id) return
    const active = jobs().filter(hasActiveJobId)
    if (active.length === 0) {
      stopPollingIfIdle()
      return
    }

    const crawlActive = active.some((job) => job.kind === 'crawl')
    const crawlJobs = crawlActive ? await listCrawlJobs(id).catch(() => []) : []

    await Promise.allSettled(
      active.map(async (job) => {
        if (job.kind === 'crawl') {
          startCrawlEventStream(job.key, job.id)
          const match = crawlJobs.find((entry) => entry.id === job.id)
          if (match?.status) updateJob(job.key, { status: match.status })
          return
        }
        try {
          const fresh = await getImportJob(id, job.id)
          updateJob(job.key, {
            status: fresh.status,
            ...(fresh.error ? { error: fresh.error } : {}),
          })
        } catch {
          // Transient poll failure — keep the existing status, retry next tick.
        }
      }),
    )
    stopPollingIfIdle()
  }

  const switchMode = (next: IngestMode) => {
    setMode(next)
    if (next !== 'link') clearPreview()
    if (next !== 'crawl') setDiscovery(null)
    if (next !== 'products') {
      setProductExtraction(null)
      setProductSummary(null)
    }
  }

  const startCrawlEventStream = (key: string, runId: string) => {
    const id = orgId()
    const job = jobs().find((entry) => entry.key === key)
    if (!id || crawlStreams.has(key) || isTerminal(job?.status ?? '')) return
    const controller = new AbortController()
    crawlStreams.set(key, controller)
    void streamCrawlRunEvents(
      id,
      runId,
      {
        onEvent: (event) => appendJobEvent(key, event),
        onError: (message) => appendJobEvent(key, { detail: message, event: 'error', status: 'error' }),
        onDone: () => {
          crawlStreams.delete(key)
          stopPollingIfIdle()
        },
      },
      controller.signal,
      // Honor the server-provided event-stream path (job-id-keyed SSE).
      job?.eventStream,
    ).finally(() => {
      crawlStreams.delete(key)
      stopPollingIfIdle()
    })
  }

  createEffect(
    () => crawlJobsStorageKey(),
    (key) => {
      if (!key || hydratedJobsKey === key) return
      const restored = readClientJson(key, isPersistedJobs) ?? []
      keySeq = restored.reduce((max, job) => Math.max(max, parseJobKey(job.key)), 0)
      setJobs(restored)
      hydratedJobsKey = key
      for (const job of restored.filter(hasActiveJobId)) {
        if (job.kind === 'crawl') startCrawlEventStream(job.key, job.id)
      }
      if (restored.some(hasActiveJobId)) ensurePolling()
    },
  )

  createEffect(
    () => orgId(),
    (id) => {
      if (!id || hydratedProfilesOrg === id) return
      hydratedProfilesOrg = id
      void refreshBrowserProfiles()
    },
  )

  createEffect(
    () => ({ key: crawlJobsStorageKey(), jobsSnapshot: jobs() }),
    ({ key, jobsSnapshot }) => {
      if (!key || hydratedJobsKey !== key) return
      writeClientJson(key, jobsSnapshot)
    },
  )

  createEffect(
    () => browserPreviewStorageKey(),
    (key) => {
      if (!key || hydratedBrowserPreviewKey === key) return
      hydratedBrowserPreviewKey = key
      const restored = readClientJson(key, isPersistedBrowserPreview)
      if (!restored) return
      if (restored.preview.browserSession?.session.zdr) {
        removeClientValue(key)
        return
      }
      // Retain the saved extraction without changing Crawl or opening an old session.
      setPendingSavedPreview(restored)
    },
  )

  createEffect(
    () => ({ key: browserPreviewStorageKey(), current: preview(), rationales: browserRationales() }),
    ({ key, current, rationales }) => {
      if (!key || hydratedBrowserPreviewKey !== key) return
      // Clearing is explicit; initial empty state must never delete a saved preview.
      if (!current) return
      if (current.browserSession?.session.zdr) {
        removeClientValue(key)
        return
      }
      writeClientJson(key, {
        browserRationales: rationales,
        preview: current,
        savedAt: new Date().toISOString(),
      } satisfies PersistedBrowserPreview)
    },
  )

  const startCrawlJob = async (target: string) => {
    setDiscovery(null)
    const maxPages = crawlMaxPages()
    // W2-U65-51: the scope the user typed; empty when the flag is off or nothing was chosen.
    const scope = scrapeOptions
      ? buildCrawlScopeOptions({
          allowSubdomains: crawlAllowSubdomains(),
          excludeText: crawlExcludeText(),
          includeText: crawlIncludeText(),
          maxDepth: crawlMaxDepth(),
        })
      : {}
    // Phase 6 selective ingest: resolve the user's crawl_ingest_mode → ingest
    // flag (shared resolver; unset defaults to 'auto' so an explicit add-to-
    // Knowledge crawl actually persists — owner=user, private until shared).
    const ingest = await resolveCrawlIngest(() =>
      window.confirm(
        i18n.tr(
          'Lagre disse sidene i kunnskapsbasen? (synlig for organisasjonen din)',
          'Save these pages to the knowledge base? (visible to your organization)',
        ),
      ),
    )
    const key = addJob({
      agentMode: agentMode(),
      detail: agentMode()
        ? i18n.tr('Starter agentstyrt crawl-workflow ...', 'Starting agent-run crawl workflow ...')
        : i18n.tr(`Crawler opptil ${maxPages} sider ...`, `Crawling up to ${maxPages} pages ...`),
      ingestRequested: ingest,
      kind: 'crawl',
      label: target,
      status: 'pending',
    })
    try {
      if (agentMode()) {
        const execution = await executeAction('knowledge.crawl_site', {
          orgId: orgId(),
          type: 'human',
          userId: ctx()?.userId ?? '',
        }, {
          ingest,
          maxPages,
          ...scope,
          url: target,
        })
        updateJob(key, {
          eventStream: execution.eventStream,
          id: execution.runId,
          status: execution.status || 'queued',
        })
        startCrawlEventStream(key, execution.runId)
      } else {
        const job = await startCrawl(orgId(), { ingest, maxPages, ...scope, url: target })
        updateJob(key, { eventStream: job.eventStream, id: job.id, status: job.status || 'running' })
        startCrawlEventStream(key, job.id)
      }
      setUrl('')
    } catch (reason) {
      const message = translateApiError(reason, i18n.tr, { no: 'Crawl kunne ikke startes.', en: 'Crawl could not be started.' })
      updateJob(key, { status: 'failed', error: message })
      setFormError(message)
    }
  }

  // Crawl mode, "Velg sider": discover the site's pages (quarry /v1/map, read-only)
  // and open the picker. No ingest happens until the user confirms a selection.
  const discoverPages = async () => {
    if (!canDiscover()) return
    const target = normalizeUrl(url())
    if (!target) {
      setFormError(i18n.tr('Skriv inn en gyldig nettadresse, f.eks. nettsted.example eller https://nettsted.example.', 'Enter a valid web address, for example nettsted.example or https://nettsted.example.'))
      return
    }
    setFormError(null)
    setDiscovery(null)
    setDiscovering(true)
    try {
      const result = await discoverCrawlPages(orgId(), { url: target, limit: 200 })
      if (result.pages.length === 0) {
        setFormError(i18n.tr('Fant ingen sider å crawle på dette nettstedet. Prøv en annen adresse eller bruk hel-crawl.', 'No crawlable pages were found on this site. Try another address or use a full-site crawl.'))
        return
      }
      setDiscovery(result)
    } catch (reason) {
      setFormError(translateApiError(reason, i18n.tr, { no: 'Kunne ikke oppdage sider.', en: 'Could not discover pages.' }))
    } finally {
      setDiscovering(false)
    }
  }

  // Crawl exactly the selected pages via quarry's durable /v1/batch. Same run-event
  // stream as a whole-site crawl, so live progress is scoped to the selection.
  const crawlSelected = async (urls: string[]) => {
    if (urls.length === 0 || !ready() || submitting()) return
    setFormError(null)
    setSubmitting(true)
    const host = hostnameOf(discovery()?.url || urls[0] || '')
    // Phase 6 selective ingest, CrawlPagePicker parity fix: this flow never
    // resolved the user's crawl_ingest_mode at all, so "Velg sider" could
    // never persist to the knowledge base regardless of preference. Same shared
    // resolver as startCrawlJob (unset → 'auto'): picking specific pages to add
    // is an explicit persist intent.
    const ingest = await resolveCrawlIngest(() =>
      window.confirm(
        i18n.tr(
          'Lagre disse sidene i kunnskapsbasen? (synlig for organisasjonen din)',
          'Save these pages to the knowledge base? (visible to your organization)',
        ),
      ),
    )
    const key = addJob({
      detail: i18n.tr(`Crawler ${urls.length} valgte sider ...`, `Crawling ${urls.length} selected pages ...`),
      ingestRequested: ingest,
      kind: 'crawl',
      label: `${host} · ${urls.length} sider`,
      status: 'pending',
    })
    try {
      const job = await crawlSelectedPages(orgId(), urls, ingest)
      updateJob(key, { eventStream: job.eventStream, id: job.id, status: job.status || 'running' })
      startCrawlEventStream(key, job.id)
      setDiscovery(null)
      setUrl('')
    } catch (reason) {
      const message = translateApiError(reason, i18n.tr, { no: 'Crawl kunne ikke startes.', en: 'Crawl could not be started.' })
      updateJob(key, { status: 'failed', error: message })
      setFormError(message)
    } finally {
      setSubmitting(false)
      stopPollingIfIdle()
    }
  }

  // Products mode: render a listing page and extract its products as structured
  // cards. Selection + AI summary then happen in the ProductPicker below.
  const extractProductsFromUrl = async (target: string) => {
    setProductExtraction(null)
    setProductSummary(null)
    try {
      const result = await extractProducts(orgId(), { url: target })
      if (result.products.length === 0) {
        setFormError(i18n.tr('Fant ingen produkter på siden. Prøv en produktliste-URL, eller en annen side.', 'No products were found on the page. Try a product-list URL or another page.'))
        return
      }
      setProductExtraction(result)
    } catch (reason) {
      setFormError(translateApiError(reason, i18n.tr, { no: 'Kunne ikke hente produkter.', en: 'Could not fetch products.' }))
    }
  }

  const refreshBrowserProfiles = async () => {
    const id = orgId()
    if (!id || browserProfilesLoading()) return
    setBrowserProfilesLoading(true)
    setBrowserProfileError(null)
    try {
      const result = await listBrowserProfiles(id)
      setBrowserProfiles(result.profiles)
      const selectedId = selectedBrowserProfileId()
      if (selectedId && !result.profiles.some((profile) => profile.profile_id === selectedId)) {
        setBrowserProfileChoice(isolatedProfileChoice)
        setBrowserProfileProbe(null)
      }
    } catch (reason) {
      setBrowserProfileError(browserErrorMessage(reason, i18n.tr, { no: 'Nettleserprofiler kunne ikke hentes.', en: 'Browser profiles could not be loaded.' }))
    } finally {
      setBrowserProfilesLoading(false)
    }
  }

  const probeSelectedBrowserProfile = async () => {
    const id = orgId()
    const profileId = selectedBrowserProfileId()
    const target = normalizeUrl(url())
    if (!id || !profileId || !target || browserProfileProbing()) return
    setBrowserProfileProbing(true)
    setBrowserProfileError(null)
    try {
      setBrowserProfileProbe(await probeBrowserProfile(id, profileId, target))
    } catch (reason) {
      setBrowserProfileProbe(null)
      setBrowserProfileError(browserErrorMessage(reason, i18n.tr, { no: 'Profilen kunne ikke sjekkes.', en: 'The profile could not be checked.' }))
    } finally {
      setBrowserProfileProbing(false)
    }
  }

  const selectBrowserProfile = (profileId: string) => {
    setBrowserProfileChoice(profileId)
    setBrowserProfileProbe(null)
    setBrowserProfileError(null)
    setRenamingProfile(false)
    setBrowserProfileDeleteArmed(false)
  }

  const createNamedBrowserProfile = async () => {
    const id = orgId()
    if (!id || browserProfileCreating() || zdrRequested()) return
    setBrowserProfileCreating(true)
    setBrowserProfileError(null)
    try {
      const name = newProfileName().trim()
      const created = await createBrowserProfile(id, {
        ...(name ? { name } : {}),
        scope: newProfileScope(),
      })
      setBrowserProfiles((current) => [created, ...current.filter((profile) => profile.profile_id !== created.profile_id)])
      selectBrowserProfile(created.profile_id)
      setNewProfileName('')
    } catch (reason) {
      setBrowserProfileError(browserErrorMessage(reason, i18n.tr, { no: 'Profilen kunne ikke opprettes.', en: 'The profile could not be created.' }))
    } finally {
      setBrowserProfileCreating(false)
    }
  }

  const startRenamingSelectedBrowserProfile = () => {
    const summary = selectedBrowserProfileSummary()
    if (!summary) return
    setRenameProfileName(summary.name ?? '')
    setBrowserProfileDeleteArmed(false)
    setRenamingProfile(true)
  }

  const cancelRenamingBrowserProfile = () => {
    setRenamingProfile(false)
    setRenameProfileName('')
  }

  const confirmRenameSelectedBrowserProfile = async () => {
    const id = orgId()
    const profileId = selectedBrowserProfileId()
    const name = renameProfileName().trim()
    if (!id || !profileId || !name || browserProfileMutating()) return
    setBrowserProfileMutating(true)
    setBrowserProfileError(null)
    try {
      const updated = await renameBrowserProfile(id, profileId, { name })
      setBrowserProfiles((current) => current.map((profile) => (profile.profile_id === updated.profile_id ? updated : profile)))
      cancelRenamingBrowserProfile()
    } catch (reason) {
      setBrowserProfileError(browserErrorMessage(reason, i18n.tr, { no: 'Kunne ikke gi profilen nytt navn.', en: 'Could not rename the profile.' }))
    } finally {
      setBrowserProfileMutating(false)
    }
  }

  const armDeleteSelectedBrowserProfile = () => {
    setRenamingProfile(false)
    setBrowserProfileDeleteArmed(true)
  }

  const cancelDeleteSelectedBrowserProfile = () => setBrowserProfileDeleteArmed(false)

  const confirmDeleteSelectedBrowserProfile = async () => {
    const id = orgId()
    const profileId = selectedBrowserProfileId()
    if (!id || !profileId || browserProfileMutating()) return
    setBrowserProfileMutating(true)
    setBrowserProfileError(null)
    try {
      await deleteBrowserProfile(id, profileId)
      setBrowserProfiles((current) => current.filter((profile) => profile.profile_id !== profileId))
      setBrowserProfileChoice(isolatedProfileChoice)
      setBrowserProfileProbe(null)
      cancelRenamingBrowserProfile()
    } catch (reason) {
      setBrowserProfileError(browserErrorMessage(reason, i18n.tr, { no: 'Profilen kunne ikke slettes.', en: 'The profile could not be deleted.' }))
    } finally {
      setBrowserProfileDeleteArmed(false)
      setBrowserProfileMutating(false)
    }
  }

  const summarizeSelection = async (selected: Product[], focus: string) => {
    if (selected.length === 0 || summarizing()) return
    setSummarizing(true)
    setProductSummary(null)
    try {
      const summary = await summarizeProducts(orgId(), selected, focus || undefined)
      setProductSummary(summary || i18n.tr('Ingen sammendrag ble generert.', 'No summary was generated.'))
    } catch (reason) {
      setFormError(translateApiError(reason, i18n.tr, { no: 'Kunne ikke lage AI-sammendrag.', en: 'Could not create an AI summary.' }))
    } finally {
      setSummarizing(false)
    }
  }

  const loadLinkPreview = async (target: string) => {
    const requestVersion = ++previewRequestVersion
    const ownerAtStart = props.browserOnly ? ctx() : undefined
    // A new link replaces any current preview: stop a running AI loop, close
    // the old browser session, and drop rationales so nothing dangles.
    browserLoop.requestStop()
    aiRunAbort?.abort()
    setAiRunRationale(null)
    closePreviewBrowserSession(preview())
    if (props.browserOnly) {
      setPreview(null)
      setNettBrowserOwner(null)
    }
    setBrowserRationales([])
    let browserSessionPromise: Promise<BrowserSessionAttempt> | null = null
    // W2-U65-51: a capture is requested only when the user ticked it (flag on).
    const capture = scrapeOptions
      ? buildCaptureRequest({ pdf: capturePdf(), screenshot: captureScreenshot() })
      : undefined
    try {
      if (props.browserOnly) {
        const browserSession = await tryCreateBrowserSession(target)
        if (requestVersion !== previewRequestVersion) {
          closeBrowserSessionById(browserSession.session?.session.id)
          return
        }
        if (!browserSession.session) {
          const refused = browserSession.errorCode === 'browser_grant_denied'
          const driverFailed = browserSession.errorCode === 'DRIVER_FAILED'
          if (refused || driverFailed) {
            const recoveryUrl = webAddress(target)
            if (recoveryUrl) setBrowserOpenRecovery({ url: recoveryUrl, kind: refused ? 'permission' : 'driver' })
          }
          if (refused) {
            const host = new URL(target).hostname
            setFormError(i18n.tr(`Nettlesertilgang til ${host} ble ikke innvilget.`, `Browser access to ${host} was not granted.`))
          } else if (driverFailed) {
            setFormError(browserSession.navigationFailure?.sentence
              ?? i18n.tr('Verevons nettleserøkt feilet mens siden ble åpnet.', 'The Verevon browser session failed while opening the page.'))
          } else {
            setFormError(browserSession.error ?? i18n.tr('Nettleserøkt kunne ikke startes.', 'Browser session could not be started.'))
          }
          return
        }
        const initialNavigationError = browserSession.session.navigationError
        let actualSession = browserSession.session
        if (actualSession.session.control?.mode !== 'human_takeover') {
          try {
            // A freshly opened Nett browser belongs to the person. Keep the
            // compare-and-set guard and render only the server's actual state.
            const outcome = await changeBrowserControl(orgId(), actualSession.session.id, 'human_takeover', actualSession.session.control?.epoch ?? null)
            actualSession = outcome.response
            if (outcome.stale && actualSession.session.control?.mode !== 'human_takeover') setFormError(i18n.tr(STALE_CONTROL_COPY.no, STALE_CONTROL_COPY.en))
          } catch (reason) {
            if (requestVersion === previewRequestVersion) setFormError(browserErrorMessage(reason, i18n.tr, { no: 'Nettleseren ble åpnet, men du fikk ikke kontroll. Prøv «Ta over».', en: 'The browser opened, but control could not be acquired. Try “Take over”.' }))
          }
        }
        if (requestVersion !== previewRequestVersion) {
          closeBrowserSessionById(actualSession.session.id)
          return
        }
        // Browsing is independent of scraping and does not save page content.
        const emptyPreview: ScrapePreview = { url: target, title: hostnameOf(target), description: '', markdown: '', blocks: [], charCount: 0, source: 'empty' }
        setPreview(attachBrowserSession(emptyPreview, actualSession))
        if (initialNavigationError?.code === 'browser_initial_navigation_failed') {
          setBrowserNavigationError(navigationHttpResponseFailure(target, initialNavigationError.httpStatus, i18n.tr, initialNavigationError.redirectCount))
        }
        const sessionId = actualSession.session.id
        if (sessionId && ownerAtStart && matchesCurrentOwner(ownerAtStart)) {
          setNettBrowserOwner({ ...ownerAtStart, sessionId })
        }
        if (sessionId) void refreshBrowserOwnerTimeline(sessionId)
        return
      }
      browserSessionPromise = tryCreateBrowserSession(target)
      const result = await scrapePreview(orgId(), {
        url: target,
        signals: {
          actions: ['render_page'],
          screenshot: capture?.screenshot === true,
          pdf: capture?.pdf === true,
          prior_block_signals: 0,
          profile_required: false,
          url_type: 'Default',
        },
        render: {
          waitForTimeoutMs: 1800,
        },
      })
      const browserSession = await browserSessionPromise
      if (requestVersion !== previewRequestVersion) {
        closeBrowserSessionById(browserSession.session?.session.id)
        return
      }
      // The preview route never takes captures, so the ticked ones come from one /scrape call.
      // A capture that fails does not fail the preview; the user sees a notice instead.
      let captureView = toScrapePreview(target, result).captures
      if (capture) {
        try {
          const captured = await scrapeUrl(orgId(), { capture, url: target })
          captureView = toScrapePreview(target, captured).captures ?? captureView
        } catch (reason) {
          setFormError(browserErrorMessage(reason, i18n.tr, { no: 'Skjermbilde eller PDF kunne ikke tas.', en: 'The screenshot or PDF could not be captured.' }))
        }
      }
      const nextPreview = {
        ...attachBrowserSession(toScrapePreview(target, result), browserSession.session),
        ...(captureView ? { captures: captureView } : {}),
        browserSessionError: browserSession.error,
      }
      if (requestVersion !== previewRequestVersion) {
        closeBrowserSessionById(browserSession.session?.session.id)
        return
      }
      setPreview(nextPreview)
      const sessionId = browserSession.session?.session.id
      if (sessionId) void refreshBrowserOwnerTimeline(sessionId)
    } catch (reason) {
      void browserSessionPromise?.then((attempt) => {
        closeBrowserSessionById(attempt.session?.session.id)
      })
      if (requestVersion === previewRequestVersion) setFormError(browserErrorMessage(reason, i18n.tr, { no: 'Skraping kunne ikke fullføres.', en: 'Scraping could not be completed.' }))
    }
  }

  const tryCreateBrowserSession = async (target: string): Promise<BrowserSessionAttempt> => {
    const id = orgId()
    if (!id) return { error: i18n.tr('Arbeidsområde mangler for nettleserøkt.', 'Workspace is missing for the browser session.'), session: null }
    const controller = new AbortController()
    const timeout = window.setTimeout(() => controller.abort(), BROWSER_SESSION_TIMEOUT_MS)
    // A ZDR session never carries a profile field, regardless of whatever the
    // picker happened to show — this mirrors (never replaces) the gateway's
    // `reject_zdr_persistent_profile` guard, which rejects the request
    // independently if this client ever got it wrong.
    const zdr = zdrRequested()
    const selectedProfile = zdr ? null : selectedBrowserProfileSummary()
    try {
      const session = await createBrowserSession(id, {
        ...(props.browserOnly ? { purpose: 'personal_browsing' } : {}),
        ...(zdr ? { zdr: true } : {}),
        ...(selectedProfile ? { profileId: selectedProfile.profile_id, scope: selectedProfile.scope } : {}),
        url: target,
        viewport: { width: 1280, height: 800 },
      }, controller.signal)
      const returnedProfile = session.session.profile
      if (returnedProfile.storage === 'persistent' && returnedProfile.id) {
        const returnedId = returnedProfile.id
        setBrowserProfiles((current) =>
          current.some((profile) => profile.profile_id === returnedId)
            ? current
            : [{ profile_id: returnedId, name: selectedProfile?.name ?? null, scope: returnedProfile.scope }, ...current],
        )
        setBrowserProfileChoice(returnedId)
      }
      return { error: null, session }
    } catch (reason) {
      const apiError = reason instanceof ApiError ? reason : null
      const driverBlock = apiError?.code === 'DRIVER_FAILED' && apiError.blockReason !== null
      const details = apiError?.code === 'DRIVER_FAILED' && !driverBlock ? apiError.details : null
      const httpResponseFailed = details?.http_status !== 407 && (
        details?.reason === 'navigation_http_response'
        || (typeof details?.error === 'string' && /\bnet::ERR_HTTP_RESPONSE_CODE_FAILURE\b/.test(details.error))
      )
      return {
        error: browserErrorMessage(reason, i18n.tr, { no: 'Nettleserøkt kunne ikke startes.', en: 'Browser session could not be started.' }),
        ...(apiError && !driverBlock ? { errorCode: apiError.code } : {}),
        ...(httpResponseFailed ? { navigationFailure: navigationHttpResponseFailure(target, details?.http_status, i18n.tr, details?.redirect_count) } : {}),
        session: null,
      }
    } finally {
      window.clearTimeout(timeout)
    }
  }

  /**
   * WS8.D: one keep-alive call for the open browser session. The chrome runs its timer only for a
   * session that advertises human_browsing, which Quarry does not project yet, so nothing calls this
   * until it does. A session that can no longer be renewed is 'expired' (the timer stops once).
   */
  const keepAliveBrowser = async (): Promise<KeepAliveOutcome> => {
    const id = orgId()
    const sessionId = preview()?.browserSession?.session.id
    if (!id || !sessionId) return 'failed'
    try {
      await keepAliveBrowserSession(id, sessionId)
      return 'alive'
    } catch (error) {
      return error instanceof ApiError && error.code === 'browser_session_expired' ? 'expired' : 'failed'
    }
  }

  const closeBrowserSessionById = (sessionId?: string | null) => {
    const id = orgId()
    if (!id || !sessionId) return
    void closeBrowserSession(id, sessionId).catch(() => undefined)
  }

  const closePreviewBrowserSession = (current: ScrapePreview | null) => {
    closeBrowserSessionById(current?.browserSession?.session.id)
  }

  /** Hydrates compact history from Quarry after a browser session is created,
   * restored, or mutated. The reducer checks the id again so a slow response
   * can never overwrite a newer session's activity. */
  const refreshBrowserOwnerTimeline = async (sessionId: string) => {
    const id = orgId()
    if (!id || !sessionId) return
    try {
      const timeline = await getBrowserOwnerTimeline(id, sessionId, { limit: 100 })
      setPreview((current) => {
        if (current?.browserSession?.session.id !== sessionId) return current
        return attachBrowserOwnerTimeline(current, timeline)
      })
    } catch (reason) {
      const message = browserErrorMessage(reason, i18n.tr, {
        no: 'Den varige nettleseraktiviteten kunne ikke lastes.',
        en: 'The durable browser activity could not be loaded.',
      })
      setPreview((current) => {
        if (current?.browserSession?.session.id !== sessionId) return current
        return withBrowserOwnerTimelineError(current, message)
      })
    }
  }

  const clearStoredBrowserPreview = () => {
    const key = browserPreviewStorageKey()
    if (key) removeClientValue(key)
  }

  const clearPreview = () => {
    cancelTabRefresh()
    previewRequestVersion += 1
    if (props.browserOnly) {
      setNettBrowserOwner(null)
      setAssistantHandoff(null)
    }
    const current = preview()
    lastAiRun = null
    browserLoop.requestStop()
    aiRunAbort?.abort()
    setAiRunRationale(null)
    closePreviewBrowserSession(current)
    if (current) {
      clearStoredBrowserPreview()
      setPendingSavedPreview(null)
    }
    setBrowserRationales([])
    setPreview(null)
  }

  const performBrowserAction = async (action: BrowserAction) => {
    const current = preview()
    const sessionId = current?.browserSession?.session.id
    const id = orgId()
    if (!current || !sessionId || !id || browserBusy() || assistantHandoff()) return
    const requestVersion = previewRequestVersion
    const stillCurrent = () => requestVersion === previewRequestVersion && orgId() === id
      && preview()?.browserSession?.session.id === sessionId
    setBrowserBusy(true)
    setFormError(null)
    try {
      const nextSession = await runBrowserAction(id, sessionId, action, { actor: 'human' })
      if (!stillCurrent()) return
      setPreview(attachBrowserSession(current, nextSession))
      scheduleTabRefresh(sessionId)
      if (action.type === 'navigate' || action.type === 'back' || action.type === 'forward' || action.type === 'reload') setBrowserNavigationError(null)
      void refreshBrowserOwnerTimeline(sessionId)
    } catch (reason) {
      if (!stillCurrent()) return
      const failure = navigationFailureFromError(action, reason, i18n.tr)
      if (failure) {
        setBrowserNavigationError(failure)
      } else {
        setFormError(browserErrorMessage(reason, i18n.tr, { no: 'Nettleserhandlingen kunne ikke fullføres.', en: 'The browser action could not be completed.' }))
      }
    } finally {
      setBrowserBusy(false)
    }
  }

  const updateBrowserControlMode = async (mode: BrowserControlMode, expectedEpoch?: number | null) => {
    const current = preview()
    const sessionId = current?.browserSession?.session.id
    const id = orgId()
    if (!current || !sessionId || !id || browserBusy() || assistantHandoff()) return
    if (mode === 'human_takeover') browserLoop.requestPause()
    setBrowserBusy(true)
    setFormError(null)
    try {
      // Compare-and-set on the epoch the person saw (WS9.I); a stale epoch refetches once.
      const outcome = await changeBrowserControl(
        id,
        sessionId,
        mode,
        expectedEpoch ?? current.browserSession?.session.control?.epoch ?? null,
      )
      setPreview(attachBrowserSession(current, outcome.response))
      if (outcome.stale) setFormError(i18n.tr(STALE_CONTROL_COPY.no, STALE_CONTROL_COPY.en))
      void refreshBrowserOwnerTimeline(sessionId)
    } catch (reason) {
      setFormError(browserErrorMessage(reason, i18n.tr, { no: 'Kunne ikke endre nettleserkontroll.', en: 'Could not change browser control.' }))
    } finally {
      setBrowserBusy(false)
    }
  }

  // "Try another source" in the blocked notice. The action stops the AI run (if
  // there was one) and closes the session on the server; only when it succeeds does the
  // preview go the way Discard sends it, back to the link field. A failure keeps the
  // preview, so Discard still works, and says why.
  const tryOtherSourceAfterBlock = async () => {
    const current = preview()
    const sessionId = current?.browserSession?.session.id
    const id = orgId()
    if (!current || !sessionId || !id || browserBusy()) return
    setBrowserBusy(true)
    setFormError(null)
    setLeaveNotice(null)
    let left: unknown
    try {
      left = await executeAction(
        'browser.blocked.try_other_source',
        { orgId: id, type: 'human', userId: ctx()?.userId ?? '' },
        lastAiRun?.sessionId === sessionId ? { sessionId, runId: lastAiRun.runId } : { sessionId },
      )
    } catch (reason) {
      setFormError(browserErrorMessage(reason, i18n.tr, { no: 'Kunne ikke forlate nettstedet.', en: 'Could not leave the site.' }))
      return
    } finally {
      setBrowserBusy(false)
    }
    clearPreview()
    if (leftWithRunControlsDisabled(left)) setLeaveNotice(runControlsDisabledNotice(i18n.tr))
  }

  const createPreviewBrowserTab = async () => {
    const current = preview()
    const sessionId = current?.browserSession?.session.id
    const id = orgId()
    if (!current || !sessionId || !id || browserBusy() || assistantHandoff()) return
    setBrowserBusy(true)
    setFormError(null)
    try {
      const nextTabs = await createBrowserTab(id, sessionId)
      setPreview(attachBrowserTabs(current, nextTabs))
      void refreshBrowserOwnerTimeline(sessionId)
    } catch (reason) {
      setFormError(browserErrorMessage(reason, i18n.tr, { no: 'Kunne ikke opprette ny nettleserfane.', en: 'Could not create a new browser tab.' }))
    } finally {
      setBrowserBusy(false)
    }
  }

  const selectPreviewBrowserTab = async (tabId: string) => {
    cancelTabRefresh()
    const current = preview()
    const sessionId = current?.browserSession?.session.id
    const id = orgId()
    if (!current || !sessionId || !id || browserBusy() || assistantHandoff()) return
    setBrowserBusy(true)
    setFormError(null)
    try {
      const nextTabs = await selectBrowserTab(id, sessionId, tabId)
      setPreview(attachBrowserTabs(current, nextTabs))
      void refreshBrowserOwnerTimeline(sessionId)
    } catch (reason) {
      setFormError(browserErrorMessage(reason, i18n.tr, { no: 'Kunne ikke bytte nettleserfane.', en: 'Could not switch browser tab.' }))
    } finally {
      setBrowserBusy(false)
    }
  }

  const closePreviewBrowserTab = async (tabId: string) => {
    cancelTabRefresh()
    const current = preview()
    const sessionId = current?.browserSession?.session.id
    const id = orgId()
    if (!current || !sessionId || !id || browserBusy() || assistantHandoff()) return
    setBrowserBusy(true)
    setFormError(null)
    try {
      const nextTabs = await closeBrowserTab(id, sessionId, tabId)
      setPreview(attachBrowserTabs(current, nextTabs))
      void refreshBrowserOwnerTimeline(sessionId)
    } catch (reason) {
      setFormError(browserErrorMessage(reason, i18n.tr, { no: 'Kunne ikke lukke nettleserfanen.', en: 'Could not close the browser tab.' }))
    } finally {
      setBrowserBusy(false)
    }
  }

  const handleBrowserSocketObservation = (observation: BrowserObservation, session?: BrowserSession) => {
    const current = preview()
    if (!current?.browserSession) return
    const next = session
      ? attachBrowserSession(current, { observation, session })
      : attachBrowserObservation(current, observation, { controlMode: 'human_takeover' })
    setPreview(next)
    scheduleTabRefresh(current.browserSession.session.id)
  }

  // A click can complete before the destination commits. Quarry streams the
  // immediate observation once, while later frames can already show the new
  // page. Read the owner's actual tab metadata again after the action settles.
  const cancelTabRefresh = () => {
    tabRefreshGeneration += 1
    for (const timer of tabRefreshTimers) window.clearTimeout(timer)
    tabRefreshTimers = []
  }
  const scheduleTabRefresh = (sessionId: string) => {
    if (!props.browserOnly || !sessionId || !orgId()) return
    cancelTabRefresh()
    const generation = tabRefreshGeneration
    const ownerOrg = orgId()
    latestAppliedTabRefresh = 0
    for (const [index, delay] of [350, 1500, 4000].entries()) {
      tabRefreshTimers.push(window.setTimeout(() => {
        void listBrowserTabs(ownerOrg, sessionId).then((response) => {
          if (generation !== tabRefreshGeneration || orgId() !== ownerOrg || index < latestAppliedTabRefresh) return
          setPreview((current) => {
            if (!current || current.browserSession?.session.id !== sessionId) return current
            latestAppliedTabRefresh = index
            return attachBrowserTabs(current, response)
          })
        }).catch(() => undefined)
      }, delay))
    }
  }

  if (props.browserOnly) {
    let settledRunId: string | null = null
    createEffect(
      () => {
        const view = assistantBrowser.view()
        const owner = nettBrowserOwner()
        return props.browserContextVisible && !browserBusy() && owner && matchesCurrentOwner(owner) && view?.usesExistingTab
          && view.borrowedTabId && view.sessionId === owner.sessionId && ['completed', 'stopped', 'failed'].includes(view.status)
          ? view.runId : null
      },
      (runId) => {
        const owner = untrack(nettBrowserOwner)
        const tabId = untrack(assistantBrowser.view)?.borrowedTabId
        if (!runId || runId === settledRunId || !owner || !tabId) return
        const requestVersion = previewRequestVersion
        const controller = new AbortController()
        let retryTimer: number | undefined
        let retry = 0
        const currentSession = () => untrack(() => {
          const current = preview()
          return requestVersion === previewRequestVersion && matchesCurrentOwner(owner)
            && current?.browserSession?.session.id === owner.sessionId ? current : null
        })
        const finish = (response: BrowserSessionResponse | null, returned: boolean, expired = false) => {
          const current = currentSession()
          if (controller.signal.aborted || !current) return
          stopPolling()
          settledRunId = runId
          if (response) setPreview(attachBrowserSession(current, response))
          setAssistantHandoff(returned ? null : 'unavailable')
          if (returned) return
          const retryUrl = webAddress(response?.session.url || current.url) ?? webAddress(url())
          if (retryUrl) setBrowserOpenRecovery({ url: retryUrl, kind: 'handoff' })
          setFormError(expired
            ? i18n.tr('Nettleserøkten er avsluttet. Åpne siden på nytt for å fortsette.', 'The browser session has ended. Reopen the page to continue.')
            : i18n.tr('Nettleseren kunne ikke bekrefte at fanen ble levert tilbake. Åpne siden på nytt.', 'The browser could not confirm that the tab was returned. Reopen the page.'))
        }
        const deadline = window.setTimeout(() => finish(null, false), 30_000)
        const stopPolling = () => {
          controller.abort()
          window.clearTimeout(deadline)
          if (retryTimer !== undefined) window.clearTimeout(retryTimer)
        }
        const poll = async () => {
          if (controller.signal.aborted || !currentSession()) return
          try {
            const response = await getBrowserSession(owner.orgId, owner.sessionId, controller.signal)
            if (response.session.status !== 'live') {
              finish(response, false, true)
              return
            }
            const holder = response.session.tabs?.find((tab) => tab.tabId === tabId)?.holder
            if (holder !== 'agent') {
              finish(response, holder === 'human')
              return
            }
          } catch (reason) {
            if (reason instanceof ApiError && (reason.code === 'browser_session_expired' || reason.code === 'browser_session_not_found')) {
              finish(null, false, true)
              return
            }
          }
          if (!controller.signal.aborted) retryTimer = window.setTimeout(() => void poll(), Math.min(250 * 2 ** retry++, 2_000))
        }
        cancelTabRefresh()
        setAssistantHandoff('waiting')
        void poll()
        return () => {
          stopPolling()
          setAssistantHandoff((state) => state === 'waiting' ? null : state)
        }
      },
    )
  }

  // The model rationale for each AI-driven step, keyed by the step number of
  // the observation the executed action produced. Rendered verbatim in the
  // timeline detail panel — the rationale is exactly what Model Plane returned.
  const recordBrowserRationale = (
    goal: string,
    suggestion: BrowserActionSuggestionResponse,
    result: BrowserSessionResponse,
  ) => {
    const step = result.observation?.step
    if (typeof step !== 'number') return
    setBrowserRationales((current) => withStepRationale(current, {
      actionType: suggestion.suggestion.action?.type ?? null,
      confidence: suggestion.suggestion.confidence ?? null,
      done: suggestion.suggestion.done ?? false,
      goal,
      modelUsed: suggestion.model_used ?? null,
      reason: suggestion.suggestion.reason ?? null,
      step,
    }))
  }

  const performBrowserSuggestedAction = async (goal: string): Promise<BrowserActionSuggestionResponse | null> => {
    const current = preview()
    const sessionId = current?.browserSession?.session.id
    const id = orgId()
    if (!current || !sessionId || !id || browserBusy()) return null
    setBrowserBusy(true)
    setFormError(null)
    try {
      const suggestion = await suggestBrowserAction(id, sessionId, {
        goal,
        includeScreenshot: Boolean(current.browserSession?.session.frame?.artifactId),
      })
      const action = suggestion.suggestion.action
      if (action && !suggestion.suggestion.done) {
        const nextSession = await runBrowserAction(id, sessionId, action, { actor: 'agent' })
        recordBrowserRationale(goal, suggestion, nextSession)
        setPreview(attachBrowserSession(current, nextSession))
        void refreshBrowserOwnerTimeline(sessionId)
      }
      return suggestion
    } catch (reason) {
      setFormError(browserErrorMessage(reason, i18n.tr, { no: 'AI-steget kunne ikke fullføres.', en: 'The AI browser step could not be completed.' }))
      return null
    } finally {
      setBrowserBusy(false)
    }
  }

  // Phase 2 (durable browser-agent run): the AI loop is a server-side run on
  // the Model Plane backbone. This replaces the old client-side suggest/act
  // `for` loop (`performBrowserAutoRun`): progress arrives as SSE run events
  // (`browser_action_dispatched` with the model's rationale,
  // `browser_observation_received`, `browser_run_paused`/`_resumed`), and
  // pause/resume/stop go through `controlBrowserAiRun` via
  // `browserLoop.attachRun`. The visible tab stays interactive: the run
  // executes in its own Quarry session, sharing cookies only through the
  // session's profile (server-derived, see gateway `start_ai_run`).
  const beginBrowserAiRun = async (goal: string): Promise<BrowserActionSuggestionResponse | null> => {
    const current = preview()
    const sessionId = current?.browserSession?.session.id
    const id = orgId()
    if (!current || !sessionId || !id) return null
    if (!browserLoop.begin(goal)) return null
    setFormError(null)
    setAiRunRationale(null)
    setBrowserApprovals([])
    setDecidingApprovalKeys([])
    aiRunAbort?.abort()
    const controller = new AbortController()
    aiRunAbort = controller
    let dispatchCount = 0
    try {
      const startParams = { goal, maxSteps: BROWSER_AI_LOOP_MAX_STEPS }
      let run: Awaited<ReturnType<typeof startBrowserAiRun>>
      try {
        run = await startBrowserAiRun(id, sessionId, startParams, controller.signal, {
          idempotencyKey: aiRunStartKeys.keyFor(sessionId, startParams),
        })
        aiRunStartKeys.succeeded()
      } catch (startFailure) {
        aiRunStartKeys.failed(startFailure)
        throw startFailure
      }
      browserLoop.attachRun({ orgId: id, runId: run.runId })
      lastAiRun = { sessionId, runId: run.runId }
      await streamRunEvents(run.runId, {
        onBrowserAction: (event) => {
          dispatchCount += 1
          browserLoop.onActionDispatched()
          setAiRunRationale({
            actionType: event.actionType ?? null,
            confidence: null,
            done: false,
            goal,
            modelUsed: null,
            reason: event.reason?.trim() ? event.reason : null,
            step: dispatchCount,
          })
        },
        onBrowserObservation: () => {
          browserLoop.onObservationReceived()
        },
        onBrowserRunPaused: () => browserLoop.onRunPaused(),
        onBrowserRunResumed: () => browserLoop.onRunResumed(),
        // Phase 5 HITL gate: a risky browser action (or the run-level
        // persistent-cookie-use check) is blocking on a human decision.
        // Distinct from onBrowserRunPaused above, which is the user-initiated
        // pause/resume from Phase 2 — this is never user-initiated.
        onBrowserActionApprovalRequired: (event) => {
          setBrowserApprovals((entries) => withBrowserApprovalRequested(entries, event))
          browserLoop.onApprovalRequired()
        },
        onBrowserActionDecided: (event) => {
          setBrowserApprovals((entries) => withBrowserApprovalDecided(entries, event))
          browserLoop.onApprovalDecided(event.decision === 'granted' ? 'granted' : event.decision === 'timed_out' ? 'timed_out' : 'denied')
        },
        onError: (streamError) => {
          // A deliberate abort (new link, preview cleared, unmount) is not an
          // error the user should see.
          if (controller.signal.aborted) {
            browserLoop.finish('stopped')
            return
          }
          const message = browserErrorMessage(streamError, i18n.tr, {
            no: 'AI-loopen mistet forbindelsen til kjøringen.',
            en: 'The AI browser loop lost its run connection.',
          })
          browserLoop.finish('stopped', message)
          setFormError(message)
        },
        onDone: () => {
          browserLoop.finish('done')
        },
      }, controller.signal)
      return null
    } catch (reason) {
      if (controller.signal.aborted) {
        browserLoop.finish('stopped')
        return null
      }
      const message = browserErrorMessage(reason, i18n.tr, { no: 'AI-loopen kunne ikke startes.', en: 'The AI browser loop could not be started.' })
      browserLoop.finish('stopped', message)
      setFormError(message)
      return null
    } finally {
      if (aiRunAbort === controller) aiRunAbort = null
    }
  }

  // Phase 5 HITL gate: approve or reject a pending browser-action approval,
  // reusing the SAME generic `/api/v1/orchestration/approvals/:id/decide`
  // route AgentRunConsole's approval deck calls — there is no browser-specific
  // decide endpoint. `entryKey` (not the durable approval id) identifies the
  // card: execution-core publishes the `browser_action_approval_required`
  // event with an empty `approval_id` (the durable `CreateApproval` call
  // happens right after, so the id genuinely doesn't exist yet when the
  // event that announces the gate goes out) — so the id is frequently still
  // unknown when the user acts. When that happens, fall back to the generic
  // `listApprovals(runId)` to resolve it: the in-loop browser gate only ever
  // has at most one pending approval per run, so "the pending one" is
  // unambiguous. Unlike AgentRunConsole's outer tool-call gate, a browser
  // in-loop grant resumes the SAME action by itself (execution-core polls the
  // approval and continues the loop in place) — no separate resume/cancel
  // call is needed or correct here.
  const handleDecideBrowserApproval = async (entryKey: string, decision: ApprovalDecision) => {
    const entry = browserApprovals().find((candidate) => candidate.key === entryKey)
    if (!entry) return
    setDecidingApprovalKeys((keys) => (keys.includes(entryKey) ? keys : [...keys, entryKey]))
    try {
      let approvalId = entry.approvalId
      if (!approvalId) {
        if (!entry.runId) throw new LocalizedError({ no: 'Fant ikke kjøringen denne handlingen tilhører.', en: 'Could not find the run this action belongs to.' })
        const pending = await listApprovals(entry.runId)
        const match = pending.find((candidate) => (candidate.status ?? '').toUpperCase() === 'PENDING')
        if (!match) {
          setFormError(i18n.tr('Fant ingen ventende godkjenning for denne handlingen ennå — prøv igjen om et øyeblikk.', 'No pending approval was found for this action yet — try again in a moment.'))
          return
        }
        approvalId = match.id
      }
      await decideApproval(approvalId, decision)
      // Optimistic, in case the run's own SSE stream is slow or has already
      // ended — `withBrowserApprovalDecided` is idempotent, so the real
      // `browser_action_decided` event (when it arrives) simply confirms this.
      const resolved = decision === 'approve' ? 'granted' : 'denied'
      setBrowserApprovals((entries) => withBrowserApprovalDecided(entries, {
        runId: entry.runId,
        actionId: entry.actionId,
        approvalId,
        decision: resolved,
      }))
      browserLoop.onApprovalDecided(resolved)
    } catch (reason) {
      const message = browserErrorMessage(reason, i18n.tr, { no: 'Kunne ikke registrere beslutningen.', en: 'Could not record the decision.' })
      setFormError(message)
    } finally {
      setDecidingApprovalKeys((keys) => keys.filter((key) => key !== entryKey))
    }
  }

  // Link mode scrapes first (preview, no ingest yet); crawl mode kicks off an
  // async site crawl. "Add to knowledge base" below the preview does the ingest.
  const submitUrl = async (event: SubmitEvent) => {
    event.preventDefault()
    if (!canSubmitUrl()) return
    const target = normalizeUrl(url())
    if (!target) {
      setFormError(i18n.tr('Skriv inn en gyldig nettadresse, f.eks. nettsted.example eller https://nettsted.example.', 'Enter a valid web address, for example nettsted.example or https://nettsted.example.'))
      return
    }
    setFormError(null)
    setLeaveNotice(null)
    setSubmitting(true)

    try {
      if (mode() === 'crawl') {
        await startCrawlJob(target)
      } else if (mode() === 'products') {
        await extractProductsFromUrl(target)
      } else {
        await loadLinkPreview(target)
      }
    } finally {
      setSubmitting(false)
      stopPollingIfIdle()
    }
  }

  const addToKnowledge = async (selectedMarkdown: string, allSelected: boolean) => {
    const current = preview()
    if (!current || adding() || !ready()) return
    setAdding(true)
    setFormError(null)
    const key = addJob({
      detail: allSelected
        ? i18n.tr('Indekserer hele siden ...', 'Indexing the full page ...')
        : i18n.tr('Indekserer valgte seksjoner ...', 'Indexing selected sections ...'),
      kind: 'link',
      label: current.title || current.url,
      status: 'pending',
    })
    try {
      // Ingest the already-scraped markdown straight into Data Plane v2 via the
      // gateway (documents-api). Whole-page uses the complete markdown; a curated
      // subset uses only the chosen sections. We deliberately do NOT round-trip
      // through quarry's scrape+ingest here — its Data-Plane ingest path is
      // unreliable (500s) and we already hold the content from the preview scrape.
      const content = allSelected ? current.markdown || selectedMarkdown : selectedMarkdown
      await createDocument(orgId(), {
        title: current.title,
        content,
        sourceUrl: current.url,
      })
      updateJob(key, { status: 'completed' })
      browserLoop.requestStop()
      lastAiRun = null
      closePreviewBrowserSession(current)
      clearStoredBrowserPreview()
      setBrowserRationales([])
      setPreview(null)
      setUrl('')
    } catch (reason) {
      const message = translateApiError(reason, i18n.tr, { no: 'Kunne ikke legge til i kunnskapsbasen.', en: 'Could not add to the knowledge base.' })
      updateJob(key, { status: 'failed', error: message })
      setFormError(message)
    } finally {
      setAdding(false)
      stopPollingIfIdle()
    }
  }

  const uploadFiles = async (files: FileList | null) => {
    const list = files ? Array.from(files) : []
    if (list.length === 0 || !ready()) return
    setFormError(null)
    const key = addJob({
      detail: i18n.tr(
        `${list.length} fil${list.length === 1 ? '' : 'er'} lastes opp ...`,
        `${list.length} file${list.length === 1 ? '' : 's'} uploading ...`,
      ),
      kind: 'upload',
      label: list.map((file) => file.name).join(', '),
      status: 'pending',
    })
    try {
      const job = await importUpload(orgId(), list)
      updateJob(key, { id: job.id, status: job.status || 'pending' })
    } catch (reason) {
      const message = translateApiError(reason, i18n.tr, { no: 'Opplasting feilet.', en: 'Upload failed.' })
      updateJob(key, { status: 'failed', error: message })
      setFormError(message)
    } finally {
      stopPollingIfIdle()
    }
  }

  // Phase 3 continuation: everything the live browser chrome's profile
  // popover needs for full CRUD, keyed off the same state the pre-session
  // picker above already uses — `current.browserSession?.session.zdr` is the
  // one thing that differs per-preview (the *live session's* ZDR flag, not
  // the pre-session checkbox, which is meaningless once a session exists).
  const browserProfileManagerProps = (current: ScrapePreview): BrowserProfileManagerProps => ({
    creating: browserProfileCreating(),
    deleteArmed: browserProfileDeleteArmed(),
    error: browserProfileError(),
    loading: browserProfilesLoading(),
    mutating: browserProfileMutating(),
    newProfileName: newProfileName(),
    newProfileScope: newProfileScope(),
    onArmDelete: armDeleteSelectedBrowserProfile,
    onCancelDelete: cancelDeleteSelectedBrowserProfile,
    onCancelRename: cancelRenamingBrowserProfile,
    onConfirmDelete: () => void confirmDeleteSelectedBrowserProfile(),
    onConfirmRename: () => void confirmRenameSelectedBrowserProfile(),
    onCreateProfile: () => void createNamedBrowserProfile(),
    onNewProfileNameChange: setNewProfileName,
    onNewProfileScopeChange: setNewProfileScope,
    onProbeProfile: () => void probeSelectedBrowserProfile(),
    onRefreshProfiles: () => void refreshBrowserProfiles(),
    onRenameProfileNameChange: setRenameProfileName,
    onSelectProfile: selectBrowserProfile,
    onStartRename: startRenamingSelectedBrowserProfile,
    profiles: browserProfiles(),
    renameProfileName: renameProfileName(),
    renaming: renamingProfile(),
    selectedProfileId: selectedBrowserProfileId(),
    zdrActive: Boolean(current.browserSession?.session.zdr),
  })

  onCleanup(() => {
    cancelTabRefresh()
    previewRequestVersion += 1
    if (props.browserOnly) closePreviewBrowserSession(preview())
    if (pollTimer !== undefined) window.clearInterval(pollTimer)
    for (const controller of crawlStreams.values()) controller.abort()
  })

  // Nett provides an explicit URL; wait for the workspace identity before
  // using the same browser/session path as the existing page preview.
  const openBrowserUrl = (target: string) => {
    setFormError(null)
    setBrowserOpenRecovery(null)
    setBrowserNavigationError(null)
    setAssistantHandoff(null)
    setUrl(target)
    setBrowserOpening(true)
    const attempt = loadLinkPreview(target)
    const version = previewRequestVersion
    void attempt.finally(() => {
      if (version === previewRequestVersion) setBrowserOpening(false)
    })
  }
  let lastBrowserUrl = ''
  createEffect(() => ({ input: props.browserOnly ? props.browserUrl : null, workspaceReady: ready() }), ({ input, workspaceReady }) => {
    if (!input || !workspaceReady || input === lastBrowserUrl) return
    const target = normalizeUrl(input)
    if (!target) {
      setFormError(i18n.tr('Skriv inn en gyldig nettadresse.', 'Enter a valid web address.'))
      return
    }
    lastBrowserUrl = input
    untrack(() => openBrowserUrl(target))
  })

  return (
    <div class="verevon-panel-in verevon-dashboard-composer-card dashboard-knowledge-composer">
      <form class="dashboard-knowledge-composer__url" onSubmit={submitUrl}>
        <Show when={!props.browserOnly}><div
          class={['dashboard-knowledge-composer__mode-row', { 'dashboard-knowledge-composer__mode-row--inline': Boolean(props.inlinePlanLabel || props.inlineTitle) }]}
        >
          <Show when={props.inlinePlanLabel}>
            {(planLabel) => (
              <span class="dashboard-knowledge-composer__inline-plan">
                <DashboardPlanBadge planLabel={planLabel()} />
              </span>
            )}
          </Show>
          <div class="dashboard-knowledge-composer__modes" role="group" aria-label={i18n.tr('Innhentingsmodus', 'Ingestion mode')}>
            <button
              type="button"
              class={{ 'dashboard-knowledge-composer__mode--active': mode() === 'crawl' }}
              onClick={() => switchMode('crawl')}
              aria-pressed={mode() === 'crawl' ? 'true' : 'false'}
            >
              <Globe2 class="size-3.5" /> {i18n.tr('Crawl', 'Crawl')}
            </button>
            <button
              type="button"
              class={{ 'dashboard-knowledge-composer__mode--active': mode() === 'link' || mode() === 'products' }}
              onClick={() => switchMode('link')}
              aria-pressed={mode() === 'link' || mode() === 'products' ? 'true' : 'false'}
            >
              <Link2 class="size-3.5" /> {i18n.tr('Extract', 'Extract')}
            </button>
            <button
              type="button"
              class={{ 'dashboard-knowledge-composer__mode--active': mode() === 'monitor' }}
              onClick={() => switchMode('monitor')}
              aria-pressed={mode() === 'monitor' ? 'true' : 'false'}
            >
              <ListChecks class="size-3.5" /> {i18n.tr('Kontroll', 'Monitor')}
            </button>
          </div>
          <Show when={props.inlineTitle}>
            {(title) => <strong class="dashboard-knowledge-composer__inline-title">{title()}</strong>}
          </Show>
        </div></Show>

        <Show when={!props.browserOnly && mode() !== 'monitor' && (props.previewCollapsed || !preview())}>
          <Show when={mode() === 'link' && pendingSavedPreview() && (pendingSavedPreview()!.preview.markdown.trim() || pendingSavedPreview()!.preview.blocks.length)}>
            <button type="button" onClick={() => {
              const saved = pendingSavedPreview()
              if (!saved) return
              setPendingSavedPreview(null)
              setUrl(saved.preview.url)
              // The former browser session may have expired; restore content only.
              setPreview({ ...saved.preview, browserSession: undefined })
              setBrowserRationales(saved.browserRationales)
            }}>{i18n.tr('Fortsett forrige uthenting', 'Continue previous extraction')}</button>
          </Show>
          <Show when={mode() === 'link' || mode() === 'products'}>
            <div class="dashboard-knowledge-composer__modes" role="group" aria-label={i18n.tr('Hva vil du hente ut?', 'What would you like to extract?')}>
              <button type="button" aria-pressed={mode() === 'link' ? 'true' : 'false'} class={{ 'dashboard-knowledge-composer__mode--active': mode() === 'link' }} onClick={() => switchMode('link')}>{i18n.tr('Nettside', 'Web page')}</button>
              <button type="button" aria-pressed={mode() === 'products' ? 'true' : 'false'} class={{ 'dashboard-knowledge-composer__mode--active': mode() === 'products' }} onClick={() => switchMode('products')}><ShoppingBag class="size-3.5" /> {i18n.tr('Produkter', 'Products')}</button>
            </div>
          </Show>
          <div class="verevon-glass-input dashboard-knowledge-composer__input-wrap">
            <Link2 class="size-4 shrink-0 text-[#9A9188]" aria-hidden="true" />
            <input
              value={url()}
              onInput={(event) => setUrl(event.currentTarget.value)}
              class="dashboard-knowledge-composer__input"
              placeholder={knowledgeUrlPlaceholder(mode(), i18n)}
              inputmode="url"
              autocomplete="off"
              disabled={!ready()}
            />
            <button
              type="submit"
              class="dashboard-knowledge-composer__submit"
              disabled={!canSubmitUrl()}
              aria-label={knowledgeSubmitLabel(mode(), i18n)}
              title={knowledgeSubmitLabel(mode(), i18n)}
            >
              <Show when={!submitting()} fallback={<Loader2 class="size-4 dashboard-xsearch-spin" />}>
                <ArrowRight class="size-4" />
              </Show>
            </button>
          </div>

          <Show when={scrapeOptions && mode() === 'link'}>
            <div class="dashboard-knowledge-composer__crawl-options" data-testid="scrape-capture-options">
              <label class="dashboard-knowledge-composer__agent-toggle">
                <input
                  type="checkbox"
                  checked={captureScreenshot()}
                  onChange={(event) => setCaptureScreenshot(event.currentTarget.checked)}
                />
                <span>{i18n.tr('Ta skjermbilde', 'Capture a screenshot')}</span>
              </label>
              <label class="dashboard-knowledge-composer__agent-toggle">
                <input
                  type="checkbox"
                  checked={capturePdf()}
                  onChange={(event) => setCapturePdf(event.currentTarget.checked)}
                />
                <span>{i18n.tr('Lag PDF', 'Capture a PDF')}</span>
              </label>
            </div>
          </Show>

          <Show when={mode() === 'link'}>
            <div class="dashboard-knowledge-composer__browser-options">
              <label class="dashboard-knowledge-composer__browser-profile">
                <KeyRound class="size-3.5" aria-hidden="true" />
                <select
                  value={browserProfileChoice()}
                  onChange={(event) => {
                    setBrowserProfileChoice(event.currentTarget.value)
                    setBrowserProfileProbe(null)
                    setBrowserProfileError(null)
                    cancelRenamingBrowserProfile()
                    setBrowserProfileDeleteArmed(false)
                  }}
                  disabled={!ready() || zdrRequested()}
                  aria-label={i18n.tr('Nettleserprofil', 'Browser profile')}
                >
                  <option value={isolatedProfileChoice}>{i18n.tr('Isolert', 'Isolated')}</option>
                  <option value={newProfileChoice}>{i18n.tr('Ny profil', 'New profile')}</option>
                  <For each={browserProfiles()}>
                    {(profile) => <option value={profile.profile_id}>{profileOptionLabel(profile)}</option>}
                  </For>
                </select>
              </label>
              <button
                type="button"
                class="dashboard-knowledge-composer__browser-tool"
                onClick={() => void refreshBrowserProfiles()}
                disabled={!ready() || browserProfilesLoading()}
                aria-label={i18n.tr('Oppdater nettleserprofiler', 'Refresh browser profiles')}
                title={i18n.tr('Oppdater', 'Refresh')}
              >
                <Show when={!browserProfilesLoading()} fallback={<Loader2 class="size-3.5 dashboard-xsearch-spin" />}>
                  <RefreshCw class="size-3.5" />
                </Show>
              </button>
              <button
                type="button"
                class="dashboard-knowledge-composer__browser-tool"
                onClick={() => void probeSelectedBrowserProfile()}
                disabled={!ready() || !selectedBrowserProfileId() || browserProfileProbing() || !normalizeUrl(url())}
                aria-label={i18n.tr('Sjekk nettleserprofil', 'Check browser profile')}
                title={i18n.tr('Sjekk profil', 'Check profile')}
              >
                <Show when={!browserProfileProbing()} fallback={<Loader2 class="size-3.5 dashboard-xsearch-spin" />}>
                  <ShieldCheck class="size-3.5" />
                </Show>
              </button>
              <label class="dashboard-knowledge-composer__browser-zdr">
                <input
                  type="checkbox"
                  checked={zdrRequested()}
                  onChange={(event) => setZdrRequested(event.currentTarget.checked)}
                  disabled={!ready()}
                />
                <span>{i18n.tr('ZDR — ingen lagring', 'ZDR — no persistence')}</span>
              </label>
              <Show when={browserProfileProbe()}>
                {(probe) => (
                  <span class="dashboard-knowledge-composer__browser-profile-status">
                    <ShieldCheck class="size-3.5" />
                    {profileProbeSummary(probe(), i18n)}
                  </span>
                )}
              </Show>
              <Show when={browserProfileError()}>
                {(message) => (
                  <span class="dashboard-knowledge-composer__browser-profile-error">
                    <AlertCircle class="size-3.5" />
                    {message()}
                  </span>
                )}
              </Show>

              <Show when={browserProfileChoice() === newProfileChoice && !zdrRequested()}>
                <div class="dashboard-knowledge-composer__browser-profile-create">
                  <input
                    value={newProfileName()}
                    onInput={(event) => setNewProfileName(event.currentTarget.value)}
                    placeholder={i18n.tr('Profilnavn (valgfritt)', 'Profile name (optional)')}
                    aria-label={i18n.tr('Navn på ny profil', 'New profile name')}
                    disabled={!ready() || browserProfileCreating()}
                  />
                  <select
                    value={newProfileScope()}
                    onChange={(event) => setNewProfileScope(event.currentTarget.value as CreatableBrowserProfileScope)}
                    aria-label={i18n.tr('Omfang for ny profil', 'New profile scope')}
                    disabled={!ready() || browserProfileCreating()}
                  >
                    <option value="user_private">{PROFILE_SCOPE_LABELS.user_private}</option>
                    <option value="org_shared">{PROFILE_SCOPE_LABELS.org_shared}</option>
                    <option value="run_scoped">{PROFILE_SCOPE_LABELS.run_scoped}</option>
                  </select>
                  <button
                    type="button"
                    class="dashboard-knowledge-composer__browser-profile-create-submit"
                    onClick={() => void createNamedBrowserProfile()}
                    disabled={!ready() || browserProfileCreating()}
                  >
                    <Show when={!browserProfileCreating()} fallback={<Loader2 class="size-3.5 dashboard-xsearch-spin" />}>
                      <Plus class="size-3.5" />
                    </Show>
                    {i18n.tr('Opprett', 'Create')}
                  </button>
                </div>
              </Show>

              <Show when={selectedBrowserProfileId()}>
                <div class="dashboard-knowledge-composer__browser-profile-manage">
                  <Show
                    when={renamingProfile()}
                    fallback={
                      <button
                        type="button"
                        class="dashboard-knowledge-composer__browser-tool"
                        onClick={startRenamingSelectedBrowserProfile}
                        disabled={browserProfileMutating()}
                        aria-label={i18n.tr('Gi profilen nytt navn', 'Rename the profile')}
                        title={i18n.tr('Nytt navn', 'Rename')}
                      >
                        <Pencil class="size-3.5" />
                      </button>
                    }
                  >
                    <input
                      value={renameProfileName()}
                      onInput={(event) => setRenameProfileName(event.currentTarget.value)}
                      placeholder={i18n.tr('Profilnavn', 'Profile name')}
                      aria-label={i18n.tr('Nytt profilnavn', 'New profile name')}
                    />
                    <button
                      type="button"
                      onClick={() => void confirmRenameSelectedBrowserProfile()}
                      disabled={browserProfileMutating() || renameProfileName().trim().length === 0}
                    >
                      {i18n.tr('Lagre', 'Save')}
                    </button>
                    <button type="button" onClick={cancelRenamingBrowserProfile}>{i18n.tr('Avbryt', 'Cancel')}</button>
                  </Show>
                  <Show
                    when={browserProfileDeleteArmed()}
                    fallback={
                      <button
                        type="button"
                        class="dashboard-knowledge-composer__browser-profile-delete"
                        onClick={armDeleteSelectedBrowserProfile}
                        disabled={browserProfileMutating()}
                        aria-label={i18n.tr('Slett profilen', 'Delete the profile')}
                        title={i18n.tr('Slett', 'Delete')}
                      >
                        <Trash2 class="size-3.5" />
                      </button>
                    }
                  >
                    <span>{i18n.tr('Slette permanent?', 'Delete permanently?')}</span>
                    <button
                      type="button"
                      class="dashboard-knowledge-composer__browser-profile-delete-confirm"
                      onClick={() => void confirmDeleteSelectedBrowserProfile()}
                      disabled={browserProfileMutating()}
                    >
                      {i18n.tr('Bekreft', 'Confirm')}
                    </button>
                    <button type="button" onClick={cancelDeleteSelectedBrowserProfile}>{i18n.tr('Avbryt', 'Cancel')}</button>
                  </Show>
                </div>
              </Show>
            </div>
          </Show>

          <Show when={mode() === 'crawl'}>
            <div class="dashboard-knowledge-composer__crawl-options">
              <label>
                <span>{i18n.tr('Sider', 'Pages')}</span>
                <input
                  type="number"
                  min="1"
                  max="5000"
                  value={crawlMaxPages()}
                  onInput={(event) => setCrawlMaxPages(clampCrawlPages(event.currentTarget.valueAsNumber))}
                />
              </label>
              <label class="dashboard-knowledge-composer__agent-toggle">
                <input
                  type="checkbox"
                  checked={agentMode()}
                  onChange={(event) => setAgentMode(event.currentTarget.checked)}
                />
                <span>{i18n.tr('Agentstyrt workflow', 'Agent-run workflow')}</span>
              </label>
              <Show when={scrapeOptions}>
                <label>
                  <span>{i18n.tr('Dybde', 'Depth')}</span>
                  <input
                    type="number"
                    min="1"
                    max={CRAWL_MAX_DEPTH}
                    placeholder="-"
                    value={crawlMaxDepth() ?? ''}
                    onInput={(event) => {
                      const value = event.currentTarget.valueAsNumber
                      setCrawlMaxDepth(Number.isFinite(value) ? Math.min(CRAWL_MAX_DEPTH, Math.max(1, Math.round(value))) : undefined)
                    }}
                  />
                </label>
                <label class="dashboard-knowledge-composer__agent-toggle">
                  <input
                    type="checkbox"
                    checked={crawlAllowSubdomains()}
                    onChange={(event) => setCrawlAllowSubdomains(event.currentTarget.checked)}
                  />
                  <span>{i18n.tr('Inkluder underdomener', 'Include subdomains')}</span>
                </label>
                <label>
                  <span>{i18n.tr('Ta med stier', 'Include paths')}</span>
                  <input
                    type="text"
                    value={crawlIncludeText()}
                    placeholder="/docs/**, /blog/**"
                    aria-label={i18n.tr('Stier som skal tas med', 'Paths to include')}
                    onInput={(event) => setCrawlIncludeText(event.currentTarget.value)}
                  />
                </label>
                <label>
                  <span>{i18n.tr('Utelat stier', 'Exclude paths')}</span>
                  <input
                    type="text"
                    value={crawlExcludeText()}
                    placeholder="/admin/**, re:/tag/.*"
                    aria-label={i18n.tr('Stier som skal utelates', 'Paths to exclude')}
                    onInput={(event) => setCrawlExcludeText(event.currentTarget.value)}
                  />
                </label>
              </Show>
              <button
                type="button"
                class="dashboard-knowledge-composer__discover"
                onClick={() => void discoverPages()}
                disabled={!canDiscover()}
                title={i18n.tr('Oppdag sidene på nettstedet og velg hvilke som skal crawles', 'Discover pages on the site and choose which ones to crawl')}
              >
                <Show when={!discovering()} fallback={<Loader2 class="size-3.5 dashboard-xsearch-spin" />}>
                  <ListChecks class="size-3.5" />
                </Show>
                {i18n.tr('Velg sider', 'Choose pages')}
              </button>
            </div>
          </Show>
        </Show>
      </form>

      <Show when={!props.browserOnly && mode() === 'monitor'}><KnowledgeMonitoring orgId={orgId()} /></Show>

      <Show when={props.previewCollapsed ? null : preview()} keyed>
        {(current) => (
          <ScrapePreviewPanel
            browsing={props.browserOnly}
            adding={adding()}
            browserApprovals={browserApprovals()}
            browserBusy={browserBusy() || Boolean(assistantHandoff())}
            browserLoop={browserLoopState()}
            browserLoopRationale={aiRunRationale()}
            browserRationales={browserRationales()}
            decidingBrowserApprovalKeys={decidingApprovalKeys()}
            onBrowserAction={(action) => void performBrowserAction(action)}
            browserNavigationError={browserNavigationError()}
            onBrowserNavigationDismiss={() => setBrowserNavigationError(null)}
            onBrowserNavigationRetry={(url) => void performBrowserAction({ type: 'navigate', url })}
            onBrowserAutoRun={current.browserSession?.session.capabilities.includes('human_browsing') ? undefined : beginBrowserAiRun}
            onBrowserControlMode={(mode, expectedEpoch) => void updateBrowserControlMode(mode, expectedEpoch)}
            onBrowserLoopPause={() => browserLoop.requestPause()}
            onBrowserLoopResume={() => browserLoop.requestResume()}
            onBrowserLoopStop={() => browserLoop.requestStop()}
            onDecideBrowserApproval={(key, decision) => void handleDecideBrowserApproval(key, decision)}
            onBrowserNewTab={createPreviewBrowserTab}
            onKeepAlive={keepAliveBrowser}
            onBrowserSelectTab={selectPreviewBrowserTab}
            onBrowserCloseTab={closePreviewBrowserTab}
            onBrowserSuggestAction={current.browserSession?.session.capabilities.includes('human_browsing') ? undefined : performBrowserSuggestedAction}
            onBrowserSocketObservation={handleBrowserSocketObservation}
            onBrowserTryOtherSource={() => void tryOtherSourceAfterBlock()}
            onAdd={(markdown, allSelected) => void addToKnowledge(markdown, allSelected)}
            onDiscard={clearPreview}
            preview={current}
            profileManager={browserProfileManagerProps(current)}
            profileProbe={browserProfileProbe()}
          />
        )}
      </Show>

      <Show when={props.previewCollapsed ? null : discovery()} keyed>
        {(current) => (
          <CrawlPagePicker
            discovery={current}
            submitting={submitting()}
            onCrawl={(urls) => void crawlSelected(urls)}
            onDiscard={() => setDiscovery(null)}
          />
        )}
      </Show>

      <Show when={props.previewCollapsed ? null : crawlApproval()} keyed>
        {(pending) => (
          <CrawlApprovalPrompt
            request={pending.request}
            estimate={pending.estimate}
            onDecided={(decision) => {
              // Approve starts the crawl the approval was asked for; reject drops it. Either way the
              // prompt goes away.
              setCrawlApproval(null)
              if (decision.decision === 'approve') void crawlSelected(pending.urls)
            }}
          />
        )}
      </Show>

      <Show when={props.previewCollapsed ? null : productExtraction()} keyed>
        {(current) => (
          <ProductPicker
            extraction={current}
            summarizing={summarizing()}
            summary={productSummary()}
            onSummarize={(selected, focus) => void summarizeSelection(selected, focus)}
            onDiscard={() => { setProductExtraction(null); setProductSummary(null) }}
          />
        )}
      </Show>

      <Show when={!props.browserOnly && mode() !== 'monitor'}><div class="dashboard-knowledge-composer__actions">
        <button
          type="button"
          class="dashboard-knowledge-composer__upload"
          onClick={openFileDialog}
          disabled={!ready()}
        >
          <FileUp class="size-4" />
          {i18n.tr('Last opp filer', 'Upload files')}
        </button>
        <input
          ref={(element) => { fileInputRef = element }}
          type="file"
          multiple
          class="sr-only"
          aria-label={i18n.tr('Last opp dokumenter', 'Upload documents')}
          onChange={(event) => {
            void uploadFiles(event.currentTarget.files)
            event.currentTarget.value = ''
          }}
        />
        <a href="/knowledge" link class="dashboard-knowledge-composer__link">
          {i18n.tr('Åpne kunnskapsbase', 'Open knowledge base')}
          <ArrowRight class="size-3.5" />
        </a>
      </div></Show>

      <Show when={!ready() && ctx.loading}>
        <p class="dashboard-knowledge-composer__status">{i18n.tr('Kobler til arbeidsområdet ...', 'Connecting to the workspace ...')}</p>
      </Show>
      <Show when={formError()}>
        {(message) => <p class="dashboard-knowledge-composer__error" role={props.browserOnly ? 'alert' : undefined}>{message()}</p>}
      </Show>
      <Show when={props.browserOnly && browserOpening()}>
        <p class="dashboard-knowledge-composer__status" role="status">{i18n.tr('Åpner nettleseren ...', 'Opening the browser ...')}</p>
      </Show>
      <Show when={assistantHandoff() === 'waiting'}>
        <p class="dashboard-knowledge-composer__status" role="status">{i18n.tr('Venter på at Verevon leverer tilbake fanen …', 'Waiting for Verevon to return the tab…')}</p>
      </Show>
      <Show when={props.browserOnly && browserOpenRecovery()}>{(recovery) => <>
        <Show when={recovery().kind === 'permission' && import.meta.env.VITE_BROWSER_SITES === 'true'}>
          <p class="dashboard-knowledge-composer__status">{i18n.tr('Se nettsidevalget for dette domenet. Hvis et nettsidevalg kreves, kan du velge «Lesing og handling» der arbeidsområdet tillater det.', 'Review the site choice for this domain. If a site choice is required, you can select “Reading and acting” when your workspace allows it.')}</p>
        </Show>
        <div class="flex flex-wrap items-center gap-x-4 gap-y-2">
          <Show when={recovery().kind === 'permission' && import.meta.env.VITE_BROWSER_SITES === 'true'}>
            <a class="dashboard-knowledge-composer__link" href={`/settings/browser-sites?host=${encodeURIComponent(new URL(recovery().url).hostname)}`} target="_blank" rel="noopener noreferrer">{i18n.tr('Se nettsidevalg', 'Review site choice')}</a>
          </Show>
          <button class="dashboard-knowledge-composer__link" type="button" disabled={!ready() || browserOpening()} onClick={() => openBrowserUrl(recovery().url)}>
            {recovery().kind === 'permission' ? i18n.tr('Prøv igjen', 'Try again') : i18n.tr('Start ny nettleserøkt', 'Start a new browser session')}
          </button>
          <Show when={recovery().kind !== 'handoff'}>
            <a class="dashboard-knowledge-composer__link" href={recovery().url} target="_blank" rel="noopener noreferrer">{i18n.tr('Åpne utenfor Verevon', 'Open outside Verevon')}</a>
          </Show>
        </div>
      </>}</Show>
      <Show when={leaveNotice()}>
        {(message) => <p class="dashboard-knowledge-composer__status" role="status">{message()}</p>}
      </Show>

      <Show when={!props.browserOnly && mode() !== 'monitor' && jobs().length > 0}>
        <ul class="dashboard-knowledge-composer__jobs">
          <For each={jobs()}>
            {(job) => <IngestJobRow job={job} />}
          </For>
        </ul>
      </Show>
    </div>
  )
}

function IngestJobRow(props: { job: IngestJob }) {
  const i18n = useI18n()
  const running = () => !isTerminal(props.job.status)
  const failed = () => /fail|error|cancel/.test(props.job.status.toLowerCase())
  const kindLabel = () => {
    if (props.job.kind === 'crawl') return props.job.agentMode ? i18n.tr('Agent-crawl', 'Agent crawl') : i18n.tr('Crawl', 'Crawl')
    return props.job.kind === 'upload' ? i18n.tr('Opplasting', 'Upload') : i18n.tr('Lenke', 'Link')
  }
  const progress = () => normalizeProgress(props.job.progress)

  return (
    <li class="dashboard-knowledge-composer__job">
      <span class={cn('dashboard-knowledge-composer__job-icon', failed() && 'dashboard-knowledge-composer__job-icon--error')}>
        <Switch fallback={<CheckCircle2 class="size-4" />}>
          <Match when={running()}>
            <Loader2 class="size-4 dashboard-xsearch-spin" />
          </Match>
          <Match when={failed()}>
            <AlertCircle class="size-4" />
          </Match>
        </Switch>
      </span>
      <span class="dashboard-knowledge-composer__job-body">
        <span class="dashboard-knowledge-composer__job-label">{props.job.label}</span>
        <span class="dashboard-knowledge-composer__job-detail">
          {kindLabel()} · {props.job.error ?? (running() ? props.job.detail : terminalDetail(props.job, i18n))}
        </span>
        <Show when={(props.job.pagesBlockedByRobots ?? 0) > 0}>
          <span class="dashboard-knowledge-composer__job-robots">
            {robotsBlockedText(props.job.pagesBlockedByRobots, (no, en) => i18n.tr(no, en))}
          </span>
        </Show>
        <Show when={progress() !== null}>
          <span class="dashboard-knowledge-composer__job-progress">
            <span style={{ width: `${progress() ?? 0}%` }} />
          </span>
        </Show>
        <Show when={(props.job.events ?? []).length > 0}>
          <span class="dashboard-knowledge-composer__job-events">
            <For each={(props.job.events ?? []).slice(0, 2)}>
              {(event) => <span>{event.detail}</span>}
            </For>
          </span>
        </Show>
      </span>
    </li>
  )
}

/** Completed-status label. For a `crawl`/`batch` job (kind `'crawl'`), quarry's
 * own run completion is NEVER by itself proof anything was durably indexed —
 * that requires `ingest: true` to have been requested AND a real
 * `store_record_written` event to have been observed (see `appendJobEvent`).
 * Other kinds (`link`, `upload`) already gate their own "completed" status on
 * a real Data Plane / imports-core write, so they keep the direct label. */
function completedLabel(job: IngestJob, i18n: ReturnType<typeof useI18n>): string {
  if (job.kind !== 'crawl') return i18n.tr('Ferdig indeksert', 'Indexed')
  if (!job.ingestRequested) return i18n.tr('Crawlet (ikke lagret i kunnskapsbasen)', 'Crawled (not saved to knowledge base)')
  if (job.ingestWritten) return i18n.tr('Ferdig indeksert', 'Indexed')
  if (job.ingestFailed) return i18n.tr('Crawlet, men lagring feilet', 'Crawled, but saving failed')
  // Ingest was requested but neither confirmed nor failed yet (e.g. the
  // ingest write is still in flight, or the event stream disconnected
  // before it resolved) — stay neutral rather than claim success.
  return i18n.tr('Crawlet', 'Crawled')
}

function statusLabel(job: IngestJob, i18n: ReturnType<typeof useI18n>): string {
  const normalized = job.status.trim().toLowerCase()
  if (normalized === 'completed' || normalized === 'complete' || normalized === 'succeeded') return completedLabel(job, i18n)
  if (normalized === 'failed' || normalized === 'error') return i18n.tr('Mislyktes', 'Failed')
  if (normalized === 'cancelled') return i18n.tr('Avbrutt', 'Cancelled')
  return job.status
}

/** Terminal-row detail: status label, plus the crawled page count when the
 * run reported one (so a completed crawl reads "Indexed · 5 pages" rather
 * than dropping to a bare status with 0 pages). */
function terminalDetail(job: IngestJob, i18n: ReturnType<typeof useI18n>): string {
  const label = statusLabel(job, i18n)
  if (job.kind === 'crawl' && job.pages !== undefined) {
    const pages = i18n.tr(
      `${job.pages} ${job.pages === 1 ? 'side' : 'sider'}`,
      `${job.pages} ${job.pages === 1 ? 'page' : 'pages'}`,
    )
    return `${label} · ${pages}`
  }
  return label
}

function knowledgeUrlPlaceholder(mode: IngestMode, i18n: ReturnType<typeof useI18n>): string {
  if (mode === 'crawl') return i18n.tr('nettsted.example - crawl hele nettstedet', 'nettsted.example - crawl the full site')
  if (mode === 'products') return i18n.tr('nettsted.example/.../produkt - hent produkter fra siden', 'nettsted.example/.../product - extract products from the page')
  return i18n.tr('nettsted.example - skrap og forhåndsvis', 'nettsted.example - scrape and preview')
}

function knowledgeSubmitLabel(mode: IngestMode, i18n: ReturnType<typeof useI18n>): string {
  if (mode === 'crawl') return i18n.tr('Start crawl', 'Start crawl')
  if (mode === 'products') return i18n.tr('Hent produkter', 'Extract products')
  return i18n.tr('Skrap side', 'Scrape page')
}

function clampCrawlPages(value: number): number {
  if (!Number.isFinite(value)) return 1
  return Math.min(5000, Math.max(1, Math.round(value)))
}

function normalizeProgress(value: number | undefined): number | null {
  if (value === undefined || !Number.isFinite(value)) return null
  const pct = value <= 1 ? value * 100 : value
  return Math.min(100, Math.max(0, Math.round(pct)))
}

function isProfileId(value: string): boolean {
  return /^prof_[A-Za-z0-9_-]{3,}$/.test(value)
}

function shortProfileLabel(profileId: string): string {
  return profileId.length <= 18 ? profileId : `${profileId.slice(0, 10)}...${profileId.slice(-5)}`
}

/** Dropdown option text for a real stored profile: its name when it has one
 * (set via `createBrowserProfile`/`renameBrowserProfile`), else a truncated
 * id — plus its scope, so the picker never hides what a selection implies. */
function profileOptionLabel(profile: BrowserProfileSummary): string {
  const label = profile.name?.trim() || shortProfileLabel(profile.profile_id)
  return `${label} · ${PROFILE_SCOPE_LABELS[profile.scope]}`
}

function profileProbeSummary(probe: BrowserProfileRestoreProbe, i18n: ReturnType<typeof useI18n>): string {
  const stored = probe.cookies_count + probe.local_storage_count + probe.session_storage_count + probe.indexed_db_count
  return probe.restorable ? i18n.tr(`${stored} lagrede signaler`, `${stored} saved signals`) : i18n.tr('Tom profil', 'Empty profile')
}

function parseJobKey(key: string): number {
  const match = /^job-(\d+)$/.exec(key)
  return match ? Number(match[1]) : 0
}

function isPersistedJobs(value: unknown): value is IngestJob[] {
  return Array.isArray(value) && value.every(isPersistedJob)
}

function isPersistedJob(value: unknown): value is IngestJob {
  if (!value || typeof value !== 'object') return false
  const record = value as Record<string, unknown>
  return typeof record.key === 'string'
    && typeof record.detail === 'string'
    && typeof record.kind === 'string'
    && (record.kind === 'link' || record.kind === 'crawl' || record.kind === 'upload')
    && typeof record.label === 'string'
    && typeof record.status === 'string'
}

function isPersistedBrowserPreview(value: unknown): value is PersistedBrowserPreview {
  if (!isRecord(value)) return false
  return Array.isArray(value.browserRationales)
    && value.browserRationales.every(isBrowserStepRationale)
    && isScrapePreview(value.preview)
    && typeof value.savedAt === 'string'
}

function isScrapePreview(value: unknown): value is ScrapePreview {
  if (!isRecord(value)) return false
  return Array.isArray(value.blocks)
    && value.blocks.every(isScrapeBlock)
    && typeof value.charCount === 'number'
    && typeof value.description === 'string'
    && typeof value.markdown === 'string'
    && typeof value.source === 'string'
    && typeof value.title === 'string'
    && typeof value.url === 'string'
    && optional(value.browserSession, isBrowserSessionResponse)
    && optionalNullableString(value.browserSessionError)
}

function isScrapeBlock(value: unknown): value is ScrapeBlock {
  if (!isRecord(value)) return false
  return typeof value.heading === 'boolean'
    && typeof value.raw === 'string'
    && typeof value.text === 'string'
}

function isBrowserStepRationale(value: unknown): value is BrowserStepRationale {
  if (!isRecord(value)) return false
  return (typeof value.actionType === 'string' || value.actionType === null)
    && (typeof value.confidence === 'number' || value.confidence === null)
    && typeof value.done === 'boolean'
    && typeof value.goal === 'string'
    && (typeof value.modelUsed === 'string' || value.modelUsed === null)
    && (typeof value.reason === 'string' || value.reason === null)
    && typeof value.step === 'number'
}

function isBrowserSessionResponse(value: unknown): value is BrowserSessionResponse {
  if (!isRecord(value) || !isRecord(value.session)) return false
  const session = value.session
  return Array.isArray(session.capabilities)
    && session.capabilities.every(isString)
    && optional(session.frame, isBrowserFrame)
    && optional(session.devtools, isBrowserDevtools)
    && optionalNullableString(session.devtoolsUrl)
    && typeof session.id === 'string'
    && isRecord(session.profile)
    && optionalNullableString(session.profile.id)
    && typeof session.profile.scope === 'string'
    && typeof session.profile.storage === 'string'
    && typeof session.renderMode === 'string'
    && typeof session.status === 'string'
    && optionalArray(session.tabs, isBrowserTab)
    && optionalNullableString(session.tabsUrl)
    && optionalArray(session.timeline, isBrowserTimelineEntry)
    && optional(session.replay, isBrowserReplay)
    && typeof session.title === 'string'
    && typeof session.url === 'string'
    && optional(session.visual, isBrowserVisualEvidence)
    && isRecord(session.viewport)
    && typeof session.viewport.height === 'number'
    && typeof session.viewport.width === 'number'
    && (session.zdr === undefined || typeof session.zdr === 'boolean')
    && optional(value.observation, isBrowserObservation)
}

function isBrowserFrame(value: unknown): value is NonNullable<BrowserSessionResponse['session']['frame']> {
  if (!isRecord(value)) return false
  return value.kind === 'screenshot'
    && optionalNullableString(value.artifactId)
    && optionalNullableString(value.mediaType)
    && optionalNullableString(value.url)
}

function isBrowserTab(value: unknown): value is NonNullable<BrowserSessionResponse['session']['tabs']>[number] {
  if (!isRecord(value)) return false
  return typeof value.active === 'boolean'
    && typeof value.tabId === 'string'
    && optionalNullableString(value.title)
    && optionalNullableString(value.url)
}

function isBrowserDevtools(value: unknown): value is NonNullable<BrowserSessionResponse['session']['devtools']> {
  if (!isRecord(value)) return false
  return Array.isArray(value.events)
    && value.events.every(isBrowserDevtoolsEvent)
    && (value.eventCount === undefined || typeof value.eventCount === 'number')
    && (value.lastSequence === undefined || value.lastSequence === null || typeof value.lastSequence === 'number')
}

function isBrowserDevtoolsEvent(value: unknown): boolean {
  if (!isRecord(value)) return false
  return typeof value.category === 'string'
    && typeof value.name === 'string'
    && typeof value.sequence === 'number'
    && typeof value.timestampMs === 'number'
    && optionalNullableString(value.level)
    && optionalNullableString(value.method)
    && optionalNullableString(value.tabId)
    && optionalNullableString(value.text)
    && optionalNullableString(value.url)
    && (value.status === undefined || value.status === null || typeof value.status === 'number')
}

function isBrowserVisualEvidence(value: unknown): value is NonNullable<BrowserSessionResponse['session']['visual']> {
  if (!isRecord(value)) return false
  return optionalNullableString(value.observationArtifactId)
    && optionalNullableString(value.observationUrl)
}

function isBrowserTimelineEntry(value: unknown): value is NonNullable<BrowserSessionResponse['session']['timeline']>[number] {
  if (!isRecord(value)) return false
  return typeof value.step === 'number'
    && optionalNullableString(value.observedAt)
    && optionalNullableString(value.screenshotArtifactId)
    && optionalNullableString(value.screenshotUrl)
    && optionalNullableString(value.title)
    && optionalNullableString(value.url)
    && optionalNullableString(value.visualObservationArtifactId)
    && optionalNullableString(value.visualObservationUrl)
}

function isBrowserReplay(value: unknown): value is NonNullable<BrowserSessionResponse['session']['replay']> {
  if (!isRecord(value)) return false
  return Array.isArray(value.events)
    && value.events.every(isBrowserReplayEvent)
    && (value.eventCount === undefined || typeof value.eventCount === 'number')
}

function isBrowserReplayEvent(value: unknown): boolean {
  if (!isRecord(value)) return false
  return typeof value.id === 'string'
    && typeof value.kind === 'string'
    && (value.step === undefined || value.step === null || typeof value.step === 'number')
    && (value.timestampMs === undefined || value.timestampMs === null || typeof value.timestampMs === 'number')
    && optionalNullableString(value.actor)
    && optionalNullableString(value.actionType)
    && optionalNullableString(value.controlMode)
    && optionalNullableString(value.observedAt)
    && optionalNullableString(value.operation)
    && optionalNullableString(value.screenshotArtifactId)
    && optionalNullableString(value.screenshotUrl)
    && optionalNullableString(value.tabId)
    && optionalNullableString(value.title)
    && optionalNullableString(value.url)
    && optionalNullableString(value.visualObservationArtifactId)
    && optionalNullableString(value.visualObservationUrl)
    && (value.zdr === undefined || typeof value.zdr === 'boolean')
}

function isBrowserObservation(value: unknown): value is NonNullable<BrowserSessionResponse['observation']> {
  if (!isRecord(value)) return false
  return typeof value.run_id === 'string'
    && typeof value.step === 'number'
    && typeof value.url === 'string'
    && optionalNullableString(value.screenshot_artifact_id)
    && optionalNullableString(value.title)
    && optionalNullableString(value.visual_observation_artifact_id)
}

function optional<T>(value: unknown, guard: (candidate: unknown) => candidate is T): value is T | undefined | null {
  return value === undefined || value === null || guard(value)
}

function optionalArray<T>(value: unknown, guard: (candidate: unknown) => candidate is T): value is T[] | undefined {
  return value === undefined || (Array.isArray(value) && value.every(guard))
}

function optionalNullableString(value: unknown): value is string | null | undefined {
  return value === undefined || value === null || typeof value === 'string'
}

function isString(value: unknown): value is string {
  return typeof value === 'string'
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === 'object' && !Array.isArray(value)
}
