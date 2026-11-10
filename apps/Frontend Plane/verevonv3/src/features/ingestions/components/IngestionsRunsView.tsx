import { createEffect, createMemo, createSignal, For, Show } from 'solid-js'
import { AlertTriangle, ArrowUpRight, Loader, Pause, Play, RotateCcw, Search, Square, X } from '@/shared/icons'
import { useI18n } from '@/shared/i18n'
import { cn } from '@/shared/lib/cn'
import { Button } from '@/shared/ui/Button'
import { VerevonInput } from '@/shared/ui/verevon/VerevonInput'
import { VerevonSegmented, VerevonSegmentedButton } from '@/shared/ui/verevon/VerevonSegmented'
import { VerevonSelect } from '@/shared/ui/verevon/VerevonSelect'
import {
  createIngestionRun,
  exportIngestionJobResults,
  getIngestionArtifactText,
  getIngestionJob,
  getIngestionJobErrors,
  getIngestionJobResults,
  type ArtifactText,
  type EvidenceTimeline,
  type JobDetail,
  type JobErrors,
  type JobResultItem,
  type JobResultsPage,
  type RunItem,
} from '@/shared/api/ingestions-client'
import { jobResultsMessage } from '@/features/ingestions/lib/errors'
import {
  INGEST_BATCH_LIMIT,
  ingestBatchRequest,
  ingestSelectedEnabled,
  ingestSelectedMessage,
  planIngest,
  togglePick,
} from '@/features/ingestions/lib/ingest-selected'
import {
  countRunsByGroup,
  filterRuns,
  formatDuration,
  runDurationMs,
  runProgressPercent,
  runStatusGroup,
  type RunFilter,
  type RunStatusFilter,
} from '@/features/ingestions/lib/runs'
import {
  DownloadButton,
  EmptyState,
  StatusBadge,
  absoluteTime,
  downloadText,
  kindLabel,
  relativeTime,
  type Tr,
} from '@/features/ingestions/components/ingestions-ui'

/**
 * Apify-style run history: status chips with counts, kind and text filters, a
 * table with duration and progress, and a run detail with overview, the Quarry
 * event log, and output (stats plus the failed and blocked URLs of the job).
 * Pause, resume and cancel are the real job-control action; they show only for
 * the kinds Quarry can control.
 */
