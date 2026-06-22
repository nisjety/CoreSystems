import { A } from '@solidjs/router'
import {
  ArrowUpRight,
  CalendarClock,
  Database,
  FileSearch,
  Globe,
  Play,
  RefreshCw,
  ScanSearch,
  ShieldCheck,
  Telescope,
  TimerReset,
  type LucideProps,
} from 'lucide-solid'
import {
  createEffect,
  createMemo,
  createResource,
  createSignal,
  For,
  onCleanup,
  onMount,
  Show,
  type Component,
} from 'solid-js'
import {
  createIngestionRun,
  createIngestionSchedule,
  getIngestionEvidence,
  listIngestionProfiles,
  listIngestionRuns,
  listIngestionSchedules,
  listIngestionSources,
  runIngestionScheduleAction,
  type EvidenceTimeline,
  type ManualEvidence,
  type ProfilePayload,
  type RunCreateRequest,
  type RunItem,
  type ScheduleItem,
  type SourcePayload,
} from '@/shared/api/ingestions-client'
import {
  checkUrlNow,
  getChangeHistory,
  type BaselineSnapshot,
  type ChangeRecord,
} from '@/shared/api/monitoring-client'
import { cn } from '@/shared/lib/cn'
import { Button } from '@/shared/ui/Button'
import { VelionInput } from '@/shared/ui/velion/VelionInput'
import { VelionSegmented, VelionSegmentedButton } from '@/shared/ui/velion/VelionSegmented'
import { VelionSelect } from '@/shared/ui/velion/VelionSelect'
import { VelionTextarea } from '@/shared/ui/velion/VelionTextarea'

type IngestionView = 'runs' | 'schedules' | 'monitoring' | 'sources' | 'evidence' | 'profiles'

type RunFormState = {
  kind: string
  url: string
  urls: string
  prompt: string
}

type ScheduleFormState = {
  name: string
  kind: string
  targetUrl: string
  cron: string
}

const views: Array<{ id: IngestionView; label: string; icon: Component<LucideProps> }> = [
  { id: 'runs', label: 'Runs', icon: ScanSearch },
  { id: 'schedules', label: 'Schedules', icon: CalendarClock },
  { id: 'monitoring', label: 'Monitoring', icon: Telescope },
  { id: 'sources', label: 'Sources', icon: Globe },
  { id: 'evidence', label: 'Evidence', icon: FileSearch },
  { id: 'profiles', label: 'Profiles', icon: ShieldCheck },
]

