import { CalendarClock, Loader2, Trash2 } from '@/shared/icons'
import { createMemo, createSignal, For, Show } from 'solid-js'

import { createResource } from '@/shared/lib/create-resource-compat'
import { deleteCronSchedule, updateCronSchedule, type CronSchedule } from '@/shared/api/cron-client'
import { cronExprToLabel, listMySchedules } from '@/shared/api/schedule-from-chat'
import { translateApiError, useI18n } from '@/shared/i18n'
import { getSession } from '@/shared/session/session-store'

/**
 * "Planlagte kjøringer" — list/pause/delete a schedule from chat (chat-wave2
 * "schedule" spec §2.1). A lightweight sibling of the admin Settings page's
 * `CronSchedulesSection`, filtered to the signed-in user's OWN schedules and
 * with no workflow/JSON authoring — this surface only ever created schedules
 * through `ScheduleDialog`, which never sets `workflow_type` or
 * `workflow_input`.
 *
 * "Se resultater" (Phase B) is rendered disabled with a "Kommer snart" note
 * for every row: capability-core has no `GET /v1/cron/{id}/runs` endpoint and
 * `cron_fires` has no `thread_id` column yet (spec §2.3), so there is nothing
 * to link to today. The user-core/gateway read-DECISION fix this task also
 * ships (personal-Space owners can now obtain a read decision for their
 * schedule's result thread) is the other half of Phase B and is deliberately
 * a backend-only, unwired capability until that listing endpoint exists —
 * shipping the link before both halves are live would 403 on every click.
 */
function formatWhen(value?: string | null, locale?: string): string {
  if (!value) return '—'
  try {
    return new Date(value).toLocaleString(locale ?? 'nb-NO', { dateStyle: 'short', timeStyle: 'short' })
  } catch {
    return value
  }
}

export function ScheduledRunsPanel() {
  const i18n = useI18n()
  const session = getSession()
  const orgId = createMemo(() => session.activeOrg?.id?.trim() ?? '')
  const myUserId = createMemo(() => session.user?.id ?? '')

  const [schedules, { refetch }] = createResource(
    () => (orgId() && myUserId() ? ([orgId(), myUserId()] as const) : undefined),
    ([org, user]) => listMySchedules(org, user),
  )

  const [busyId, setBusyId] = createSignal<string | null>(null)
  const [actionError, setActionError] = createSignal<string | null>(null)
  const list = createMemo(() => schedules() ?? [])

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
    if (typeof window !== 'undefined' && !window.confirm(`${i18n.tr('Slette planen', 'Delete the schedule')} «${schedule.name}»?`)) {
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
    <div class="verevon-scheduled-runs-panel">
      <Show when={actionError()}>
        {(message) => (
          <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
            {message()}
          </p>
        )}
      </Show>
      <Show when={schedules.error}>
        <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
          {i18n.tr('Kunne ikke laste planlagte kjøringer.', 'Could not load scheduled runs.')}
        </p>
      </Show>
      <Show
        when={!schedules.loading}
        fallback={
          <p role="status" aria-busy="true">
            <Loader2 size={14} aria-hidden="true" /> {i18n.tr('Laster planer…', 'Loading schedules…')}
          </p>
        }
      >
        <Show
          when={list().length > 0}
          fallback={
            <p>
              <CalendarClock size={14} aria-hidden="true" />{' '}
              {i18n.tr('Ingen planlagte kjøringer ennå.', 'No scheduled runs yet.')}
            </p>
          }
        >
          <For each={list()}>
            {(schedule) => (
              <div class="verevon-scheduled-runs-panel__row">
                <div>
                  <p>{schedule.name}</p>
                  <span class="verevon-scheduled-runs-panel__row-meta">
                    {cronExprToLabel(schedule.schedule_expr, i18n.locale() === 'no')} · {schedule.timezone}
                    {' · '}
                    {schedule.enabled
                      ? i18n.tr('aktiv', 'active')
                      : i18n.tr('pauset', 'paused')}
                    {' · '}
                    {i18n.tr('neste', 'next')}: {formatWhen(schedule.next_fire_at, i18n.locale() === 'no' ? 'nb-NO' : 'en-US')}
                  </span>
                </div>
                <div class="verevon-scheduled-runs-panel__actions">
                  <button
                    type="button"
                    disabled
                    title={i18n.tr('Kommer snart', 'Coming soon')}
                    aria-disabled="true"
                  >
                    {i18n.tr('Se resultater', 'See results')}
                  </button>
                  <button
                    type="button"
                    disabled={busyId() === schedule.id}
                    onClick={() => void handleToggle(schedule)}
                    aria-label={`${schedule.enabled ? i18n.tr('Pause', 'Pause') : i18n.tr('Aktiver', 'Resume')} ${schedule.name}`}
                  >
                    {schedule.enabled ? i18n.tr('Pause', 'Pause') : i18n.tr('Aktiver', 'Resume')}
                  </button>
                  <button
                    type="button"
                    disabled={busyId() === schedule.id}
                    onClick={() => void handleDelete(schedule)}
                    aria-label={`${i18n.tr('Slett', 'Delete')} ${schedule.name}`}
                  >
                    <Trash2 size={14} aria-hidden="true" />
                  </button>
                </div>
              </div>
            )}
          </For>
        </Show>
      </Show>
    </div>
  )
}