export function IngestionsRunsView(props: {
  runs: RunItem[]
  filter: RunFilter
  onFilterChange: (patch: Partial<RunFilter>) => void
  selectedRunId: string | null
  onSelectRun: (id: string | null) => void
  evidence: EvidenceTimeline | null
  evidenceLoading: boolean
  onRerun: (run: RunItem) => void
  onControl: (jobId: string, action: 'pause' | 'resume' | 'cancel') => Promise<void>
  controlError: string | null
  /** A batch started from a run's pages: the page reloads its run list. */
  onIngestStarted?: () => void
}) {
  const i18n = useI18n()
  const tr = i18n.tr
  const visible = createMemo(() => filterRuns(props.runs, props.filter))
  const counts = createMemo(() => countRunsByGroup(props.runs))
  const kinds = createMemo(() => [...new Set(props.runs.map((run) => run.kind))].sort())
  const selected = createMemo(() => props.runs.find((run) => run.id === props.selectedRunId) ?? null)
  const chips: Array<{ id: RunStatusFilter; label: () => string; count: () => number }> = [
    { id: 'all', label: () => tr('Alle', 'All'), count: () => props.runs.length },
    { id: 'active', label: () => tr('Aktive', 'Active'), count: () => counts().active },
    { id: 'succeeded', label: () => tr('Fullført', 'Succeeded'), count: () => counts().succeeded },
    { id: 'failed', label: () => tr('Feilet', 'Failed'), count: () => counts().failed },
  ]

  return (
    <section class={cn('ingestions-runs', selected() && 'ingestions-runs--with-detail')}>
      <div class="verevon-panel ingestions-card ingestions-runs__list">
        <div class="ingestions-card__header">
          <div>
            <h2>{tr('Kjøringshistorikk', 'Run history')}</h2>
            <p>
              {tr(
                'Varige crawl-, uttrekk-, batch-, søk- og agentjobber fra Quarry. Velg en rad for logg, feil og utdata.',
                'Durable crawl, extract, batch, search and agent jobs from Quarry. Pick a row for log, errors and output.',
              )}
            </p>
          </div>
          <a href="/knowledge" link class="ingestions-inline-link">
            {tr('Åpne Kunnskap', 'Open Knowledge')}
            <ArrowUpRight class="size-4" strokeWidth={1.9} />
          </a>
        </div>

        <div class="ingestions-filter-bar">
          <div class="ingestions-pill-row" role="group" aria-label={tr('Statusfilter', 'Status filter')}>
            <For each={chips}>
              {(chip) => (
                <button
                  type="button"
                  class={cn('ingestions-filter-chip', props.filter.status === chip.id && 'ingestions-filter-chip--active')}
                  aria-pressed={props.filter.status === chip.id ? 'true' : 'false'}
                  onClick={() => props.onFilterChange({ status: chip.id })}
                >
                  {chip.label()}
                  <span>{chip.count()}</span>
                </button>
              )}
            </For>
          </div>
          <div class="ingestions-filter-bar__inputs">
            <label class="ingestions-search-input">
              <Search class="size-4" strokeWidth={1.9} aria-hidden="true" />
              <VerevonInput
                aria-label={tr('Filtrer kjøringer', 'Filter runs')}
                value={props.filter.text}
                onInput={(event) => props.onFilterChange({ text: event.currentTarget.value })}
                placeholder={tr('Mål eller ID', 'Target or ID')}
              />
            </label>
            <VerevonSelect aria-label={tr('Type', 'Kind')} value={props.filter.kind} onChange={(event) => props.onFilterChange({ kind: event.currentTarget.value })}>
              <option value="all">{tr('Alle typer', 'All kinds')}</option>
              <For each={kinds()}>{(kind) => <option value={kind}>{kindLabel(kind, tr)}</option>}</For>
            </VerevonSelect>
          </div>
        </div>

        <div class="ingestions-table-wrap">
          <table class="ingestions-table">
            <thead>
              <tr>
                <th>{tr('Status', 'Status')}</th>
                <th>{tr('Type', 'Kind')}</th>
                <th>{tr('Mål', 'Target')}</th>
                <th>{tr('Startet', 'Started')}</th>
                <th>{tr('Varighet', 'Duration')}</th>
                <th>{tr('Fremdrift', 'Progress')}</th>
              </tr>
            </thead>
            <tbody>
              <For
                each={visible()}
                fallback={
                  <tr>
                    <td colspan="6" class="ingestions-empty-cell">
                      {props.runs.length === 0 ? tr('Ingen varige kjøringer ennå.', 'No durable runs yet.') : tr('Ingen kjøringer passer filteret.', 'No runs match the filter.')}
                    </td>
                  </tr>
                }
              >
                {(run) => {
                  const percent = () => runProgressPercent(run)
                  return (
                    <tr
                      class={cn('ingestions-row', props.selectedRunId === run.id && 'ingestions-row--selected')}
                      tabindex={0}
                      aria-selected={props.selectedRunId === run.id ? 'true' : 'false'}
                      onClick={() => props.onSelectRun(run.id)}
                      onKeyDown={(event) => {
                        if (event.key === 'Enter' || event.key === ' ') {
                          event.preventDefault()
                          props.onSelectRun(run.id)
                        }
                      }}
                    >
                      <td>
                        <StatusBadge status={run.status} />
                      </td>
                      <td>{kindLabel(run.kind, tr)}</td>
                      <td class="ingestions-truncate" title={run.target}>
                        {run.target}
                      </td>
                      <td class="ingestions-muted-cell" title={absoluteTime(run.startedAt ?? run.createdAt, i18n)}>
                        {relativeTime(run.startedAt ?? run.createdAt, i18n)}
                      </td>
                      <td class="ingestions-muted-cell">{formatDuration(runDurationMs(run), tr)}</td>
                      <td>
                        <div class="ingestions-progress-cell">
                          <span>
                            {run.progress.completed ?? 0}
                            {run.progress.total ? ` / ${run.progress.total}` : ''}
                          </span>
                          <Show when={percent() !== null}>
                            <span class="ingestions-progress" aria-hidden="true">
                              <span style={{ width: `${percent()}%` }} />
                            </span>
                          </Show>
                        </div>
                      </td>
                    </tr>
                  )
                }}
              </For>
            </tbody>
          </table>
        </div>
      </div>

      <Show when={selected()}>
        {(run) => (
          <RunDetail
            run={run()}
            evidence={props.evidence}
            evidenceLoading={props.evidenceLoading}
            controlError={props.controlError}
            onClose={() => props.onSelectRun(null)}
            onRerun={() => props.onRerun(run())}
            onControl={props.onControl}
            onIngestStarted={props.onIngestStarted}
          />
        )}
      </Show>
    </section>
  )
}

