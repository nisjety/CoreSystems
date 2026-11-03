import { CalendarClock, Loader2, Trash2 } from '@/shared/icons'
import { createEffect, createMemo, createSignal, For, Show } from 'solid-js'

import { ApiError } from '@/shared/api/api-error'
import {
  deleteCronSchedule,
  listCronSchedules,
  listScheduleFires,
  updateCronSchedule,
  type CronSchedule,
  type CronScheduleFire,
} from '@/shared/api/cron-client'
import { cronExprToLabel, listMySchedules, SCHEDULE_PRESETS } from '@/shared/api/schedule-from-chat'
import { isZdrCode, zdrRefusalPresentation } from '@/shared/api/zdr-refusals'
import { ScheduleDialog } from '@/features/chat/components/ScheduleDialog'
import { chatThreadHref } from '@/features/chat/lib/chat-thread-deep-link'
import { selectChatThread } from '@/features/chat/lib/chat-thread-history'
import { translateApiError, useI18n } from '@/shared/i18n'
import { createResource } from '@/shared/lib/create-resource-compat'
import { hasWorkspaceAdminAccess } from '@/shared/session/access'
import { getSession } from '@/shared/session/session-store'
import { Badge } from '@/shared/ui/Badge'
import { Button } from '@/shared/ui/Button'
import {
  approvalLapsed,
  approvalOf,
  automationStateOf,
  countAutomations,
  filterAutomations,
  filterOf,
  goalOf,
  type AutomationFilter,
  type AutomationRow,
  type AutomationState,
} from '../lib/automation-state'

/**
 * The one schedule list (EP-6-S1 hub, EP-6-S4 shared). It reads schedules through the calls the
 * chat panel and Settings already made, and every row action calls the same cron-client
 * functions, so the WS20 guards exist in one place.
 *
 * - `mine` is the signed-in person's own schedules. `org` is every schedule in the org, for the
 *   Settings page, where pause and delete are open to an admin and nothing else is offered.
 * - `compact` is the chat mount: the first `limit` rows, no filter tabs, no edits. Its "See
 *   results" stays on screen, disabled, while the results flag is off, as the chat always showed it.
 * - The full hub reads a paused schedule's reason from its last fire, once per pause. The other
 *   mounts show the pause without a reason, as they always did.
 *
 * A lapsed approval is never resumed by a bare toggle: renewal is the way back (WS20-S8). A goal and
 * an approval are never edited in place, because the template digest is signed.
 */

type AutomationsScope = 'mine' | 'org'

interface AutomationsListProps {
  readonly scope: AutomationsScope
  readonly compact?: boolean
  readonly limit?: number
  /** Changes when a parent wrote a schedule itself (a create), so the list reads the org again. */
  readonly revision?: number
}

type Tr = ReturnType<typeof useI18n>['tr']
type RetimeDraft = { readonly id: string; readonly presetId: string; readonly custom: string }
type ListKey = { readonly scope: AutomationsScope; readonly org: string; readonly user: string; readonly revision: number }

const FILTERS: readonly AutomationFilter[] = ['active', 'paused', 'expired']
// The create dialog's name field accepts 120 characters, so a copy's name is cut to fit.
const NAME_MAX = 120
// The workflow a schedule fires when it names none. Settings shows it for schedules that name none.
const DEFAULT_WORKFLOW_TYPE = 'InteractiveRunSupervision'

function scheduleResultsEnabled(): boolean {
  return import.meta.env.VITE_SCHEDULE_RESULTS === 'true'
}

function formatWhen(value?: string | null, locale?: string): string {
  if (!value) return '—'
  try {
    return new Date(value).toLocaleString(locale ?? 'nb-NO', { dateStyle: 'short', timeStyle: 'short' })
  } catch {
    return value
  }
}

function filterLabel(filter: AutomationFilter, tr: Tr): string {
  switch (filter) {
    case 'active':
      return tr('Aktive', 'Active')
    case 'paused':
      return tr('Pausede', 'Paused')
    default:
      return tr('Utløpte', 'Expired')
  }
}

function stateLabel(state: AutomationState, tr: Tr): string {
  switch (state) {
    case 'active':
      return tr('Aktiv', 'Active')
    case 'paused_failures':
      return tr('Pauset etter feil', 'Paused after failures')
    case 'expired':
      return tr('Utløpt', 'Expired')
    default:
      return tr('Pauset', 'Paused')
  }
}

