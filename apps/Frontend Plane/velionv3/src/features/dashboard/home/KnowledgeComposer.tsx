import { A } from '@solidjs/router'
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
  RefreshCw,
  ShieldCheck,
  ShoppingBag,
} from 'lucide-solid'
import { createEffect, createMemo, createResource, createSignal, For, Match, onCleanup, Show, Switch, untrack } from 'solid-js'
import { executeAction } from '@/shared/actions/action-client'
import { getAuthSession, getSessionContext } from '@/shared/api/auth-client'
import {
  closeBrowserSession,
  createBrowserTab,
  createBrowserSession,
  listBrowserProfiles,
  probeBrowserProfile,
  runBrowserAction,
  selectBrowserTab,
  setBrowserControlMode,
  suggestBrowserAction,
  type BrowserAction,
  type BrowserActionSuggestionResponse,
  type BrowserControlMode,
  type BrowserObservation,
  type BrowserProfileRestoreProbe,
  type BrowserSession,
  type BrowserSessionResponse,
} from '@/shared/api/browser-client'
import { startBrowserAiRun } from '@/shared/api/browser-run-client'
import { streamRunEvents } from '@/shared/api/run-console-client'
import {
  crawlSelectedPages,
  createDocument,
  discoverCrawlPages,
  extractProducts,
  getImportJob,
  importUpload,
  listCrawlJobs,
  scrapePreview,
  startCrawl,
  streamCrawlRunEvents,
  summarizeProducts,
  type CrawlDiscovery,
  type CrawlWorkflowEvent,
  type Product,
  type ProductExtraction,
} from '@/shared/api/knowledge-client'
import { getPreferences } from '@/shared/api/settings-client'
import { cn } from '@/shared/lib/cn'
import { useI18n } from '@/shared/i18n'
import { readClientJson, removeClientValue, writeClientJson } from '@/shared/session/client-storage'
import { CrawlPagePicker } from './CrawlPagePicker'
import { DashboardPlanBadge } from './DashboardPlanBadge'
import { ProductPicker } from './ProductPicker'
import { ScrapePreviewPanel } from './KnowledgeScrapePreview'
import { createBrowserLoopController, type BrowserLoopState } from './browser-loop'
import {
  attachBrowserObservation,
  attachBrowserSession,
  attachBrowserTabs,
  withStepRationale,
  type BrowserStepRationale,
} from './browser-session'
import {
  hostnameOf,
  normalizeUrl,
  toScrapePreview,
  type ScrapeBlock,
  type ScrapePreview,
} from './knowledge-preview'

type IngestMode = 'link' | 'crawl' | 'products'
type IngestKind = 'link' | 'crawl' | 'upload'