function canRerun(run: RunItem) {
  return ['crawl', 'scrape', 'extract'].includes(run.kind) && /^https?:\/\//i.test(run.target)
}

/** A job's origin as Quarry records it: a schedule that fired it, or a person or API call. */
export function originLabel(job: Pick<JobDetail, 'origin' | 'scheduleId'> | null, tr: Tr): string {
  if (job?.scheduleId) return tr(`Tidsplan ${job.scheduleId}`, `Schedule ${job.scheduleId}`)
  const type = job?.origin?.type
  if (!type) return tr('Ukjent', 'Unknown')
  switch (type) {
    case 'schedule':
      return tr('Tidsplan', 'Schedule')
    case 'api':
      return 'API'
    case 'manual':
    case 'web':
    case 'unknown':
      return tr('Manuell', 'Manual')
    default:
      return type
  }
}

/** Quarry's credit figure for a job, or why there is none. Credits are an estimate of page fetches, never an invoice. */
export function creditsLabel(job: Pick<JobDetail, 'kind' | 'credits'> | null, tr: Tr): string {
  if (!job) return '—'
  const credits = job.credits
  if (credits) {
    const used = Math.round(credits.used * 10) / 10
    const state = credits.final ? tr('endelig', 'final') : tr('kan fortsatt øke', 'can still rise')
    return tr(
      `${used} kreditter, ${credits.pricePerPage} per side · ${state} · estimert`,
      `${used} credits, ${credits.pricePerPage} per page · ${state} · estimated`,
    )
  }
  if (['crawl', 'batch', 'scrape', 'extract'].includes(job.kind)) {
    return tr(
      'Ikke tilgjengelig: QUARRY_CONTROL__JOB_CREDITS er av på Quarry.',
      'Not available: QUARRY_CONTROL__JOB_CREDITS is off on Quarry.',
    )
  }
  return tr('Beregnes ikke for denne jobbtypen.', 'Not calculated for this kind of job.')
}