function stateTone(state: AutomationState): 'accent' | 'neutral' | 'risk' {
  if (state === 'active') return 'accent'
  if (state === 'expired') return 'risk'
  return 'neutral'
}

function copyNameOf(name: string, tr: Tr): string {
  return tr(`Kopi av ${name}`, `Copy of ${name}`).slice(0, NAME_MAX)
}

/** The cadence a draft names: the preset's expression, or the custom text when the preset is "custom". */
function draftExpr(draft: RetimeDraft): string {
  const preset = SCHEDULE_PRESETS.find((candidate) => candidate.id === draft.presetId)
  return (preset?.cronExpr ?? draft.custom).trim()
}

function retimeDraftFor(schedule: CronSchedule): RetimeDraft {
  const expr = schedule.schedule_expr.trim()
  const preset = SCHEDULE_PRESETS.find((candidate) => candidate.cronExpr === expr)
  return { id: schedule.id, presetId: preset?.id ?? 'custom', custom: preset ? '' : expr }
}

function workflowTypeOf(schedule: CronSchedule): string {
  const template = schedule.task_template
  if (template && typeof template === 'object' && !Array.isArray(template)) {
    const value = (template as { workflow_type?: unknown }).workflow_type
    if (typeof value === 'string' && value.trim()) return value.trim()
  }
  return DEFAULT_WORKFLOW_TYPE
}

/**
 * A failure's words. A zero-retention refusal reads as the scheduled-runs refusal,
 * with its next step, the same copy the chat shows for it; anything else keeps the
 * caller's own sentence.
 */
