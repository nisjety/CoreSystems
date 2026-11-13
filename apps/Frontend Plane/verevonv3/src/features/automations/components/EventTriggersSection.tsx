import { Loader2, Plus, Webhook, X } from '@/shared/icons'
import { createMemo, createSignal, For, onCleanup, Show } from 'solid-js'

import { isLocalRetentionAllowed, subscribeLocalRetention } from '@/features/chat/lib/chat-retention'
import { ApiError, hasErrorCode } from '@/shared/api/api-error'
import { listCronSchedules, type CronSchedule } from '@/shared/api/cron-client'
import {
  createEventTrigger,
  dryRunEventTrigger,
  encodeWakeFilter,
  listEventTriggers,
  updateEventTrigger,
  WAKE_FILTER_LIMITS,
  type EventTrigger,
  type EventTriggerDryRun,
  type EventTriggerStatus,
  type WakeFilterProblem,
} from '@/shared/api/event-trigger-client'
import { listConnections, type IntegrationConnection } from '@/shared/api/integrations-client'
import { cronExprToLabel } from '@/shared/api/schedule-from-chat'
import { listSpaces } from '@/shared/api/spaces-client/core'
import { isZdrCode, zdrRefusalPresentation } from '@/shared/api/zdr-refusals'
import { translateApiError, useI18n } from '@/shared/i18n'
import { createResource } from '@/shared/lib/create-resource-compat'
import { getSession } from '@/shared/session/session-store'
import { Badge } from '@/shared/ui/Badge'
import { Button } from '@/shared/ui/Button'
import { approvalOf } from '../lib/automation-state'

/**
 * The event triggers section of the automations hub (Wave 4 EP-7, F-t3-047, W4-T8). It sits under the schedule list and
 * is mounted only while VITE_EVENT_TRIGGERS is the literal "true". A trigger lets a webhook event on one connection start
 * a run of one of the person's own schedules in one Space. The schedule's approval still decides every run, and the
 * binding grants nothing, so the form offers only the person's own schedules that hold a current approval.
 *
 * - The list shows each trigger's name, connection, Space, status and bound schedule. Pause and resume are reversible;
 *   disable is final and asks first. A dry run takes one stored webhook event id and reports whether the trigger would
 *   wake and which rules the event meets. It records nothing and returns no payload.
 * - The section is hidden for a zero-retention organisation, where the server refuses the trigger record, and where the
 *   feature is switched off on the deployment (a 404 on the list).
 */

type Tr = ReturnType<typeof useI18n>['tr']
type Copy = { readonly no: string; readonly en: string }

const NAME_MAX = 120
const FIELD_RULE = /^field_equals\[(\d+)\]$/

// What a dry run's closed reason means to the person. A reason not listed here is not shown rather than guessed at.
const DRY_RUN_REASONS: Partial<Record<string, Copy>> = {
  no_wake_filter: {
    no: 'Utløseren har ingen vekkingsfilter, så den vekker alltid.',
    en: 'The trigger has no wake filter, so it always wakes.',
  },
  filter_outside_schema: {
    no: 'Filteret er utenfor det lukkede skjemaet, så det stenger alltid.',
    en: 'The filter is outside the closed schema, so it always closes.',
  },
  matched: {
    no: 'Hendelsen oppfyller filteret.',
    en: 'The event meets the filter.',
  },
  event_type_not_listed: {
    no: 'Hendelsestypen står ikke i filteret.',
    en: 'The event type is not in the filter.',
  },
  provider_not_listed: {
    no: 'Leverandøren står ikke i filteret.',
    en: 'The provider is not in the filter.',
  },
  field_not_equal: {
    no: 'Et felt i hendelsen stemmer ikke med filteret.',
    en: 'A field in the event does not match the filter.',
  },
  subject_not_matched: {
    no: 'Emnet inneholder ingen av ordene i filteret.',
    en: 'The subject contains none of the words in the filter.',
  },
}

// The rule names a dry run reports, in the person's words.
const RULE_LABELS: Partial<Record<string, Copy>> = {
  event_types: { no: 'Hendelsestyper', en: 'Event types' },
  providers: { no: 'Leverandører', en: 'Providers' },
  subject_contains: { no: 'Emne inneholder', en: 'Subject contains' },
}