export default function VelionIngestionsPage() {
  const [activeView, setActiveView] = createSignal<IngestionView>('runs')
  const [loading, setLoading] = createSignal(false)
  const [error, setError] = createSignal<string | null>(null)
  const [runs, setRuns] = createSignal<RunItem[]>([])
  const [schedules, setSchedules] = createSignal<ScheduleItem[]>([])
  const [sources, setSources] = createSignal<SourcePayload | null>(null)
  const [profiles, setProfiles] = createSignal<ProfilePayload | null>(null)
  const [selectedRunId, setSelectedRunId] = createSignal<string | null>(null)
  const [runEvidence, setRunEvidence] = createSignal<EvidenceTimeline | null>(null)
  const [manualEvidence, setManualEvidence] = createSignal<ManualEvidence | null>(null)
  const [runForm, setRunForm] = createSignal<RunFormState>({
    kind: 'scrape',
    url: '',
    urls: '',
    prompt: '',
  })
  const [scheduleForm, setScheduleForm] = createSignal<ScheduleFormState>({
    name: '',
    kind: 'crawl',
    targetUrl: '',
    cron: '0 7 * * *',
  })

  const selectedRun = createMemo(() => runs().find((run) => run.id === selectedRunId()) ?? null)

  async function loadWorkspace(signal?: AbortSignal) {
    setLoading(true)
    setError(null)
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
      setSelectedRunId((current) => current ?? runData[0]?.id ?? null)
    } catch (nextError) {
      if (signal?.aborted) return
      setError(nextError instanceof Error ? nextError.message : 'Could not load ingestion workspace.')
    } finally {
      if (!signal?.aborted) setLoading(false)
    }
  }

  onMount(() => {
    const controller = new AbortController()
    void loadWorkspace(controller.signal)
    onCleanup(() => controller.abort())
  })

  createEffect(() => {
    const runId = selectedRunId()
    if (!runId) {
      setRunEvidence(null)
      return
    }

    const controller = new AbortController()
    getIngestionEvidence(runId, controller.signal)
      .then(setRunEvidence)
      .catch(() => {
        if (!controller.signal.aborted) setRunEvidence(null)
      })
    onCleanup(() => controller.abort())
  })

  async function submitRun() {
    setError(null)
    try {
      const form = runForm()
      const payload: RunCreateRequest =
        form.kind === 'batch'
          ? {
              kind: 'batch',
              urls: form.urls
                .split(/\n|,/)
                .map((value) => value.trim())
                .filter(Boolean),
            }
          : {
              kind: form.kind,
              url: form.url.trim(),
              prompt: form.prompt.trim() || undefined,
            }
      const created = await createIngestionRun(payload)
      if (created.evidence) {
        setManualEvidence(created.evidence)
        setActiveView('evidence')
      } else {
        setSelectedRunId(created.run.id)
        setActiveView('runs')
      }
      await loadWorkspace()
    } catch (nextError) {
      setError(nextError instanceof Error ? nextError.message : 'Run could not be started.')
    }
  }

  async function createSchedule() {
    setError(null)
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
    } catch (nextError) {
      setError(nextError instanceof Error ? nextError.message : 'Schedule could not be created.')
    }
  }

  async function runScheduleAction(action: string, scheduleId: string) {
    setError(null)
    try {
      await runIngestionScheduleAction(action, scheduleId)
      await loadWorkspace()
    } catch (nextError) {
      setError(nextError instanceof Error ? nextError.message : 'Schedule action failed.')
    }
  }

  return (
    <div class="velion-page-surface ingestions-page">
      <div class="ingestions-page__content">
        <header class="ingestions-header">
          <div class="ingestions-header__copy">
            <h1>Ingestions</h1>
            <p>
              Run crawls and extracts, inspect evidence, manage recurring schedules, and hand trusted sources back into
              Knowledge.
            </p>
          </div>

          <div class="ingestions-header__actions">
            <VelionSegmented class="ingestions-tabs" aria-label="Ingestion views">
              <For each={views}>
                {(view) => {
                  const Icon = view.icon
                  return (
                    <VelionSegmentedButton
                      class="ingestions-tab"
                      selected={activeView() === view.id}
                      onClick={() => setActiveView(view.id)}
                    >
                      <Icon class="size-4" strokeWidth={1.9} />
                      {view.label}
                    </VelionSegmentedButton>
                  )
                }}
              </For>
            </VelionSegmented>
            <Button class="ingestions-refresh" onClick={() => void loadWorkspace()}>
              <RefreshCw class={cn('size-4', loading() && 'ingestions-spin')} strokeWidth={1.9} />
              Refresh
            </Button>
          </div>
        </header>

        <Show when={error()}>
          {(message) => (
            <section class="ingestions-alert" role="alert">
              {message()}
            </section>
          )}
        </Show>

        <Show when={activeView() === 'runs'}>
          <section class="ingestions-two-column ingestions-two-column--runs">
            <RunComposer
              form={runForm()}
              onFormChange={(patch) => setRunForm((current) => ({ ...current, ...patch }))}
              onSubmit={submitRun}
            />
            <RunsPanel
              runs={runs()}
              selectedRunId={selectedRunId()}
              onSelectRun={(runId) => {
                setSelectedRunId(runId)
                setActiveView('evidence')
              }}
            />
          </section>
        </Show>

        <Show when={activeView() === 'schedules'}>
          <section class="ingestions-two-column ingestions-two-column--runs">
            <ScheduleComposer
              form={scheduleForm()}
              onFormChange={(patch) => setScheduleForm((current) => ({ ...current, ...patch }))}
              onSubmit={createSchedule}
            />
            <SchedulesPanel schedules={schedules()} onAction={runScheduleAction} />
          </section>
        </Show>

        <Show when={activeView() === 'monitoring'}>
          <MonitoringPanel />
        </Show>

        <Show when={activeView() === 'sources'}>
          <SourcesPanel sources={sources()} />
        </Show>

        <Show when={activeView() === 'evidence'}>
          <EvidencePanel
            selectedRun={selectedRun()}
            selectedRunId={selectedRunId()}
            runEvidence={runEvidence()}
            manualEvidence={manualEvidence()}
            runs={runs()}
            onSelectRun={setSelectedRunId}
          />
        </Show>

        <Show when={activeView() === 'profiles'}>
          <ProfilesPanel profiles={profiles()} />
        </Show>
      </div>
    </div>
  )
}

