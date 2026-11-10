import { CalendarClock, Loader2, Trash2 } from '@/shared/icons'
import { createMemo, createSignal, For, Show } from 'solid-js'

import { ApiError } from '@/shared/api/api-error'
import {
  deleteCronSchedule,
  listCronSchedules,
  listScheduleFires,
  runScheduleNow,
  updateCronSchedule,
  type CronSchedule,
} from '@/shared/api/cron-client'
import { listOrganizationMembers } from '@/shared/api/organization-client'
import { cronExprToLabel, SCHEDULE_PRESETS } from '@/shared/api/schedule-from-chat'
import { isZdrCode, zdrRefusalPresentation } from '@/shared/api/zdr-refusals'
import { ResultFeedback } from '@/features/chat/components/ResultFeedback'
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
 * - `mine` is the signed-in person's own schedules, asked of the server with `creator=self` and kept
 *   to the person's own rows here too. `org` is every schedule in the org, for the Settings page,
 *   where pause and delete are open to an admin and nothing else is offered; an admin also sees who
 *   made each schedule, from the org's member list.
 * - `compact` is the chat mount: the first `limit` rows, no filter tabs, no edits. Its "See
 *   results" stays on screen, disabled, while the results flag is off, as the chat always showed it.
 * - The full hub offers Run now (EP-6-S3) to the schedule's creator, when the server reports
 *   `features.run_now`, and never on a lapsed approval or on a goal-bound schedule (EP-7 S6), which runs
 *   only on its own cadence and carries a "Goal" chip. A paused row names its reason from the
 *   `last_outcome` the list carries, so every mount names it the same way.
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
// What a refused Run now means and what the person can do next (EP-6-S2). A refusal not listed
// here keeps the shared copy.
const RUN_NOW_REFUSALS: Partial<Record<string, { no: string; en: string }>> = {
  run_now_in_flight: {
    no: 'Planen kjører allerede. Vent til kjøringen er ferdig, og prøv igjen.',
    en: 'This schedule is already running. Wait for that run to finish, then try again.',
  },
  run_now_rate_limited: {
    no: 'Planen har kjørt for mange ganger den siste timen. Prøv igjen senere.',
    en: 'This schedule has run too many times in the last hour. Try again later.',
  },
  approval_expired: {
    no: 'Godkjenningen er utløpt, så planen kan ikke kjøres nå. Forny godkjenningen først.',
    en: 'The approval has expired, so the schedule cannot run now. Renew the approval first.',
  },
  fire_not_authorized: {
    no: 'Kjøringen ble ikke godkjent, så planen kjørte ikke. Sjekk planen og godkjenningen.',
    en: 'This run was not authorized, so the schedule did not run. Check the schedule and its approval.',
  },
  goal_schedule_run_now_refused: {
    no: 'Planen er knyttet til et mål og kjører bare etter sin egen tidsplan.',
    en: 'This schedule is linked to a goal, so it runs only on its own schedule.',
  },
}

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
 * A goal-bound schedule (EP-7 S6) names its thread goal: its template's `purpose` is "goal", or it sets a
 * `goal_id`. That is the rule capability-core applies to Run now, so the hub's chip and its missing button agree
 * with the server's refusal.
 */
function goalBoundOf(schedule: CronSchedule): boolean {
  const template = schedule.task_template
  if (!template || typeof template !== 'object') return false
  if ('purpose' in template && template.purpose === 'goal') return true
  return 'goal_id' in template && template.goal_id !== null && template.goal_id !== ''
}

/** The thread a goal-bound schedule's template names, when it names one. The goal chip links there. */
function goalThreadIdOf(schedule: CronSchedule): string | null {
  const template = schedule.task_template
  if (!template || typeof template !== 'object' || !('thread_id' in template)) return null
  const threadId = template.thread_id
  return typeof threadId === 'string' && threadId.trim() !== '' ? threadId.trim() : null
}