// A refusal the person can act on. A refusal not listed here keeps the shared copy, and a zero-retention refusal keeps its own.
const TRIGGER_REFUSALS: Partial<Record<string, Copy>> = {
  trigger_schedule_mismatch: {
    no: 'Planen kan ikke knyttes til denne utløseren. Den må være din egen aktive plan i samme rom, med godkjenning.',
    en: 'That schedule cannot be bound to this trigger. It must be your own live schedule in the same Space, with an approval.',
  },
  invalid_event_trigger: {
    no: 'Utløseren er ugyldig. Sjekk navnet og filteret.',
    en: 'The trigger is not valid. Check the name and the filter.',
  },
  event_trigger_revision_conflict: {
    no: 'Utløseren er endret siden du så den. Listen er lastet på nytt.',
    en: 'The trigger changed since you read it. The list has been reloaded.',
  },
  event_trigger_closed: {
    no: 'En deaktivert utløser kan ikke endres.',
    en: 'A disabled trigger cannot be changed.',
  },
  event_trigger_not_found: {
    no: 'Fant ikke utløseren.',
    en: 'The trigger was not found.',
  },
  event_trigger_member_only: {
    no: 'Bare personen som eier utløseren kan endre den.',
    en: 'Only the person who owns the trigger can change it.',
  },
  event_triggers_unavailable: {
    no: 'Utløserne kan ikke nås akkurat nå. Prøv igjen om litt.',
    en: 'Event triggers cannot be reached right now. Try again shortly.',
  },
  event_trigger_dry_run_under_zdr_refused: {
    no: 'Tørrkjøring er ikke tilgjengelig under null datalagring.',
    en: 'Dry runs are not available under zero data retention.',
  },
  event_trigger_dry_run_rate_limited: {
    no: 'Utløseren er tørrkjørt for ofte det siste minuttet. Prøv igjen om et minutt.',
    en: 'This trigger has been dry run too often in the last minute. Try again in a minute.',
  },
  wake_payload_unavailable: {
    no: 'Hendelsen kunne ikke leses akkurat nå. Prøv igjen.',
    en: 'The webhook event could not be read right now. Try again.',
  },
  invalid_request: {
    no: 'Skriv inn id-en til en lagret webhook-hendelse.',
    en: 'Enter the id of a stored webhook event.',
  },
}

function statusLabel(status: string, tr: Tr): string {
  switch (status) {
    case 'active':
      return tr('Aktiv', 'Active')
    case 'paused':
      return tr('Pauset', 'Paused')
    case 'disabled':
      return tr('Deaktivert', 'Disabled')
    default:
      return status
  }
}

function statusTone(status: string): 'accent' | 'neutral' | 'risk' {
  if (status === 'active') return 'accent'
  if (status === 'disabled') return 'risk'
  return 'neutral'
}

function statusNotice(status: EventTriggerStatus, tr: Tr): string {
  switch (status) {
    case 'active':
      return tr('Utløseren er aktiv igjen.', 'The trigger is active again.')
    case 'paused':
      return tr('Utløseren er pauset.', 'The trigger is paused.')
    default:
      return tr('Utløseren er deaktivert.', 'The trigger is disabled.')
  }
}

/** A failure's words: a refusal the person can act on, a zero-retention refusal in its own copy, or the shared copy. */
function triggerMessage(err: unknown, tr: Tr, fallback: Copy): string {
  if (err instanceof ApiError && err.code !== null) {
    const known = TRIGGER_REFUSALS[err.code]
    if (known) return tr(known.no, known.en)
    if (isZdrCode(err.code)) {
      const shown = zdrRefusalPresentation(err.code, 'scheduled_step', tr)
      return shown.next === null ? shown.message : `${shown.message} ${shown.next}`
    }
  }
  return translateApiError(err, tr, fallback)
}

function filterProblemText(problem: WakeFilterProblem, tr: Tr): string {
  switch (problem) {
    case 'too_many':
      return tr('Filteret har for mange verdier.', 'The filter has too many values.')
    case 'too_long':
      return tr(
        `En verdi i filteret er for lang (høyst ${WAKE_FILTER_LIMITS.tokenChars} tegn).`,
        `A value in the filter is too long (at most ${WAKE_FILTER_LIMITS.tokenChars} characters).`,
      )
    default:
      return tr('Filteret er for stort.', 'The filter is too large.')
  }
}

