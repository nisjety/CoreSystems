import {
  Activity,
  AlertTriangle,
  ArrowUpRight,
  BarChart3,
  CalendarClock,
  CheckCircle2,
  Clock,
  Database,
  FileSearch,
  Globe,
  Loader,
  Play,
  Plus,
  RefreshCw,
  ScanSearch,
  ShieldCheck,
  Telescope,
  TimerReset,
  Trash2,
  Zap,
  type LucideProps,
} from '@/shared/icons'
import {
  createEffect,
  createMemo,
  createSignal,
  For,
  Show,
  type Component,
} from 'solid-js'
import { createResource } from '@/shared/lib/create-resource-compat'
import {
  createIngestionRun,
  createIngestionSchedule,
  createIngestionSource,
  deleteIngestionSource,
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
  type SourceCreateInput,
  type SourcePayload,
} from '@/shared/api/ingestions-client'
import {
  checkUrlNow,
  getChangeHistory,
  type BaselineSnapshot,
  type ChangeRecord,
} from '@/shared/api/monitoring-client'
import { localeDateTime, useI18n } from '@/shared/i18n'
import { cn } from '@/shared/lib/cn'
import { Button } from '@/shared/ui/Button'
import { VerevonInput } from '@/shared/ui/verevon/VerevonInput'
import { VerevonSegmented, VerevonSegmentedButton } from '@/shared/ui/verevon/VerevonSegmented'
import { VerevonSelect } from '@/shared/ui/verevon/VerevonSelect'
import { VerevonTextarea } from '@/shared/ui/verevon/VerevonTextarea'

type IngestionView = 'runs' | 'schedules' | 'monitoring' | 'sources' | 'evidence' | 'profiles'

type RunFormState = {
  kind: string
  url: string
  urls: string
  prompt: string
  schema: string
  extractMode: 'product' | 'article' | 'general'
}

type ScheduleFormState = {
  name: string
  kind: string
  targetUrl: string
  cron: string
}

type SourceFormState = {
  name: string
  url: string
  kind: string
  monitor: boolean
}

const views: Array<{ id: IngestionView; icon: Component<LucideProps> }> = [
  { id: 'runs', icon: ScanSearch },
  { id: 'schedules', icon: CalendarClock },
  { id: 'monitoring', icon: Telescope },
  { id: 'sources', icon: Globe },
  { id: 'evidence', icon: FileSearch },
  { id: 'profiles', icon: ShieldCheck },
]

function viewLabel(id: IngestionView, i18n: ReturnType<typeof useI18n>) {
  switch (id) {
    case 'runs':
      return i18n.tr('Kjøringer', 'Runs')
    case 'schedules':
      return i18n.tr('Tidsplaner', 'Schedules')
    case 'monitoring':
      return i18n.tr('Overvåking', 'Monitoring')
    case 'sources':
      return i18n.tr('Kilder', 'Sources')
    case 'evidence':
      return i18n.tr('Bevis', 'Evidence')
    case 'profiles':
      return i18n.tr('Profiler', 'Profiles')
  }
}

