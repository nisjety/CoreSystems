import { CalendarClock, ChevronDown, ChevronRight, Loader2, Trash2 } from '@/shared/icons'
import { createMemo, createSignal, For, Show } from 'solid-js'

import { createResource } from '@/shared/lib/create-resource-compat'
import {
  deleteCronSchedule,
  listScheduleFires,
  updateCronSchedule,
  type CronSchedule,
} from '@/shared/api/cron-client'
import { cronExprToLabel, listMySchedules } from '@/shared/api/schedule-from-chat'
import { translateApiError, useI18n } from '@/shared/i18n'
import { getSession } from '@/shared/session/session-store'
import { chatThreadHref } from '../lib/chat-thread-deep-link'
import { selectChatThread } from '../lib/chat-thread-history'
import { ScheduleDialog } from './ScheduleDialog'

/**
 * "Planlagte kjøringer" — list/pause/delete a schedule from chat (chat-wave2
 * "schedule" spec §2.1). A lightweight sibling of the admin Settings page's
 * `CronSchedulesSection`, filtered to the signed-in user's OWN schedules and
 * with no workflow/JSON authoring — this surface only ever created schedules
 * through `ScheduleDialog`, which never sets `workflow_type` or
 * `workflow_input`.
 *
 * "Se resultater" opens the last fires of a schedule, each linking to the
 * scheduled thread that holds its answer. It stays disabled unless
 * VITE_SCHEDULE_RESULTS is the literal "true", so a rollback restores the
 * disabled button without a code change.
 *
 * A schedule whose approval has lapsed (WS20-S8) cannot be resumed by a bare
 * toggle. The panel offers "Forny godkjenning" instead, which opens the
 * consent card again with the schedule's goal; the renewal is a new schedule.
 */
function formatWhen(value?: string | null, locale?: string): string {
  if (!value) return '—'
  try {
    return new Date(value).toLocaleString(locale ?? 'nb-NO', { dateStyle: 'short', timeStyle: 'short' })
  } catch {
    return value
  }
}

const scheduleResultsEnabled = () => import.meta.env.VITE_SCHEDULE_RESULTS === 'true'

/** The expiry of a stored approval's `expires_at`, or null when it cannot be read. */
function approvalExpiry(text: unknown): Date | null {
  if (typeof text !== 'string') return null
  try {
    const parsed: unknown = JSON.parse(text)
    const raw = parsed && typeof parsed === 'object' ? (parsed as { expires_at?: unknown }).expires_at : undefined
    if (typeof raw !== 'string') return null
    const expires = new Date(raw)
    return Number.isNaN(expires.getTime()) ? null : expires
  } catch {
    return null
  }
}

/**
 * The consent contract a schedule carries (WS20-S4), read from its stored template.
 * An approval that cannot be read or has passed its expiry cannot fire, which is
 * the same rule capability-core applies (WS20-S8).
 */
function approvalOf(schedule: CronSchedule, now: number): { expiresAt: Date | null; lapsed: boolean } | null {
  const template = schedule.task_template
  if (!template || typeof template !== 'object' || !('approval_contract' in template)) return null
  const expiresAt = approvalExpiry(template.approval_contract)
  return { expiresAt, lapsed: !expiresAt || expiresAt.getTime() <= now }
}

function approvalLapsed(schedule: CronSchedule): boolean {
  return approvalOf(schedule, Date.now())?.lapsed ?? false
}

/** The goal a schedule was created with, which the renewal's consent card starts from. */
function goalOf(schedule: CronSchedule): string {
  const template = schedule.task_template
  if (!template || typeof template !== 'object' || !('description' in template)) return ''
  return typeof template.description === 'string' ? template.description : ''
}

// Closed tokens from cron_fires and the run's outcome. A token outside this
// list is shown as-is rather than guessed at.
function fireTokenLabel(token: string, i18n: ReturnType<typeof useI18n>): string {
  switch (token) {
    case 'pending':
      return i18n.tr('Venter', 'Pending')
    case 'running':
      return i18n.tr('Kjører', 'Running')
    case 'completed':
      return i18n.tr('Fullført', 'Completed')
    case 'failed':
      return i18n.tr('Feilet', 'Failed')
    case 'unknown':
      return i18n.tr('Ukjent', 'Unknown')
    case 'paused':
      return i18n.tr('Pauset', 'Paused')
    case 'unknown_outcome':
      return i18n.tr('Ukjent utfall', 'Unknown outcome')
    case 'needs_review':
      return i18n.tr('Trenger gjennomgang', 'Needs review')
    case 'paused_expired':
      return i18n.tr('Utløpt', 'Expired')
    case 'paused_failures':
      return i18n.tr('Pauset etter feil', 'Paused after failures')
    default:
      return token
  }
}