function reasonText(reason: string, tr: Tr): string | null {
  const known = DRY_RUN_REASONS[reason]
  return known ? tr(known.no, known.en) : null
}

function ruleLabel(rule: string, tr: Tr): string {
  const known = RULE_LABELS[rule]
  if (known) return tr(known.no, known.en)
  const field = FIELD_RULE.exec(rule)
  if (field) {
    const position = Number(field[1]) + 1
    return tr(`Feltregel ${position}`, `Field rule ${position}`)
  }
  return rule
}

function connectionLabel(connection: IntegrationConnection): string {
  const name = connection.displayName || connection.providerName || connection.providerId
  return connection.status === 'connected' ? name : `${name} (${connection.status})`
}

/** A schedule a trigger may bind: the person's own, in the chosen Space, with an approval that has not lapsed. */
function bindableSchedule(schedule: CronSchedule, spaceRef: string, userId: string, now: number): boolean {
  if (spaceRef === '' || userId === '') return false
  if (schedule.creator_subject_id !== userId || schedule.space_ref !== spaceRef) return false
  const approval = approvalOf(schedule, now)
  return approval !== null && !approval.lapsed
}

/** The list with one more value, unless the value is blank, already there or the list is full. */
function appendToken(current: readonly string[], draft: string, limit: number): readonly string[] {
  const value = draft.trim()
  if (value === '' || current.includes(value) || current.length >= limit) return current
  return [...current, value]
}

/** A zero-retention refusal of the list: the organisation's posture, so the section stays hidden. */
function zdrRefused(error: unknown): boolean {
  return error instanceof ApiError && error.code !== null && isZdrCode(error.code)
}

/** A 404 on the list: the feature is switched off on this deployment, so the section stays hidden. */
function featureAbsent(error: unknown): boolean {
  return error instanceof ApiError && error.status === 404
}

type WakeTokenFieldProps = {
  readonly label: string
  readonly placeholder: string
  readonly values: readonly string[]
  readonly limit: number
  readonly draft: string
  readonly disabled: boolean
  readonly onDraft: (value: string) => void
  readonly onAdd: () => void
  readonly onRemove: (value: string) => void
}

/** One list of the wake filter builder: a text entry, an add button and the values already in the list. */
function WakeTokenField(props: WakeTokenFieldProps) {
  const i18n = useI18n()
  const full = () => props.values.length >= props.limit
  return (
    <div class="verevon-automations-hub__retime" role="group" aria-label={props.label}>
      <label>
        <span>{props.label}</span>
        <input
          value={props.draft}
          maxlength={WAKE_FILTER_LIMITS.tokenChars}
          placeholder={props.placeholder}
          disabled={props.disabled || full()}
          onInput={(event) => props.onDraft(event.currentTarget.value)}
          onKeyDown={(event) => {
            if (event.key !== 'Enter') return
            event.preventDefault()
            props.onAdd()
          }}
        />
      </label>
      <Button
        variant="outline"
        size="sm"
        disabled={props.disabled || full() || props.draft.trim() === ''}
        onClick={() => props.onAdd()}
      >
        <Plus size={14} aria-hidden="true" /> {i18n.tr('Legg til', 'Add')}
      </Button>
      <Show when={props.values.length > 0}>
        <div class="verevon-automations-hub__meta">
          <For each={props.values}>
            {(value) => (
              <Badge tone="neutral">
                {value}{' '}
                <button
                  type="button"
                  disabled={props.disabled}
                  aria-label={`${i18n.tr('Fjern', 'Remove')} ${value}`}
                  onClick={() => props.onRemove(value)}
                >
                  <X size={12} aria-hidden="true" />
                </button>
              </Badge>
            )}
          </For>
        </div>
      </Show>
    </div>
  )
}

/**
 * The dry run of one trigger (W4-T6): a stored webhook event id in, the verdict, its reason and the rules the event meets
 * out. Nothing is recorded or started, and no payload or field value is shown.
 */