export default function VerevonIngestionsPage() {
  const i18n = useI18n()
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
    schema: '',
    extractMode: 'general',
  })
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

  const selectedRun = createMemo(() => runs().find((run) => run.id === selectedRunId()) ?? null)
  const activeRuns = createMemo(() => runs().filter((r) => ['running', 'queued', 'pending', 'active', 'in_progress'].includes(r.status.toLowerCase())))
  const completedRuns = createMemo(() => runs().filter((r) => ['completed', 'success', 'done'].includes(r.status.toLowerCase())))
  const failedRuns = createMemo(() => runs().filter((r) => ['failed', 'error', 'timeout'].includes(r.status.toLowerCase())))
  const successRate = createMemo(() => {
    const total = runs().length
    if (total === 0) return 100
    return Math.round((completedRuns().length / total) * 100)
  })

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
      setError(
        nextError instanceof Error
          ? nextError.message
          : i18n.tr('Kunne ikke laste inn arbeidsområdet for innhenting.', 'Could not load ingestion workspace.'),
      )
    } finally {
      if (!signal?.aborted) setLoading(false)
    }
  }

  createEffect(
    () => undefined,
    () => {
      const controller = new AbortController()
      void loadWorkspace(controller.signal)
      return () => controller.abort()
    },
  )

  // Live polling: refresh every 3s when there are active runs (Firecrawl parity: live tracking)
  createEffect(
    () => activeRuns().length > 0,
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
      getIngestionEvidence(evidenceId, controller.signal)
        .then(setRunEvidence)
        .catch(() => {
          if (!controller.signal.aborted) setRunEvidence(null)
        })
      return () => controller.abort()
    },
  )

  async function submitRun() {
    setError(null)
    // Captured before the request so a catch-block reconciliation can tell a
    // run that already existed apart from one this attempt just created.
    const attemptStartedAt = Date.now()
    const form = runForm()
    try {
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
              schema: form.kind === 'extract' && form.schema.trim() ? (() => { try { return JSON.parse(form.schema) } catch { return form.schema } })() : undefined,
              extractMode: form.kind === 'extract' ? form.extractMode : undefined,
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
      // createIngestionRun can fail (e.g. a transient 502) after the run was
      // already durably created server-side. Re-fetch and check whether a
      // run matching this submission now exists before asserting failure,
      // instead of trusting the network error alone. This is a best-effort
      // match (no id survives a lost response): a non-batch submission
      // matches on its exact target URL; either kind must also have been
      // created no earlier than this attempt started (with a small buffer
      // for clock skew) so an unrelated pre-existing run can't false-match.
      let reconciled = true
      try {
        await loadWorkspace()
      } catch {
        reconciled = false
      }
      const submittedTarget = form.kind === 'batch' ? null : form.url.trim()
      const landed = reconciled && runs().some((run) => {
        if (new Date(run.createdAt).getTime() < attemptStartedAt - 10_000) return false
        return submittedTarget ? run.target === submittedTarget : true
      })
      if (landed) {
        setActiveView('runs')
      } else if (reconciled) {
        setError(
          nextError instanceof Error
            ? nextError.message
            : i18n.tr('Kjøringen kunne ikke startes.', 'Run could not be started.'),
        )
      } else {
        setError(i18n.tr(
          'Vi fikk ikke bekreftet om kjøringen ble startet. Vent litt før du prøver på nytt.',
          "We couldn't confirm whether the run was started. Please wait a moment before trying again.",
        ))
      }
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
      setError(
        nextError instanceof Error
          ? nextError.message
          : i18n.tr('Tidsplanen kunne ikke opprettes.', 'Schedule could not be created.'),
      )
    }
  }

  async function runScheduleAction(action: string, scheduleId: string) {
    setError(null)
    try {
      await runIngestionScheduleAction(action, scheduleId)
      await loadWorkspace()
    } catch (nextError) {
      setError(
        nextError instanceof Error
          ? nextError.message
          : i18n.tr('Tidsplanhandlingen mislyktes.', 'Schedule action failed.'),
      )
    }
  }

  async function createSource() {
    setError(null)
    const form = sourceForm()
    if (!form.name.trim() || !form.url.trim()) {
      setError(i18n.tr('Kildenavn og URL er påkrevd.', 'A source name and URL are required.'))
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
    } catch (nextError) {
      setError(
        nextError instanceof Error
          ? nextError.message
          : i18n.tr('Kilden kunne ikke opprettes.', 'Source could not be created.'),
      )
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
    } catch (nextError) {
      // deleteIngestionSource can fail (e.g. a transient 502) after the
      // source was already durably removed server-side. Re-fetch and check
      // whether it's still listed before asserting failure, instead of
      // trusting the network error alone.
      let reconciled = true
      try {
        await loadWorkspace()
      } catch {
        reconciled = false
      }
      const stillPresent = reconciled && (sources()?.quarrySources ?? []).some((source) => source.id === id)
      if (reconciled && stillPresent) {
        setError(
          nextError instanceof Error
            ? nextError.message
            : i18n.tr('Kilden kunne ikke fjernes.', 'Source could not be removed.'),
        )
      } else if (!reconciled) {
        setError(i18n.tr(
          'Vi fikk ikke bekreftet om kilden ble fjernet. Vent litt før du prøver på nytt.',
          "We couldn't confirm whether the source was removed. Please wait a moment before trying again.",
        ))
      }
      // Else: confirmed removed — loadWorkspace() already synced the
      // sources list the UI renders from; no further state change needed.
    } finally {
      setDeletingSourceId(null)
    }
  }

  return (
    <div class="verevon-page-surface ingestions-page">
      <div class="ingestions-page__content">
        <header class="ingestions-header">
          <div class="ingestions-header__copy">
            <h1>{i18n.tr('Innhenting', 'Ingestions')}</h1>
            <p>
              {i18n.tr(
                'Kjør crawler og uttrekk, inspiser bevis, administrer gjentakende tidsplaner, og gi tiltrodde kilder tilbake til Kunnskap.',
                'Run crawls and extracts, inspect evidence, manage recurring schedules, and hand trusted sources back into Knowledge.',
              )}
            </p>
          </div>

          <div class="ingestions-header__actions">
            <VerevonSegmented class="ingestions-tabs" aria-label={i18n.tr('Innhentingsvisninger', 'Ingestion views')}>
              <For each={views}>
                {(view) => {
                  const Icon = view.icon
                  return (
                    <VerevonSegmentedButton
                      class="ingestions-tab"
                      selected={activeView() === view.id}
                      onClick={() => setActiveView(view.id)}
                    >
                      <Icon class="size-4" strokeWidth={1.9} />
                      {viewLabel(view.id, i18n)}
                    </VerevonSegmentedButton>
                  )
                }}
              </For>
            </VerevonSegmented>
            <Button class="ingestions-refresh" onClick={() => void loadWorkspace()}>
              <RefreshCw class={cn('size-4', loading() && 'ingestions-spin')} strokeWidth={1.9} />
              {i18n.tr('Oppdater', 'Refresh')}
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

        {/* Live Scraping Overview — Firecrawl parity + ahead: real-time stats, progress, health */}
        <section class="verevon-panel ingestions-card" style="margin-bottom: 16px; border-left: 3px solid var(--verevon-accent, #ee7a50);">
          <div style="display: flex; align-items: center; gap: 8px; margin-bottom: 12px;">
            <Activity class="size-4" strokeWidth={2} style="color: var(--verevon-accent, #ee7a50);" />
            <h2 style="font-size: 14px; font-weight: 600; margin: 0;">{i18n.tr('Live Scraping Oversikt', 'Live Scraping Overview')}</h2>
            <Show when={activeRuns().length > 0}>
              <span style="margin-left: auto; display: inline-flex; align-items: center; gap: 6px; font-size: 12px; color: #16a34a; background: #f0fdf4; border: 1px solid #bbf7d0; padding: 2px 8px; border-radius: 9999px;">
                <span style="width: 8px; height: 8px; background: #16a34a; border-radius: 50%; display: inline-block; animation: ingestions-spin 1s linear infinite; opacity: 0.9;" />
                {i18n.tr('Live', 'Live')} • {activeRuns().length} {i18n.tr('aktive', 'active')}
              </span>
            </Show>
            <Show when={activeRuns().length === 0 && runs().length > 0}>
              <span style="margin-left: auto; font-size: 12px; color: #6b7280; background: #f9fafb; border: 1px solid #e5e7eb; padding: 2px 8px; border-radius: 9999px;">{i18n.tr('Ingen aktive', 'No active')} • {i18n.tr('auto-oppdaterer ved aktivitet', 'auto-refresh on activity')}</span>
            </Show>
          </div>

          {/* Stats grid */}
          <div style="display: grid; grid-template-columns: repeat(4, 1fr); gap: 12px; margin-bottom: 16px;">
            <div style="background: #f8fafc; border: 1px solid #e2e8f0; border-radius: 10px; padding: 12px;">
              <div style="display: flex; align-items: center; gap: 6px; font-size: 11px; text-transform: uppercase; letter-spacing: 0.05em; color: #64748b; margin-bottom: 6px;"><Zap class="size-3.5" />{i18n.tr('Aktive', 'Active')}</div>
              <div style="font-size: 22px; font-weight: 700; line-height: 1;">{activeRuns().length}</div>
              <div style="font-size: 11px; color: #64748b; margin-top: 4px;">{runs().length} {i18n.tr('totalt', 'total')}</div>
            </div>
            <div style="background: #f0fdf4; border: 1px solid #bbf7d0; border-radius: 10px; padding: 12px;">
              <div style="display: flex; align-items: center; gap: 6px; font-size: 11px; text-transform: uppercase; letter-spacing: 0.05em; color: #15803d; margin-bottom: 6px;"><CheckCircle2 class="size-3.5" />{i18n.tr('Fullført', 'Completed')}</div>
              <div style="font-size: 22px; font-weight: 700; line-height: 1; color: #15803d;">{completedRuns().length}</div>
              <div style="font-size: 11px; color: #15803d; margin-top: 4px;">{successRate()}% {i18n.tr('suksess', 'success')}</div>
            </div>
            <div style="background: #fffbeb; border: 1px solid #fde68a; border-radius: 10px; padding: 12px;">
              <div style="display: flex; align-items: center; gap: 6px; font-size: 11px; text-transform: uppercase; letter-spacing: 0.05em; color: #92400e; margin-bottom: 6px;"><AlertTriangle class="size-3.5" />{i18n.tr('Feilet', 'Failed')}</div>
              <div style="font-size: 22px; font-weight: 700; line-height: 1; color: #b45309;">{failedRuns().length}</div>
              <div style="font-size: 11px; color: #92400e; margin-top: 4px;">{i18n.tr('krever oppmerksomhet', 'needs attention')}</div>
            </div>
            <div style="background: #eff6ff; border: 1px solid #bfdbfe; border-radius: 10px; padding: 12px;">
              <div style="display: flex; align-items: center; gap: 6px; font-size: 11px; text-transform: uppercase; letter-spacing: 0.05em; color: #1d4ed8; margin-bottom: 6px;"><BarChart3 class="size-3.5" />{i18n.tr('Kilder', 'Sources')}</div>
              <div style="font-size: 22px; font-weight: 700; line-height: 1; color: #1d4ed8;">{(sources()?.quarrySources?.length ?? 0) + (sources()?.integrations?.length ?? 0)}</div>
              <div style="font-size: 11px; color: #1d4ed8; margin-top: 4px;">{schedules().length} {i18n.tr('tidsplaner', 'schedules')}</div>
            </div>
          </div>

          {/* Live progress bars */}
          <Show when={activeRuns().length > 0}>
            <div style="border-top: 1px solid #e2e8f0; padding-top: 12px;">
              <div style="display: flex; align-items: center; gap: 6px; font-size: 12px; font-weight: 600; color: #334155; margin-bottom: 8px;"><Clock class="size-3.5" />{i18n.tr('Pågående kjøringer', 'In-flight runs')}</div>
              <For each={activeRuns().slice(0, 3)}>
                {(run) => {
                  const pct = () => {
                    const t = run.progress.total ?? 0
                    const c = run.progress.completed ?? 0
                    if (!t || t === 0) return run.status === 'running' ? 45 : 15
                    return Math.min(99, Math.round((c / t) * 100))
                  }
                  return (
                    <div style="display: flex; align-items: center; gap: 10px; padding: 8px; background: #f8fafc; border: 1px solid #e2e8f0; border-radius: 8px; margin-bottom: 6px;">
                      <Loader class="size-3.5 ingestions-spin" style="color: #0ea5e9; flex-shrink: 0;" />
                      <div style="min-width: 0; flex: 1;">
                        <div style="display: flex; align-items: center; gap: 6px;">
                          <span style="font-size: 12px; font-weight: 600; text-transform: capitalize;">{run.kind}</span>
                          <span style="font-size: 11px; color: #64748b; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;">{run.target}</span>
                        </div>
                        <div style="height: 6px; background: #e2e8f0; border-radius: 9999px; overflow: hidden; margin-top: 6px;">
                          <div style={`height: 100%; width: ${pct()}%; background: linear-gradient(90deg, #0ea5e9, #8b5cf6); border-radius: 9999px; transition: width 0.5s ease;`} />
                        </div>
                      </div>
                      <span style="font-size: 11px; font-weight: 600; color: #0ea5e9; flex-shrink: 0;">{pct()}%</span>
                    </div>
                  )
                }}
              </For>
            </div>
          </Show>

          {/* Recent evidence quick strip */}
          <Show when={runs().length > 0 && activeRuns().length === 0}>
            <div style="display: flex; align-items: center; gap: 8px; font-size: 12px; color: #64748b; border-top: 1px solid #f1f5f9; padding-top: 12px; margin-top: 4px; flex-wrap: wrap;">
              <span style="font-weight: 600; color: #334155;">{i18n.tr('Siste:', 'Latest:')}</span>
              <For each={runs().slice(0, 3)}>
                {(run) => (
                  <span style="display: inline-flex; align-items: center; gap: 4px; background: #f8fafc; border: 1px solid #e2e8f0; padding: 2px 8px; border-radius: 9999px; font-size: 11px;">
                    <span style={`width: 6px; height: 6px; border-radius: 50%; display: inline-block; background: ${run.status === 'completed' ? '#16a34a' : run.status === 'failed' ? '#dc2626' : '#64748b'};`} />
                    {run.kind} • {run.target.slice(0, 32)}
                  </span>
                )}
              </For>
              <span style="margin-left: auto; font-size: 11px;">{schedules().length} {i18n.tr('tidsplaner aktive', 'schedules active')} • {profiles()?.profiles?.length ?? 0} {i18n.tr('profiler', 'profiles')}</span>
            </div>
          </Show>
        </section>

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
  const i18n = useI18n()
  const isBatch = () => props.form.kind === 'batch'

  return (
    <form
      class="verevon-panel ingestions-card ingestions-composer"
      onSubmit={(event) => {
        event.preventDefault()
        void props.onSubmit()
      }}
    >
      <div>
        <h2>{i18n.tr('Start en kjøring', 'Start a run')}</h2>
        <p>{i18n.tr('Manuell parallell til scrape, crawl, uttrekk og batch-kjøring.', 'Manual parity for scrape, crawl, extract, and batch execution.')}</p>
      </div>
      <label class="ingestions-field">
        {i18n.tr('Kjøringstype', 'Run type')}
        <VerevonSelect value={props.form.kind} onChange={(event) => props.onFormChange({ kind: event.currentTarget.value })}>
          <option value="scrape">{i18n.tr('Scrape', 'Scrape')}</option>
          <option value="crawl">{i18n.tr('Crawl', 'Crawl')}</option>
          <option value="extract">{i18n.tr('Uttrekk', 'Extract')}</option>
          <option value="batch">{i18n.tr('Batch', 'Batch')}</option>
        </VerevonSelect>
      </label>
      <Show
        when={isBatch()}
        fallback={
          <label class="ingestions-field">
            {i18n.tr('Mål-URL', 'Target URL')}
            <VerevonInput
              value={props.form.url}
              onInput={(event) => props.onFormChange({ url: event.currentTarget.value })}
              placeholder="https://example.com"
            />
          </label>
        }
      >
        <label class="ingestions-field">
          {i18n.tr('URL-er', 'URLs')}
          <VerevonTextarea
            rows={6}
            value={props.form.urls}
            onInput={(event) => props.onFormChange({ urls: event.currentTarget.value })}
            placeholder={'https://example.com/pricing\nhttps://example.com/docs'}
          />
        </label>
      </Show>
      <Show when={props.form.kind === 'extract'}>
        <div style="display: flex; flex-direction: column; gap: 10px; padding: 12px; background: #f8fafc; border: 1px solid #e2e8f0; border-radius: 10px;">
          <label class="ingestions-field" style="margin: 0;">
            {i18n.tr('Uttrekksmodus', 'Extraction mode')}
            <VerevonSelect value={props.form.extractMode} onChange={(e) => props.onFormChange({ extractMode: e.currentTarget.value as any })}>
              <option value="general">{i18n.tr('Generell', 'General')}</option>
              <option value="product">{i18n.tr('Produkt (pris, lager, bilder)', 'Product (price, stock, images)')}</option>
              <option value="article">{i18n.tr('Artikkel (forfatter, dato, innhold)', 'Article (author, date, content)')}</option>
            </VerevonSelect>
          </label>
          <label class="ingestions-field" style="margin: 0;">
            {i18n.tr('Uttrekksprompt', 'Extraction prompt')}
            <VerevonTextarea
              rows={3}
              value={props.form.prompt}
              onInput={(event) => props.onFormChange({ prompt: event.currentTarget.value })}
              placeholder={props.form.extractMode === 'product' ? i18n.tr('Trekk ut produktnavn, pris, tilgjengelighet, bilder, rating for hvert produkt på siden.', 'Extract product name, price, availability, images, rating for each product on page.') : props.form.extractMode === 'article' ? i18n.tr('Trekk ut tittel, forfatter, publiseringsdato, hovedinnhold og nøkkelord.', 'Extract title, author, publish date, main content and keywords.') : i18n.tr('Trekk ut sentrale supporttemaer, kontaktkanaler og prissignaler.', 'Extract key support topics, contact channels, and pricing signals.')}
            />
          </label>
          <label class="ingestions-field" style="margin: 0;">
            <span style="display: flex; align-items: center; justify-content: space-between;">
              {i18n.tr('JSON schema (valgfritt)', 'JSON schema (optional)')}
              <button type="button" style="font-size: 11px; color: #0ea5e9; background: none; border: none; cursor: pointer;" onClick={() => {
                const presets: Record<string, string> = {
                  product: '{\n  "type": "object",\n  "properties": {\n    "products": {\n      "type": "array",\n      "items": {\n        "type": "object",\n        "properties": {\n          "name": { "type": "string" },\n          "price": { "type": "string" },\n          "currency": { "type": "string" },\n          "availability": { "type": "string" },\n          "image": { "type": "string" },\n          "rating": { "type": "string" }\n        }\n      }\n    }\n  }\n}',
                  article: '{\n  "type": "object",\n  "properties": {\n    "title": { "type": "string" },\n    "author": { "type": "string" },\n    "published_at": { "type": "string" },\n    "content": { "type": "string" },\n    "keywords": { "type": "array", "items": { "type": "string" } }\n  }\n}',
                  general: '{\n  "type": "object",\n  "properties": {\n    "summary": { "type": "string" },\n    "topics": { "type": "array", "items": { "type": "string" } }\n  }\n}'
                }
                const preset = presets[props.form.extractMode] ?? presets.general
                props.onFormChange({ schema: preset })
              }}>{i18n.tr('Last preset', 'Load preset')}</button>
            </span>
            <VerevonTextarea
              rows={4}
              value={props.form.schema}
              onInput={(event) => props.onFormChange({ schema: event.currentTarget.value })}
              placeholder={'{"type":"object","properties":{"name":{"type":"string"}}}'}
              style="font-family: ui-monospace, monospace; font-size: 12px;"
            />
          </label>
          <div style="font-size: 11px; color: #64748b; display: flex; align-items: center; gap: 4px;"><Zap class="size-3" />{i18n.tr('Firecrawl-paritet: strukturert uttrekk med LLM + source-tracing. Agentic modus bruker browser-actions + LLM-loop.', 'Firecrawl parity: structured extract with LLM + source-tracing. Agentic mode uses browser-actions + LLM loop.')}</div>
        </div>
      </Show>
      <Show when={props.form.kind === 'agent'}>
        <div style="display: flex; flex-direction: column; gap: 10px; padding: 12px; background: #fefce8; border: 1px solid #fde68a; border-radius: 10px;">
          <div style="display: flex; align-items: center; gap: 6px; font-size: 12px; font-weight: 600; color: #92400e;"><Telescope class="size-3.5" />{i18n.tr('Agentic Scraping (autonom)', 'Agentic Scraping (autonomous)')}</div>
          <label class="ingestions-field" style="margin: 0;">
            {i18n.tr('Oppgave', 'Task')}
            <VerevonTextarea
              rows={3}
              value={props.form.prompt}
              onInput={(event) => props.onFormChange({ prompt: event.currentTarget.value })}
              placeholder={i18n.tr('Finn alle produkter under 500 kr, legg til i handlekurv og ta screenshot.', 'Find all products under 500 NOK, add to cart and take screenshot.')}
            />
          </label>
          <div style="font-size: 11px; color: #92400e;">{i18n.tr('Quarry kjører browser-actions (click/type/scroll) i loop med LLM-beslutninger (max 25 steg) — Paritet med Firecrawl browser-agent.', 'Quarry runs browser-actions (click/type/scroll) in loop with LLM decisions (max 25 steps) — parity with Firecrawl browser-agent.')}</div>
        </div>
      </Show>
      <Button variant="primary" fullWidth type="submit">
        <Play class="size-4" strokeWidth={1.9} />
        {i18n.tr('Start kjøring', 'Start run')}
      </Button>
    </form>
  )
}

