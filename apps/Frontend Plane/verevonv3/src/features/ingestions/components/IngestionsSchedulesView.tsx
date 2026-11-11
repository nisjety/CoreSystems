import { createEffect, createSignal, For, Show } from 'solid-js'
import { CalendarClock, Play, RefreshCw, TimerReset, Trash2 } from '@/shared/icons'
import { useI18n } from '@/shared/i18n'
import { cn } from '@/shared/lib/cn'
import { Button } from '@/shared/ui/Button'
import { VerevonInput } from '@/shared/ui/verevon/VerevonInput'
import { VerevonSelect } from '@/shared/ui/verevon/VerevonSelect'
import {
  listScheduleRuns,
  type ScheduleItem,
  type ScheduleRun,
  type SchedulePauseAction,
  type ScheduleStateAction,
} from '@/shared/api/ingestions-client'
import { cronPresetLabel, cronPresets, describeCron } from '@/features/ingestions/lib/cron'
import {
  EmptyState,
  StatusBadge,
  kindLabel,
  relativeTime,
  type Tr,
} from '@/features/ingestions/components/ingestions-ui'

export type ScheduleFormState = {
  name: string
  kind: string
  targetUrl: string
  cron: string
}

/** Apify-style schedules: a cron composer with presets and a plain-language reading, and the lifecycle list. */
export function IngestionsSchedulesView(props: {
  form: ScheduleFormState
  kinds: string[]
  canTrigger: boolean
  schedules: ScheduleItem[]
  onFormChange: (patch: Partial<ScheduleFormState>) => void
  onSubmit: () => Promise<void>
  onAction: (action: SchedulePauseAction | ScheduleStateAction, scheduleId: string) => Promise<void>
}) {
  const i18n = useI18n()
  const tr = i18n.tr
  const reading = () => describeCron(props.form.cron, tr)
  // The rows whose latest run is open. A row reads its run only when it opens (WS15.S4e).
  const [openRuns, setOpenRuns] = createSignal<ReadonlySet<string>>(new Set())
  const runsOpen = (scheduleId: string) => openRuns().has(scheduleId)
  function toggleRuns(scheduleId: string) {
    setOpenRuns((current) => {
      const next = new Set(current)
      if (next.has(scheduleId)) next.delete(scheduleId)
      else next.add(scheduleId)
      return next
    })
  }

  return (
    <section class="ingestions-two-column ingestions-two-column--runs">
      <form
        class="verevon-panel ingestions-card ingestions-composer"
        onSubmit={(event) => {
          event.preventDefault()
          void props.onSubmit()
        }}
      >
        <div>
          <h2>{tr('Opprett tidsplan', 'Create schedule')}</h2>
          <p>{tr('Gjentakende innhenting for kilder som skal holdes ferske uten manuelle kjøringer.', 'Recurring ingestion for sources that should stay fresh without manual runs.')}</p>
        </div>
        <label class="ingestions-field">
          {tr('Navn', 'Name')}
          <VerevonInput
            value={props.form.name}
            onInput={(event) => props.onFormChange({ name: event.currentTarget.value })}
            placeholder={tr('Dokumentasjon-crawl', 'Docs crawl')}
          />
        </label>
        <label class="ingestions-field">
          {tr('Type', 'Kind')}
          <VerevonSelect value={props.form.kind} onChange={(event) => props.onFormChange({ kind: event.currentTarget.value })}>
            <For each={props.kinds}>
              {(kind) => <option value={kind}>{kind === 'scrape' ? tr('Skraping', 'Scrape') : kindLabel(kind, tr)}</option>}
            </For>
          </VerevonSelect>
        </label>
        <label class="ingestions-field">
          {tr('Mål-URL', 'Target URL')}
          <VerevonInput
            value={props.form.targetUrl}
            onInput={(event) => props.onFormChange({ targetUrl: event.currentTarget.value })}
            placeholder="https://example.com/docs"
          />
        </label>
        <div class="ingestions-field">
          <label for="ingestions-schedule-cron">{tr('Frekvens (cron)', 'Frequency (cron)')}</label>
          <div class="ingestions-pill-row" role="group" aria-label={tr('Forhåndsvalg', 'Presets')}>
            <For each={cronPresets}>
              {(preset) => (
                <button
                  type="button"
                  class={cn('ingestions-chip-button', props.form.cron === preset.cron && 'ingestions-chip-button--active')}
                  onClick={() => props.onFormChange({ cron: preset.cron })}
                >
                  {cronPresetLabel(preset.id, tr)}
                </button>
              )}
            </For>
          </div>
          <VerevonInput
            id="ingestions-schedule-cron"
            class="ingestions-mono"
            value={props.form.cron}
            onInput={(event) => props.onFormChange({ cron: event.currentTarget.value })}
            placeholder="0 7 * * *"
          />
          <small class="ingestions-field__help">
            {reading() ?? tr('Egendefinert uttrykk (minutt time dag måned ukedag, UTC).', 'Custom expression (minute hour day month weekday, UTC).')}
          </small>
        </div>
        <Show when={props.kinds.length === 0}>
          <div class="ingestions-empty-box" role="status">
            {tr(
              'Ingen tidsplantyper er aktivert på denne installasjonen ennå. Lagring er av til bakgrunnstjenesten kan kjøre dem.',
              'No schedule types are enabled on this installation yet. Saving stays off until the backend can run them.',
            )}
          </div>
        </Show>
        <Button variant="primary" fullWidth type="submit" disabled={!props.kinds.includes(props.form.kind)}>
          <CalendarClock class="size-4" strokeWidth={1.9} />
          {tr('Lagre tidsplan', 'Save schedule')}
        </Button>
      </form>

      <section class="verevon-panel ingestions-card">
        <div>
          <h2>{tr('Tidsplansyklus', 'Schedule lifecycle')}</h2>
          <p>{tr('Sett på pause, gjenoppta, utløs og legg ned gjentakende jobber fra én flate.', 'Pause, resume, trigger, and retire recurring jobs from one surface.')}</p>
        </div>
        <div class="ingestions-card-list">
          <For
            each={props.schedules}
            fallback={
              <EmptyState icon={<CalendarClock class="size-5" strokeWidth={1.8} />} title={tr('Ingen gjentakende tidsplaner ennå.', 'No recurring schedules yet.')}>
                {tr('Velg et forhåndsvalg til venstre for å komme i gang.', 'Pick a preset on the left to get started.')}
              </EmptyState>
            }
          >
            {(schedule) => (
              <article class="ingestions-list-card">
                <div class="ingestions-list-card__main">
                  <div class="ingestions-list-card__copy">
                    <div class="ingestions-title-row">
                      <h3>{schedule.name}</h3>
                      <StatusBadge status={schedule.status} />
                    </div>
                    <p class="ingestions-truncate">{schedule.target}</p>
                    <div class="ingestions-meta-row">
                      <span>{kindLabel(schedule.kind, tr)}</span>
                      <span title={schedule.cron ?? undefined}>
                        {(schedule.cron && describeCron(schedule.cron, tr)) || schedule.cron || schedule.scheduleAt || tr('Manuell frekvens', 'Manual cadence')}
                      </span>
                      <span>
                        {tr('Neste', 'Next')}:{' '}
                        {schedule.nextRunAt ? relativeTime(schedule.nextRunAt, i18n) : tr('Ikke planlagt', 'Not scheduled')}
                      </span>
                      <Show when={schedule.lastRunAt}>
                        <span>{tr('Sist', 'Last')}: {relativeTime(schedule.lastRunAt, i18n)}</span>
                      </Show>
                      <Show when={(schedule.skippedOverlapCount ?? 0) > 0}>
                        <span>{overlapSkipText(schedule.skippedOverlapCount ?? 0, tr)}</span>
                      </Show>
                    </div>
                  </div>
                  <div class="ingestions-actions-row">
                    <Show
                      when={schedule.status === 'paused'}
                      fallback={
                        <Button size="xs" onClick={() => void props.onAction('pause_schedule', schedule.id)}>
                          <TimerReset class="size-4" strokeWidth={1.9} />
                          {tr('Pause', 'Pause')}
                        </Button>
                      }
                    >
                      <Button size="xs" onClick={() => void props.onAction('unpause_schedule', schedule.id)}>
                        <Play class="size-4" strokeWidth={1.9} />
                        {tr('Gjenoppta', 'Resume')}
                      </Button>
                    </Show>
                    <Show when={props.canTrigger}>
                      <Button size="xs" onClick={() => void props.onAction('trigger_schedule', schedule.id)}>
                        <RefreshCw class="size-4" strokeWidth={1.9} />
                        {tr('Utløs', 'Trigger')}
                      </Button>
                    </Show>
                    <Button
                      size="xs"
                      aria-expanded={runsOpen(schedule.id) ? 'true' : 'false'}
                      onClick={() => toggleRuns(schedule.id)}
                    >
                      {runsOpen(schedule.id) ? tr('Skjul kjøringer', 'Hide runs') : tr('Vis kjøringer', 'Show runs')}
                    </Button>
                    <Button size="xs" onClick={() => void props.onAction('delete_schedule', schedule.id)}>
                      <Trash2 class="size-4" strokeWidth={1.9} />
                      {tr('Legg ned', 'Retire')}
                    </Button>
                  </div>
                </div>
                <Show when={runsOpen(schedule.id)}>
                  <ScheduleLatestRun scheduleId={schedule.id} />
                </Show>
              </article>
            )}
          </For>
        </div>
      </section>
    </section>
  )
}