function RunComposer(props: {
  form: RunFormState
  onFormChange: (patch: Partial<RunFormState>) => void
  onSubmit: () => Promise<void>
}) {
  const isBatch = () => props.form.kind === 'batch'

  return (
    <form
      class="velion-panel ingestions-card ingestions-composer"
      onSubmit={(event) => {
        event.preventDefault()
        void props.onSubmit()
      }}
    >
      <div>
        <h2>Start a run</h2>
        <p>Manual parity for scrape, crawl, extract, and batch execution.</p>
      </div>
      <label class="ingestions-field">
        Run type
        <VelionSelect value={props.form.kind} onChange={(event) => props.onFormChange({ kind: event.currentTarget.value })}>
          <option value="scrape">Scrape</option>
          <option value="crawl">Crawl</option>
          <option value="extract">Extract</option>
          <option value="batch">Batch</option>
        </VelionSelect>
      </label>
      <Show
        when={isBatch()}
        fallback={
          <label class="ingestions-field">
            Target URL
            <VelionInput
              value={props.form.url}
              onInput={(event) => props.onFormChange({ url: event.currentTarget.value })}
              placeholder="https://example.com"
            />
          </label>
        }
      >
        <label class="ingestions-field">
          URLs
          <VelionTextarea
            rows={6}
            value={props.form.urls}
            onInput={(event) => props.onFormChange({ urls: event.currentTarget.value })}
            placeholder={'https://example.com/pricing\nhttps://example.com/docs'}
          />
        </label>
      </Show>
      <Show when={props.form.kind === 'extract'}>
        <label class="ingestions-field">
          Extraction prompt
          <VelionTextarea
            rows={4}
            value={props.form.prompt}
            onInput={(event) => props.onFormChange({ prompt: event.currentTarget.value })}
            placeholder="Extract key support topics, contact channels, and pricing signals."
          />
        </label>
      </Show>
      <Button variant="primary" fullWidth type="submit">
        <Play class="size-4" strokeWidth={1.9} />
        Start run
      </Button>
    </form>
  )
}

function RunsPanel(props: {
  runs: RunItem[]
  selectedRunId: string | null
  onSelectRun: (runId: string) => void
}) {
  return (
    <section class="velion-panel ingestions-card ingestions-runs-panel">
      <div class="ingestions-card__header">
        <div>
          <h2>Recent runs</h2>
          <p>Durable crawl, extract, batch, search, and agent jobs from Quarry.</p>
        </div>
        <A href="/knowledge" class="ingestions-inline-link">
          Open Knowledge
          <ArrowUpRight class="size-4" strokeWidth={1.9} />
        </A>
      </div>
      <div class="ingestions-table-wrap">
        <table class="ingestions-table">
          <thead>
            <tr>
              <th>Kind</th>
              <th>Target</th>
              <th>Status</th>
              <th>Created</th>
              <th>Progress</th>
            </tr>
          </thead>
          <tbody>
            <For
              each={props.runs}
              fallback={
                <tr>
                  <td colspan="5" class="ingestions-empty-cell">
                    No durable runs yet.
                  </td>
                </tr>
              }
            >
              {(run) => (
                <tr
                  class={cn('ingestions-row', props.selectedRunId === run.id && 'ingestions-row--selected')}
                  onClick={() => props.onSelectRun(run.id)}
                >
                  <td class="ingestions-capitalize">{run.kind}</td>
                  <td class="ingestions-truncate">{run.target}</td>
                  <td>
                    <StatusBadge status={run.status} />
                  </td>
                  <td class="ingestions-muted-cell">{relativeTime(run.createdAt)}</td>
                  <td>
                    {run.progress.completed ?? 0}
                    {run.progress.total ? ` / ${run.progress.total}` : ''}
                  </td>
                </tr>
              )}
            </For>
          </tbody>
        </table>
      </div>
    </section>
  )
}