function messageFor(err: unknown, tr: Tr, fallback: { no: string; en: string }): string {
  if (err instanceof ApiError && err.code !== null && isZdrCode(err.code)) {
    const shown = zdrRefusalPresentation(err.code, 'scheduled_step', tr)
    return shown.next === null ? shown.message : `${shown.message} ${shown.next}`
  }
  return translateApiError(err, tr, fallback)
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

export function AutomationsList(props: AutomationsListProps) {
  const i18n = useI18n()
  const session = getSession()
  const orgId = createMemo(() => session.activeOrg?.id?.trim() ?? '')
  const myUserId = createMemo(() => session.user?.id ?? '')
  const isAdmin = createMemo(() => hasWorkspaceAdminAccess(session))
  const localeTag = () => (i18n.locale() === 'no' ? 'nb-NO' : 'en-US')
  // Only the full hub has filters, the paused reason and the edits that belong to the creator.
  const full = () => props.scope === 'mine' && !props.compact
  // Pause and delete belong to the creator and, on the Settings page, to an org admin. Renew and copy are the creator's alone.
  const canManage = () => props.scope === 'mine' || isAdmin()

  // capability-core still returns every schedule in the org, goal text included (D2-121). A `mine`
  // list shows only the signed-in person's own, a client filter until the server offers creator=self.
  const [schedules, { refetch }] = createResource(
    () => {
      const org = orgId()
      const user = myUserId()
      if (!org || (props.scope === 'mine' && !user)) return undefined
      const key: ListKey = { scope: props.scope, org, user, revision: props.revision ?? 0 }
      return key
    },
    (key) =>
      key.scope === 'org'
        ? listCronSchedules(key.org).then((result) => result.schedules)
        : listMySchedules(key.org, key.user),
  )

  const [filter, setFilter] = createSignal<AutomationFilter>('active')
  const [busyId, setBusyId] = createSignal<string | null>(null)
  const [actionError, setActionError] = createSignal<string | null>(null)
  const [openResultsId, setOpenResultsId] = createSignal<string | null>(null)
  const [renewing, setRenewing] = createSignal<CronSchedule | null>(null)
  const [copying, setCopying] = createSignal<CronSchedule | null>(null)
  const [retiming, setRetiming] = createSignal<RetimeDraft | null>(null)
  const [lastFires, setLastFires] = createSignal<Readonly<Record<string, CronScheduleFire | null>>>({})

  const rows = createMemo((): AutomationRow[] => {
    const now = Date.now()
    return (schedules() ?? []).map((schedule) => ({
      schedule,
      state: automationStateOf(schedule, lastFires()[schedule.id], now),
    }))
  })
  const counts = createMemo(() => countAutomations(rows()))
  const visible = createMemo(() => {
    const listed = full() ? filterAutomations(rows(), filter()) : rows()
    return props.limit === undefined ? listed : listed.slice(0, props.limit)
  })

  // The reason a schedule is paused lives on its last fire, so the full hub reads it only for paused
  // rows on screen, and once per pause: the key carries the schedule's own updated_at, which a pause
  // or a resume changes.
  const requestedFires = new Set<string>()
  createEffect(
    () => {
      const org = orgId()
      const pending = full()
        ? visible()
            .filter((row) => filterOf(row.state) === 'paused')
            .map((row) => ({ id: row.schedule.id, key: `${row.schedule.id}@${row.schedule.updated_at ?? ''}` }))
        : []
      return { org, pending }
    },
    ({ org, pending }) => {
      for (const { id, key } of pending) {
        if (!org || requestedFires.has(key)) continue
        requestedFires.add(key)
        listScheduleFires(org, id, 1)
          .then((fires) => setLastFires((current) => ({ ...current, [id]: fires[0] ?? null })))
          .catch(() => setLastFires((current) => ({ ...current, [id]: null })))
      }
    },
  )

  const handleToggle = async (schedule: CronSchedule) => {
    const org = orgId()
    if (!org || busyId()) return
    setBusyId(schedule.id)
    setActionError(null)
    try {
      await updateCronSchedule(org, schedule.id, { enabled: !schedule.enabled })
      await refetch()
    } catch (err) {
      setActionError(messageFor(err, i18n.tr, { no: 'Kunne ikke oppdatere planen.', en: 'Could not update the schedule.' }))
    } finally {
      setBusyId(null)
    }
  }

  const startRetime = (schedule: CronSchedule) => {
    setActionError(null)
    setRetiming(retimeDraftFor(schedule))
  }

  const saveRetime = async (schedule: CronSchedule) => {
    const org = orgId()
    const draft = retiming()
    if (!org || !draft || busyId()) return
    const expr = draftExpr(draft)
    if (!expr) {
      setActionError(i18n.tr('Cron-uttrykk er påkrevd.', 'A cron expression is required.'))
      return
    }
    if (expr === schedule.schedule_expr.trim()) {
      setRetiming(null)
      return
    }
    setBusyId(schedule.id)
    setActionError(null)
    try {
      // The time is the only field this sends: the goal, the approval and the creator stay as they are.
      await updateCronSchedule(org, schedule.id, { schedule_expr: expr })
      setRetiming(null)
      await refetch()
    } catch (err) {
      setActionError(messageFor(err, i18n.tr, { no: 'Kunne ikke endre tidspunktet.', en: 'Could not change the time.' }))
    } finally {
      setBusyId(null)
    }
  }

  const handleDelete = async (schedule: CronSchedule) => {
    const org = orgId()
    if (!org || busyId()) return
    const confirmed =
      typeof window === 'undefined' ||
      window.confirm(
        i18n.tr(
          `Slette planen «${schedule.name}»? Den kan ikke gjenopprettes.`,
          `Delete the schedule «${schedule.name}»? It cannot be restored.`,
        ),
      )
    if (!confirmed) return
    setBusyId(schedule.id)
    setActionError(null)
    try {
      await deleteCronSchedule(org, schedule.id)
      await refetch()
    } catch (err) {
      setActionError(messageFor(err, i18n.tr, { no: 'Kunne ikke slette planen.', en: 'Could not delete the schedule.' }))
    } finally {
      setBusyId(null)
    }
  }

  const emptyNotice = () => {
    if (props.scope === 'org') {
      return (
        <p class="verevon-automations-hub__empty">
          {i18n.tr('Ingen planlagte kjøringer ennå. Legg til en nedenfor.', 'No scheduled runs yet. Add one below.')}
        </p>
      )
    }
    if (props.compact) {
      return (
        <p class="verevon-automations-hub__empty">
          <CalendarClock size={14} aria-hidden="true" /> {i18n.tr('Ingen planlagte kjøringer ennå.', 'No scheduled runs yet.')}
        </p>
      )
    }
    return (
      <p class="verevon-automations-hub__empty">
        <CalendarClock size={14} aria-hidden="true" />{' '}
        {i18n.tr(
          'Ingen automatiseringer ennå. Planlegg en forespørsel fra en samtale med «Planlegg dette», så vises den her.',
          'No automations yet. Schedule a request from a conversation with "Schedule this" and it will appear here.',
        )}{' '}
        <a href="/chat" link>
          {i18n.tr('Gå til samtalene', 'Go to conversations')}
        </a>
      </p>
    )
  }

  return (
    <div class="verevon-automations-hub">
      <Show when={actionError()}>
        {(message) => (
          <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
            {message()}
          </p>
        )}
      </Show>

      <Show
        when={!schedules.loading}
        fallback={
          <p role="status" aria-busy="true">
            <Loader2 size={14} aria-hidden="true" /> {i18n.tr('Laster automatiseringer …', 'Loading automations…')}
          </p>
        }
      >
        <Show
          when={!schedules.error}
          fallback={
            <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
              {messageFor(schedules.error, i18n.tr, {
                no: 'Kunne ikke laste automatiseringene.',
                en: 'Could not load automations.',
              })}
            </p>
          }
        >
          <Show when={rows().length > 0} fallback={emptyNotice()}>
            <Show when={full()}>
              <div class="verevon-automations-hub__filters" role="group" aria-label={i18n.tr('Filtrer automatiseringer', 'Filter automations')}>
                <For each={FILTERS}>
                  {(item) => (
                    <Button
                      variant={filter() === item ? 'primary' : 'outline'}
                      size="sm"
                      aria-pressed={filter() === item ? 'true' : 'false'}
                      onClick={() => setFilter(item)}
                    >
                      {`${filterLabel(item, i18n.tr)} (${counts()[item]})`}
                    </Button>
                  )}
                </For>
              </div>
            </Show>

            <Show
              when={visible().length > 0}
              fallback={<p class="verevon-automations-hub__empty">{i18n.tr('Ingen automatiseringer i denne visningen.', 'No automations in this view.')}</p>}
            >
              <ul class="verevon-automations-hub__list">
                <For each={visible()} keyed={(row) => row.schedule.id}>
                  {(row) => {
                    const scheduleOf = () => row().schedule
                    const stateOf = () => row().state
                    // The rows are keyed by schedule id, so this id never changes for the row.
                    const cronId = () => scheduleOf().id
                    return (
                      <li class="verevon-automations-hub__row">
                        <div class="verevon-automations-hub__summary">
                          <p class="verevon-automations-hub__name">{scheduleOf().name}</p>
                          <div class="verevon-automations-hub__meta">
                            <Badge tone={stateTone(stateOf())}>{stateLabel(stateOf(), i18n.tr)}</Badge>
                            <span>
                              {cronExprToLabel(scheduleOf().schedule_expr, i18n.locale() === 'no')} · {scheduleOf().timezone}
                            </span>
                            <Show when={stateOf() === 'active'}>
                              <span>
                                {i18n.tr('neste', 'next')}: {formatWhen(scheduleOf().next_fire_at, localeTag())}
                              </span>
                            </Show>
                            <Show when={approvalOf(scheduleOf(), Date.now())}>
                              {(approval) => (
                                <span>
                                  {approval().lapsed
                                    ? i18n.tr('godkjenning utløpt', 'approval expired')
                                    : `${i18n.tr('godkjenning til', 'approval until')} ${formatWhen(approval().expiresAt?.toISOString(), localeTag())}`}
                                </span>
                              )}
                            </Show>
                            <Show when={props.scope === 'org'}>
                              <span>
                                <code>{workflowTypeOf(scheduleOf())}</code>
                              </span>
                              <Show when={scheduleOf().last_fire_at}>
                                {(last) => (
                                  <span>
                                    {i18n.tr('sist', 'last')}: {formatWhen(last(), localeTag())}
                                  </span>
                                )}
                              </Show>
                            </Show>
                          </div>
                        </div>

                        <div class="verevon-automations-hub__actions">
                          <Show when={props.scope === 'mine' && (scheduleResultsEnabled() || props.compact)}>
                            <Show
                              when={scheduleResultsEnabled()}
                              fallback={
                                <Button
                                  variant="outline"
                                  size="sm"
                                  disabled
                                  aria-disabled="true"
                                  title={i18n.tr('Kommer snart', 'Coming soon')}
                                >
                                  {i18n.tr('Se resultater', 'See results')}
                                </Button>
                              }
                            >
                              <Button
                                variant="outline"
                                size="sm"
                                aria-expanded={openResultsId() === cronId() ? 'true' : 'false'}
                                aria-controls={`automation-fires-${cronId()}`}
                                onClick={() => setOpenResultsId((current) => (current === cronId() ? null : cronId()))}
                              >
                                {i18n.tr('Se resultater', 'See results')}
                              </Button>
                            </Show>
                          </Show>
                          <Show when={canManage() && (scheduleOf().enabled || !approvalLapsed(scheduleOf()))}>
                            <Button
                              variant="outline"
                              size="sm"
                              disabled={busyId() === cronId()}
                              onClick={() => void handleToggle(scheduleOf())}
                              aria-label={`${scheduleOf().enabled ? i18n.tr('Pause', 'Pause') : i18n.tr('Aktiver', 'Resume')} ${scheduleOf().name}`}
                            >
                              {scheduleOf().enabled ? i18n.tr('Pause', 'Pause') : i18n.tr('Aktiver', 'Resume')}
                            </Button>
                          </Show>
                          <Show when={full() && !approvalLapsed(scheduleOf())}>
                            <Button
                              variant="outline"
                              size="sm"
                              disabled={busyId() === cronId()}
                              onClick={() => startRetime(scheduleOf())}
                              aria-label={`${i18n.tr('Endre tidspunkt', 'Change time')} ${scheduleOf().name}`}
                            >
                              {i18n.tr('Endre tidspunkt', 'Change time')}
                            </Button>
                          </Show>
                          <Show when={props.scope === 'mine' && approvalLapsed(scheduleOf())}>
                            <Button
                              variant="outline"
                              size="sm"
                              disabled={busyId() === cronId()}
                              onClick={() => setRenewing(scheduleOf())}
                              aria-label={`${i18n.tr('Forny godkjenning', 'Renew approval')} ${scheduleOf().name}`}
                            >
                              {i18n.tr('Forny godkjenning', 'Renew approval')}
                            </Button>
                          </Show>
                          <Show when={full()}>
                            <Button
                              variant="outline"
                              size="sm"
                              disabled={busyId() === cronId()}
                              onClick={() => setCopying(scheduleOf())}
                              aria-label={`${i18n.tr('Dupliser og endre', 'Duplicate with changes')} ${scheduleOf().name}`}
                            >
                              {i18n.tr('Dupliser og endre', 'Duplicate with changes')}
                            </Button>
                          </Show>
                          <Show when={canManage()}>
                            <Button
                              variant="ghost"
                              size="sm"
                              disabled={busyId() === cronId()}
                              onClick={() => void handleDelete(scheduleOf())}
                              aria-label={`${i18n.tr('Slett', 'Delete')} ${scheduleOf().name}`}
                            >
                              <Trash2 size={14} aria-hidden="true" />
                            </Button>
                          </Show>
                        </div>

                        <Show when={full() && retiming()?.id === cronId()}>
                          <div class="verevon-automations-hub__retime">
                            <label>
                              <span>{i18n.tr('Gjentakelse', 'Recurrence')}</span>
                              <select
                                value={retiming()?.presetId ?? 'custom'}
                                disabled={busyId() === cronId()}
                                onChange={(event) =>
                                  setRetiming((current) =>
                                    current ? { ...current, presetId: event.currentTarget.value } : current,
                                  )
                                }
                              >
                                <For each={SCHEDULE_PRESETS}>
                                  {(preset) => <option value={preset.id}>{i18n.tr(preset.labelNo, preset.labelEn)}</option>}
                                </For>
                              </select>
                            </label>
                            <Show when={retiming()?.presetId === 'custom'}>
                              <label>
                                <span>{i18n.tr('Cron-uttrykk', 'Cron expression')}</span>
                                <input
                                  value={retiming()?.custom ?? ''}
                                  disabled={busyId() === cronId()}
                                  onInput={(event) =>
                                    setRetiming((current) =>
                                      current ? { ...current, custom: event.currentTarget.value } : current,
                                    )
                                  }
                                />
                              </label>
                            </Show>
                            <Button variant="primary" size="sm" disabled={busyId() === cronId()} onClick={() => void saveRetime(scheduleOf())}>
                              {i18n.tr('Lagre tidspunkt', 'Save time')}
                            </Button>
                            <Button variant="ghost" size="sm" onClick={() => setRetiming(null)}>
                              {i18n.tr('Avbryt', 'Cancel')}
                            </Button>
                          </div>
                        </Show>

                        <Show when={scheduleResultsEnabled() && openResultsId() === cronId()}>
                          <div id={`automation-fires-${cronId()}`}>
                            <ScheduleFires orgId={orgId()} cronId={cronId()} />
                          </div>
                        </Show>
                      </li>
                    )
                  }}
                </For>
              </ul>
            </Show>
          </Show>
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
      <Show when={copying()}>
        {(target) => (
          <ScheduleDialog
            seedTitle={copyNameOf(target().name, i18n.tr)}
            seedGoal={goalOf(target())}
            threadOriginHint="composer"
            onClose={() => setCopying(null)}
            onCreated={() => void refetch()}
          />
        )}
      </Show>
    </div>
  )
}