type ScheduleRunReasonLabel = (tr: Tr) => string

/**
 * The reasons a schedule fire can end with, worded for the page (WS15.S4e). The skip codes are the closed set
 * the edge's preflight and the orchestrator write. The run does not carry its skip code yet, so a skipped run
 * reads `schedule_skipped`. A reason outside this map gets no label.
 */
const SCHEDULE_RUN_REASON_LABELS = {
  skipped_zdr: (tr: Tr) => tr('Dataoppbevaring er på (ZDR), så kjøringen ble hoppet over.', 'Zero data retention is on (ZDR), so the run was skipped.'),
  skipped_posture_unknown: (tr: Tr) => tr('Personvernstatusen kunne ikke leses, så kjøringen ble hoppet over.', 'The privacy posture could not be read, so the run was skipped.'),
  skipped_no_entitlement: (tr: Tr) => tr('Kvoten for denne typen er brukt opp.', 'The allowance for this kind is used up.'),
  skipped_overlap: (tr: Tr) => tr('En tidligere kjøring pågikk fortsatt.', 'An earlier run was still going.'),
  skipped_kind_disabled: (tr: Tr) => tr('Denne typen er ikke aktivert på installasjonen.', 'This kind is not enabled on this installation.'),
  skipped_disabled: (tr: Tr) => tr('Tidsplanen er slått av.', 'The schedule is turned off.'),
  preflight_denied: (tr: Tr) => tr('Forhåndssjekken avviste kjøringen.', 'The pre-run check refused the run.'),
  schedule_skipped: (tr: Tr) => tr('Tidsplanen hoppet over kjøringen.', 'The schedule skipped this run.'),
  run_failed: (tr: Tr) => tr('Kjøringen feilet.', 'The run failed.'),
  run_timed_out: (tr: Tr) => tr('Kjøringen fikk tidsavbrudd.', 'The run timed out.'),
  run_cancelled: (tr: Tr) => tr('Kjøringen ble avbrutt.', 'The run was cancelled.'),
} satisfies Record<string, ScheduleRunReasonLabel>