function DryRunPanel(props: { readonly triggerId: string; readonly bound: boolean }) {
  const i18n = useI18n()
  const [eventId, setEventId] = createSignal('')
  const [running, setRunning] = createSignal(false)
  const [verdict, setVerdict] = createSignal<EventTriggerDryRun | null>(null)
  const [failure, setFailure] = createSignal<string | null>(null)

  const run = async () => {
    const webhookEventId = eventId().trim()
    if (webhookEventId === '' || running()) return
    setRunning(true)
    setFailure(null)
    setVerdict(null)
    try {
      setVerdict(await dryRunEventTrigger(props.triggerId, webhookEventId))
    } catch (err) {
      setFailure(triggerMessage(err, i18n.tr, { no: 'Kunne ikke kjøre tørrkjøringen.', en: 'Could not run the dry run.' }))
    } finally {
      setRunning(false)
    }
  }

  return (
    <div
      class="verevon-automations-hub__retime"
      id={`event-trigger-dry-run-${props.triggerId}`}
      role="group"
      aria-label={i18n.tr('Tørrkjøring', 'Dry run')}
    >
      <label>
        <span>{i18n.tr('Id for webhook-hendelse', 'Webhook event id')}</span>
        <input
          value={eventId()}
          maxlength={128}
          disabled={running()}
          onInput={(event) => setEventId(event.currentTarget.value)}
        />
      </label>
      <Button variant="outline" size="sm" disabled={running() || eventId().trim() === ''} onClick={() => void run()}>
        {running() ? i18n.tr('Kjører …', 'Running …') : i18n.tr('Kjør tørrkjøring', 'Run dry run')}
      </Button>
      <p class="verevon-automations-hub__empty">
        {i18n.tr(
          'Ingenting lagres eller startes, og ingen innhold vises.',
          'Nothing is recorded or started, and no content is shown.',
        )}
      </p>
      <Show when={failure()}>
        {(message) => (
          <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
            {message()}
          </p>
        )}
      </Show>
      <Show when={verdict()}>
        {(result) => (
          <div class="verevon-automations-hub__meta" role="status">
            <Badge tone={result().wouldWake ? 'accent' : 'neutral'}>
              {result().wouldWake
                ? i18n.tr('Ville vekket kjøring', 'Would wake a run')
                : i18n.tr('Ville ikke vekket kjøring', 'Would not wake a run')}
            </Badge>
            <Show when={reasonText(result().reason, i18n.tr)}>{(text) => <span>{text()}</span>}</Show>
            <Show when={result().matched.length > 0}>
              <span>
                {i18n.tr('Regler som stemmer', 'Rules that match')}:{' '}
                {result()
                  .matched.map((rule) => ruleLabel(rule, i18n.tr))
                  .join(', ')}
              </span>
            </Show>
            <Show when={!props.bound}>
              <span>
                {i18n.tr(
                  'Ingen plan er knyttet til utløseren, så den starter ingen kjøring.',
                  'No schedule is bound to this trigger, so it starts no run.',
                )}
              </span>
            </Show>
          </div>
        )}
      </Show>
    </div>
  )
}