function RunsPanel(props: {
  runs: RunItem[]
  selectedRunId: string | null
  onSelectRun: (runId: string) => void
}) {
  const i18n = useI18n()
  return (
    <section class="verevon-panel ingestions-card ingestions-runs-panel">
      <div class="ingestions-card__header">
        <div>
          <h2>{i18n.tr('Nylige kjøringer', 'Recent runs')}</h2>
          <p>{i18n.tr('Varige crawl-, uttrekk-, batch-, søk- og agentjobber fra Quarry.', 'Durable crawl, extract, batch, search, and agent jobs from Quarry.')}</p>
        </div>
        <a href="/knowledge" link class="ingestions-inline-link">
          {i18n.tr('Åpne Kunnskap', 'Open Knowledge')}
          <ArrowUpRight class="size-4" strokeWidth={1.9} />
        </a>
      </div>
      <div class="ingestions-table-wrap">
        <table class="ingestions-table">
          <thead>
            <tr>
              <th>{i18n.tr('Type', 'Kind')}</th>
              <th>{i18n.tr('Mål', 'Target')}</th>
              <th>{i18n.tr('Status', 'Status')}</th>
              <th>{i18n.tr('Opprettet', 'Created')}</th>
              <th>{i18n.tr('Fremdrift', 'Progress')}</th>
            </tr>
          </thead>
          <tbody>
            <For
              each={props.runs}
              fallback={
                <tr>
                  <td colspan="5" class="ingestions-empty-cell">
                    {i18n.tr('Ingen varige kjøringer ennå.', 'No durable runs yet.')}
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
                  <td class="ingestions-muted-cell">{relativeTime(run.createdAt, i18n)}</td>
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
  const i18n = useI18n()
  return (
    <form
      class="verevon-panel ingestions-card ingestions-composer"
      onSubmit={(event) => {
        event.preventDefault()
        void props.onSubmit()
      }}
    >
      <div>
        <h2>{i18n.tr('Opprett tidsplan', 'Create schedule')}</h2>
        <p>{i18n.tr('Gjentakende innhenting for kilder som skal holdes ferske uten manuelle kjøringer.', 'Recurring ingestion for sources that should stay fresh without manual runs.')}</p>
      </div>
      <label class="ingestions-field">
        {i18n.tr('Navn', 'Name')}
        <VerevonInput
          value={props.form.name}
          onInput={(event) => props.onFormChange({ name: event.currentTarget.value })}
          placeholder={i18n.tr('Dokumentasjon-crawl', 'Docs crawl')}
        />
      </label>
      <label class="ingestions-field">
        {i18n.tr('Type', 'Kind')}
        <VerevonSelect value={props.form.kind} onChange={(event) => props.onFormChange({ kind: event.currentTarget.value })}>
          <option value="crawl">{i18n.tr('Crawl', 'Crawl')}</option>
          <option value="extract">{i18n.tr('Uttrekk', 'Extract')}</option>
          <option value="search">{i18n.tr('Søk', 'Search')}</option>
          <option value="batch">{i18n.tr('Batch', 'Batch')}</option>
          <option value="agent">{i18n.tr('Agent', 'Agent')}</option>
        </VerevonSelect>
      </label>
      <label class="ingestions-field">
        {i18n.tr('Mål-URL', 'Target URL')}
        <VerevonInput
          value={props.form.targetUrl}
          onInput={(event) => props.onFormChange({ targetUrl: event.currentTarget.value })}
          placeholder="https://example.com/docs"
        />
      </label>
      <label class="ingestions-field">
        {i18n.tr('Cron', 'Cron')}
        <VerevonInput
          value={props.form.cron}
          onInput={(event) => props.onFormChange({ cron: event.currentTarget.value })}
          placeholder="0 7 * * *"
        />
      </label>
      <Button variant="primary" fullWidth type="submit">
        <CalendarClock class="size-4" strokeWidth={1.9} />
        {i18n.tr('Lagre tidsplan', 'Save schedule')}
      </Button>
    </form>
  )
}

function SchedulesPanel(props: {
  schedules: ScheduleItem[]
  onAction: (action: string, scheduleId: string) => Promise<void>
}) {
  const i18n = useI18n()
  return (
    <section class="verevon-panel ingestions-card">
      <div>
        <h2>{i18n.tr('Tidsplansyklus', 'Schedule lifecycle')}</h2>
        <p>{i18n.tr('Sett på pause, gjenoppta, utløs og legg ned gjentakende jobber fra én flate.', 'Pause, resume, trigger, and retire recurring jobs from one surface.')}</p>
      </div>
      <div class="ingestions-card-list">
        <For
          each={props.schedules}
          fallback={<div class="ingestions-empty-box">{i18n.tr('Ingen gjentakende tidsplaner ennå.', 'No recurring schedules yet.')}</div>}
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
                    <span>{schedule.cron || schedule.scheduleAt || i18n.tr('Manuell frekvens', 'Manual cadence')}</span>
                    <span>
                      {i18n.tr('Neste', 'Next')}:{' '}
                      {schedule.nextRunAt ? relativeTime(schedule.nextRunAt, i18n) : i18n.tr('Ikke planlagt', 'Not scheduled')}
                    </span>
                  </div>
                </div>
                <div class="ingestions-actions-row">
                  <Show
                    when={schedule.status === 'paused'}
                    fallback={
                      <Button size="xs" onClick={() => void props.onAction('pause_schedule', schedule.id)}>
                        <TimerReset class="size-4" strokeWidth={1.9} />
                        {i18n.tr('Pause', 'Pause')}
                      </Button>
                    }
                  >
                    <Button size="xs" onClick={() => void props.onAction('unpause_schedule', schedule.id)}>
                      <Play class="size-4" strokeWidth={1.9} />
                      {i18n.tr('Gjenoppta', 'Resume')}
                    </Button>
                  </Show>
                  <Button size="xs" onClick={() => void props.onAction('trigger_schedule', schedule.id)}>
                    <RefreshCw class="size-4" strokeWidth={1.9} />
                    {i18n.tr('Utløs', 'Trigger')}
                  </Button>
                  <Button size="xs" onClick={() => void props.onAction('delete_schedule', schedule.id)}>
                    {i18n.tr('Legg ned', 'Retire')}
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
  const i18n = useI18n()
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
      setError(i18n.tr('Angi en URL som skal sjekkes.', 'Enter a URL to check.'))
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
      setError(
        nextError instanceof Error
          ? nextError.message
          : i18n.tr('Sjekken kunne ikke fullføres.', 'Check could not be completed.'),
      )
    } finally {
      setChecking(false)
    }
  }

  return (
    <section class="ingestions-two-column ingestions-two-column--runs">
      <form
        class="verevon-panel ingestions-card ingestions-composer"
        onSubmit={(event) => {
          event.preventDefault()
          void check()
        }}
      >
        <div>
          <h2>{i18n.tr('Sjekk en side for endringer', 'Check a page for changes')}</h2>
          <p>
            {i18n.tr(
              'Kun on-demand: Verevon henter siden nå, fingeravtrykker innholdet, og sammenligner det med den siste basislinjen dette arbeidsområdet har fanget opp. Gjentakende overvåking i stor skala er ikke tilgjengelig ennå.',
              'On-demand only: Verevon fetches the page now, fingerprints its content, and compares it to the last baseline this workspace captured. Recurring at-scale monitoring is not yet available.',
            )}
          </p>
        </div>
        <label class="ingestions-field">
          {i18n.tr('Side-URL', 'Page URL')}
          <VerevonInput
            value={urlInput()}
            onInput={(event) => setUrlInput(event.currentTarget.value)}
            placeholder="https://example.com/pricing"
          />
        </label>
        <Button variant="primary" fullWidth type="submit" disabled={checking()}>
          <ScanSearch class={cn('size-4', checking() && 'ingestions-spin')} strokeWidth={1.9} />
          {checking() ? i18n.tr('Sjekker …', 'Checking…') : i18n.tr('Sjekk nå', 'Check now')}
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
                <h3>{i18n.tr('Siste sjekk', 'Latest check')}</h3>
                <ChangeStatusBadge status={result().status} />
              </div>
              <div class="ingestions-meta-row ingestions-meta-row--spread">
                <span class="ingestions-truncate">{result().sourceUrl}</span>
                <span>{relativeTime(result().checkedAt, i18n)}</span>
              </div>
              <Show
                when={result().prevBaseline}
                fallback={
                  <p>
                    {i18n.tr(
                      'Ingen tidligere basislinje — dette er første gang dette arbeidsområdet har sjekket denne siden.',
                      'No earlier baseline — this is the first time this workspace has checked this page.',
                    )}
                  </p>
                }
              >
                {(prev) => (
                  <p>
                    {i18n.tr('Forrige basislinje ble fanget', 'Previous baseline captured')} {relativeTime(prev().capturedAt, i18n)}.
                  </p>
                )}
              </Show>
            </div>
          )}
        </Show>
      </form>

      <section class="verevon-panel ingestions-card ingestions-runs-panel">
        <div class="ingestions-card__header">
          <div>
            <h2>{i18n.tr('Endringshistorikk', 'Change history')}</h2>
            <p>{i18n.tr('Fangede basislinjer for siden over, nyeste først. Tom helt til en sjekk registrerer en basislinje.', 'Captured baselines for the page above, newest first. Empty until a check records a baseline.')}</p>
          </div>
        </div>
        <Show
          when={watchedUrl()}
          fallback={<div class="ingestions-empty-box">{i18n.tr('Kjør en sjekk for å se basislinjehistorikken til denne siden.', "Run a check to see this page's baseline history.")}</div>}
        >
          <Show
            when={!history.loading}
            fallback={<div class="ingestions-empty-box">{i18n.tr('Laster historikk …', 'Loading history…')}</div>}
          >
            <div class="ingestions-table-wrap">
              <table class="ingestions-table">
                <thead>
                  <tr>
                    <th>{i18n.tr('Fanget', 'Captured')}</th>
                    <th>{i18n.tr('Fingeravtrykk', 'Fingerprint')}</th>
                    <th>{i18n.tr('Kjøring', 'Run')}</th>
                  </tr>
                </thead>
                <tbody>
                  <For
                    each={history() ?? []}
                    fallback={
                      <tr>
                        <td colspan="3" class="ingestions-empty-cell">
                          {i18n.tr('Ingen basislinjer registrert for denne siden ennå.', 'No baselines recorded for this page yet.')}
                        </td>
                      </tr>
                    }
                  >
                    {(snapshot: BaselineSnapshot) => (
                      <tr class="ingestions-row">
                        <td class="ingestions-muted-cell">{relativeTime(snapshot.capturedAt, i18n)}</td>
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

function SourcesPanel(props: {
  sources: SourcePayload | null
  form: SourceFormState
  onFormChange: (patch: Partial<SourceFormState>) => void
  onCreate: () => Promise<void>
  creating: boolean
  deletingId: string | null
  onDelete: (id: string) => Promise<void>
}) {
  const i18n = useI18n()
  const canSubmit = () =>
    !props.creating && props.form.name.trim().length > 0 && props.form.url.trim().length > 0

  return (
    <section class="ingestions-sources-grid">
      <div class="verevon-panel ingestions-card">
        <div class="ingestions-card__header">
          <div>
            <h2>{i18n.tr('Tilkoblede kilder', 'Connected sources')}</h2>
            <p>{i18n.tr('Integrasjonsbasert kunnskap og nettsted-mål for innhenting synlige for Verevon.', 'Integration-backed knowledge and website ingestion targets visible to Verevon.')}</p>
          </div>
          <a href="/knowledge" link class="ingestions-inline-link">
            {i18n.tr('Åpne Kunnskap', 'Open Knowledge')}
            <ArrowUpRight class="size-4" strokeWidth={1.9} />
          </a>
        </div>
        <div class="ingestions-card-list">
          <For
            each={props.sources?.integrations ?? []}
            fallback={<div class="ingestions-empty-box">{i18n.tr('Ingen tilkoblede integrasjoner ennå.', 'No connected integrations yet.')}</div>}
          >
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
      <div class="verevon-panel ingestions-card">
        <h2>{i18n.tr('Sporede nettkilder', 'Tracked web sources')}</h2>
        <p>{i18n.tr('Varige kilderessurser registrert i Quarry for gjentakende oppdatering og gjennomgang.', 'Durable source resources registered in Quarry for recurring refresh and review.')}</p>
        <form
          class="ingestions-source-form"
          onSubmit={(event) => {
            event.preventDefault()
            void props.onCreate()
          }}
        >
          <label class="ingestions-field">
            {i18n.tr('Navn', 'Name')}
            <VerevonInput
              value={props.form.name}
              onInput={(event) => props.onFormChange({ name: event.currentTarget.value })}
              placeholder={i18n.tr('Acme prisside', 'Acme pricing page')}
              disabled={props.creating}
            />
          </label>
          <label class="ingestions-field">
            {i18n.tr('URL', 'URL')}
            <VerevonInput
              value={props.form.url}
              onInput={(event) => props.onFormChange({ url: event.currentTarget.value })}
              placeholder="https://example.com/pricing"
              disabled={props.creating}
            />
          </label>
          <label class="ingestions-field">
            {i18n.tr('Type', 'Kind')}
            <VerevonSelect
              value={props.form.kind}
              onChange={(event) => props.onFormChange({ kind: event.currentTarget.value })}
              disabled={props.creating}
            >
              <option value="crawl">{i18n.tr('Crawl', 'Crawl')}</option>
              <option value="scrape">{i18n.tr('Scrape', 'Scrape')}</option>
              <option value="search">{i18n.tr('Søk', 'Search')}</option>
            </VerevonSelect>
          </label>
          <label class="ingestions-checkbox-field">
            <input
              type="checkbox"
              checked={props.form.monitor}
              onChange={(event) => props.onFormChange({ monitor: event.currentTarget.checked })}
              disabled={props.creating}
            />
            {i18n.tr('Overvåk for endringer (daglig)', 'Monitor for changes (daily)')}
          </label>
          <Button variant="primary" fullWidth type="submit" disabled={!canSubmit()}>
            <Show
              when={props.creating}
              fallback={
                <>
                  <Plus class="size-4" strokeWidth={1.9} />
                  {i18n.tr('Legg til kilde', 'Add source')}
                </>
              }
            >
              <Loader class="size-4 ingestions-spin" strokeWidth={1.9} />
              {i18n.tr('Legger til …', 'Adding…')}
            </Show>
          </Button>
        </form>
        <div class="ingestions-card-list">
          <For
            each={props.sources?.quarrySources ?? []}
            fallback={<div class="ingestions-empty-box">{i18n.tr('Quarry har ingen varige kilderegistreringer ennå.', 'Quarry has no durable source records yet.')}</div>}
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
                  <span class="ingestions-source-meta-actions">
                    <Show when={source.updatedAt}>{relativeTime(source.updatedAt, i18n)}</Show>
                    <Button
                      size="xs"
                      variant="ghost"
                      disabled={props.deletingId === source.id}
                      onClick={() => void props.onDelete(source.id)}
                    >
                      <Show
                        when={props.deletingId === source.id}
                        fallback={<Trash2 class="size-4" strokeWidth={1.9} />}
                      >
                        <Loader class="size-4 ingestions-spin" strokeWidth={1.9} />
                      </Show>
                      {i18n.tr('Fjern', 'Remove')}
                    </Button>
                  </span>
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
  const i18n = useI18n()
  return (
    <section class="ingestions-two-column ingestions-two-column--evidence">
      <aside class="verevon-panel ingestions-card">
        <h2>{i18n.tr('Bevisfokus', 'Evidence focus')}</h2>
        <p>{i18n.tr('Inspiser opphav og advarsler før du stoler på eller tar i bruk et resultat.', 'Inspect provenance and warnings before trusting or operationalizing a result.')}</p>
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
      <div class="verevon-panel ingestions-card">
        <div class="ingestions-card__header">
          <div>
            <h2>{i18n.tr('Driftsbevis', 'Operational evidence')}</h2>
            <p>
              <Show
                when={props.selectedRun}
                fallback={i18n.tr('Siste manuelle scrape- eller uttrekksresultat.', 'Most recent manual scrape or extract output.')}
              >
                {(run) => (
                  <>{i18n.tr(`Tidslinje og advarsler for ${run().target}.`, `Timeline and warnings for ${run().target}.`)}</>
                )}
              </Show>
            </p>
          </div>
          <a href="/knowledge" link class="ingestions-inline-link">
            {i18n.tr('Send til Kunnskap', 'Send to Knowledge')}
            <ArrowUpRight class="size-4" strokeWidth={1.9} />
          </a>
        </div>

        <Show when={props.manualEvidence}>
          {(evidence) => (
            <div class="ingestions-code-card">
              <div class="ingestions-title-row">
                <Database class="size-4" strokeWidth={1.9} />
                <h3>{i18n.tr(`Siste manuelle ${evidence().kind}`, `Latest manual ${evidence().kind}`)}</h3>
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
                {i18n.tr('Ingen varig bevistidslinje er tilgjengelig for denne kjøringen ennå.', 'No durable evidence timeline is available for this run yet.')}
              </div>
            </Show>
          }
        >
          {(evidence) => (
            <div class="ingestions-card-list">
              <Show when={evidence().warnings.length > 0}>
                <div class="ingestions-warning-box">
                  <h3>{i18n.tr('Advarsler', 'Warnings')}</h3>
                  <ul>
                    <For each={evidence().warnings}>
                      {(warning) => <li>{warning.summary}</li>}
                    </For>
                  </ul>
                </div>
              </Show>
              <div class="ingestions-code-card">
                <h3>{i18n.tr('Kjøringstidslinje', 'Run timeline')}</h3>
                <div class="ingestions-card-list">
                  <For each={evidence().timeline}>
                    {(event) => (
                      <div class="ingestions-timeline-event">
                        <div class="ingestions-title-row ingestions-title-row--spread">
                          <div class="ingestions-title-row">
                            <StatusBadge status={event.status} />
                            <strong class="ingestions-capitalize">{event.stage}</strong>
                          </div>
                          <span>{relativeTime(event.timestamp, i18n)}</span>
                        </div>
                        <p>
                          {i18n.tr(
                            `Fullført ${event.completed}${event.total ? ` / ${event.total}` : ''}, i kø ${event.queued}, oppdaget ${event.discovered}, blokker ${event.blocks}`,
                            `Completed ${event.completed}${event.total ? ` / ${event.total}` : ''}, queued ${event.queued}, discovered ${event.discovered}, blocks ${event.blocks}`,
                          )}
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
  const i18n = useI18n()
  return (
    <section class="verevon-panel ingestions-card">
      <h2>{i18n.tr('Profiler', 'Profiles')}</h2>
      <p>{i18n.tr('Nettleser-/øktprofiler for beskyttede kilder og tilstandsbaserte oppdateringsflyter.', 'Browser/session profiles for protected sources and stateful refresh flows.')}</p>
      <div class="ingestions-profile-grid">
        <For
          each={props.profiles?.profiles ?? []}
          fallback={<div class="ingestions-empty-box">{i18n.tr('Ingen lagrede profiler ennå.', 'No saved profiles yet.')}</div>}
        >
          {(profile) => (
            <article class="ingestions-list-card">
              <div class="ingestions-title-row ingestions-title-row--spread">
                <h3>{profile.id}</h3>
                <StatusBadge status={profile.restorable ? 'ready' : 'review'} />
              </div>
              <div class="ingestions-profile-details">
                <span>{i18n.tr('Informasjonskapsler', 'Cookies')}: {profile.cookies}</span>
                <span>{i18n.tr('Lagringsoppføringer', 'Storage entries')}: {profile.storage}</span>
                <span>
                  {profile.locale || i18n.tr('Ingen lokalitet', 'No locale')} - {profile.timezone || i18n.tr('Ingen tidssone', 'No timezone')}
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

function relativeTime(value: string | null | undefined, i18n: ReturnType<typeof useI18n>) {
  if (!value) return i18n.tr('Ukjent', 'Unknown')
  const time = Date.parse(value)
  if (!Number.isFinite(time)) return value
  const deltaMs = time - Date.now()
  const rtf = new Intl.RelativeTimeFormat(localeDateTime(i18n.locale()), { numeric: 'auto' })
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
