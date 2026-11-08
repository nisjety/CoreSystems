import {
  Activity,
  AlertTriangle,
  ArrowUpRight,
  CalendarClock,
  CheckCircle2,
  FileText,
  FlaskConical,
  Globe,
  ListChecks,
  RefreshCw,
  ShieldCheck,
  Telescope,
  Zap,
  type LucideProps,
} from '@/shared/icons'
import { createEffect, createMemo, createSignal, For, Show, type Component } from 'solid-js'
import {
  createIngestionRun,
  createIngestionSchedule,
  createIngestionSource,
  deleteIngestionSource,
  getIngestionEvidence,
  getIngestionUsage,
  getScheduleCapabilities,
  listIngestionProfiles,
  listIngestionRuns,
  listIngestionSchedules,
  listIngestionSources,
  NO_SCHEDULE_CAPABILITIES,
  runIngestionScheduleAction,
  type CreditUsage,
  type EvidenceTimeline,
  type ExtractItem,
  type ProfilePayload,
  type RunItem,
  type ScheduleCapabilities,
  type SchedulePauseAction,
  type ScheduleItem,
  type ScheduleStateAction,
  type SourceCreateInput,
  type SourcePayload,
} from '@/shared/api/ingestions-client'
import { getSessionContext } from '@/shared/api/auth-client'
import { discoverCrawlPages, scrapeUrl, startCrawl } from '@/shared/api/knowledge-client'
import { listMonitors, type ChangeMonitor } from '@/shared/api/monitoring-client'
import { runWebSearch } from '@/shared/api/search-client'
import { executeAction } from '@/shared/actions/action-client'
import type { ActionActor } from '@/shared/actions/types'
import { translateApiError, useI18n } from '@/shared/i18n'
import { cn } from '@/shared/lib/cn'
import { Button } from '@/shared/ui/Button'
import { VerevonSegmented, VerevonSegmentedButton } from '@/shared/ui/verevon/VerevonSegmented'
import {
  agentInput,
  boundedInt,
  buildActions,
  crawlRequestFromState,
  defaultPlaygroundState,
  isAsyncEndpoint,
  missingInput,
  parseSchema,
  scrapeRequestFromState,
  searchFiltersFromState,
  splitList,
  type PlaygroundState,
} from '@/features/ingestions/lib/playground'
import { countRunsByGroup, defaultRunFilter, runProgressPercent, type RunFilter, type RunStatusFilter } from '@/features/ingestions/lib/runs'
import { toScrapeView } from '@/features/ingestions/lib/scrape-view'
import { jobControlMessage, playgroundErrorMessage } from '@/features/ingestions/lib/errors'
import { IngestionsPlayground } from '@/features/ingestions/components/IngestionsPlayground'
import {
  IngestionsPlaygroundResult,
  type IngestState,
  type PlaygroundResult,
} from '@/features/ingestions/components/IngestionsPlaygroundResult'
import { IngestionsRunsView } from '@/features/ingestions/components/IngestionsRunsView'
import { IngestionsSchedulesView, type ScheduleFormState } from '@/features/ingestions/components/IngestionsSchedulesView'
import { IngestionsMonitoringView } from '@/features/ingestions/components/IngestionsMonitoringView'
import { DocumentReadPanel } from '@/features/ingestions/components/DocumentReadPanel'
import { ProfilesPanel, SourcesPanel, type SourceFormState } from '@/features/ingestions/components/IngestionsSourcesView'
import { StatusBadge, kindLabel, relativeTime } from '@/features/ingestions/components/ingestions-ui'
import '@/features/ingestions/components/ingestions.css'

type IngestionView = 'playground' | 'documents' | 'runs' | 'schedules' | 'monitoring' | 'sources' | 'profiles'

const views: Array<{ id: IngestionView; icon: Component<LucideProps> }> = [
  { id: 'playground', icon: FlaskConical },
  { id: 'documents', icon: FileText },
  { id: 'runs', icon: ListChecks },
  { id: 'schedules', icon: CalendarClock },
  { id: 'monitoring', icon: Telescope },
  { id: 'sources', icon: Globe },
  { id: 'profiles', icon: ShieldCheck },
]

