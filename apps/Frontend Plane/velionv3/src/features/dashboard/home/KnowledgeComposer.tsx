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
  Sparkles,
} from 'lucide-solid'
import { createEffect, createMemo, createResource, createSignal, For, Match, onCleanup, Show, Switch, untrack } from 'solid-js'
import { executeAction } from '@/shared/actions/action-client'
import { getAuthSession, getSessionContext } from '@/shared/api/auth-client'
import {
  closeBrowserSession,
  createBrowserSession,
  listBrowserProfiles,
  probeBrowserProfile,
  runBrowserAction,
  type BrowserAction,
  type BrowserProfileRestoreProbe,
  type BrowserSessionResponse,
} from '@/shared/api/browser-client'
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
import { cn } from '@/shared/lib/cn'
import { readClientJson, writeClientJson } from '@/shared/session/client-storage'
import { CrawlPagePicker } from './CrawlPagePicker'
import { ProductPicker } from './ProductPicker'
import { ScrapePreviewPanel } from './KnowledgeScrapePreview'
import { attachBrowserSession } from './browser-session'
import {
  hostnameOf,
  normalizeUrl,
  toScrapePreview,
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

const TERMINAL = new Set(['completed', 'complete', 'failed', 'error', 'succeeded', 'cancelled'])
const POLL_INTERVAL_MS = 2500
const BROWSER_SESSION_TIMEOUT_MS = 30000
const crawlJobsStoragePrefix = 'velion.dashboard.crawl.jobs'
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
  onPreviewActiveChange?: (active: boolean) => void
  previewCollapsed?: boolean
}) {
  let pollTimer: number | undefined
  let hydratedJobsKey: string | null = null
  let keySeq = 0
  const crawlStreams = new Map<string, AbortController>()
  const [ctx] = createResource(loadOrgContext)
  const orgId = createMemo(() => ctx()?.orgId ?? '')
  const crawlJobsStorageKey = createMemo(() => orgId() ? `${crawlJobsStoragePrefix}.${orgId()}` : null)
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
    if (!id || crawlStreams.has(key) || isTerminal(jobs().find((job) => job.key === key)?.status ?? '')) return
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

  const startCrawlJob = async (target: string) => {
    setDiscovery(null)
    const maxPages = crawlMaxPages()
    const key = addJob({
      agentMode: agentMode(),
      detail: agentMode() ? 'Starter agentstyrt crawl-workflow…' : `Crawler opptil ${maxPages} sider…`,
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
        const job = await startCrawl(orgId(), { maxPages, url: target })
        updateJob(key, { eventStream: job.eventStream, id: job.id, status: job.status || 'running' })
        startCrawlEventStream(key, job.id)
      }
      setUrl('')
    } catch (reason) {
      const message = reason instanceof Error ? reason.message : 'Crawl kunne ikke startes.'
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
      setFormError('Skriv inn en gyldig nettadresse, f.eks. vg.no eller https://aquatiq.com.')
      return
    }
    setFormError(null)
    setDiscovery(null)
    setDiscovering(true)
    try {
      const result = await discoverCrawlPages(orgId(), { url: target, limit: 200 })
      if (result.pages.length === 0) {
        setFormError('Fant ingen sider å crawle på dette nettstedet — prøv en annen adresse eller bruk hel-crawl.')
        return
      }
      setDiscovery(result)
    } catch (reason) {
      setFormError(reason instanceof Error ? reason.message : 'Kunne ikke oppdage sider.')
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
      detail: `Crawler ${urls.length} valgte sider…`,
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
      const message = reason instanceof Error ? reason.message : 'Crawl kunne ikke startes.'
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
        setFormError('Fant ingen produkter på siden — prøv en produktliste-URL, eller en annen side.')
        return
      }
      setProductExtraction(result)
    } catch (reason) {
      setFormError(reason instanceof Error ? reason.message : 'Kunne ikke hente produkter.')
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
      setBrowserProfileError(reason instanceof Error ? reason.message : 'Nettleserprofiler kunne ikke hentes.')
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
      setBrowserProfileError(reason instanceof Error ? reason.message : 'Profilen kunne ikke sjekkes.')
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
      setProductSummary(summary || 'Ingen sammendrag ble generert.')
    } catch (reason) {
      setFormError(reason instanceof Error ? reason.message : 'Kunne ikke lage AI-sammendrag.')
    } finally {
      setSummarizing(false)
    }
  }

  const loadLinkPreview = async (target: string) => {
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
      setFormError(reason instanceof Error ? reason.message : 'Skraping kunne ikke fullføres.')
    }
  }

  const tryCreateBrowserSession = async (target: string): Promise<BrowserSessionAttempt> => {
    const id = orgId()
    if (!id) return { error: 'Arbeidsområde mangler for nettleserøkt.', session: null }
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
        error: reason instanceof Error ? reason.message : 'Nettleserøkt kunne ikke startes.',
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

  const clearPreview = () => {
    const current = preview()
    closePreviewBrowserSession(current)
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
      const nextSession = await runBrowserAction(id, sessionId, action)
      setPreview(attachBrowserSession(current, nextSession))
    } catch (reason) {
      setFormError(reason instanceof Error ? reason.message : 'Nettleserhandlingen kunne ikke fullføres.')
    } finally {
      setBrowserBusy(false)
    }
  }

  // Link mode scrapes first (preview, no ingest yet); crawl mode kicks off an
  // async site crawl. "Add to knowledge base" below the preview does the ingest.
  const submitUrl = async (event: SubmitEvent) => {
    event.preventDefault()
    if (!canSubmitUrl()) return
    const target = normalizeUrl(url())
    if (!target) {
      setFormError('Skriv inn en gyldig nettadresse, f.eks. vg.no eller https://aquatiq.com.')
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
      detail: allSelected ? 'Indekserer hele siden…' : 'Indekserer valgte seksjoner…',
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
      closePreviewBrowserSession(current)
      setPreview(null)
      setUrl('')
    } catch (reason) {
      const message = reason instanceof Error ? reason.message : 'Kunne ikke legge til i kunnskapsbasen.'
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
      detail: `${list.length} fil${list.length === 1 ? '' : 'er'} lastes opp…`,
      kind: 'upload',
      label: list.map((file) => file.name).join(', '),
      status: 'pending',
    })
    try {
      const job = await importUpload(orgId(), list)
      updateJob(key, { id: job.id, status: job.status || 'pending' })
    } catch (reason) {
      const message = reason instanceof Error ? reason.message : 'Opplasting feilet.'
      updateJob(key, { status: 'failed', error: message })
      setFormError(message)
    } finally {
      stopPollingIfIdle()
    }
  }

  onCleanup(() => {
    if (pollTimer !== undefined) window.clearInterval(pollTimer)
    for (const controller of crawlStreams.values()) controller.abort()
    closePreviewBrowserSession(untrack(preview))
  })

  return (
    <div class="velion-panel-in velion-dashboard-composer-card dashboard-knowledge-composer">
      <div class="dashboard-knowledge-composer__header">
        <span class="dashboard-knowledge-composer__icon">
          <Sparkles class="size-4" />
        </span>
        <div>
          <p>Crawl inn kunnskap</p>
          <small>Skrap og forhåndsvis en lenke, crawl valgte sider eller last opp dokumenter — Velion indekserer alt.</small>
        </div>
      </div>

      <form class="dashboard-knowledge-composer__url" onSubmit={submitUrl}>
        <div class="dashboard-knowledge-composer__modes" role="group" aria-label="Innhentingsmodus">
          <button
            type="button"
            classList={{ 'dashboard-knowledge-composer__mode--active': mode() === 'link' }}
            onClick={() => switchMode('link')}
            aria-pressed={mode() === 'link'}
          >
            <Link2 class="size-3.5" /> Lenke
          </button>
          <button
            type="button"
            classList={{ 'dashboard-knowledge-composer__mode--active': mode() === 'crawl' }}
            onClick={() => switchMode('crawl')}
            aria-pressed={mode() === 'crawl'}
          >
            <Globe2 class="size-3.5" /> Crawl
          </button>
          <button
            type="button"
            classList={{ 'dashboard-knowledge-composer__mode--active': mode() === 'products' }}
            onClick={() => switchMode('products')}
            aria-pressed={mode() === 'products'}
          >
            <ShoppingBag class="size-3.5" /> Produkter
          </button>
        </div>

        <div class="velion-glass-input dashboard-knowledge-composer__input-wrap">
          <Link2 class="size-4 shrink-0 text-[#9A9188]" aria-hidden="true" />
          <input
            value={url()}
            onInput={(event) => setUrl(event.currentTarget.value)}
            class="dashboard-knowledge-composer__input"
            placeholder={
              mode() === 'crawl'
                ? 'vg.no — crawl hele nettstedet'
                : mode() === 'products'
                  ? 'elkjop.no/…/mac — hent produkter fra siden'
                  : 'vg.no eller aquatiq.com — skrap og forhåndsvis'
            }
            inputmode="url"
            autocomplete="off"
            disabled={!ready()}
          />
          <button
            type="submit"
            class="dashboard-knowledge-composer__submit"
            disabled={!canSubmitUrl()}
            aria-label={mode() === 'crawl' ? 'Start crawl' : mode() === 'products' ? 'Hent produkter' : 'Skrap side'}
            title={mode() === 'crawl' ? 'Start crawl' : mode() === 'products' ? 'Hent produkter' : 'Skrap side'}
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
                aria-label="Nettleserprofil"
              >
                <option value={isolatedProfileChoice}>Isolert</option>
                <option value={newProfileChoice}>Ny profil</option>
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
              aria-label="Oppdater nettleserprofiler"
              title="Oppdater"
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
              aria-label="Sjekk nettleserprofil"
              title="Sjekk profil"
            >
              <Show when={!browserProfileProbing()} fallback={<Loader2 class="size-3.5 dashboard-xsearch-spin" />}>
                <ShieldCheck class="size-3.5" />
              </Show>
            </button>
            <Show when={browserProfileProbe()}>
              {(probe) => (
                <span class="dashboard-knowledge-composer__browser-profile-status">
                  <ShieldCheck class="size-3.5" />
                  {profileProbeSummary(probe())}
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
              <span>Sider</span>
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
              <span>Agentstyrt workflow</span>
            </label>
            <button
              type="button"
              class="dashboard-knowledge-composer__discover"
              onClick={() => void discoverPages()}
              disabled={!canDiscover()}
              title="Oppdag sidene på nettstedet og velg hvilke som skal crawles"
            >
              <Show when={!discovering()} fallback={<Loader2 class="size-3.5 dashboard-xsearch-spin" />}>
                <ListChecks class="size-3.5" />
              </Show>
              Velg sider
            </button>
          </div>
        </Show>
      </form>

      <Show when={props.previewCollapsed ? null : preview()} keyed>
        {(current) => (
          <ScrapePreviewPanel
            adding={adding()}
            browserBusy={browserBusy()}
            onBrowserAction={(action) => void performBrowserAction(action)}
            onAdd={(markdown, allSelected) => void addToKnowledge(markdown, allSelected)}
            onDiscard={clearPreview}
            preview={current}
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
          Last opp filer
        </button>
        <input
          ref={(element) => { fileInputRef = element }}
          type="file"
          multiple
          class="sr-only"
          aria-label="Last opp dokumenter"
          onChange={(event) => {
            void uploadFiles(event.currentTarget.files)
            event.currentTarget.value = ''
          }}
        />
        <A href="/knowledge" class="dashboard-knowledge-composer__link">
          Åpne kunnskapsbase
          <ArrowRight class="size-3.5" />
        </A>
      </div>

      <Show when={!ready() && ctx.loading}>
        <p class="dashboard-knowledge-composer__status">Kobler til arbeidsområdet…</p>
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
  const running = () => !isTerminal(props.job.status)
  const failed = () => /fail|error|cancel/.test(props.job.status.toLowerCase())
  const kindLabel = () => {
    if (props.job.kind === 'crawl') return props.job.agentMode ? 'Agent crawl' : 'Crawl'
    return props.job.kind === 'upload' ? 'Opplasting' : 'Lenke'
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
          {kindLabel()} · {props.job.error ?? (running() ? props.job.detail : statusLabel(props.job.status))}
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

function statusLabel(status: string): string {
  const normalized = status.trim().toLowerCase()
  if (normalized === 'completed' || normalized === 'complete' || normalized === 'succeeded') return 'Ferdig indeksert'
  if (normalized === 'failed' || normalized === 'error') return 'Mislyktes'
  if (normalized === 'cancelled') return 'Avbrutt'
  return status
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

function profileProbeSummary(probe: BrowserProfileRestoreProbe): string {
  const stored = probe.cookies_count + probe.local_storage_count + probe.session_storage_count + probe.indexed_db_count
  return probe.restorable ? `${stored} lagrede signaler` : 'Tom profil'
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