function ScheduleFires(props: { orgId: string; cronId: string }) {
  const i18n = useI18n()
  const [fires] = createResource(
    () => [props.orgId, props.cronId] as const,
    ([org, id]) => listScheduleFires(org, id),
  )
  const openTranscript = (event: MouseEvent, threadId: string) => {
    if (event.button !== 0 || event.metaKey || event.altKey || event.ctrlKey || event.shiftKey) return
    event.preventDefault()
    selectChatThread(threadId)
  }
  return (
    <div class="verevon-scheduled-runs-panel__fires">
      <Show
        when={!fires.loading}
        fallback={
          <p role="status" aria-busy="true">
            <Loader2 size={14} aria-hidden="true" /> {i18n.tr('Laster resultater…', 'Loading results…')}
          </p>
        }
      >
        <Show
          when={!fires.error}
          fallback={
            <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
              {i18n.tr('Kunne ikke laste resultatene.', 'Could not load the results.')}
            </p>
          }
        >
          <Show
            when={(fires() ?? []).length > 0}
            fallback={<p>{i18n.tr('Ingen kjøringer ennå.', 'No runs yet.')}</p>}
          >
            <ul>
              <For each={fires()}>
                {(fire) => (
                  <li class="verevon-scheduled-runs-panel__fire">
                    <time datetime={fire.fired_at}>{formatWhen(fire.fired_at, i18n.locale() === 'no' ? 'nb-NO' : 'en-US')}</time>
                    <span class="verevon-scheduled-runs-panel__chip">{fireTokenLabel(fire.status, i18n)}</span>
                    <Show when={fire.outcome}>
                      {(outcome) => (
                        <span class="verevon-scheduled-runs-panel__chip">
                          {fireTokenLabel(outcome(), i18n)}
                        </span>
                      )}
                    </Show>
                    <span class="verevon-scheduled-runs-panel__chip">{i18n.tr('Plan', 'Schedule')}</span>
                    <Show when={fire.withheld_effects > 0}>
                      <span>
                        {i18n.tr('Holdt tilbake', 'Withheld')}: {fire.withheld_effects}
                      </span>
                    </Show>
                    <Show when={fire.thread_id}>
                      {(threadId) => (
                        <a link href={chatThreadHref(threadId())} onClick={(event) => openTranscript(event, threadId())}>
                          {i18n.tr('Åpne samtalen', 'Open transcript')}
                        </a>
                      )}
                    </Show>
                  </li>
                )}
              </For>
            </ul>
          </Show>
        </Show>
      </Show>
    </div>
  )
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
  const [openResultsId, setOpenResultsId] = createSignal<string | null>(null)
  const [renewing, setRenewing] = createSignal<CronSchedule | null>(null)
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
              <>
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
                      <Show when={approvalOf(schedule, Date.now())}>
                        {(approval) => (
                          <>
                            {' · '}
                            {approval().lapsed
                              ? i18n.tr('godkjenning utløpt', 'approval expired')
                              : `${i18n.tr('godkjenning til', 'approval until')} ${formatWhen(approval().expiresAt?.toISOString(), i18n.locale() === 'no' ? 'nb-NO' : 'en-US')}`}
                          </>
                        )}
                      </Show>
                    </span>
                  </div>
                  <div class="verevon-scheduled-runs-panel__actions">
                    <Show
                      when={scheduleResultsEnabled()}
                      fallback={
                        <button type="button" disabled title={i18n.tr('Kommer snart', 'Coming soon')} aria-disabled="true">
                          {i18n.tr('Se resultater', 'See results')}
                        </button>
                      }
                    >
                      <button
                        type="button"
                        aria-expanded={openResultsId() === schedule.id ? 'true' : 'false'}
                        aria-controls={`scheduled-fires-${schedule.id}`}
                        onClick={() => {
                          setOpenResultsId((current) => (current === schedule.id ? null : schedule.id))
                        }}
                      >
                        {openResultsId() === schedule.id ? (
                          <ChevronDown size={14} aria-hidden="true" />
                        ) : (
                          <ChevronRight size={14} aria-hidden="true" />
                        )}{' '}
                        {i18n.tr('Se resultater', 'See results')}
                      </button>
                    </Show>
                    <Show when={schedule.enabled || !approvalLapsed(schedule)}>
                      <button
                        type="button"
                        disabled={busyId() === schedule.id}
                        onClick={() => void handleToggle(schedule)}
                        aria-label={`${schedule.enabled ? i18n.tr('Pause', 'Pause') : i18n.tr('Aktiver', 'Resume')} ${schedule.name}`}
                      >
                        {schedule.enabled ? i18n.tr('Pause', 'Pause') : i18n.tr('Aktiver', 'Resume')}
                      </button>
                    </Show>
                    <Show when={approvalLapsed(schedule)}>
                      <button
                        type="button"
                        disabled={busyId() === schedule.id}
                        onClick={() => setRenewing(schedule)}
                        aria-label={`${i18n.tr('Forny godkjenning', 'Renew approval')} ${schedule.name}`}
                      >
                        {i18n.tr('Forny godkjenning', 'Renew approval')}
                      </button>
                    </Show>
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
                <Show when={scheduleResultsEnabled() && openResultsId() === schedule.id}>
                  <div id={`scheduled-fires-${schedule.id}`}>
                    <ScheduleFires orgId={orgId()} cronId={schedule.id} />
                  </div>
                </Show>
              </>
            )}
          </For>
        </Show>
      </Show>
      <Show when={renewing()}>
        {(target) => (
          <ScheduleDialog
            seedTitle={target().name}
            seedGoal={goalOf(target())}
            threadOriginHint="composer"
            onClose={() => setRenewing(null)}
            onCreated={() => void refetch()}
          />
        )}
      </Show>
    </div>
  )
}