function RunDetail(props: {
  run: RunItem
  evidence: EvidenceTimeline | null
  evidenceLoading: boolean
  controlError: string | null
  onClose: () => void
  onRerun: () => void
  onControl: (jobId: string, action: 'pause' | 'resume' | 'cancel') => Promise<void>
  onIngestStarted?: () => void
}) {
  const i18n = useI18n()
  const tr = i18n.tr
  const [tab, setTab] = createSignal<'overview' | 'log' | 'output'>('overview')
  const [job, setJob] = createSignal<JobDetail | null>(null)
  const [errors, setErrors] = createSignal<JobErrors | null>(null)
  const [errorsLoading, setErrorsLoading] = createSignal(false)
  const [busy, setBusy] = createSignal(false)
  const percent = () => runProgressPercent(props.run)
  const exportJson = () => JSON.stringify({ run: props.run, job: job(), errors: errors(), evidence: props.evidence }, null, 2)
  const statLines = createMemo(() => Object.entries(props.run.stats).filter(([, value]) => typeof value === 'number' || typeof value === 'string'))
  const isPaused = () => job()?.status === 'paused' || job()?.phase === 'paused'
  const isActive = () => runStatusGroup(props.run.status) === 'active'

  // Re-read the job whenever the row in the list moves on, so the detail follows the run.
  createEffect(
    () => `${props.run.id}|${props.run.status}|${props.run.progress.completed ?? 0}`,
    () => {
      const controller = new AbortController()
      getIngestionJob(props.run.id, controller.signal)
        .then(setJob)
        .catch(() => {
          if (!controller.signal.aborted) setJob(null)
        })
      return () => controller.abort()
    },
  )

  createEffect(
    () => `${tab()}|${props.run.id}|${props.run.status}`,
    () => {
      if (tab() !== 'output') return
      const controller = new AbortController()
      setErrorsLoading(true)
      getIngestionJobErrors(props.run.id, { limit: 100 }, controller.signal)
        .then(setErrors)
        .catch(() => {
          if (!controller.signal.aborted) setErrors(null)
        })
        .finally(() => {
          if (!controller.signal.aborted) setErrorsLoading(false)
        })
      return () => controller.abort()
    },
  )

  async function control(action: 'pause' | 'resume' | 'cancel') {
    setBusy(true)
    try {
      await props.onControl(props.run.id, action)
    } finally {
      setBusy(false)
    }
  }

  return (
    <aside class="verevon-panel ingestions-card ingestions-run-detail" aria-label={tr('Kjøringsdetaljer', 'Run details')}>
      <header class="ingestions-run-detail__head">
        <div class="ingestions-min-width">
          <div class="ingestions-title-row">
            <StatusBadge status={props.run.status} />
            <h2>{kindLabel(props.run.kind, tr)}</h2>
          </div>
          <p class="ingestions-truncate" title={props.run.target}>
            {props.run.target}
          </p>
        </div>
        <button type="button" class="ingestions-icon-button" aria-label={tr('Lukk detaljer', 'Close details')} onClick={() => props.onClose()}>
          <X class="size-4" strokeWidth={1.9} />
        </button>
      </header>

      <div class="ingestions-actions-row">
        <Show when={canRerun(props.run)}>
          <Button size="xs" onClick={() => props.onRerun()}>
            <RotateCcw class="size-4" strokeWidth={1.9} />
            {tr('Kjør på nytt', 'Run again')}
          </Button>
        </Show>
        <Show when={job()?.controllable}>
          <Show when={isActive() && !isPaused()}>
            <Button size="xs" disabled={busy()} onClick={() => void control('pause')}>
              <Pause class="size-4" strokeWidth={1.9} />
              {tr('Pause', 'Pause')}
            </Button>
          </Show>
          <Show when={isPaused()}>
            <Button size="xs" disabled={busy()} onClick={() => void control('resume')}>
              <Play class="size-4" strokeWidth={1.9} />
              {tr('Gjenoppta', 'Resume')}
            </Button>
          </Show>
          <Show when={isActive()}>
            <Button size="xs" disabled={busy()} onClick={() => void control('cancel')}>
              <Square class="size-4" strokeWidth={1.9} />
              {tr('Avbryt', 'Cancel')}
            </Button>
          </Show>
        </Show>
        <DownloadButton filename={`${props.run.id || 'run'}.json`} content={exportJson} label={tr('Eksporter JSON', 'Export JSON')} />
      </div>
      <Show when={props.controlError}>
        {(message) => (
          <div class="ingestions-alert" role="alert">
            <AlertTriangle class="size-4" strokeWidth={1.9} />
            {message()}
          </div>
        )}
      </Show>

      <VerevonSegmented aria-label={tr('Detaljvisning', 'Detail view')}>
        <VerevonSegmentedButton selected={tab() === 'overview'} onClick={() => setTab('overview')}>
          {tr('Oversikt', 'Overview')}
        </VerevonSegmentedButton>
        <VerevonSegmentedButton selected={tab() === 'log'} onClick={() => setTab('log')}>
          {tr('Logg', 'Log')}
          <Show when={props.evidence?.timeline.length}>{(count) => <span class="ingestions-count-pill">{count()}</span>}</Show>
        </VerevonSegmentedButton>
        <VerevonSegmentedButton selected={tab() === 'output'} onClick={() => setTab('output')}>
          {tr('Utdata', 'Output')}
        </VerevonSegmentedButton>
      </VerevonSegmented>

      <Show when={tab() === 'overview'}>
        <Show when={percent() !== null}>
          <div class="ingestions-progress ingestions-progress--large" role="progressbar" aria-valuenow={percent() ?? 0} aria-valuemin={0} aria-valuemax={100}>
            <span style={{ width: `${percent()}%` }} />
          </div>
        </Show>
        <dl class="ingestions-kv">
          <dt>{tr('Jobb-ID', 'Job ID')}</dt>
          <dd class="ingestions-mono">{props.run.id || '—'}</dd>
          <dt>{tr('Kjørings-ID', 'Run ID')}</dt>
          <dd class="ingestions-mono">{props.run.runId || '—'}</dd>
          <dt>{tr('Opprinnelse', 'Origin')}</dt>
          <dd>{originLabel(job(), tr)}</dd>
          <dt>{tr('Opprettet', 'Created')}</dt>
          <dd>{absoluteTime(job()?.createdAt ?? props.run.createdAt, i18n)}</dd>
          <dt>{tr('Startet', 'Started')}</dt>
          <dd>{absoluteTime(props.run.startedAt, i18n)}</dd>
          <dt>{tr('Fullført', 'Finished')}</dt>
          <dd>{absoluteTime(job()?.finishedAt ?? props.run.completedAt, i18n)}</dd>
          <dt>{tr('Varighet', 'Duration')}</dt>
          <dd>{formatDuration(runDurationMs(props.run), tr)}</dd>
          <dt>{tr('Fase', 'Phase')}</dt>
          <dd>{job()?.phase ?? props.run.phase ?? '—'}</dd>
          <dt>{tr('Kreditter', 'Credits')}</dt>
          <dd>{creditsLabel(job(), tr)}</dd>
          <dt>{tr('Fremdrift', 'Progress')}</dt>
          <dd>
            {props.run.progress.completed ?? 0}
            {props.run.progress.total ? ` / ${props.run.progress.total}` : ''}
            <Show when={job()}>
              {(current) => (
                <span class="ingestions-muted">
                  {' '}
                  · {current().counts.failed} {tr('feilet', 'failed')} · {current().counts.blocked} {tr('blokkert', 'blocked')} · {current().counts.retried}{' '}
                  {tr('nye forsøk', 'retries')}
                </span>
              )}
            </Show>
          </dd>
        </dl>
        <Show when={job()?.warnings.length}>
          <div class="ingestions-warning-box">
            <h3>{tr('Advarsler', 'Warnings')}</h3>
            <ul>
              <For each={job()?.warnings ?? []}>{(warning) => <li>{warning.message}</li>}</For>
            </ul>
          </div>
        </Show>
      </Show>

      <Show when={tab() === 'log'}>
        <Show
          when={!props.evidenceLoading}
          fallback={
            <div class="ingestions-result__loading">
              <Loader class="size-4 ingestions-spin" strokeWidth={1.9} />
              {tr('Laster logg …', 'Loading log…')}
            </div>
          }
        >
          <Show
            when={props.evidence}
            fallback={
              <EmptyState title={tr('Ingen logg ennå', 'No log yet')}>
                {props.run.runId
                  ? tr('Quarry har ikke registrert hendelser for denne kjøringen.', 'Quarry has recorded no events for this run.')
                  : tr('Loggen kommer når jobben har fått en kjørings-ID.', 'The log appears once the job has a run ID.')}
              </EmptyState>
            }
          >
            {(evidence) => (
              <>
                <Show when={evidence().warnings.length > 0}>
                  <div class="ingestions-warning-box">
                    <h3>{tr('Advarsler', 'Warnings')}</h3>
                    <ul>
                      <For each={evidence().warnings}>{(warning) => <li>{warning.summary}</li>}</For>
                    </ul>
                  </div>
                </Show>
                <ol class="ingestions-log">
                  <For each={evidence().timeline} fallback={<li class="ingestions-muted">{tr('Ingen hendelser.', 'No events.')}</li>}>
                    {(event) => (
                      <li>
                        <time class="ingestions-mono">{absoluteTime(event.timestamp, i18n)}</time>
                        <StatusBadge status={event.status} />
                        <span class="ingestions-log__stage">{event.stage}</span>
                        <span class="ingestions-log__counts">
                          {tr(
                            `${event.completed}${event.total ? `/${event.total}` : ''} ferdig · ${event.queued} i kø · ${event.discovered} oppdaget${event.retries ? ` · ${event.retries} nye forsøk` : ''}${event.blocks ? ` · ${event.blocks} blokkert` : ''}`,
                            `${event.completed}${event.total ? `/${event.total}` : ''} done · ${event.queued} queued · ${event.discovered} discovered${event.retries ? ` · ${event.retries} retries` : ''}${event.blocks ? ` · ${event.blocks} blocked` : ''}`,
                          )}
                        </span>
                      </li>
                    )}
                  </For>
                </ol>
              </>
            )}
          </Show>
        </Show>
      </Show>

      <Show when={tab() === 'output'}>
        <Show when={statLines().length > 0}>
          <dl class="ingestions-kv">
            <For each={statLines()}>
              {([key, value]) => (
                <>
                  <dt class="ingestions-mono">{key}</dt>
                  <dd>{String(value)}</dd>
                </>
              )}
            </For>
          </dl>
        </Show>
        <Show
          when={!errorsLoading()}
          fallback={
            <div class="ingestions-result__loading">
              <Loader class="size-4 ingestions-spin" strokeWidth={1.9} />
              {tr('Laster feil …', 'Loading errors…')}
            </div>
          }
        >
          <Show
            when={errors()}
            fallback={<EmptyState title={tr('Ingen feilliste', 'No error list')}>{tr('Quarry har ingen feilliste for denne jobben.', 'Quarry has no error list for this job.')}</EmptyState>}
          >
            {(list) => (
              <>
                <h3 class="ingestions-section-title">
                  {tr(`Feil per URL (${list().errors.length})`, `Errors per URL (${list().errors.length})`)}
                </h3>
                <Show when={list().errors.length > 0} fallback={<p class="ingestions-muted">{tr('Ingen URL-er feilet.', 'No URLs failed.')}</p>}>
                  <ul class="ingestions-error-list">
                    <For each={list().errors}>
                      {(item) => (
                        <li>
                          <a href={item.url ?? undefined} target="_blank" rel="noreferrer" class="ingestions-truncate">
                            {item.url ?? '—'}
                          </a>
                          <span class="ingestions-mono">
                            {[item.code, item.status, item.category].filter(Boolean).join(' · ')}
                          </span>
                          <Show when={item.retryable}>
                            <span class="ingestions-status ingestions-status--info">{tr('kan prøves igjen', 'retryable')}</span>
                          </Show>
                        </li>
                      )}
                    </For>
                  </ul>
                </Show>
                <Show when={list().blocked.length > 0}>
                  <h3 class="ingestions-section-title">{tr(`Blokkerte sider (${list().blocked.length})`, `Blocked pages (${list().blocked.length})`)}</h3>
                  <ul class="ingestions-error-list">
                    <For each={list().blocked}>
                      {(item) => (
                        <li>
                          <span class="ingestions-truncate">{item.url ?? '—'}</span>
                          <span class="ingestions-mono">{item.reason ?? ''}</span>
                        </li>
                      )}
                    </For>
                  </ul>
                </Show>
                <Show when={list().robotsBlocked.length > 0}>
                  <h3 class="ingestions-section-title">
                    {tr(`Holdt utenfor av robots.txt (${list().robotsBlocked.length})`, `Kept out by robots.txt (${list().robotsBlocked.length})`)}
                  </h3>
                  <ul class="ingestions-error-list">
                    <For each={list().robotsBlocked}>{(item) => <li class="ingestions-truncate">{item.url ?? '—'}</li>}</For>
                  </ul>
                </Show>
                <Show when={list().nextSeq !== null}>
                  <p class="ingestions-muted">{tr('Flere feil finnes. Vises de første 100.', 'More errors exist. The first 100 are shown.')}</p>
                </Show>
              </>
            )}
          </Show>
        </Show>
        <RunResults runId={props.run.id} onIngestStarted={props.onIngestStarted} />
        <Show when={statLines().length === 0 && !errorsLoading() && !errors()}>
          <pre class="ingestions-json-pre">{JSON.stringify(props.run.stats, null, 2)}</pre>
        </Show>
      </Show>
    </aside>
  )
}

