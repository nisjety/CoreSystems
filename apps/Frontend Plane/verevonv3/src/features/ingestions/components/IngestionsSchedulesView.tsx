import { For, Show } from 'solid-js'
import { CalendarClock, Play, RefreshCw, TimerReset, Trash2 } from '@/shared/icons'
import { useI18n } from '@/shared/i18n'
import { cn } from '@/shared/lib/cn'
import { Button } from '@/shared/ui/Button'
import { VerevonInput } from '@/shared/ui/verevon/VerevonInput'
import { VerevonSelect } from '@/shared/ui/verevon/VerevonSelect'
import type { ScheduleItem, SchedulePauseAction, ScheduleStateAction } from '@/shared/api/ingestions-client'
import { cronPresetLabel, cronPresets, describeCron } from '@/features/ingestions/lib/cron'
import { EmptyState, StatusBadge, kindLabel, relativeTime } from '@/features/ingestions/components/ingestions-ui'

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
                    <Button size="xs" onClick={() => void props.onAction('delete_schedule', schedule.id)}>
                      <Trash2 class="size-4" strokeWidth={1.9} />
                      {tr('Legg ned', 'Retire')}
                    </Button>
                  </div>
                </div>
              </article>
            )}
          </For>
        </div>
      </section>
    </section>
  )
}
