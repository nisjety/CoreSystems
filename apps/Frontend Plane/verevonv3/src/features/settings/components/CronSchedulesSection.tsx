import { CalendarClock, Clock, Loader2, Trash2 } from 'lucide-solid'
import { createMemo, createResource, createSignal, For, Show } from 'solid-js'
import {
  createCronSchedule,
  deleteCronSchedule,
  listCronSchedules,
  updateCronSchedule,
  type CronSchedule,
} from '@/shared/api/cron-client'
import { translateApiError, useI18n } from '@/shared/i18n'
import { SectionHeader, SettingsButton } from '@/features/settings/components/settings-ui'
import { VerevonInput } from '@/shared/ui/verevon/VerevonInput'
import { getSession } from '@/shared/session/session-store'
import { hasWorkspaceAdminAccess } from '@/shared/session/access'

/**
 * Planlagte kjøringer (cron) — tidsstyrte oppgaver agenten kjører automatisk.
 *
 * Hver plan har et cron-uttrykk («min time dag måned ukedag»). capability-core
 * sin sweeper fyrer forfalte planer: den lager en oppgave fra malen, logger
 * kjøringen og beregner neste kjøring. Bare administratorer kan opprette, endre
 * eller slette planer.
 */
function formatWhen(value?: string | null): string {
  if (!value) return '—'
  try {
    return new Date(value).toLocaleString('nb-NO', { dateStyle: 'short', timeStyle: 'short' })
  } catch {
    return value
  }
}