function ScheduleComposer(props: {
  form: ScheduleFormState
  onFormChange: (patch: Partial<ScheduleFormState>) => void
  onSubmit: () => Promise<void>
}) {
  return (
    <form
      class="velion-panel ingestions-card ingestions-composer"
      onSubmit={(event) => {
        event.preventDefault()
        void props.onSubmit()
      }}
    >
      <div>
        <h2>Create schedule</h2>
        <p>Recurring ingestion for sources that should stay fresh without manual runs.</p>
      </div>
      <label class="ingestions-field">
        Name
        <VelionInput
          value={props.form.name}
          onInput={(event) => props.onFormChange({ name: event.currentTarget.value })}
          placeholder="Docs crawl"
        />
      </label>
      <label class="ingestions-field">
        Kind
        <VelionSelect value={props.form.kind} onChange={(event) => props.onFormChange({ kind: event.currentTarget.value })}>
          <option value="crawl">Crawl</option>
          <option value="extract">Extract</option>
          <option value="search">Search</option>
          <option value="batch">Batch</option>
          <option value="agent">Agent</option>
        </VelionSelect>
      </label>
      <label class="ingestions-field">
        Target URL
        <VelionInput
          value={props.form.targetUrl}
          onInput={(event) => props.onFormChange({ targetUrl: event.currentTarget.value })}
          placeholder="https://example.com/docs"
        />
      </label>
      <label class="ingestions-field">
        Cron
        <VelionInput
          value={props.form.cron}
          onInput={(event) => props.onFormChange({ cron: event.currentTarget.value })}
          placeholder="0 7 * * *"
        />
      </label>
      <Button variant="primary" fullWidth type="submit">
        <CalendarClock class="size-4" strokeWidth={1.9} />
        Save schedule
      </Button>
    </form>
  )
}

function SchedulesPanel(props: {
  schedules: ScheduleItem[]
  onAction: (action: string, scheduleId: string) => Promise<void>
}) {
  return (
    <section class="velion-panel ingestions-card">
      <div>
        <h2>Schedule lifecycle</h2>
        <p>Pause, resume, trigger, and retire recurring jobs from one surface.</p>
      </div>
      <div class="ingestions-card-list">
        <For
          each={props.schedules}
          fallback={<div class="ingestions-empty-box">No recurring schedules yet.</div>}
        >
          {(schedule) => (
            <article class="ingestions-list-card">
              <div class="ingestions-list-card__main">
                <div class="ingestions-list-card__copy">
                  <div class="ingestions-title-row">
                    <h3>{schedule.name}</h3>
                    <StatusBadge status={schedule.status} />
                  </div>
                  <p>{schedule.target}</p>
                  <div class="ingestions-meta-row">
                    <span class="ingestions-capitalize">{schedule.kind}</span>
                    <span>{schedule.cron || schedule.scheduleAt || 'Manual cadence'}</span>
                    <span>Next: {schedule.nextRunAt ? relativeTime(schedule.nextRunAt) : 'Not scheduled'}</span>
                  </div>
                </div>
                <div class="ingestions-actions-row">
                  <Show
                    when={schedule.status === 'paused'}
                    fallback={
                      <Button size="xs" onClick={() => void props.onAction('pause_schedule', schedule.id)}>
                        <TimerReset class="size-4" strokeWidth={1.9} />
                        Pause
                      </Button>
                    }
                  >
                    <Button size="xs" onClick={() => void props.onAction('unpause_schedule', schedule.id)}>
                      <Play class="size-4" strokeWidth={1.9} />
                      Resume
                    </Button>
                  </Show>
                  <Button size="xs" onClick={() => void props.onAction('trigger_schedule', schedule.id)}>
                    <RefreshCw class="size-4" strokeWidth={1.9} />
                    Trigger
                  </Button>
                  <Button size="xs" onClick={() => void props.onAction('delete_schedule', schedule.id)}>
                    Retire
                  </Button>
                </div>
              </div>
            </article>
          )}
        </For>
      </div>
    </section>
  )
}