function viewLabel(id: IngestionView, tr: (no: string, en: string) => string) {
  switch (id) {
    case 'playground':
      return tr('Lekeplass', 'Playground')
    case 'documents':
      return tr('Dokumenter', 'Documents')
    case 'runs':
      return tr('Kjøringer', 'Runs')
    case 'schedules':
      return tr('Tidsplaner', 'Schedules')
    case 'monitoring':
      return tr('Overvåking', 'Monitoring')
    case 'sources':
      return tr('Kilder', 'Sources')
    case 'profiles':
      return tr('Profiler', 'Profiles')
  }
}

export default function VerevonIngestionsPage() {
  const i18n = useI18n()
  const tr = i18n.tr
  const [activeView, setActiveView] = createSignal<IngestionView>('playground')
  const [loading, setLoading] = createSignal(false)
  const [error, setError] = createSignal<string | null>(null)
  const [runs, setRuns] = createSignal<RunItem[]>([])
  const [schedules, setSchedules] = createSignal<ScheduleItem[]>([])
  const [scheduleCapabilities, setScheduleCapabilities] = createSignal<ScheduleCapabilities>(NO_SCHEDULE_CAPABILITIES)
  const [sources, setSources] = createSignal<SourcePayload | null>(null)
  const [profiles, setProfiles] = createSignal<ProfilePayload | null>(null)
  const [monitors, setMonitors] = createSignal<ChangeMonitor[] | null>([])
  const [usage, setUsage] = createSignal<CreditUsage | null>(null)
  const [selectedRunId, setSelectedRunId] = createSignal<string | null>(null)
  const [runEvidence, setRunEvidence] = createSignal<EvidenceTimeline | null>(null)
  const [evidenceLoading, setEvidenceLoading] = createSignal(false)
  const [controlError, setControlError] = createSignal<string | null>(null)
  const [runFilter, setRunFilter] = createSignal<RunFilter>(defaultRunFilter)
  const [playground, setPlayground] = createSignal<PlaygroundState>(defaultPlaygroundState)
  const [playgroundResult, setPlaygroundResult] = createSignal<PlaygroundResult | null>(null)
  const [playgroundError, setPlaygroundError] = createSignal<string | null>(null)
  const [playgroundRunning, setPlaygroundRunning] = createSignal(false)
  const [ingestState, setIngestState] = createSignal<IngestState>('idle')
  const [orgId, setOrgId] = createSignal('')
  const [userId, setUserId] = createSignal('')
  const [scheduleForm, setScheduleForm] = createSignal<ScheduleFormState>({
    name: '',
    kind: 'crawl',
    targetUrl: '',
    cron: '0 7 * * *',
  })
  const [sourceForm, setSourceForm] = createSignal<SourceFormState>({
    name: '',
    url: '',
    kind: 'crawl',
    monitor: false,
  })
  const [sourcePending, setSourcePending] = createSignal(false)
  const [deletingSourceId, setDeletingSourceId] = createSignal<string | null>(null)

  const actor = (): ActionActor => ({ orgId: orgId(), type: 'human', userId: userId() })
  const selectedRun = createMemo(() => runs().find((run) => run.id === selectedRunId()) ?? null)
  const runCounts = createMemo(() => countRunsByGroup(runs()))
  const successRate = createMemo(() => {
    const finished = runCounts().succeeded + runCounts().failed
    return finished === 0 ? null : Math.round((runCounts().succeeded / finished) * 100)
  })
  const sourceCount = createMemo(() => (sources()?.quarrySources?.length ?? 0) + (sources()?.integrations?.length ?? 0))
  const usageLabel = createMemo(() => {
    const current = usage()
    if (!current) return '—'
    const used = Math.round(current.creditsUsed * 10) / 10
    return current.creditsLimit ? `${used} / ${Math.round(current.creditsLimit)}` : `${used}`
  })

  async function loadWorkspace(signal?: AbortSignal) {
    setLoading(true)
    setError(null)
    // Monitors and usage are their own reads: a failure there must not blank the run list.
    void listMonitors(signal)
      .then((next) => {
        if (!signal?.aborted) setMonitors(next)
      })
      .catch(() => {
        if (!signal?.aborted) setMonitors(null)
      })
    void getIngestionUsage('7d', signal)
      .then((next) => {
        if (!signal?.aborted) setUsage(next)
      })
      .catch(() => {
        if (!signal?.aborted) setUsage(null)
      })
    try {
      const [runData, scheduleData, sourceData, profileData] = await Promise.all([
        listIngestionRuns(signal),
        listIngestionSchedules(signal),
        listIngestionSources(signal),
        listIngestionProfiles(signal),
      ])
      setRuns(runData)
      setSchedules(scheduleData)
      setSources(sourceData)
      setProfiles(profileData)
    } catch (caught) {
      if (signal?.aborted) return
      setError(
        translateApiError(caught, tr, {
          no: 'Kunne ikke laste inn arbeidsområdet for innhenting.',
          en: 'Could not load ingestion workspace.',
        }),
      )
    } finally {
      if (!signal?.aborted) setLoading(false)
    }
  }

  async function reloadMonitors() {
    try {
      setMonitors(await listMonitors())
    } catch {
      setMonitors(null)
    }
  }

  createEffect(
    () => undefined,
    () => {
      const controller = new AbortController()
      void loadWorkspace(controller.signal)
      // The knowledge routes (scrape, crawl, map) and the actions are scoped to the
      // session's own org; the gateway checks membership, so this is the right org.
      void getSessionContext()
        .then((ctx) => {
          if (controller.signal.aborted) return
          setOrgId(ctx.orgId ?? ctx.orgs[0]?.id ?? '')
          setUserId(ctx.userId)
        })
        .catch(() => undefined)
      return () => controller.abort()
    },
  )

  // Read once on mount: what the Schedules surface may offer (WS15.S0). Never polled.
  createEffect(
    () => undefined,
    () => {
      const controller = new AbortController()
      void getScheduleCapabilities(controller.signal).then((next) => {
        if (controller.signal.aborted) return
        setScheduleCapabilities(next)
        // Keep the composer on a kind this installation can actually run.
        const fallbackKind = next.kinds[0]
        setScheduleForm((current) =>
          fallbackKind === undefined || next.kinds.includes(current.kind)
            ? current
            : { ...current, kind: fallbackKind },
        )
      })
      return () => controller.abort()
    },
  )

  // Live polling while anything is in flight, so progress moves without a click.
  createEffect(
    () => runCounts().active > 0,
    (hasActive) => {
      if (!hasActive) return
      const id = setInterval(() => {
        void loadWorkspace()
      }, 3000)
      return () => clearInterval(id)
    },
  )

  createEffect(
    () => ({ runId: selectedRunId(), run: selectedRun() }),
    ({ runId, run }) => {
      if (!runId) {
        setRunEvidence(null)
        return
      }
      // The evidence endpoint only accepts Temporal run ids (`run_…`); the list
      // rows are keyed by durable job id (`job_…`), which 400s there. Prefer the
      // row's runId and fall back to the selection id (scrape runs' id IS a run id).
      const evidenceId = run?.runId ?? runId
      const controller = new AbortController()
      setEvidenceLoading(true)
      getIngestionEvidence(evidenceId, controller.signal)
        .then(setRunEvidence)
        .catch(() => {
          if (!controller.signal.aborted) setRunEvidence(null)
        })
        .finally(() => {
          if (!controller.signal.aborted) setEvidenceLoading(false)
        })
      return () => controller.abort()
    },
  )

  function patchPlayground(patch: Partial<PlaygroundState>) {
    setPlayground((current) => ({ ...current, ...patch }))
  }

  function missingMessage(missing: NonNullable<ReturnType<typeof missingInput>>) {
    switch (missing) {
      case 'query':
        return tr('Skriv inn noe å søke etter.', 'Enter something to search for.')
      case 'urls':
        return tr('Legg inn minst én URL.', 'Add at least one URL.')
      case 'goal':
        return tr('Beskriv hva agenten skal oppnå.', 'Describe what the agent should achieve.')
      case 'url':
        return tr('Angi en mål-URL.', 'Enter a target URL.')
      case 'ai_schema':
        return tr('Skriv et JSON-skjema (et objekt) for det strukturerte svaret.', 'Write a JSON schema (an object) for the structured answer.')
      case 'ai_question':
        return tr('Skriv spørsmålet du vil stille til siden.', 'Write the question you want to ask the page.')
      case 'ai_query':
        return tr('Skriv hva som skal finnes på siden.', 'Write what should be found on the page.')
    }
  }

  async function submitPlayground(override?: PlaygroundState) {
    const state = override ?? playground()
    const missing = missingInput(state)
    if (missing) {
      setPlaygroundError(missingMessage(missing))
      return
    }
    if (state.endpoint === 'scrape') {
      const steps = buildActions(state.actions)
      if (!steps.ok) {
        setPlaygroundError(
          tr(
            `Steg ${steps.index + 1} mangler ${actionFieldLabel(steps.field, 'no')}.`,
            `Step ${steps.index + 1} is missing ${actionFieldLabel(steps.field, 'en')}.`,
          ),
        )
        return
      }
    }
    if (state.endpoint === 'extract' && state.prompt.trim().length > 10_000) {
      setPlaygroundError(tr('Instruksen kan være høyst 10 000 tegn.', 'The instruction may be at most 10,000 characters.'))
      return
    }
    let schema: ReturnType<typeof parseSchema> | null = null
    if (state.endpoint === 'extract') {
      schema = parseSchema(state.schema)
      if (!schema.ok) {
        setPlaygroundError(
          schema.reason === 'not_object'
            ? tr('Skjemaet må være et JSON-objekt.', 'The schema must be a JSON object.')
            : tr('Skjemaet er ikke gyldig JSON.', 'The schema is not valid JSON.'),
        )
        return
      }
    }
    const needsOrg = state.endpoint === 'scrape' || state.endpoint === 'crawl' || state.endpoint === 'map' || state.endpoint === 'agent'
    if (needsOrg && !orgId()) {
      setPlaygroundError(tr('Fant ingen organisasjon for økten. Last siden på nytt.', 'No organization found for this session. Reload the page.'))
      return
    }
    setPlaygroundError(null)
    setPlaygroundRunning(true)
    setIngestState('idle')
    // Captured before the request so a failure-path reconciliation can tell a
    // run that already existed apart from one this attempt just created.
    const attemptStartedAt = Date.now()
    try {
      switch (state.endpoint) {
        case 'scrape': {
          const raw = await scrapeUrl(orgId(), scrapeRequestFromState(state))
          setPlaygroundResult({ kind: 'scrape', view: toScrapeView(raw, state.url.trim()), raw })
          break
        }
        case 'extract': {
          const schemaObject = schema?.ok ? schema.schema : null
          const prompt = state.prompt.trim()
          const created = await createIngestionRun({
            kind: 'extract',
            url: state.url.trim(),
            ...(schemaObject ? { schema: schemaObject } : {}),
            ...(prompt ? { prompt } : {}),
          })
          const items = created.evidence?.kind === 'extract' ? (created.evidence.results as ExtractItem[]) : []
          setPlaygroundResult({ kind: 'extract', url: state.url.trim(), items, schemaUsed: schemaObject !== null, raw: created })
          break
        }
        case 'map': {
          const discovery = await discoverCrawlPages(orgId(), {
            url: state.url.trim(),
            ...(state.mapSearch.trim() ? { search: state.mapSearch.trim() } : {}),
            limit: boundedInt(state.mapLimit, 1, 1000) ?? 100,
            includeSubdomains: state.allowSubdomains,
          })
          setPlaygroundResult({ kind: 'map', url: discovery.url || state.url.trim(), pages: discovery.pages })
          break
        }
        case 'search': {
          const payload = await runWebSearch({
            query: state.query.trim(),
            limit: boundedInt(state.searchLimit, 1, 20) ?? 5,
            includeAnswer: state.searchIncludeAnswer,
            filters: searchFiltersFromState(state),
          })
          setPlaygroundResult({
            kind: 'search',
            query: state.query.trim(),
            results: payload.results ?? [],
            answer: payload.answer ?? null,
            citations: payload.citations ?? [],
            answerRequested: state.searchIncludeAnswer,
            answerStatus: payload.answerStatus ?? null,
            answerReason: payload.answerReason ?? null,
          })
          break
        }
        case 'agent': {
          const execution = await executeAction('ingestions.start_agent_run', actor(), agentInput(state))
          const result = (execution.result ?? {}) as Record<string, unknown>
          setPlaygroundResult({
            kind: 'agent',
            url: state.url.trim(),
            goal: state.goal.trim(),
            runId: String(result.run_id ?? ''),
            threadId: String(result.thread_id ?? ''),
            sessionId: String(result.session_id ?? ''),
          })
          break
        }
        case 'crawl': {
          const job = await startCrawl(orgId(), crawlRequestFromState(state))
          setPlaygroundResult({
            kind: 'job',
            endpoint: 'crawl',
            id: job.id,
            target: state.url.trim(),
            acceptedAt: job.acceptedAt ?? job.createdAt ?? '',
          })
          void loadWorkspace()
          break
        }
        case 'batch': {
          const urls = splitList(state.urls)
          const created = await createIngestionRun({ kind: 'batch', urls })
          setPlaygroundResult({
            kind: 'job',
            endpoint: 'batch',
            id: created.run.id,
            target: created.run.target || `${urls.length} URLs`,
            acceptedAt: created.run.createdAt,
          })
          void loadWorkspace()
          break
        }
      }
    } catch (caught) {
      const fallback = { no: 'Kjøringen kunne ikke fullføres.', en: 'The run could not be completed.' }
      if (!isAsyncEndpoint(state.endpoint)) {
        setPlaygroundResult(null)
        setPlaygroundError(playgroundErrorMessage(caught, tr, fallback, { endpoint: state.endpoint, requested: state.formats }))
        return
      }
      // A durable hand-off can fail (e.g. a transient 502) after the job was
      // already created upstream. Re-fetch and check whether a matching run now
      // exists before asserting failure, instead of trusting the network error
      // alone. Best-effort match (no id survives a lost response): a crawl
      // matches its target URL, or its kind when the row carries no URL; either
      // kind must have been created no earlier than this attempt started (with a
      // small buffer for clock skew) so an older run cannot false-match.
      let reconciled = true
      let fresh: RunItem[] = []
      try {
        fresh = await listIngestionRuns()
        setRuns(fresh)
      } catch {
        reconciled = false
      }
      const target = state.endpoint === 'crawl' ? state.url.trim() : null
      const landed = reconciled
        ? fresh.find((run) => {
            if (new Date(run.createdAt).getTime() < attemptStartedAt - 10_000) return false
            if (run.kind !== state.endpoint) return false
            return target ? run.target === target || !/^https?:\/\//i.test(run.target) : true
          })
        : undefined
      if (landed) {
        setPlaygroundResult({
          kind: 'job',
          endpoint: state.endpoint === 'crawl' ? 'crawl' : 'batch',
          id: landed.id,
          target: landed.target,
          acceptedAt: landed.createdAt,
        })
      } else if (reconciled) {
        setPlaygroundResult(null)
        setPlaygroundError(playgroundErrorMessage(caught, tr, fallback, { endpoint: state.endpoint }))
      } else {
        setPlaygroundResult(null)
        setPlaygroundError(
          tr(
            'Vi fikk ikke bekreftet om kjøringen ble startet. Vent litt før du prøver på nytt.',
            "We couldn't confirm whether the run was started. Please wait a moment before trying again.",
          ),
        )
      }
    } finally {
      setPlaygroundRunning(false)
    }
  }

  function actionFieldLabel(field: 'value' | 'selector' | 'text' | 'key' | 'ms' | 'pixels', locale: 'no' | 'en') {
    const labels = {
      value: { no: 'verdi', en: 'a value' },
      selector: { no: 'CSS-selektor', en: 'a CSS selector' },
      text: { no: 'tekst', en: 'text' },
      key: { no: 'tast', en: 'a key' },
      ms: { no: 'millisekunder', en: 'milliseconds' },
      pixels: { no: 'piksler', en: 'pixels' },
    }
    return labels[field][locale]
  }

  /** `knowledge.scrape_url`: the same scrape again with `ingest: true`, so the page lands in Kunnskap. */
  async function ingestScrape() {
    const result = playgroundResult()
    if (result?.kind !== 'scrape' || !orgId()) return
    setIngestState('sending')
    try {
      // Ingest the page itself; a screenshot or PDF capture adds nothing to Kunnskap.
      const current = playground()
      const formats = current.formats.filter((format) => format !== 'screenshot' && format !== 'pdf')
      await scrapeUrl(orgId(), scrapeRequestFromState({ ...current, formats, url: result.view.requestedUrl }, true))
      setIngestState('sent')
    } catch {
      setIngestState('failed')
    }
  }

  function scrapeFromResult(url: string) {
    const next: PlaygroundState = { ...playground(), endpoint: 'scrape', url }
    setPlayground(next)
    void submitPlayground(next)
  }

  function batchFromResult(urls: string[]) {
    const next: PlaygroundState = { ...playground(), endpoint: 'batch', urls: urls.join('\n') }
    setPlayground(next)
    void submitPlayground(next)
  }

  function openRun(id: string) {
    setRunFilter(defaultRunFilter)
    if (id) setSelectedRunId(id)
    setActiveView('runs')
  }

  function rerun(run: RunItem) {
    const endpoint: PlaygroundState['endpoint'] = run.kind === 'crawl' || run.kind === 'extract' ? run.kind : 'scrape'
    setPlayground((current) => ({ ...current, endpoint, url: run.target }))
    setPlaygroundResult(null)
    setPlaygroundError(null)
    setActiveView('playground')
  }

  /** Pause, resume or cancel a crawl or batch job through the `ingestions.job_control` action. */
  async function controlJob(jobId: string, action: 'pause' | 'resume' | 'cancel') {
    setControlError(null)
    try {
      await executeAction('ingestions.job_control', actor(), { jobId, action })
      await loadWorkspace()
    } catch (caught) {
      setControlError(jobControlMessage(caught, tr))
    }
  }

  function showRuns(status: RunStatusFilter) {
    setRunFilter({ ...defaultRunFilter, status })
    setActiveView('runs')
  }

  async function createSchedule() {
    setError(null)
    if (!scheduleCapabilities().kinds.includes(scheduleForm().kind)) {
      setError(tr('Denne tidsplantypen er ikke aktivert på denne installasjonen.', 'This schedule type is not enabled on this installation.'))
      return
    }
    try {
      const form = scheduleForm()
      await createIngestionSchedule({
        name: form.name,
        kind: form.kind,
        targetUrl: form.targetUrl,
        cron: form.cron,
      })
      setScheduleForm((current) => ({ ...current, name: '', targetUrl: '' }))
      await loadWorkspace()
    } catch (caught) {
      setError(translateApiError(caught, tr, { no: 'Tidsplanen kunne ikke opprettes.', en: 'Schedule could not be created.' }))
    }
  }

  async function runScheduleAction(action: SchedulePauseAction | ScheduleStateAction, scheduleId: string) {
    setError(null)
    try {
      await runIngestionScheduleAction(action, scheduleId)
      await loadWorkspace()
    } catch (caught) {
      setError(translateApiError(caught, tr, { no: 'Tidsplanhandlingen mislyktes.', en: 'Schedule action failed.' }))
    }
  }

  async function createSource() {
    setError(null)
    const form = sourceForm()
    if (!form.name.trim() || !form.url.trim()) {
      setError(tr('Kildenavn og URL er påkrevd.', 'A source name and URL are required.'))
      return
    }
    setSourcePending(true)
    try {
      const input: SourceCreateInput = {
        name: form.name.trim(),
        url: form.url.trim(),
        kind: form.kind,
        monitor: form.monitor,
      }
      await createIngestionSource(input)
      setSourceForm((current) => ({ ...current, name: '', url: '' }))
      await loadWorkspace()
    } catch (caught) {
      setError(translateApiError(caught, tr, { no: 'Kilden kunne ikke opprettes.', en: 'Source could not be created.' }))
    } finally {
      setSourcePending(false)
    }
  }

  async function removeSource(id: string) {
    setError(null)
    setDeletingSourceId(id)
    try {
      await deleteIngestionSource(id)
      await loadWorkspace()
    } catch (caught) {
      // deleteIngestionSource can fail (e.g. a transient 502) after the
      // source was already durably removed server-side. Re-fetch and check
      // whether it's still listed before asserting failure, instead of
      // trusting the network error alone.
      let reconciled = true
      let stillPresent = false
      try {
        const fresh = await listIngestionSources()
        setSources(fresh)
        stillPresent = (fresh.quarrySources ?? []).some((source) => source.id === id)
      } catch {
        reconciled = false
      }
      if (reconciled && stillPresent) {
        setError(translateApiError(caught, tr, { no: 'Kilden kunne ikke fjernes.', en: 'Source could not be removed.' }))
      } else if (!reconciled) {
        setError(
          tr(
            'Vi fikk ikke bekreftet om kilden ble fjernet. Vent litt før du prøver på nytt.',
            "We couldn't confirm whether the source was removed. Please wait a moment before trying again.",
          ),
        )
      }
      // Else: confirmed removed — the re-fetch already synced the list.
    } finally {
      setDeletingSourceId(null)
    }
  }

  return (
    <div class="verevon-page-surface ingestions-page">
      <div class="ingestions-page__content">
        <header class="ingestions-header">
          <div class="ingestions-header__copy">
            <h1>{tr('Innhenting', 'Ingestions')}</h1>
            <p>
              {tr(
                'Kjør crawler og uttrekk, inspiser bevis, administrer gjentakende tidsplaner, og gi tiltrodde kilder tilbake til Kunnskap.',
                'Run crawls and extracts, inspect evidence, manage recurring schedules, and hand trusted sources back into Knowledge.',
              )}
            </p>
          </div>
          <div class="ingestions-header__actions">
            <Show when={runCounts().active > 0}>
              <span class="ingestions-live-pill">
                <span class="ingestions-live-dot" aria-hidden="true" />
                {tr(`${runCounts().active} aktive nå`, `${runCounts().active} running now`)}
              </span>
            </Show>
            <Button class="ingestions-refresh" onClick={() => void loadWorkspace()}>
              <RefreshCw class={cn('size-4', loading() && 'ingestions-spin')} strokeWidth={1.9} />
              {tr('Oppdater', 'Refresh')}
            </Button>
          </div>
        </header>

        <section class="ingestions-overview" aria-label={tr('Oversikt', 'Overview')}>
          <button type="button" class="ingestions-stat" aria-label={tr('Vis aktive', 'Show active')} onClick={() => showRuns('active')}>
            <span class="ingestions-stat__label">
              <Zap class="size-3.5" strokeWidth={2} />
              {tr('Aktive', 'Active')}
            </span>
            <strong>{runCounts().active}</strong>
            <small>{tr(`av ${runs().length} totalt`, `of ${runs().length} total`)}</small>
          </button>
          <button type="button" class="ingestions-stat ingestions-stat--good" aria-label={tr('Vis fullførte', 'Show succeeded')} onClick={() => showRuns('succeeded')}>
            <span class="ingestions-stat__label">
              <CheckCircle2 class="size-3.5" strokeWidth={2} />
              {tr('Fullført', 'Succeeded')}
            </span>
            <strong>{runCounts().succeeded}</strong>
            <small>{successRate() === null ? tr('ingen ferdige ennå', 'none finished yet') : tr(`${successRate()} % suksess`, `${successRate()}% success`)}</small>
          </button>
          <button type="button" class="ingestions-stat ingestions-stat--bad" aria-label={tr('Vis feilede', 'Show failed')} onClick={() => showRuns('failed')}>
            <span class="ingestions-stat__label">
              <AlertTriangle class="size-3.5" strokeWidth={2} />
              {tr('Feilet', 'Failed')}
            </span>
            <strong>{runCounts().failed}</strong>
            <small>{runCounts().failed > 0 ? tr('krever oppmerksomhet', 'needs attention') : tr('alt i orden', 'all clear')}</small>
          </button>
          <button type="button" class="ingestions-stat" aria-label={tr('Vis planlagte jobber', 'Show scheduled jobs')} onClick={() => setActiveView('monitoring')}>
            <span class="ingestions-stat__label">
              <Activity class="size-3.5" strokeWidth={2} />
              {tr('Planlagt', 'Scheduled')}
            </span>
            <strong>{schedules().length + (monitors()?.length ?? 0)}</strong>
            <small>{tr(`${monitors()?.length ?? 0} overvåkere · ${sourceCount()} kilder`, `${monitors()?.length ?? 0} monitors · ${sourceCount()} sources`)}</small>
          </button>
          <div class="ingestions-stat" aria-label={tr('Kreditter siste 7 dager', 'Credits, last 7 days')}>
            <span class="ingestions-stat__label">
              <Activity class="size-3.5" strokeWidth={2} />
              {tr('Kreditter · 7 d', 'Credits · 7 d')}
            </span>
            <strong>{usageLabel()}</strong>
            <small>{usage()?.creditsLimit ? tr(`${Math.round(usage()?.utilizationPercent ?? 0)} % brukt`, `${Math.round(usage()?.utilizationPercent ?? 0)}% used`) : tr('ingen tak satt', 'no ceiling set')}</small>
          </div>
        </section>

        <VerevonSegmented class="ingestions-tabs" aria-label={tr('Innhentingsvisninger', 'Ingestion views')}>
          <For each={views}>
            {(view) => {
              const Icon = view.icon
              return (
                <VerevonSegmentedButton class="ingestions-tab" selected={activeView() === view.id} onClick={() => setActiveView(view.id)}>
                  <Icon class="size-4" strokeWidth={1.9} />
                  {viewLabel(view.id, tr)}
                  <Show when={view.id === 'runs' && runCounts().active > 0}>
                    <span class="ingestions-count-pill">{runCounts().active}</span>
                  </Show>
                </VerevonSegmentedButton>
              )
            }}
          </For>
        </VerevonSegmented>

        <Show when={error()}>
          {(message) => (
            <section class="ingestions-alert" role="alert">
              {message()}
            </section>
          )}
        </Show>

        <Show when={activeView() === 'playground'}>
          <section class="ingestions-playground">
            <IngestionsPlayground state={playground()} onChange={patchPlayground} onSubmit={() => void submitPlayground()} submitting={playgroundRunning()} />
            <IngestionsPlaygroundResult
              result={playgroundResult()}
              running={playgroundRunning()}
              error={playgroundError()}
              ingestState={ingestState()}
              onIngest={() => void ingestScrape()}
              onScrapeUrl={scrapeFromResult}
              onBatchUrls={batchFromResult}
              onOpenRun={openRun}
            />
          </section>
          <RecentRuns runs={runs()} onOpenRun={openRun} onShowAll={() => showRuns('all')} />
        </Show>

        <Show when={activeView() === 'documents'}>
          <DocumentReadPanel />
        </Show>

        <Show when={activeView() === 'runs'}>
          <IngestionsRunsView
            runs={runs()}
            filter={runFilter()}
            onFilterChange={(patch) => setRunFilter((current) => ({ ...current, ...patch }))}
            selectedRunId={selectedRunId()}
            onSelectRun={setSelectedRunId}
            evidence={runEvidence()}
            evidenceLoading={evidenceLoading()}
            onRerun={rerun}
            onControl={controlJob}
            controlError={controlError()}
            onIngestStarted={() => void loadWorkspace()}
          />
        </Show>

        <Show when={activeView() === 'schedules'}>
          <IngestionsSchedulesView
            form={scheduleForm()}
            kinds={scheduleCapabilities().kinds}
            canTrigger={scheduleCapabilities().trigger}
            schedules={schedules()}
            onFormChange={(patch) => setScheduleForm((current) => ({ ...current, ...patch }))}
            onSubmit={createSchedule}
            onAction={runScheduleAction}
          />
        </Show>

        <Show when={activeView() === 'monitoring'}>
          <IngestionsMonitoringView monitors={monitors()} onMonitorsChanged={reloadMonitors} orgId={orgId()} />
        </Show>

        <Show when={activeView() === 'sources'}>
          <SourcesPanel
            sources={sources()}
            form={sourceForm()}
            onFormChange={(patch) => setSourceForm((current) => ({ ...current, ...patch }))}
            onCreate={createSource}
            creating={sourcePending()}
            deletingId={deletingSourceId()}
            onDelete={removeSource}
          />
        </Show>

        <Show when={activeView() === 'profiles'}>
          <ProfilesPanel profiles={profiles()} />
        </Show>
      </div>
    </div>
  )
}