export function CronSchedulesSection() {
  const i18n = useI18n()
  const session = getSession()
  const orgId = createMemo(() => session.activeOrg?.id ?? '')
  const isAdmin = createMemo(() => hasWorkspaceAdminAccess(session))

  const [schedules, { refetch }] = createResource(
    () => orgId() || undefined,
    (id) => listCronSchedules(id).then((result) => result.schedules),
  )

  const [name, setName] = createSignal('')
  const [expr, setExpr] = createSignal('')
  const [timezone, setTimezone] = createSignal('UTC')
  const [title, setTitle] = createSignal('')

  const [submitting, setSubmitting] = createSignal(false)
  const [busyId, setBusyId] = createSignal<string | null>(null)
  const [formError, setFormError] = createSignal<string | null>(null)
  const [actionError, setActionError] = createSignal<string | null>(null)

  const list = createMemo(() => schedules() ?? [])

  const resetForm = () => {
    setName('')
    setExpr('')
    setTimezone('UTC')
    setTitle('')
  }

  const handleCreate = async (event: Event) => {
    event.preventDefault()
    const id = orgId()
    if (!id || submitting()) return

    const trimmedName = name().trim()
    const trimmedExpr = expr().trim()
    if (!trimmedName || !trimmedExpr) {
      setFormError('Navn og cron-uttrykk er påkrevd.')
      return
    }

    setSubmitting(true)
    setFormError(null)
    try {
      await createCronSchedule(id, {
        name: trimmedName,
        schedule_expr: trimmedExpr,
        timezone: timezone().trim() || 'UTC',
        task_template: {
          kind: 'cron',
          title: title().trim() || trimmedName,
        },
        enabled: true,
      })
      resetForm()
      await refetch()
    } catch (err) {
      setFormError(
        translateApiError(err, i18n.tr, { no: 'Kunne ikke opprette planen.', en: 'Could not create the schedule.' }),
      )
    } finally {
      setSubmitting(false)
    }
  }

  const handleToggle = async (schedule: CronSchedule) => {
    const id = orgId()
    if (!id || busyId()) return
    setBusyId(schedule.id)
    setActionError(null)
    try {
      await updateCronSchedule(id, schedule.id, { enabled: !schedule.enabled })
      await refetch()
    } catch (err) {
      setActionError(
        translateApiError(err, i18n.tr, { no: 'Kunne ikke oppdatere planen.', en: 'Could not update the schedule.' }),
      )
    } finally {
      setBusyId(null)
    }
  }

  const handleDelete = async (schedule: CronSchedule) => {
    const id = orgId()
    if (!id || busyId()) return
    if (
      typeof window !== 'undefined' &&
      !window.confirm(`Slette planen «${schedule.name}»?`)
    ) {
      return
    }
    setBusyId(schedule.id)
    setActionError(null)
    try {
      await deleteCronSchedule(id, schedule.id)
      await refetch()
    } catch (err) {
      setActionError(
        translateApiError(err, i18n.tr, { no: 'Kunne ikke slette planen.', en: 'Could not delete the schedule.' }),
      )
    } finally {
      setBusyId(null)
    }
  }

  return (
    <>
      <SectionHeader
        title="Planlagte kjøringer"
        description="Tidsstyrte oppgaver agenten kjører automatisk. Hver plan har et cron-uttrykk; sweeperen lager en oppgave, logger kjøringen og beregner neste kjøring."
      />

      <Show when={actionError()}>
        {(message) => (
          <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
            {message()}
          </p>
        )}
      </Show>

      <Show when={schedules.error}>
        <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
          Kunne ikke laste planlagte kjøringer.
        </p>
      </Show>

      <Show
        when={!schedules.loading}
        fallback={
          <p class="verevon-settings-subnote" role="status" aria-busy="true">
            Laster planer…
          </p>
        }
      >
        <Show
          when={list().length > 0}
          fallback={
            <div class="verevon-settings-list-card">
              <p class="verevon-settings-empty-row">
                Ingen planlagte kjøringer ennå. Legg til en nedenfor.
              </p>
            </div>
          }
        >
          <div class="verevon-settings-list-card">
            <For each={list()}>
              {(schedule) => (
                <div class="verevon-settings-integration-row">
                  <div>
                    <p>
                      {schedule.name}{' '}
                      <span class="verevon-trust-chip" aria-label={`Status: ${schedule.enabled ? 'aktiv' : 'pauset'}`}>
                        {schedule.enabled ? 'aktiv' : 'pauset'}
                      </span>
                    </p>
                    <span>
                      <code>{schedule.schedule_expr}</code> · {schedule.timezone}
                      {' '}· <Clock size={12} aria-hidden="true" /> neste: {formatWhen(schedule.next_fire_at)}
                      <Show when={schedule.last_fire_at}> · sist: {formatWhen(schedule.last_fire_at)}</Show>
                    </span>
                  </div>
                  <Show when={isAdmin()}>
                    <div>
                      <SettingsButton
                        settingsSize="sm"
                        disabled={busyId() === schedule.id}
                        onClick={() => void handleToggle(schedule)}
                        aria-label={`${schedule.enabled ? 'Pause' : 'Aktiver'} ${schedule.name}`}
                      >
                        <Show
                          when={busyId() === schedule.id}
                          fallback={<>{schedule.enabled ? 'Pause' : 'Aktiver'}</>}
                        >
                          <Loader2 size={14} aria-hidden="true" /> Lagrer…
                        </Show>
                      </SettingsButton>{' '}
                      <SettingsButton
                        settingsSize="sm"
                        danger
                        disabled={busyId() === schedule.id}
                        onClick={() => void handleDelete(schedule)}
                        aria-label={`Slett ${schedule.name}`}
                      >
                        <Trash2 size={14} aria-hidden="true" /> Slett
                      </SettingsButton>
                    </div>
                  </Show>
                </div>
              )}
            </For>
          </div>
        </Show>
      </Show>

      <Show
        when={isAdmin()}
        fallback={
          <p class="verevon-settings-subnote">
            <CalendarClock size={14} aria-hidden="true" /> Bare administratorer kan
            opprette eller endre planlagte kjøringer.
          </p>
        }
      >
        <form class="verevon-settings-field-grid verevon-settings-field-grid--spaced" onSubmit={handleCreate}>
          <label for="cron-name" class="verevon-settings-field">
            <span class="verevon-settings-label">Navn</span>
            <span class="verevon-settings-input-wrap">
              <VerevonInput
                id="cron-name"
                value={name()}
                required
                placeholder="f.eks. Daglig rapport"
                onInput={(event) => setName(event.currentTarget.value)}
                class="verevon-settings-input"
              />
            </span>
          </label>

          <label for="cron-expr" class="verevon-settings-field">
            <span class="verevon-settings-label">Cron-uttrykk</span>
            <span class="verevon-settings-input-wrap">
              <VerevonInput
                id="cron-expr"
                value={expr()}
                required
                placeholder="min time dag måned ukedag — f.eks. 0 9 * * 1"
                onInput={(event) => setExpr(event.currentTarget.value)}
                class="verevon-settings-input"
              />
            </span>
            <span class="verevon-settings-help">
              5 felt. Eksempel: «0 9 * * 1» = hver mandag kl. 09:00.
            </span>
          </label>

          <label for="cron-tz" class="verevon-settings-field">
            <span class="verevon-settings-label">Tidssone</span>
            <span class="verevon-settings-input-wrap">
              <VerevonInput
                id="cron-tz"
                value={timezone()}
                placeholder="UTC"
                onInput={(event) => setTimezone(event.currentTarget.value)}
                class="verevon-settings-input"
              />
            </span>
          </label>

          <label for="cron-title" class="verevon-settings-field">
            <span class="verevon-settings-label">Oppgavetittel (valgfritt)</span>
            <span class="verevon-settings-input-wrap">
              <VerevonInput
                id="cron-title"
                value={title()}
                placeholder="Tittel på oppgaven som opprettes ved hver kjøring"
                onInput={(event) => setTitle(event.currentTarget.value)}
                class="verevon-settings-input"
              />
            </span>
          </label>

          <Show when={formError()}>
            {(message) => (
              <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
                {message()}
              </p>
            )}
          </Show>

          <div>
            <SettingsButton type="submit" variant="primary" settingsSize="sm" disabled={submitting()}>
              <Show
                when={submitting()}
                fallback={<><CalendarClock size={14} aria-hidden="true" /> Opprett plan</>}
              >
                <Loader2 size={14} aria-hidden="true" /> Oppretter…
              </Show>
            </SettingsButton>
          </div>
        </form>
      </Show>

      <p class="verevon-settings-subnote">
        <CalendarClock size={14} aria-hidden="true" /> Org-tilhørighet utledes fra den
        verifiserte økten. Planer gjelder hele organisasjonen.
      </p>
    </>
  )
}