function MonitoringPanel() {
  // The URL the user is actively working with vs. the one we've committed to
  // (submitted) — history/last-checked load only for a committed, valid URL.
  const [urlInput, setUrlInput] = createSignal('')
  const [watchedUrl, setWatchedUrl] = createSignal<string | null>(null)
  const [checking, setChecking] = createSignal(false)
  const [lastResult, setLastResult] = createSignal<ChangeRecord | null>(null)
  const [error, setError] = createSignal<string | null>(null)

  // On-demand history for the committed URL. Empty until baselines accrue —
  // every row traces to a real edge baseline; nothing is synthesized.
  const [history, { refetch: refetchHistory }] = createResource(watchedUrl, (url) =>
    getChangeHistory(url),
  )

  async function check() {
    const candidate = urlInput().trim()
    if (!candidate) {
      setError('Enter a URL to check.')
      return
    }
    setError(null)
    setChecking(true)
    try {
      setWatchedUrl(candidate)
      const result = await checkUrlNow(candidate)
      setLastResult(result)
      // The check may have produced a new baseline upstream; refresh history.
      void refetchHistory()
    } catch (nextError) {
      setError(nextError instanceof Error ? nextError.message : 'Check could not be completed.')
    } finally {
      setChecking(false)
    }
  }

  return (
    <section class="ingestions-two-column ingestions-two-column--runs">
      <form
        class="velion-panel ingestions-card ingestions-composer"
        onSubmit={(event) => {
          event.preventDefault()
          void check()
        }}
      >
        <div>
          <h2>Check a page for changes</h2>
          <p>
            On-demand only: Velion fetches the page now, fingerprints its content, and compares it to the last
            baseline this workspace captured. Recurring at-scale monitoring is not yet available.
          </p>
        </div>
        <label class="ingestions-field">
          Page URL
          <VelionInput
            value={urlInput()}
            onInput={(event) => setUrlInput(event.currentTarget.value)}
            placeholder="https://example.com/pricing"
          />
        </label>
        <Button variant="primary" fullWidth type="submit" disabled={checking()}>
          <ScanSearch class={cn('size-4', checking() && 'ingestions-spin')} strokeWidth={1.9} />
          {checking() ? 'Checking…' : 'Check now'}
        </Button>

        <Show when={error()}>
          {(message) => (
            <div class="ingestions-alert" role="alert">
              {message()}
            </div>
          )}
        </Show>

        <Show when={lastResult()}>
          {(result) => (
            <div class="ingestions-code-card">
              <div class="ingestions-title-row">
                <Telescope class="size-4" strokeWidth={1.9} />
                <h3>Latest check</h3>
                <ChangeStatusBadge status={result().status} />
              </div>
              <div class="ingestions-meta-row ingestions-meta-row--spread">
                <span class="ingestions-truncate">{result().sourceUrl}</span>
                <span>{relativeTime(result().checkedAt)}</span>
              </div>
              <Show
                when={result().prevBaseline}
                fallback={<p>No earlier baseline — this is the first time this workspace has checked this page.</p>}
              >
                {(prev) => <p>Previous baseline captured {relativeTime(prev().capturedAt)}.</p>}
              </Show>
            </div>
          )}
        </Show>
      </form>

      <section class="velion-panel ingestions-card ingestions-runs-panel">
        <div class="ingestions-card__header">
          <div>
            <h2>Change history</h2>
            <p>Captured baselines for the page above, newest first. Empty until a check records a baseline.</p>
          </div>
        </div>
        <Show
          when={watchedUrl()}
          fallback={<div class="ingestions-empty-box">Run a check to see this page's baseline history.</div>}
        >
          <Show
            when={!history.loading}
            fallback={<div class="ingestions-empty-box">Loading history…</div>}
          >
            <div class="ingestions-table-wrap">
              <table class="ingestions-table">
                <thead>
                  <tr>
                    <th>Captured</th>
                    <th>Fingerprint</th>
                    <th>Run</th>
                  </tr>
                </thead>
                <tbody>
                  <For
                    each={history() ?? []}
                    fallback={
                      <tr>
                        <td colspan="3" class="ingestions-empty-cell">
                          No baselines recorded for this page yet.
                        </td>
                      </tr>
                    }
                  >
                    {(snapshot: BaselineSnapshot) => (
                      <tr class="ingestions-row">
                        <td class="ingestions-muted-cell">{relativeTime(snapshot.capturedAt)}</td>
                        <td class="ingestions-truncate">{shortFingerprint(snapshot.fingerprint)}</td>
                        <td class="ingestions-muted-cell">{snapshot.runId || '—'}</td>
                      </tr>
                    )}
                  </For>
                </tbody>
              </table>
            </div>
          </Show>
        </Show>
      </section>
    </section>
  )
}

function ChangeStatusBadge(props: { status: ChangeRecord['status'] }) {
  const tone = () => {
    if (props.status === 'changed' || props.status === 'unreachable') return 'ingestions-status--warning'
    if (props.status === 'new') return 'ingestions-status--info'
    return 'ingestions-status--success'
  }
  return <span class={cn('ingestions-status', tone())}>{props.status}</span>
}

function shortFingerprint(value: string) {
  // Fingerprints are like `blake3:<64 hex>`; show the algorithm + a short prefix.
  const colon = value.indexOf(':')
  const algo = colon >= 0 ? value.slice(0, colon) : ''
  const hash = colon >= 0 ? value.slice(colon + 1) : value
  const head = hash.slice(0, 12)
  return algo ? `${algo}:${head}…` : `${head}…`
}

function SourcesPanel(props: { sources: SourcePayload | null }) {
  return (
    <section class="ingestions-sources-grid">
      <div class="velion-panel ingestions-card">
        <div class="ingestions-card__header">
          <div>
            <h2>Connected sources</h2>
            <p>Integration-backed knowledge and website ingestion targets visible to Velion.</p>
          </div>
          <A href="/knowledge" class="ingestions-inline-link">
            Open Knowledge
            <ArrowUpRight class="size-4" strokeWidth={1.9} />
          </A>
        </div>
        <div class="ingestions-card-list">
          <For each={props.sources?.integrations ?? []}>
            {(source) => (
              <article class="ingestions-list-card">
                <div class="ingestions-title-row ingestions-title-row--spread">
                  <div>
                    <h3>{source.title}</h3>
                    <p>
                      {source.provider} - {source.detail}
                    </p>
                  </div>
                  <StatusBadge status={source.status} />
                </div>
                <Show when={source.capabilities.length > 0}>
                  <div class="ingestions-chip-row">
                    <For each={source.capabilities.slice(0, 4)}>
                      {(capability) => <span>{capability}</span>}
                    </For>
                  </div>
                </Show>
              </article>
            )}
          </For>
        </div>
      </div>
      <div class="velion-panel ingestions-card">
        <h2>Tracked web sources</h2>
        <p>Durable source resources registered in Quarry for recurring refresh and review.</p>
        <div class="ingestions-card-list">
          <For
            each={props.sources?.quarrySources ?? []}
            fallback={<div class="ingestions-empty-box">Quarry has no durable source records yet.</div>}
          >
            {(source) => (
              <article class="ingestions-list-card">
                <div class="ingestions-title-row ingestions-title-row--spread">
                  <div class="ingestions-min-width">
                    <h3>{source.name}</h3>
                    <p class="ingestions-truncate">{source.url}</p>
                  </div>
                  <StatusBadge status={source.status} />
                </div>
                <div class="ingestions-meta-row ingestions-meta-row--spread">
                  <span class="ingestions-capitalize">{source.kind}</span>
                  <span>{relativeTime(source.updatedAt)}</span>
                </div>
              </article>
            )}
          </For>
        </div>
      </div>
    </section>
  )
}

function EvidencePanel(props: {
  selectedRun: RunItem | null
  selectedRunId: string | null
  runEvidence: EvidenceTimeline | null
  manualEvidence: ManualEvidence | null
  runs: RunItem[]
  onSelectRun: (runId: string) => void
}) {
  return (
    <section class="ingestions-two-column ingestions-two-column--evidence">
      <aside class="velion-panel ingestions-card">
        <h2>Evidence focus</h2>
        <p>Inspect provenance and warnings before trusting or operationalizing a result.</p>
        <div class="ingestions-evidence-list">
          <For each={props.runs.slice(0, 12)}>
            {(run) => (
              <button
                type="button"
                onClick={() => props.onSelectRun(run.id)}
                class={cn('ingestions-evidence-run', props.selectedRunId === run.id && 'ingestions-evidence-run--selected')}
              >
                <span>
                  <strong>{run.kind}</strong>
                  <StatusBadge status={run.status} />
                </span>
                <small>{run.target}</small>
              </button>
            )}
          </For>
        </div>
      </aside>
      <div class="velion-panel ingestions-card">
        <div class="ingestions-card__header">
          <div>
            <h2>Operational evidence</h2>
            <p>
              <Show when={props.selectedRun} fallback="Most recent manual scrape or extract output.">
                {(run) => `Timeline and warnings for ${run().target}.`}
              </Show>
            </p>
          </div>
          <A href="/knowledge" class="ingestions-inline-link">
            Send to Knowledge
            <ArrowUpRight class="size-4" strokeWidth={1.9} />
          </A>
        </div>

        <Show when={props.manualEvidence}>
          {(evidence) => (
            <div class="ingestions-code-card">
              <div class="ingestions-title-row">
                <Database class="size-4" strokeWidth={1.9} />
                <h3>Latest manual {evidence().kind}</h3>
              </div>
              <pre>{JSON.stringify(evidence(), null, 2)}</pre>
            </div>
          )}
        </Show>

        <Show
          when={props.runEvidence}
          fallback={
            <Show when={props.selectedRunId}>
              <div class="ingestions-empty-box ingestions-empty-box--spacious">
                No durable evidence timeline is available for this run yet.
              </div>
            </Show>
          }
        >
          {(evidence) => (
            <div class="ingestions-card-list">
              <Show when={evidence().warnings.length > 0}>
                <div class="ingestions-warning-box">
                  <h3>Warnings</h3>
                  <ul>
                    <For each={evidence().warnings}>
                      {(warning) => <li>{warning.summary}</li>}
                    </For>
                  </ul>
                </div>
              </Show>
              <div class="ingestions-code-card">
                <h3>Run timeline</h3>
                <div class="ingestions-card-list">
                  <For each={evidence().timeline}>
                    {(event) => (
                      <div class="ingestions-timeline-event">
                        <div class="ingestions-title-row ingestions-title-row--spread">
                          <div class="ingestions-title-row">
                            <StatusBadge status={event.status} />
                            <strong class="ingestions-capitalize">{event.stage}</strong>
                          </div>
                          <span>{relativeTime(event.timestamp)}</span>
                        </div>
                        <p>
                          Completed {event.completed}
                          {event.total ? ` / ${event.total}` : ''}, queued {event.queued}, discovered {event.discovered},
                          blocks {event.blocks}
                        </p>
                        <Show when={event.payload}>
                          {(payload) => <pre>{JSON.stringify(payload(), null, 2)}</pre>}
                        </Show>
                      </div>
                    )}
                  </For>
                </div>
              </div>
            </div>
          )}
        </Show>
      </div>
    </section>
  )
}

function ProfilesPanel(props: { profiles: ProfilePayload | null }) {
  return (
    <section class="velion-panel ingestions-card">
      <h2>Profiles</h2>
      <p>Browser/session profiles for protected sources and stateful refresh flows.</p>
      <div class="ingestions-profile-grid">
        <For
          each={props.profiles?.profiles ?? []}
          fallback={<div class="ingestions-empty-box">No saved profiles yet.</div>}
        >
          {(profile) => (
            <article class="ingestions-list-card">
              <div class="ingestions-title-row ingestions-title-row--spread">
                <h3>{profile.id}</h3>
                <StatusBadge status={profile.restorable ? 'ready' : 'review'} />
              </div>
              <div class="ingestions-profile-details">
                <span>Cookies: {profile.cookies}</span>
                <span>Storage entries: {profile.storage}</span>
                <span>
                  {profile.locale || 'No locale'} - {profile.timezone || 'No timezone'}
                </span>
              </div>
            </article>
          )}
        </For>
      </div>
    </section>
  )
}

function StatusBadge(props: { status: string }) {
  const tone = () => {
    if (['completed', 'active', 'connected', 'ready'].includes(props.status)) return 'ingestions-status--success'
    if (['running', 'queued', 'syncing'].includes(props.status)) return 'ingestions-status--info'
    if (props.status === 'paused') return 'ingestions-status--neutral'
    return 'ingestions-status--warning'
  }

  return <span class={cn('ingestions-status', tone())}>{props.status.replace(/_/g, ' ')}</span>
}

function relativeTime(value?: string | null) {
  if (!value) return 'Unknown'
  const time = Date.parse(value)
  if (!Number.isFinite(time)) return value
  const deltaMs = time - Date.now()
  const rtf = new Intl.RelativeTimeFormat('en', { numeric: 'auto' })
  const units: Array<[Intl.RelativeTimeFormatUnit, number]> = [
    ['day', 86_400_000],
    ['hour', 3_600_000],
    ['minute', 60_000],
  ]

  for (const [unit, amount] of units) {
    if (Math.abs(deltaMs) >= amount || unit === 'minute') {
      return rtf.format(Math.round(deltaMs / amount), unit)
    }
  }
  return value
}