function RecentRuns(props: { runs: RunItem[]; onOpenRun: (id: string) => void; onShowAll: () => void }) {
  const i18n = useI18n()
  const tr = i18n.tr
  return (
    <section class="verevon-panel ingestions-card ingestions-recent">
      <div class="ingestions-card__header">
        <div>
          <h2>{tr('Nylige kjøringer', 'Recent runs')}</h2>
          <p>{tr('Varige crawl-, uttrekk-, batch-, søk- og agentjobber fra Quarry.', 'Durable crawl, extract, batch, search, and agent jobs from Quarry.')}</p>
        </div>
        <div class="ingestions-actions-row">
          <button type="button" class="ingestions-inline-link" onClick={() => props.onShowAll()}>
            {tr('Se alle', 'See all')}
          </button>
          <a href="/knowledge" link class="ingestions-inline-link">
            {tr('Åpne Kunnskap', 'Open Knowledge')}
            <ArrowUpRight class="size-4" strokeWidth={1.9} />
          </a>
        </div>
      </div>
      <ul class="ingestions-recent__list">
        <For each={props.runs.slice(0, 5)} fallback={<li class="ingestions-empty-cell">{tr('Ingen varige kjøringer ennå.', 'No durable runs yet.')}</li>}>
          {(run) => {
            const percent = () => runProgressPercent(run)
            return (
              <li>
                <button type="button" class="ingestions-recent__row" onClick={() => props.onOpenRun(run.id)}>
                  <StatusBadge status={run.status} />
                  <span class="ingestions-recent__kind">{kindLabel(run.kind, tr)}</span>
                  <span class="ingestions-truncate ingestions-recent__target">{run.target}</span>
                  <Show when={percent() !== null}>
                    <span class="ingestions-progress" aria-hidden="true">
                      <span style={{ width: `${percent()}%` }} />
                    </span>
                  </Show>
                  <span class="ingestions-muted-cell">{relativeTime(run.createdAt, i18n)}</span>
                </button>
              </li>
            )
          }}
        </For>
      </ul>
    </section>
  )
}