/** A plain click opens the thread in this page's chat; a modified click keeps the browser's own link behaviour. */
function openThreadInPlace(event: MouseEvent, threadId: string): void {
  if (event.button !== 0 || event.metaKey || event.altKey || event.ctrlKey || event.shiftKey) return
  event.preventDefault()
  selectChatThread(threadId)
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

/** The words for a refused Run now (EP-6-S2). Any other failure keeps the shared copy. */
function runNowMessage(err: unknown, tr: Tr): string {
  const refusal = err instanceof ApiError && err.code !== null ? RUN_NOW_REFUSALS[err.code] : undefined
  if (refusal) return tr(refusal.no, refusal.en)
  return messageFor(err, tr, { no: 'Kunne ikke kjøre planen nå.', en: 'Could not run the schedule now.' })
}

/**
 * A fresh request id for one press of Run now (EP-6-S2). `randomUUID` exists only in a secure context, and
 * the hub is also served over plain http, where calling it would throw before the press showed anything.
 */
function newRunNowRequestId(): string {
  if (typeof crypto.randomUUID === 'function') return crypto.randomUUID()
  return Array.from(crypto.getRandomValues(new Uint8Array(16)), (byte) => byte.toString(16).padStart(2, '0')).join('')
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

function ScheduleFires(props: { orgId: string; cronId: string; revision: number }) {
  const i18n = useI18n()
  // The revision changes after a Run now, so the open results read again and show the new fire first.
  const [fires] = createResource(
    () => [props.orgId, props.cronId, props.revision] as const,
    ([org, id]) => listScheduleFires(org, id),
  )
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
                    <Show when={fire.fire_key?.startsWith('manual:')}>
                      <span class="verevon-scheduled-runs-panel__chip">{i18n.tr('Manuell', 'Manual')}</span>
                    </Show>
                    <Show when={fire.withheld_effects > 0}>
                      <span>
                        {i18n.tr('Holdt tilbake', 'Withheld')}: {fire.withheld_effects}
                      </span>
                    </Show>
                    <Show when={fire.thread_id}>
                      {(threadId) => (
                        <a link href={chatThreadHref(threadId())} onClick={(event) => openThreadInPlace(event, threadId())}>
                          {i18n.tr('Åpne samtalen', 'Open transcript')}
                        </a>
                      )}
                    </Show>
                    <Show when={fire.status === 'completed' ? fire.run_id : null}>
                      {(runId) => <ResultFeedback runId={runId()} />}
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
  // Only the full hub has filters and the edits that belong to the creator.
  const full = () => props.scope === 'mine' && !props.compact
  // Pause and delete belong to the creator and, on the Settings page, to an org admin. Renew and copy are the creator's alone.
  const canManage = () => props.scope === 'mine' || isAdmin()

  // The org list still carries every schedule's goal text (D2-121; open decision OD-7). A `mine` list asks for
  // creator=self, so no other member's schedule reaches this browser, and keeps the same rows here as a guard.
  // The same answer carries the server's feature flags, which the hub reads for Run now.
  const [listing, { refetch }] = createResource(
    () => {
      const org = orgId()
      const user = myUserId()
      if (!org || (props.scope === 'mine' && !user)) return undefined
      const key: ListKey = { scope: props.scope, org, user, revision: props.revision ?? 0 }
      return key
    },
    (key) =>
      (key.scope === 'mine' ? listCronSchedules(key.org, { creator: 'self' }) : listCronSchedules(key.org)).then((result) => ({
        schedules:
          key.scope === 'org'
            ? result.schedules
            : result.schedules.filter((schedule) => schedule.creator_subject_id === key.user),
        runNow: result.features?.run_now === true,
      })),
  )
  const schedules = () => listing()?.schedules
  const runNow = () => listing()?.runNow === true

  // On the Settings page an admin sees who made each schedule. The names come from the org's member list, read
  // only for an admin: a member never asks for it and sees no creator name.
  const [members] = createResource(
    () => (props.scope === 'org' && isAdmin() && orgId() ? orgId() : undefined),
    (org) => listOrganizationMembers(org),
  )
  const creatorNames = createMemo(() => {
    const names = new Map<string, string>()
    if (members.error) return names
    for (const member of members() ?? []) {
      if (member.name) names.set(member.userId, member.name)
    }
    return names
  })
  const creatorNameOf = (schedule: CronSchedule): string | undefined =>
    schedule.creator_subject_id ? creatorNames().get(schedule.creator_subject_id) : undefined

  const [filter, setFilter] = createSignal<AutomationFilter>('active')
  const [busyId, setBusyId] = createSignal<string | null>(null)
  const [actionError, setActionError] = createSignal<string | null>(null)
  const [notice, setNotice] = createSignal<string | null>(null)
  const [openResultsId, setOpenResultsId] = createSignal<string | null>(null)
  // Bumped after a Run now, so open results read again and show the new fire first.
  const [firesRevision, setFiresRevision] = createSignal(0)
  const [renewing, setRenewing] = createSignal<CronSchedule | null>(null)
  const [copying, setCopying] = createSignal<CronSchedule | null>(null)
  const [retiming, setRetiming] = createSignal<RetimeDraft | null>(null)

  const rows = createMemo((): AutomationRow[] => {
    const now = Date.now()
    return (schedules() ?? []).map((schedule) => ({
      schedule,
      state: automationStateOf(schedule, now),
    }))
  })
  const counts = createMemo(() => countAutomations(rows()))
  const visible = createMemo(() => {
    const listed = full() ? filterAutomations(rows(), filter()) : rows()
    return props.limit === undefined ? listed : listed.slice(0, props.limit)
  })

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

  // A press asks for one manual fire. Its request id is made once per press, and kept only while the
  // answer is unknown (no answer, or a server error): a retry of that press then gets the fire it made
  // first, not a second one (EP-6-S2). A refusal is an answer, so the next press starts with a new id.
  const runNowRequests = new Map<string, string>()

  const handleRunNow = async (schedule: CronSchedule) => {
    const org = orgId()
    if (!org || busyId()) return
    const requestId = runNowRequests.get(schedule.id) ?? newRunNowRequestId()
    setBusyId(schedule.id)
    setActionError(null)
    setNotice(null)
    try {
      await runScheduleNow(org, schedule.id, requestId)
      runNowRequests.delete(schedule.id)
      setNotice(i18n.tr('Kjøringen er startet.', 'The run has started.'))
      setOpenResultsId(schedule.id)
      setFiresRevision((current) => current + 1)
    } catch (err) {
      if (err instanceof ApiError && err.status < 500) runNowRequests.delete(schedule.id)
      else runNowRequests.set(schedule.id, requestId)
      setActionError(runNowMessage(err, i18n.tr))
    } finally {
      setBusyId(null)
    }
  }

  // Run now is the creator's, on the full hub only, and never on a lapsed approval: renewal is the way back.
  // A goal-bound schedule runs only on its own cadence (EP-7 S6), so it never offers Run now.
  const canRunNow = (row: AutomationRow) =>
    full() &&
    runNow() &&
    row.state !== 'expired' &&
    row.schedule.creator_subject_id === myUserId() &&
    !goalBoundOf(row.schedule)

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
      <Show when={notice()}>
        {(message) => (
          <p class="verevon-settings-status-message verevon-settings-status-message--success" role="status">
            {message()}
          </p>
        )}
      </Show>

      <Show
        when={!listing.loading}
        fallback={
          <p role="status" aria-busy="true">
            <Loader2 size={14} aria-hidden="true" /> {i18n.tr('Laster automatiseringer …', 'Loading automations…')}
          </p>
        }
      >
        <Show
          when={!listing.error}
          fallback={
            <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
              {messageFor(listing.error, i18n.tr, {
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
                            <Show when={goalBoundOf(scheduleOf())}>
                              <Show
                                when={goalThreadIdOf(scheduleOf())}
                                fallback={<Badge tone="neutral">{i18n.tr('Mål', 'Goal')}</Badge>}
                              >
                                {(threadId) => (
                                  <a link href={chatThreadHref(threadId())} onClick={(event) => openThreadInPlace(event, threadId())}>
                                    <Badge tone="neutral">{i18n.tr('Mål', 'Goal')}</Badge>
                                  </a>
                                )}
                              </Show>
                            </Show>
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
                              <Show when={props.scope === 'org' ? creatorNameOf(scheduleOf()) : undefined}>
                                {(name) => (
                                  <span>
                                    {i18n.tr('laget av', 'created by')} {name()}
                                  </span>
                                )}
                              </Show>
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
                          <Show when={canRunNow(row())}>
                            <Button
                              variant="outline"
                              size="sm"
                              disabled={busyId() === cronId()}
                              onClick={() => void handleRunNow(scheduleOf())}
                              aria-label={`${i18n.tr('Kjør nå', 'Run now')} ${scheduleOf().name}`}
                            >
                              {i18n.tr('Kjør nå', 'Run now')}
                            </Button>
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
                            <ScheduleFires orgId={orgId()} cronId={cronId()} revision={firesRevision()} />
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
            goalEditable
            seedScheduleExpr={target().schedule_expr}
            threadOriginHint="composer"
            onClose={() => setCopying(null)}
            onCreated={() => void refetch()}
          />
        )}
      </Show>
    </div>
  )
}