export function EventTriggersSection() {
  const i18n = useI18n()
  const session = getSession()
  const orgId = createMemo(() => session.activeOrg?.id?.trim() ?? '')
  const myUserId = createMemo(() => session.user?.id ?? '')

  // The organisation's zero-retention posture, as the server last stated it on a chat listing. Under zero retention the
  // section is hidden: a trigger is a durable record, which the server refuses to make under that posture.
  const [retentionAllowed, setRetentionAllowed] = createSignal(isLocalRetentionAllowed())
  onCleanup(subscribeLocalRetention((allowed) => setRetentionAllowed(allowed)))

  const [listing, { refetch: refetchTriggers }] = createResource(
    () => (retentionAllowed() ? 'triggers' : false),
    () => listEventTriggers(),
  )
  const [connectionsListing] = createResource(
    () => (retentionAllowed() && orgId() !== '' ? orgId() : false),
    (org) => listConnections(org),
  )
  const [spacesListing] = createResource(
    () => (retentionAllowed() ? 'spaces' : false),
    () => listSpaces(),
  )
  const [schedulesListing] = createResource(
    () => (retentionAllowed() && orgId() !== '' ? orgId() : false),
    (org) => listCronSchedules(org, { creator: 'self' }).then((result) => result.schedules),
  )

  const [createOpen, setCreateOpen] = createSignal(false)
  const [creating, setCreating] = createSignal(false)
  const [formName, setFormName] = createSignal('')
  const [formSpace, setFormSpace] = createSignal('')
  const [formConnection, setFormConnection] = createSignal('')
  const [formSchedule, setFormSchedule] = createSignal('')
  const [eventTypes, setEventTypes] = createSignal<readonly string[]>([])
  const [eventTypeDraft, setEventTypeDraft] = createSignal('')
  const [providers, setProviders] = createSignal<readonly string[]>([])
  const [providerDraft, setProviderDraft] = createSignal('')
  const [subjects, setSubjects] = createSignal<readonly string[]>([])
  const [subjectDraft, setSubjectDraft] = createSignal('')
  const [formError, setFormError] = createSignal<string | null>(null)
  const [busyId, setBusyId] = createSignal<string | null>(null)
  const [actionError, setActionError] = createSignal<string | null>(null)
  const [notice, setNotice] = createSignal<string | null>(null)
  const [openDryRunId, setOpenDryRunId] = createSignal<string | null>(null)

  const triggers = createMemo(() => listing() ?? [])
  const hidden = () => !retentionAllowed() || zdrRefused(listing.error) || featureAbsent(listing.error)
  const spaceOptions = createMemo(() =>
    (spacesListing() ?? []).filter((space) => space.lifecycle === 'active' && space.archived !== true),
  )
  const connectionOptions = createMemo(() => (connectionsListing() ?? []).filter((connection) => !connection.deletedAt))
  const schedules = createMemo(() => schedulesListing() ?? [])
  const bindable = createMemo(() => {
    const now = Date.now()
    return schedules().filter((schedule) => bindableSchedule(schedule, formSpace(), myUserId(), now))
  })

  const spaceNameOf = (spaceRef: string): string => {
    const found = (spacesListing() ?? []).find((space) => space.space_ref === spaceRef)
    return found ? found.name : i18n.tr('Ukjent rom', 'Unknown Space')
  }

  const connectionNameOf = (connectionId: string): string => {
    const found = (connectionsListing() ?? []).find((connection) => connection.id === connectionId)
    return found ? connectionLabel(found) : i18n.tr('Ukjent tilkobling', 'Unknown connection')
  }

  const scheduleNameOf = (scheduleId: string | null): string => {
    if (scheduleId === null) return i18n.tr('Ingen plan', 'No schedule')
    if (schedulesListing.loading) return i18n.tr('Laster plan …', 'Loading schedule …')
    const found = schedules().find((schedule) => schedule.id === scheduleId)
    return found ? found.name : i18n.tr('Planen er ikke tilgjengelig', 'Schedule not available')
  }

  const resetForm = () => {
    setFormName('')
    setFormSchedule('')
    setEventTypes([])
    setEventTypeDraft('')
    setProviders([])
    setProviderDraft('')
    setSubjects([])
    setSubjectDraft('')
    setFormError(null)
  }

  const addEventType = () => {
    setEventTypes((current) => appendToken(current, eventTypeDraft(), WAKE_FILTER_LIMITS.tokens))
    setEventTypeDraft('')
  }

  const addProvider = () => {
    setProviders((current) => appendToken(current, providerDraft(), WAKE_FILTER_LIMITS.tokens))
    setProviderDraft('')
  }

  const addSubject = () => {
    setSubjects((current) => appendToken(current, subjectDraft(), WAKE_FILTER_LIMITS.subjects))
    setSubjectDraft('')
  }

  const submitCreate = async () => {
    if (creating()) return
    setFormError(null)
    const name = formName().trim()
    if (name === '' || Array.from(name).length > NAME_MAX) {
      setFormError(i18n.tr('Gi utløseren et navn på 1 til 120 tegn.', 'Give the trigger a name of 1 to 120 characters.'))
      return
    }
    if (formSpace() === '') {
      setFormError(i18n.tr('Velg et rom.', 'Choose a Space.'))
      return
    }
    if (formConnection() === '') {
      setFormError(i18n.tr('Velg en tilkobling.', 'Choose a connection.'))
      return
    }
    const scheduleId = formSchedule()
    if (scheduleId !== '' && !bindable().some((schedule) => schedule.id === scheduleId)) {
      setFormError(
        i18n.tr(
          'Velg en av dine planer med godkjenning i dette rommet.',
          'Choose one of your schedules with an approval in this Space.',
        ),
      )
      return
    }
    const filter = encodeWakeFilter({
      eventTypes: eventTypes(),
      providers: providers(),
      subjectContains: subjects(),
    })
    if (!filter.ok) {
      setFormError(filterProblemText(filter.problem, i18n.tr))
      return
    }
    setCreating(true)
    setNotice(null)
    try {
      await createEventTrigger({
        spaceRef: formSpace(),
        connectionId: formConnection(),
        eventType: 'webhook_received',
        name,
        scheduleId: scheduleId === '' ? undefined : scheduleId,
        wakeFilterJson: filter.json === '' ? undefined : filter.json,
      })
      resetForm()
      setCreateOpen(false)
      setNotice(i18n.tr('Utløseren er opprettet.', 'The trigger was created.'))
      await refetchTriggers().catch(() => undefined)
    } catch (err) {
      setFormError(
        triggerMessage(err, i18n.tr, { no: 'Kunne ikke opprette utløseren.', en: 'Could not create the trigger.' }),
      )
    } finally {
      setCreating(false)
    }
  }

  // Pause and resume are reversible. Disable is final, so it asks first, and a stale revision reloads the list.
  const changeStatus = async (trigger: EventTrigger, status: EventTriggerStatus) => {
    if (busyId() !== null) return
    if (status === 'disabled') {
      const confirmed =
        typeof window === 'undefined' ||
        window.confirm(
          i18n.tr(
            `Deaktivere utløseren «${trigger.name}»? Deaktivering er endelig: utløseren kan ikke aktiveres igjen.`,
            `Disable the trigger «${trigger.name}»? Disabling is final: the trigger cannot be turned back on.`,
          ),
        )
      if (!confirmed) return
    }
    setBusyId(trigger.id)
    setActionError(null)
    setNotice(null)
    try {
      await updateEventTrigger(trigger.id, { expectedRevision: trigger.revision, updatePaths: ['status'], status })
      setNotice(statusNotice(status, i18n.tr))
      await refetchTriggers().catch(() => undefined)
    } catch (err) {
      setActionError(
        triggerMessage(err, i18n.tr, { no: 'Kunne ikke endre utløseren.', en: 'Could not change the trigger.' }),
      )
      if (hasErrorCode(err, 'event_trigger_revision_conflict')) await refetchTriggers().catch(() => undefined)
    } finally {
      setBusyId(null)
    }
  }

  return (
    <Show when={!hidden()}>
      <section class="verevon-automations-hub" aria-labelledby="event-triggers-heading">
        <h2 class="verevon-automations-hub__name" id="event-triggers-heading">
          <Webhook size={16} aria-hidden="true" /> {i18n.tr('Hendelsesutløsere', 'Event triggers')}
        </h2>
        <p class="verevon-automations-hub__empty">
          {i18n.tr(
            'En utløser starter en kjøring av en av dine planer når et webhook-event fra en tilkobling samsvarer med filteret. Planen må ha godkjenning, og hver kjøring godkjennes på nytt. Utløseren gir ingen egen tilgang.',
            'A trigger starts a run of one of your schedules when a webhook event from a connection matches its filter. The schedule must hold an approval, and every run is authorised again. A trigger grants no access of its own.',
          )}
        </p>

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
          when={!listing.error}
          fallback={
            <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
              {triggerMessage(listing.error, i18n.tr, {
                no: 'Kunne ikke laste utløserne.',
                en: 'Could not load the triggers.',
              })}
            </p>
          }
        >
          <Show
            when={listing() !== undefined}
            fallback={
              <p role="status" aria-busy="true">
                <Loader2 size={14} aria-hidden="true" /> {i18n.tr('Laster utløsere …', 'Loading triggers …')}
              </p>
            }
          >
            <Show
              when={triggers().length > 0}
              fallback={
                <p class="verevon-automations-hub__empty">
                  {i18n.tr('Ingen hendelsesutløsere ennå.', 'No event triggers yet.')}
                </p>
              }
            >
              <ul class="verevon-automations-hub__list">
                <For each={triggers()} keyed={(row) => row.id}>
                  {(row) => {
                    const trigger = () => row()
                    const id = () => row().id
                    return (
                      <li class="verevon-automations-hub__row">
                        <div class="verevon-automations-hub__summary">
                          <p class="verevon-automations-hub__name">{trigger().name}</p>
                          <div class="verevon-automations-hub__meta">
                            <Badge tone={statusTone(trigger().status)}>{statusLabel(trigger().status, i18n.tr)}</Badge>
                            <span>{connectionNameOf(trigger().connectionId)}</span>
                            <span>{spaceNameOf(trigger().spaceRef)}</span>
                            <span>{scheduleNameOf(trigger().scheduleId)}</span>
                            <Show when={trigger().wakeFilterJson !== null}>
                              <span>{i18n.tr('Med filter', 'With filter')}</span>
                            </Show>
                          </div>
                        </div>

                        <div class="verevon-automations-hub__actions">
                          <Show when={trigger().status === 'active'}>
                            <Button
                              variant="outline"
                              size="sm"
                              disabled={busyId() === id()}
                              aria-label={`${i18n.tr('Pause', 'Pause')} ${trigger().name}`}
                              onClick={() => void changeStatus(trigger(), 'paused')}
                            >
                              {i18n.tr('Pause', 'Pause')}
                            </Button>
                          </Show>
                          <Show when={trigger().status === 'paused'}>
                            <Button
                              variant="outline"
                              size="sm"
                              disabled={busyId() === id()}
                              aria-label={`${i18n.tr('Aktiver', 'Resume')} ${trigger().name}`}
                              onClick={() => void changeStatus(trigger(), 'active')}
                            >
                              {i18n.tr('Aktiver', 'Resume')}
                            </Button>
                          </Show>
                          <Show when={trigger().status !== 'disabled'}>
                            <Button
                              variant="ghost"
                              size="sm"
                              disabled={busyId() === id()}
                              aria-label={`${i18n.tr('Deaktiver', 'Disable')} ${trigger().name}`}
                              onClick={() => void changeStatus(trigger(), 'disabled')}
                            >
                              {i18n.tr('Deaktiver', 'Disable')}
                            </Button>
                            <Button
                              variant="outline"
                              size="sm"
                              aria-expanded={openDryRunId() === id() ? 'true' : 'false'}
                              aria-controls={`event-trigger-dry-run-${id()}`}
                              onClick={() => setOpenDryRunId((current) => (current === id() ? null : id()))}
                            >
                              {i18n.tr('Tørrkjøring', 'Dry run')}
                            </Button>
                          </Show>
                        </div>

                        <Show when={openDryRunId() === id()}>
                          <DryRunPanel triggerId={id()} bound={trigger().scheduleId !== null} />
                        </Show>
                      </li>
                    )
                  }}
                </For>
              </ul>
            </Show>
          </Show>
        </Show>

        <div class="verevon-automations-hub__actions">
          <Button
            variant="outline"
            size="sm"
            aria-expanded={createOpen() ? 'true' : 'false'}
            aria-controls="event-trigger-create"
            onClick={() => setCreateOpen((open) => !open)}
          >
            <Plus size={14} aria-hidden="true" />{' '}
            {createOpen() ? i18n.tr('Lukk skjemaet', 'Close the form') : i18n.tr('Ny utløser', 'New trigger')}
          </Button>
        </div>

        <Show when={createOpen()}>
          <div id="event-trigger-create" class="verevon-automations-hub">
            <div class="verevon-automations-hub__retime" role="group" aria-label={i18n.tr('Ny utløser', 'New trigger')}>
              <label>
                <span>{i18n.tr('Navn', 'Name')}</span>
                <input
                  value={formName()}
                  maxlength={NAME_MAX}
                  disabled={creating()}
                  placeholder={i18n.tr('f.eks. Ny bestilling', 'e.g. New order')}
                  onInput={(event) => setFormName(event.currentTarget.value)}
                />
              </label>
              <label>
                <span>{i18n.tr('Rom', 'Space')}</span>
                <select
                  value={formSpace()}
                  disabled={creating()}
                  onChange={(event) => {
                    setFormSpace(event.currentTarget.value)
                    setFormSchedule('')
                  }}
                >
                  <option value="">{i18n.tr('Velg et rom', 'Choose a Space')}</option>
                  <For each={spaceOptions()}>
                    {(space) => <option value={space.space_ref}>{space.name}</option>}
                  </For>
                </select>
              </label>
              <label>
                <span>{i18n.tr('Tilkobling', 'Connection')}</span>
                <select
                  value={formConnection()}
                  disabled={creating()}
                  onChange={(event) => setFormConnection(event.currentTarget.value)}
                >
                  <option value="">{i18n.tr('Velg en tilkobling', 'Choose a connection')}</option>
                  <For each={connectionOptions()}>
                    {(connection) => <option value={connection.id}>{connectionLabel(connection)}</option>}
                  </For>
                </select>
              </label>
              <label>
                <span>{i18n.tr('Plan', 'Schedule')}</span>
                <select
                  value={formSchedule()}
                  disabled={creating() || formSpace() === ''}
                  onChange={(event) => setFormSchedule(event.currentTarget.value)}
                >
                  <option value="">{i18n.tr('Ingen plan (bare registrering)', 'No schedule (record only)')}</option>
                  <For each={bindable()}>
                    {(schedule) => (
                      <option value={schedule.id}>
                        {`${schedule.name} · ${cronExprToLabel(schedule.schedule_expr, i18n.locale() === 'no')}`}
                      </option>
                    )}
                  </For>
                </select>
              </label>
            </div>
            <Show when={formSpace() !== '' && schedulesListing() !== undefined && bindable().length === 0}>
              <p class="verevon-automations-hub__empty">
                {i18n.tr(
                  'Ingen av planene dine i dette rommet har en gyldig godkjenning.',
                  'None of your schedules in this Space holds a current approval.',
                )}
              </p>
            </Show>
            <p class="verevon-automations-hub__empty">
              {i18n.tr(
                'Hendelse: en webhook mottatt fra tilkoblingen. Filteret er valgfritt: uten verdier vekker utløseren på alle hendelser.',
                'Event: a webhook received on the connection. The filter is optional: with no values the trigger wakes on every event.',
              )}
            </p>

            <WakeTokenField
              label={i18n.tr('Hendelsestyper', 'Event types')}
              placeholder={i18n.tr('f.eks. ny melding', 'e.g. new message')}
              values={eventTypes()}
              limit={WAKE_FILTER_LIMITS.tokens}
              draft={eventTypeDraft()}
              disabled={creating()}
              onDraft={(value) => setEventTypeDraft(value)}
              onAdd={addEventType}
              onRemove={(value) => setEventTypes((current) => current.filter((item) => item !== value))}
            />
            <WakeTokenField
              label={i18n.tr('Leverandører', 'Providers')}
              placeholder={i18n.tr('f.eks. outlook', 'e.g. outlook')}
              values={providers()}
              limit={WAKE_FILTER_LIMITS.tokens}
              draft={providerDraft()}
              disabled={creating()}
              onDraft={(value) => setProviderDraft(value)}
              onAdd={addProvider}
              onRemove={(value) => setProviders((current) => current.filter((item) => item !== value))}
            />
            <WakeTokenField
              label={i18n.tr('Emne inneholder', 'Subject contains')}
              placeholder={i18n.tr('f.eks. faktura', 'e.g. invoice')}
              values={subjects()}
              limit={WAKE_FILTER_LIMITS.subjects}
              draft={subjectDraft()}
              disabled={creating()}
              onDraft={(value) => setSubjectDraft(value)}
              onAdd={addSubject}
              onRemove={(value) => setSubjects((current) => current.filter((item) => item !== value))}
            />

            <Show when={formError()}>
              {(message) => (
                <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
                  {message()}
                </p>
              )}
            </Show>

            <div class="verevon-automations-hub__actions">
              <Button variant="primary" size="sm" disabled={creating()} onClick={() => void submitCreate()}>
                {creating() ? i18n.tr('Oppretter …', 'Creating …') : i18n.tr('Opprett utløser', 'Create trigger')}
              </Button>
              <Button
                variant="ghost"
                size="sm"
                disabled={creating()}
                onClick={() => {
                  resetForm()
                  setCreateOpen(false)
                }}
              >
                {i18n.tr('Avbryt', 'Cancel')}
              </Button>
            </div>
          </div>
        </Show>
      </section>
    </Show>
  )
}