/** The pages a run fetched, one page at a time, as Apify's dataset lists them. A page's markdown is read on demand. */
function RunResults(props: { runId: string; onIngestStarted?: () => void }) {
  const i18n = useI18n()
  const tr = i18n.tr
  const ingestOn = ingestSelectedEnabled()
  const [page, setPage] = createSignal<JobResultsPage | null>(null)
  const [cursor, setCursor] = createSignal<number | undefined>(undefined)
  const [earlier, setEarlier] = createSignal<Array<number | undefined>>([])
  const [latest, setLatest] = createSignal(false)
  const [loading, setLoading] = createSignal(false)
  const [error, setError] = createSignal<string | null>(null)
  const [opened, setOpened] = createSignal<JobResultItem | null>(null)
  const [markdown, setMarkdown] = createSignal<ArtifactText | null>(null)
  const [markdownError, setMarkdownError] = createSignal<string | null>(null)
  const [picked, setPicked] = createSignal<ReadonlySet<number>>(new Set())
  const [dryRun, setDryRun] = createSignal(true)
  const [ingesting, setIngesting] = createSignal(false)
  const [ingestError, setIngestError] = createSignal<string | null>(null)
  const [ingestReceipt, setIngestReceipt] = createSignal<string | null>(null)
  const [exporting, setExporting] = createSignal(false)
  const [exportError, setExportError] = createSignal<string | null>(null)
  const plan = createMemo(() => planIngest((page()?.items ?? []).map((item) => ({ seq: item.seq, url: item.url })), picked()))

  createEffect(
    () => `${props.runId}|${cursor() ?? ''}|${latest()}`,
    () => {
      const controller = new AbortController()
      setLoading(true)
      setError(null)
      // A pick is for the page on screen only: moving the page or the run clears it.
      setPicked(new Set<number>())
      getIngestionJobResults(props.runId, { afterSeq: cursor(), latest: latest(), limit: 50 }, controller.signal)
        .then((next) => {
          setPicked(new Set<number>())
          setPage(next)
        })
        .catch((cause: unknown) => {
          if (controller.signal.aborted) return
          setPage(null)
          setError(jobResultsMessage(cause, tr, { no: 'Kunne ikke hente sidene.', en: 'Could not load the pages.' }))
        })
        .finally(() => {
          if (!controller.signal.aborted) setLoading(false)
        })
      return () => controller.abort()
    },
  )

  function toggleLatest(next: boolean) {
    setEarlier([])
    setCursor(undefined)
    setLatest(next)
  }

  function nextPage() {
    const next = page()?.nextSeq
    if (next === null || next === undefined) return
    setEarlier([...earlier(), cursor()])
    setCursor(next)
  }

  function previousPage() {
    const stack = earlier()
    if (stack.length === 0) return
    setCursor(stack[stack.length - 1])
    setEarlier(stack.slice(0, -1))
  }

  async function openPage(item: JobResultItem) {
    setOpened(item)
    setMarkdown(null)
    setMarkdownError(null)
    if (!item.markdownArtifactId) return
    try {
      setMarkdown(await getIngestionArtifactText(item.markdownArtifactId))
    } catch {
      setMarkdownError(tr('Teksten er ikke lenger lagret, eller den kan ikke vises.', 'The text is no longer stored, or it cannot be shown.'))
    }
  }

  /** Saves the page on screen (same cursor and latest toggle) as a CSV or JSON Lines file. */
  async function exportPage(format: 'csv' | 'jsonl') {
    setExporting(true)
    setExportError(null)
    try {
      const file = await exportIngestionJobResults(props.runId, { format, afterSeq: cursor(), latest: latest() })
      downloadText(file.filename, file.content, file.contentType)
    } catch (cause) {
      setExportError(jobResultsMessage(cause, tr, { no: 'Kunne ikke eksportere sidene.', en: 'Could not export the pages.' }))
    } finally {
      setExporting(false)
    }
  }

  function countText() {
    if (picked().size === 0) return tr('Velg sider for å hente dem på nytt og lagre dem.', 'Pick pages to re-fetch and ingest them.')
    const skipped = plan().duplicates
    return tr(
      `${picked().size} valgt · ${plan().urls.length} unike URL-er${skipped ? ` · ${skipped} hoppes over (samme URL)` : ''}`,
      `${picked().size} selected · ${plan().urls.length} unique URLs${skipped ? ` · ${skipped} skipped (same URL)` : ''}`,
    )
  }

  async function ingestPicked() {
    const urls = plan().urls
    if (urls.length === 0) return
    setIngestError(null)
    setIngestReceipt(null)
    if (dryRun()) {
      setIngestReceipt(
        tr(
          `Tørrkjøring: ${urls.length} URL-er ville blitt hentet på nytt og lagret. Ingenting er sendt.`,
          `Dry run: ${urls.length} URLs would be re-fetched and stored. Nothing was sent.`,
        ),
      )
      return
    }
    setIngesting(true)
    try {
      const created = await createIngestionRun(ingestBatchRequest(urls))
      setPicked(new Set<number>())
      setDryRun(true)
      setIngestReceipt(
        tr(`Batch startet: ${urls.length} URL-er, jobb ${created.run.id}.`, `Batch started: ${urls.length} URLs, job ${created.run.id}.`),
      )
      props.onIngestStarted?.()
    } catch (caught) {
      setIngestError(ingestSelectedMessage(caught, tr))
    } finally {
      setIngesting(false)
    }
  }

  return (
    <>
      <h3 class="ingestions-section-title">{tr('Sider som ble hentet', 'Pages fetched')}</h3>
      <Show when={(page()?.kind ?? 'page_index') === 'page_index'}>
        <p class="ingestions-muted">
          {tr('Oversikt over sidene som ble hentet; teksten leses per side.', 'Index of the pages fetched; the page text is read per page.')}
        </p>
      </Show>
      <label class="ingestions-toggle">
        <input type="checkbox" checked={latest()} onChange={(event) => toggleLatest(event.currentTarget.checked)} />
        <span>
          <strong>{tr('Bare siste henting per URL', 'Latest fetch per URL only')}</strong>
        </span>
      </label>
      <Show when={error()}>{(message) => <div class="ingestions-alert" role="alert">{message()}</div>}</Show>
      <Show when={!error() && page()}>
        {(view) => (
          <>
            <Show when={view().scanCapped}>
              <p class="ingestions-muted">
                {tr('Kjøringen er så stor at bare de første sidene er gått gjennom.', 'The run is so large that only the first pages were checked.')}
              </p>
            </Show>
            <Show when={ingestOn && view().items.length > 0}>
              <div class="ingestions-meta-row" role="group" aria-label={tr('Hent på nytt og lagre valgte sider', 'Re-fetch and ingest the picked pages')}>
                <span class="ingestions-muted">{countText()}</span>
                <label class="ingestions-toggle">
                  <input type="checkbox" checked={dryRun()} disabled={ingesting()} onChange={(event) => setDryRun(event.currentTarget.checked)} />
                  <span>
                    <strong>{tr('Tørrkjøring', 'Dry run')}</strong>
                    <small>{tr('Ingen forespørsel sendes før du slår den av.', 'Nothing is sent until you turn it off.')}</small>
                  </span>
                </label>
                <Button size="xs" disabled={ingesting() || plan().urls.length === 0} onClick={() => void ingestPicked()}>
                  {dryRun() ? tr('Tell valgte', 'Count selected') : tr('Hent på nytt og lagre', 'Re-fetch and ingest')}
                </Button>
              </div>
              <Show when={!dryRun()}>
                <p class="ingestions-muted">
                  {tr(
                    `Listen har ikke sidenes innhold, så hver valgte side hentes på nytt før den lagres. Høyst ${INGEST_BATCH_LIMIT} per batch.`,
                    `The list has no page content, so each picked page is fetched again before it is stored. At most ${INGEST_BATCH_LIMIT} per batch.`,
                  )}
                </p>
              </Show>
              <Show when={ingestError()}>{(message) => <div class="ingestions-alert" role="alert">{message()}</div>}</Show>
              <Show when={ingestReceipt()}>{(text) => <p class="ingestions-muted" role="status">{text()}</p>}</Show>
            </Show>
            <Show
              when={view().items.length > 0}
              fallback={
                <p class="ingestions-muted">
                  {loading() ? tr('Laster sider …', 'Loading pages…') : tr('Ingen sider er hentet ennå.', 'No pages have been fetched yet.')}
                </p>
              }
            >
              <ul class="ingestions-error-list">
                <For each={view().items}>
                  {(item) => (
                    <li class={ingestOn && item.url ? 'ingestions-pick-row' : undefined}>
                      <Show when={ingestOn && item.url}>
                        <input
                          type="checkbox"
                          aria-label={tr(`Velg ${item.url}`, `Pick ${item.url}`)}
                          checked={picked().has(item.seq)}
                          disabled={!picked().has(item.seq) && picked().size >= INGEST_BATCH_LIMIT}
                          onChange={() => setPicked((current) => togglePick(current, item.seq))}
                        />
                      </Show>
                      <button type="button" class="ingestions-truncate" onClick={() => void openPage(item)}>
                        {item.url}
                      </button>
                      <span class="ingestions-mono">
                        {[item.status, item.wordCount !== null ? `${item.wordCount} ord` : null, item.driver].filter(Boolean).join(' · ')}
                      </span>
                    </li>
                  )}
                </For>
              </ul>
            </Show>
            <div class="ingestions-meta-row">
              <Button size="xs" variant="ghost" disabled={earlier().length === 0 || loading()} onClick={previousPage}>
                {tr('Forrige', 'Previous')}
              </Button>
              <Button size="xs" variant="ghost" disabled={view().nextSeq === null || loading()} onClick={nextPage}>
                {tr('Neste', 'Next')}
              </Button>
              <Button size="xs" variant="ghost" disabled={exporting() || loading() || view().items.length === 0} onClick={() => void exportPage('csv')}>
                CSV
              </Button>
              <Button size="xs" variant="ghost" disabled={exporting() || loading() || view().items.length === 0} onClick={() => void exportPage('jsonl')}>
                JSON Lines
              </Button>
            </div>
            <Show when={exportError()}>{(message) => <div class="ingestions-alert" role="alert">{message()}</div>}</Show>
          </>
        )}
      </Show>
      <Show when={opened()}>
        {(item) => (
          <div class="ingestions-code-card">
            <span class="ingestions-truncate">{item().url}</span>
            <Show
              when={markdown()}
              fallback={
                <p class="ingestions-muted">
                  {markdownError() ??
                    (item().markdownArtifactId
                      ? tr('Laster tekst …', 'Loading text…')
                      : tr('Ingen tekst er lagret for denne siden.', 'No text is stored for this page.'))}
                </p>
              }
            >
              {(text) => (
                <>
                  <pre class="ingestions-json-pre">{text().content.slice(0, 20_000)}</pre>
                  <Show when={text().truncated || text().content.length > 20_000}>
                    <p class="ingestions-muted">{tr('Teksten er kuttet i visningen.', 'The text is cut short in this view.')}</p>
                  </Show>
                </>
              )}
            </Show>
          </div>
        )}
      </Show>
    </>
  )
}