type ScheduleRunReason = keyof typeof SCHEDULE_RUN_REASON_LABELS

function isScheduleRunReason(value: string): value is ScheduleRunReason {
  return Object.prototype.hasOwnProperty.call(SCHEDULE_RUN_REASON_LABELS, value)
}

function scheduleRunReasonLabel(reason: string | null, tr: Tr): string | null {
  return reason !== null && isScheduleRunReason(reason) ? SCHEDULE_RUN_REASON_LABELS[reason](tr) : null
}

/** The skip line of a schedule row. It is shown only when Quarry counted at least one skip. */
function overlapSkipText(count: number, tr: Tr) {
  return tr(`Hoppet over (overlapp): ${count}`, `Skipped (overlap): ${count}`)
}

/**
 * The newest fire of one schedule, read when its row opens (WS15.S4e). It mounts only while the row is open,
 * so a closed row sends no request, and an unmount aborts a read that is still in flight.
 */
function ScheduleLatestRun(props: { scheduleId: string }) {
  const i18n = useI18n()
  const tr = i18n.tr
  // undefined while the read is in flight; null when the schedule has no runs yet.
  const [latest, setLatest] = createSignal<ScheduleRun | null | undefined>(undefined)
  const [failed, setFailed] = createSignal(false)

  createEffect(
    () => props.scheduleId,
    (scheduleId) => {
      const controller = new AbortController()
      setLatest(undefined)
      setFailed(false)
      listScheduleRuns(scheduleId, controller.signal)
        .then((runs) => setLatest(runs[0] ?? null))
        .catch(() => {
          if (!controller.signal.aborted) setFailed(true)
        })
      return () => controller.abort()
    },
  )

  function content() {
    if (failed()) return <span role="alert">{tr('Kunne ikke hente kjøringene.', 'Could not load the runs.')}</span>
    const run = latest()
    if (run === undefined) return <span>{tr('Henter siste kjøring…', 'Loading the latest run…')}</span>
    if (run === null) return <span>{tr('Ingen kjøringer ennå.', 'No runs yet.')}</span>
    const reason = scheduleRunReasonLabel(run.reason, tr)
    return (
      <>
        <StatusBadge status={run.status} />
        {reason ? <span>{reason}</span> : null}
        <span>
          {tr('Startet', 'Started')}: {relativeTime(run.startedAt, i18n)}
        </span>
      </>
    )
  }

  return (
    <div class="ingestions-meta-row" aria-live="polite">
      {content()}
    </div>
  )
}