type IngestJob = {
  agentMode?: boolean
  detail: string
  error?: string
  events?: IngestEvent[]
  eventStream?: string
  id?: string
  key: string
  kind: IngestKind
  label: string
  /** Pages fetched, surfaced from the crawl's run_completed event. */
  pages?: number
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
const crawlJobsStoragePrefix = 'velion.dashboard.crawl.jobs'
const browserPreviewStoragePrefix = 'velion.dashboard.browser.preview'
const isolatedProfileChoice = 'isolated'
const newProfileChoice = 'new'

async function loadOrgContext() {
  const [session, ctx] = await Promise.all([getAuthSession(), getSessionContext()])
  return {
    orgId: ctx.orgs[0]?.id ?? '',
    userId: session?.user.id ?? '',
  }
}

function isTerminal(status: string): boolean {
  return TERMINAL.has(status.trim().toLowerCase())
}

function hasActiveJobId(job: IngestJob): job is ActiveIngestJob {
  return Boolean(job.id) && !isTerminal(job.status)
}

export function KnowledgeComposer(props: {
  inlinePlanLabel?: string
  inlineTitle?: string
  onPreviewActiveChange?: (active: boolean) => void
  previewCollapsed?: boolean
}) {
  const i18n = useI18n()
  let pollTimer: number | undefined
  let hydratedJobsKey: string | null = null
  let hydratedBrowserPreviewKey: string | null = null
  let keySeq = 0
  const crawlStreams = new Map<string, AbortController>()
  const [ctx] = createResource(loadOrgContext)
  const orgId = createMemo(() => ctx()?.orgId ?? '')
  const crawlJobsStorageKey = createMemo(() => orgId() ? `${crawlJobsStoragePrefix}.${orgId()}` : null)
  const browserPreviewStorageKey = createMemo(() => orgId() ? `${browserPreviewStoragePrefix}.${orgId()}` : null)
  const [url, setUrl] = createSignal('')
  const [mode, setMode] = createSignal<IngestMode>('link')
  const [agentMode, setAgentMode] = createSignal(false)
  const [crawlMaxPages, setCrawlMaxPages] = createSignal(25)
  const [submitting, setSubmitting] = createSignal(false)
  const [adding, setAdding] = createSignal(false)
  const [browserBusy, setBrowserBusy] = createSignal(false)
  const [browserProfiles, setBrowserProfiles] = createSignal<string[]>([])
  const [browserProfilesLoading, setBrowserProfilesLoading] = createSignal(false)
  const [browserProfileChoice, setBrowserProfileChoice] = createSignal(isolatedProfileChoice)
  const [browserProfileProbe, setBrowserProfileProbe] = createSignal<BrowserProfileRestoreProbe | null>(null)
  const [browserProfileProbing, setBrowserProfileProbing] = createSignal(false)
  const [browserProfileError, setBrowserProfileError] = createSignal<string | null>(null)
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
  let aiRunAbort: AbortController | null = null
  onCleanup(() => aiRunAbort?.abort())
  const [preview, setPreview] = createSignal<ScrapePreview | null>(null)
  const [discovery, setDiscovery] = createSignal<CrawlDiscovery | null>(null)
  const [discovering, setDiscovering] = createSignal(false)
  const [productExtraction, setProductExtraction] = createSignal<ProductExtraction | null>(null)
  const [productSummary, setProductSummary] = createSignal<string | null>(null)
  const [summarizing, setSummarizing] = createSignal(false)
  const [formError, setFormError] = createSignal<string | null>(null)
  const [jobs, setJobs] = createSignal<IngestJob[]>([])
  let fileInputRef: HTMLInputElement | undefined
  let hydratedProfilesOrg: string | null = null

  // Tell the dashboard when a preview / page-picker / product-picker is on screen
  // so it can hide the info cards and surface the results/cards chevron toggle.
  createEffect(() => props.onPreviewActiveChange?.(Boolean(preview() || discovery() || productExtraction())))

  const ready = createMemo(() => orgId().length > 0)
  const canSubmitUrl = createMemo(() => url().trim().length > 0 && !submitting() && !discovering() && ready() && (mode() !== 'crawl' || crawlMaxPages() > 0))
  const canDiscover = createMemo(() => url().trim().length > 0 && !discovering() && !submitting() && ready())
  const openFileDialog = () => fileInputRef?.click()
  const selectedBrowserProfileId = createMemo(() => {
    const choice = browserProfileChoice()
    return isProfileId(choice) ? choice : null
  })
  const shouldPersistBrowserProfile = createMemo(() => browserProfileChoice() === newProfileChoice || Boolean(selectedBrowserProfileId()))

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

  createEffect(() => {
    const key = crawlJobsStorageKey()
    if (!key || hydratedJobsKey === key) return
    const restored = readClientJson(key, isPersistedJobs) ?? []
    keySeq = restored.reduce((max, job) => Math.max(max, parseJobKey(job.key)), 0)
    setJobs(restored)
    hydratedJobsKey = key
    for (const job of restored.filter(hasActiveJobId)) {
      if (job.kind === 'crawl') startCrawlEventStream(job.key, job.id)
    }
    if (restored.some(hasActiveJobId)) ensurePolling()
  })

  createEffect(() => {
    const id = orgId()
    if (!id || hydratedProfilesOrg === id) return
    hydratedProfilesOrg = id
    void refreshBrowserProfiles()
  })

  createEffect(() => {
    const key = crawlJobsStorageKey()
    if (!key || hydratedJobsKey !== key) return
    writeClientJson(key, jobs())
  })

  createEffect(() => {
    const key = browserPreviewStorageKey()
    if (!key || hydratedBrowserPreviewKey === key) return
    hydratedBrowserPreviewKey = key
    const restored = readClientJson(key, isPersistedBrowserPreview)
    if (!restored) return
    if (restored.preview.browserSession?.session.zdr) {
      removeClientValue(key)
      return
    }
    setMode('link')
    setUrl(restored.preview.url)
    setPreview(restored.preview)
    setBrowserRationales(restored.browserRationales)
  })

  createEffect(() => {
    const key = browserPreviewStorageKey()
    if (!key || hydratedBrowserPreviewKey !== key) return
    const current = preview()
    if (!current) {
      removeClientValue(key)
      return
    }
    if (current.browserSession?.session.zdr) {
      removeClientValue(key)
      return
    }
    writeClientJson(key, {
      browserRationales: browserRationales(),
      preview: current,
      savedAt: new Date().toISOString(),
    } satisfies PersistedBrowserPreview)
  })

  const startCrawlJob = async (target: string) => {
    setDiscovery(null)
    const maxPages = crawlMaxPages()
    // Phase 6 selective ingest: resolve the user's crawl_ingest_mode → ingest
    // flag. auto = always save; never = working-set only; prompt = ask now.
    // (The promote capability is quarry's /v1/crawl|/v1/batch with ingest=true,
    // wired in Phase 2 — owner=user, private. 'prompt' simply gates it on a
    // confirm so nothing is saved unless the user says so.) Default never.
    const ingestMode = (await getPreferences().catch(() => null))?.crawlIngestMode ?? 'never'
    const ingest =
      ingestMode === 'auto' ||
      (ingestMode === 'prompt' &&
        window.confirm(
          i18n.tr(
            'Lagre disse sidene i kunnskapsbasen din? (privat for deg til du deler dem)',
            'Save these pages to your knowledge base? (private to you until you share them)',
          ),
        ))
    const key = addJob({
      agentMode: agentMode(),
      detail: agentMode()
        ? i18n.tr('Starter agentstyrt crawl-workflow ...', 'Starting agent-run crawl workflow ...')
        : i18n.tr(`Crawler opptil ${maxPages} sider ...`, `Crawling up to ${maxPages} pages ...`),
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
          url: target,
        })
        updateJob(key, {
          eventStream: execution.eventStream,
          id: execution.runId,
          status: execution.status || 'queued',
        })
        startCrawlEventStream(key, execution.runId)
      } else {
        const job = await startCrawl(orgId(), { ingest, maxPages, url: target })
        updateJob(key, { eventStream: job.eventStream, id: job.id, status: job.status || 'running' })
        startCrawlEventStream(key, job.id)
      }
      setUrl('')
    } catch (reason) {
      const message = reason instanceof Error ? reason.message : i18n.tr('Crawl kunne ikke startes.', 'Crawl could not be started.')
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
      setFormError(i18n.tr('Skriv inn en gyldig nettadresse, f.eks. vg.no eller https://aquatiq.com.', 'Enter a valid web address, for example vg.no or https://aquatiq.com.'))
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
      setFormError(reason instanceof Error ? reason.message : i18n.tr('Kunne ikke oppdage sider.', 'Could not discover pages.'))
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
    const key = addJob({
      detail: i18n.tr(`Crawler ${urls.length} valgte sider ...`, `Crawling ${urls.length} selected pages ...`),
      kind: 'crawl',
      label: `${host} · ${urls.length} sider`,
      status: 'pending',
    })
    try {
      const job = await crawlSelectedPages(orgId(), urls)
      updateJob(key, { eventStream: job.eventStream, id: job.id, status: job.status || 'running' })
      startCrawlEventStream(key, job.id)
      setDiscovery(null)
      setUrl('')
    } catch (reason) {
      const message = reason instanceof Error ? reason.message : i18n.tr('Crawl kunne ikke startes.', 'Crawl could not be started.')
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
      setFormError(reason instanceof Error ? reason.message : i18n.tr('Kunne ikke hente produkter.', 'Could not fetch products.'))
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
      if (selectedBrowserProfileId() && !result.profiles.includes(selectedBrowserProfileId() ?? '')) {
        setBrowserProfileChoice(isolatedProfileChoice)
        setBrowserProfileProbe(null)
      }
    } catch (reason) {
      setBrowserProfileError(reason instanceof Error ? reason.message : i18n.tr('Nettleserprofiler kunne ikke hentes.', 'Browser profiles could not be loaded.'))
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
      setBrowserProfileError(reason instanceof Error ? reason.message : i18n.tr('Profilen kunne ikke sjekkes.', 'The profile could not be checked.'))
    } finally {
      setBrowserProfileProbing(false)
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
      setFormError(reason instanceof Error ? reason.message : i18n.tr('Kunne ikke lage AI-sammendrag.', 'Could not create an AI summary.'))
    } finally {
      setSummarizing(false)
    }
  }

  const loadLinkPreview = async (target: string) => {
    // A new link replaces any current preview: stop a running AI loop, close
    // the old browser session, and drop rationales so nothing dangles.
    browserLoop.requestStop()
    aiRunAbort?.abort()
    setAiRunRationale(null)
    closePreviewBrowserSession(preview())
    setBrowserRationales([])
    let browserSessionPromise: Promise<BrowserSessionAttempt> | null = null
    try {
      browserSessionPromise = tryCreateBrowserSession(target)
      const result = await scrapePreview(orgId(), {
        url: target,
        signals: {
          actions: ['render_page'],
          screenshot: false,
          pdf: false,
          prior_block_signals: 0,
          profile_required: false,
          url_type: 'Default',
        },
        render: {
          waitForTimeoutMs: 1800,
        },
      })
      const browserSession = await browserSessionPromise
      setPreview({
        ...attachBrowserSession(toScrapePreview(target, result), browserSession.session),
        browserSessionError: browserSession.error,
      })
    } catch (reason) {
      void browserSessionPromise?.then((attempt) => {
        closeBrowserSessionById(attempt.session?.session.id)
      })
      setFormError(reason instanceof Error ? reason.message : i18n.tr('Skraping kunne ikke fullføres.', 'Scraping could not be completed.'))
    }
  }

  const tryCreateBrowserSession = async (target: string): Promise<BrowserSessionAttempt> => {
    const id = orgId()
    if (!id) return { error: i18n.tr('Arbeidsområde mangler for nettleserøkt.', 'Workspace is missing for the browser session.'), session: null }
    const controller = new AbortController()
    const timeout = window.setTimeout(() => controller.abort(), BROWSER_SESSION_TIMEOUT_MS)
    const profileId = selectedBrowserProfileId()
    try {
      const session = await createBrowserSession(id, {
        ...(shouldPersistBrowserProfile() ? { persistentProfile: true } : {}),
        ...(profileId ? { profileId } : {}),
        url: target,
        viewport: { width: 1280, height: 800 },
      }, controller.signal)
      const returnedProfileId = session.session.profile.id
      if (session.session.profile.storage === 'persistent' && returnedProfileId) {
        setBrowserProfiles((current) => current.includes(returnedProfileId) ? current : [returnedProfileId, ...current])
        setBrowserProfileChoice(returnedProfileId)
      }
      return { error: null, session }
    } catch (reason) {
      return {
        error: reason instanceof Error ? reason.message : i18n.tr('Nettleserøkt kunne ikke startes.', 'Browser session could not be started.'),
        session: null,
      }
    } finally {
      window.clearTimeout(timeout)
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

  const clearStoredBrowserPreview = () => {
    const key = browserPreviewStorageKey()
    if (key) removeClientValue(key)
  }

  const clearPreview = () => {
    const current = preview()
    browserLoop.requestStop()
    aiRunAbort?.abort()
    setAiRunRationale(null)
    closePreviewBrowserSession(current)
    clearStoredBrowserPreview()
    setBrowserRationales([])
    setPreview(null)
  }

  const performBrowserAction = async (action: BrowserAction) => {
    const current = preview()
    const sessionId = current?.browserSession?.session.id
    const id = orgId()
    if (!current || !sessionId || !id || browserBusy()) return
    setBrowserBusy(true)
    setFormError(null)
    try {
      const nextSession = await runBrowserAction(id, sessionId, action, { actor: 'human' })
      setPreview(attachBrowserSession(current, nextSession))
    } catch (reason) {
      setFormError(reason instanceof Error ? reason.message : i18n.tr('Nettleserhandlingen kunne ikke fullføres.', 'The browser action could not be completed.'))
    } finally {
      setBrowserBusy(false)
    }
  }

  const updateBrowserControlMode = async (mode: BrowserControlMode) => {
    const current = preview()
    const sessionId = current?.browserSession?.session.id
    const id = orgId()
    if (!current || !sessionId || !id || browserBusy()) return
    if (mode === 'human_takeover') browserLoop.requestPause()
    setBrowserBusy(true)
    setFormError(null)
    try {
      const nextSession = await setBrowserControlMode(id, sessionId, mode)
      setPreview(attachBrowserSession(current, nextSession))
    } catch (reason) {
      setFormError(reason instanceof Error ? reason.message : i18n.tr('Kunne ikke endre nettleserkontroll.', 'Could not change browser control.'))
    } finally {
      setBrowserBusy(false)
    }
  }

  const createPreviewBrowserTab = async () => {
    const current = preview()
    const sessionId = current?.browserSession?.session.id
    const id = orgId()
    if (!current || !sessionId || !id || browserBusy()) return
    setBrowserBusy(true)
    setFormError(null)
    try {
      const nextTabs = await createBrowserTab(id, sessionId)
      setPreview(attachBrowserTabs(current, nextTabs))
    } catch (reason) {
      setFormError(reason instanceof Error ? reason.message : i18n.tr('Kunne ikke opprette ny nettleserfane.', 'Could not create a new browser tab.'))
    } finally {
      setBrowserBusy(false)
    }
  }

  const selectPreviewBrowserTab = async (tabId: string) => {
    const current = preview()
    const sessionId = current?.browserSession?.session.id
    const id = orgId()
    if (!current || !sessionId || !id || browserBusy()) return
    setBrowserBusy(true)
    setFormError(null)
    try {
      const nextTabs = await selectBrowserTab(id, sessionId, tabId)
      setPreview(attachBrowserTabs(current, nextTabs))
    } catch (reason) {
      setFormError(reason instanceof Error ? reason.message : i18n.tr('Kunne ikke bytte nettleserfane.', 'Could not switch browser tab.'))
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
      }
      return suggestion
    } catch (reason) {
      setFormError(reason instanceof Error ? reason.message : i18n.tr('AI-steget kunne ikke fullføres.', 'The AI browser step could not be completed.'))
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
    aiRunAbort?.abort()
    const controller = new AbortController()
    aiRunAbort = controller
    let dispatchCount = 0
    try {
      const run = await startBrowserAiRun(
        id,
        sessionId,
        { goal, maxSteps: BROWSER_AI_LOOP_MAX_STEPS },
        controller.signal,
      )
      browserLoop.attachRun({ orgId: id, runId: run.runId })
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
        onError: (streamError) => {
          // A deliberate abort (new link, preview cleared, unmount) is not an
          // error the user should see.
          if (controller.signal.aborted) {
            browserLoop.finish('stopped')
            return
          }
          const message = streamError instanceof Error
            ? streamError.message
            : i18n.tr('AI-loopen mistet forbindelsen til kjøringen.', 'The AI browser loop lost its run connection.')
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
      const message = reason instanceof Error ? reason.message : i18n.tr('AI-loopen kunne ikke startes.', 'The AI browser loop could not be started.')
      browserLoop.finish('stopped', message)
      setFormError(message)
      return null
    } finally {
      if (aiRunAbort === controller) aiRunAbort = null
    }
  }

  // Link mode scrapes first (preview, no ingest yet); crawl mode kicks off an
  // async site crawl. "Add to knowledge base" below the preview does the ingest.
  const submitUrl = async (event: SubmitEvent) => {
    event.preventDefault()
    if (!canSubmitUrl()) return
    const target = normalizeUrl(url())
    if (!target) {
      setFormError(i18n.tr('Skriv inn en gyldig nettadresse, f.eks. vg.no eller https://aquatiq.com.', 'Enter a valid web address, for example vg.no or https://aquatiq.com.'))
      return
    }
    setFormError(null)
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
      closePreviewBrowserSession(current)
      clearStoredBrowserPreview()
      setBrowserRationales([])
      setPreview(null)
      setUrl('')
    } catch (reason) {
      const message = reason instanceof Error ? reason.message : i18n.tr('Kunne ikke legge til i kunnskapsbasen.', 'Could not add to the knowledge base.')
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
      const message = reason instanceof Error ? reason.message : i18n.tr('Opplasting feilet.', 'Upload failed.')
      updateJob(key, { status: 'failed', error: message })
      setFormError(message)
    } finally {
      stopPollingIfIdle()
    }
  }

  onCleanup(() => {
    if (pollTimer !== undefined) window.clearInterval(pollTimer)
    for (const controller of crawlStreams.values()) controller.abort()
  })

  return (
    <div class="velion-panel-in velion-dashboard-composer-card dashboard-knowledge-composer">
      <form class="dashboard-knowledge-composer__url" onSubmit={submitUrl}>
        <div
          class="dashboard-knowledge-composer__mode-row"
          classList={{ 'dashboard-knowledge-composer__mode-row--inline': Boolean(props.inlinePlanLabel || props.inlineTitle) }}
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
              classList={{ 'dashboard-knowledge-composer__mode--active': mode() === 'link' }}
              onClick={() => switchMode('link')}
              aria-pressed={mode() === 'link'}
            >
              <Link2 class="size-3.5" /> {i18n.tr('Lenke', 'Link')}
            </button>
            <button
              type="button"
              classList={{ 'dashboard-knowledge-composer__mode--active': mode() === 'crawl' }}
              onClick={() => switchMode('crawl')}
              aria-pressed={mode() === 'crawl'}
            >
              <Globe2 class="size-3.5" /> {i18n.tr('Crawl', 'Crawl')}
            </button>
            <button
              type="button"
              classList={{ 'dashboard-knowledge-composer__mode--active': mode() === 'products' }}
              onClick={() => switchMode('products')}
              aria-pressed={mode() === 'products'}
            >
              <ShoppingBag class="size-3.5" /> {i18n.tr('Produkter', 'Products')}
            </button>
          </div>
          <Show when={props.inlineTitle}>
            {(title) => <strong class="dashboard-knowledge-composer__inline-title">{title()}</strong>}
          </Show>
        </div>

        <Show when={props.previewCollapsed || !preview()}>
          <div class="velion-glass-input dashboard-knowledge-composer__input-wrap">
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
                  }}
                  disabled={!ready()}
                  aria-label={i18n.tr('Nettleserprofil', 'Browser profile')}
                >
                  <option value={isolatedProfileChoice}>{i18n.tr('Isolert', 'Isolated')}</option>
                  <option value={newProfileChoice}>{i18n.tr('Ny profil', 'New profile')}</option>
                  <For each={browserProfiles()}>
                    {(profileId) => <option value={profileId}>{shortProfileLabel(profileId)}</option>}
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

      <Show when={props.previewCollapsed ? null : preview()} keyed>
        {(current) => (
          <ScrapePreviewPanel
            adding={adding()}
            browserBusy={browserBusy()}
            browserLoop={browserLoopState()}
            browserLoopRationale={aiRunRationale()}
            browserRationales={browserRationales()}
            onBrowserAction={(action) => void performBrowserAction(action)}
            onBrowserAutoRun={beginBrowserAiRun}
            onBrowserControlMode={(mode) => void updateBrowserControlMode(mode)}
            onBrowserLoopPause={() => browserLoop.requestPause()}
            onBrowserLoopResume={() => browserLoop.requestResume()}
            onBrowserLoopStop={() => browserLoop.requestStop()}
            onBrowserNewTab={createPreviewBrowserTab}
            onBrowserSelectTab={selectPreviewBrowserTab}
            onBrowserSuggestAction={performBrowserSuggestedAction}
            onBrowserSocketObservation={handleBrowserSocketObservation}
            onAdd={(markdown, allSelected) => void addToKnowledge(markdown, allSelected)}
            onDiscard={clearPreview}
            preview={current}
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

      <div class="dashboard-knowledge-composer__actions">
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
        <A href="/knowledge" class="dashboard-knowledge-composer__link">
          {i18n.tr('Åpne kunnskapsbase', 'Open knowledge base')}
          <ArrowRight class="size-3.5" />
        </A>
      </div>

      <Show when={!ready() && ctx.loading}>
        <p class="dashboard-knowledge-composer__status">{i18n.tr('Kobler til arbeidsområdet ...', 'Connecting to the workspace ...')}</p>
      </Show>
      <Show when={formError()}>
        {(message) => <p class="dashboard-knowledge-composer__error">{message()}</p>}
      </Show>

      <Show when={jobs().length > 0}>
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

function statusLabel(status: string, i18n: ReturnType<typeof useI18n>): string {
  const normalized = status.trim().toLowerCase()
  if (normalized === 'completed' || normalized === 'complete' || normalized === 'succeeded') return i18n.tr('Ferdig indeksert', 'Indexed')
  if (normalized === 'failed' || normalized === 'error') return i18n.tr('Mislyktes', 'Failed')
  if (normalized === 'cancelled') return i18n.tr('Avbrutt', 'Cancelled')
  return status
}

/** Terminal-row detail: status label, plus the crawled page count when the
 * run reported one (so a completed crawl reads "Indexed · 5 pages" rather
 * than dropping to a bare status with 0 pages). */
function terminalDetail(job: IngestJob, i18n: ReturnType<typeof useI18n>): string {
  const label = statusLabel(job.status, i18n)
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
  if (mode === 'crawl') return i18n.tr('vg.no - crawl hele nettstedet', 'vg.no - crawl the full site')
  if (mode === 'products') return i18n.tr('elkjop.no/.../mac - hent produkter fra siden', 'elkjop.no/.../mac - extract products from the page')
  return i18n.tr('vg.no eller aquatiq.com - skrap og forhåndsvis', 'vg.no or aquatiq.com - scrape and preview')
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
