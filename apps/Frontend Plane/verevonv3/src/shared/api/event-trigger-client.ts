import { clientError, requestJson } from './http'

/**
 * The event trigger calls (Wave 4 EP-7, F-t3-047, W4-T8). Session Core owns the record. Model Gateway (W4-T7) serves the
 * trigger routes and relays the dry run from capability-core (W4-T6), and the gateway's BFF relays them under the
 * person's own session: the org and the member come from the session, never from this client. Request bodies are
 * snake_case, as Model Gateway reads them. A trigger comes back camelCase, and the dry run's verdict comes back
 * snake_case; this module reads both and hands the screen one shape.
 *
 * A trigger binds one integration connection and one Space, names the connector event that may wake it, and may bind
 * one of the person's own schedules. The binding grants no authority: every run is authorised again under that
 * schedule's approval.
 */

/** The three states session-core stores. `disabled` is final: a disabled trigger takes no change. */
export type EventTriggerStatus = 'active' | 'paused' | 'disabled'

/** The connector event a trigger reacts to. Only `webhook_received` is accepted so far. */
export type EventTriggerEventType = 'webhook_received'

/** The fields an update may name (session-core's `update_paths`). */
export type EventTriggerUpdatePath = 'name' | 'clock_enabled' | 'status' | 'schedule_id' | 'wake_filter_json'

export interface EventTrigger {
  readonly id: string
  readonly spaceRef: string
  readonly connectionId: string
  readonly eventType: string
  readonly name: string
  readonly clockEnabled: boolean
  /** The stored status token. The known values are the {@link EventTriggerStatus} set; anything else is shown as sent. */
  readonly status: string
  /** The compare-and-set revision a change must name. */
  readonly revision: number
  /** The bound schedule's id, or null for a record-only trigger. */
  readonly scheduleId: string | null
  /** The stored wake filter as JSON text, or null when the trigger has none. */
  readonly wakeFilterJson: string | null
  readonly createdAt: string
  readonly updatedAt: string
}

/** What a dry run decided about one stored webhook event. No payload, field value or part of the event is in it. */
export interface EventTriggerDryRun {
  readonly wouldWake: boolean
  /** One fixed token: the wake gate's reason, or `no_wake_filter` / `filter_outside_schema` for a trigger with no gate. */
  readonly reason: string
  /** The rules the event meets: `event_types`, `providers`, `subject_contains` or `field_equals[i]`, in filter order. */
  readonly matched: readonly string[]
}

/** The closed limits session-core applies to a version 1 wake filter, which the builder keeps within. */
export const WAKE_FILTER_LIMITS = {
  /** Entries in each of `event_types` and `providers`. */
  tokens: 10,
  /** Entries in `subject_contains`. */
  subjects: 5,
  /** Characters in one entry. */
  tokenChars: 100,
  /** Bytes of the filter JSON as a member writes it. */
  bytes: 4096,
} as const

/** Why a draft filter cannot be sent: the first limit it breaks. */
export type WakeFilterProblem = 'too_many' | 'too_long' | 'too_large'

export type WakeFilterResult =
  | { readonly ok: true; readonly json: string }
  | { readonly ok: false; readonly problem: WakeFilterProblem }

/** The values the person typed for the filter builder. An empty list places no condition. */
export interface WakeFilterDraft {
  readonly eventTypes: readonly string[]
  readonly providers: readonly string[]
  readonly subjectContains: readonly string[]
}

function trimmedTokens(values: readonly string[]): string[] {
  return values.map((value) => value.trim()).filter((value) => value !== '')
}

/**
 * The version 1 wake filter for a draft, as the JSON text session-core stores, or '' when the draft names no rule (the
 * trigger then has no filter). The limits are session-core's own, checked here first, so a draft the server would
 * refuse is reported before any request is made. Blank entries are dropped and the rest keep their order.
 */
export function encodeWakeFilter(draft: WakeFilterDraft): WakeFilterResult {
  const eventTypes = trimmedTokens(draft.eventTypes)
  const providers = trimmedTokens(draft.providers)
  const subjects = trimmedTokens(draft.subjectContains)
  if (
    eventTypes.length > WAKE_FILTER_LIMITS.tokens ||
    providers.length > WAKE_FILTER_LIMITS.tokens ||
    subjects.length > WAKE_FILTER_LIMITS.subjects
  ) {
    return { ok: false, problem: 'too_many' }
  }
  const tooLong = [...eventTypes, ...providers, ...subjects].some(
    (value) => Array.from(value).length > WAKE_FILTER_LIMITS.tokenChars,
  )
  if (tooLong) return { ok: false, problem: 'too_long' }
  if (eventTypes.length === 0 && providers.length === 0 && subjects.length === 0) {
    return { ok: true, json: '' }
  }
  const filter: Record<string, unknown> = { version: 1 }
  if (eventTypes.length > 0) filter.event_types = eventTypes
  if (providers.length > 0) filter.providers = providers
  if (subjects.length > 0) filter.subject_contains = subjects
  const json = JSON.stringify(filter)
  if (new TextEncoder().encode(json).length > WAKE_FILTER_LIMITS.bytes) {
    return { ok: false, problem: 'too_large' }
  }
  return { ok: true, json }
}

type Row = Readonly<Record<string, unknown>>

function rowOf(value: unknown): Row | null {
  return value !== null && typeof value === 'object' && !Array.isArray(value) ? (value as Row) : null
}

function textOf(value: unknown): string {
  return typeof value === 'string' ? value.trim() : ''
}

function nullableTextOf(value: unknown): string | null {
  const text = textOf(value)
  return text === '' ? null : text
}

function numberOf(value: unknown): number {
  return typeof value === 'number' && Number.isFinite(value) ? value : 0
}

/** One trigger as Model Gateway sends it (camelCase), or null when the row carries no id. */
function triggerOf(value: unknown): EventTrigger | null {
  const row = rowOf(value)
  if (row === null) return null
  const id = textOf(row.id)
  if (id === '') return null
  return {
    id,
    spaceRef: textOf(row.spaceRef ?? row.space_ref),
    connectionId: textOf(row.connectionId ?? row.connection_id),
    eventType: textOf(row.eventType ?? row.event_type),
    name: textOf(row.name),
    clockEnabled: row.clockEnabled === true || row.clock_enabled === true,
    status: textOf(row.status),
    revision: numberOf(row.revision),
    scheduleId: nullableTextOf(row.scheduleId ?? row.schedule_id),
    wakeFilterJson: nullableTextOf(row.wakeFilterJson ?? row.wake_filter_json),
    createdAt: textOf(row.createdAt ?? row.created_at),
    updatedAt: textOf(row.updatedAt ?? row.updated_at),
  }
}

/** The one trigger a `{ trigger }` envelope carries. A reply without one is a failure of the call, not an empty answer. */
function triggerFrom(payload: unknown): EventTrigger {
  const trigger = triggerOf(rowOf(payload)?.trigger)
  if (trigger === null) throw clientError(502, 'event_trigger_malformed_response')
  return trigger
}

/** The triggers a `{ triggers }` envelope carries. A null or missing list is an empty one. */
function triggerListFrom(payload: unknown): EventTrigger[] {
  const rows = rowOf(payload)?.triggers
  if (!Array.isArray(rows)) return []
  return rows.flatMap((row: unknown) => {
    const trigger = triggerOf(row)
    return trigger === null ? [] : [trigger]
  })
}

/** The dry run's verdict, read from either spelling of `would_wake`. */
function dryRunFrom(payload: unknown): EventTriggerDryRun {
  const row = rowOf(payload)
  const wouldWake = row?.would_wake ?? row?.wouldWake
  if (row === null || typeof wouldWake !== 'boolean') throw clientError(502, 'event_trigger_malformed_response')
  const matched = Array.isArray(row.matched)
    ? row.matched.flatMap((rule: unknown) => (typeof rule === 'string' && rule.trim() !== '' ? [rule.trim()] : []))
    : []
  return { wouldWake, reason: textOf(row.reason), matched }
}

function triggerPath(triggerId: string): string {
  return `/api/v1/event-triggers/${encodeURIComponent(triggerId)}`
}

/** The triggers the person set up, newest first, or only those of one Space when `spaceRef` is given. */
export async function listEventTriggers(options: { readonly spaceRef?: string } = {}): Promise<EventTrigger[]> {
  const query = options.spaceRef ? `?space_ref=${encodeURIComponent(options.spaceRef)}` : ''
  const payload = await requestJson<unknown>(`/api/v1/event-triggers${query}`)
  return triggerListFrom(payload)
}

/** One trigger the person owns. Another member's trigger answers as missing. */
export async function getEventTrigger(triggerId: string): Promise<EventTrigger> {
  const payload = await requestJson<unknown>(triggerPath(triggerId))
  return triggerFrom(payload)
}

export interface CreateEventTriggerInput {
  readonly spaceRef: string
  readonly connectionId: string
  /** Defaults to `webhook_received`, the only event type accepted so far. */
  readonly eventType?: EventTriggerEventType
  readonly name: string
  /** The person's own schedule to bind, or omitted for a record-only trigger. */
  readonly scheduleId?: string
  /** The version 1 wake filter JSON from {@link encodeWakeFilter}, or omitted for no filter. */
  readonly wakeFilterJson?: string
}

/** Record a trigger for the person, active. Session Core checks the schedule binding and the filter. */
export async function createEventTrigger(input: CreateEventTriggerInput): Promise<EventTrigger> {
  const payload = await requestJson<unknown>('/api/v1/event-triggers', {
    method: 'POST',
    body: JSON.stringify({
      space_ref: input.spaceRef,
      connection_id: input.connectionId,
      event_type: input.eventType ?? 'webhook_received',
      name: input.name,
      clock_enabled: false,
      schedule_id: input.scheduleId ?? '',
      wake_filter_json: input.wakeFilterJson ?? '',
    }),
  })
  return triggerFrom(payload)
}

export interface UpdateEventTriggerInput {
  /** The revision the person read the trigger at. A stale one is refused as a conflict. */
  readonly expectedRevision: number
  /** The fields to change. Only these are read. */
  readonly updatePaths: readonly EventTriggerUpdatePath[]
  readonly name?: string
  readonly clockEnabled?: boolean
  readonly status?: EventTriggerStatus
  /** An empty string unbinds the schedule. */
  readonly scheduleId?: string
  /** An empty string removes the wake filter. */
  readonly wakeFilterJson?: string
}

/** Change one trigger under a compare-and-set on its revision: pause, resume, disable, rename, rebind or refilter. */
export async function updateEventTrigger(triggerId: string, input: UpdateEventTriggerInput): Promise<EventTrigger> {
  const payload = await requestJson<unknown>(triggerPath(triggerId), {
    method: 'PATCH',
    body: JSON.stringify({
      expected_revision: input.expectedRevision,
      update_paths: input.updatePaths,
      name: input.name ?? '',
      clock_enabled: input.clockEnabled ?? false,
      status: input.status ?? '',
      schedule_id: input.scheduleId ?? '',
      wake_filter_json: input.wakeFilterJson ?? '',
    }),
  })
  return triggerFrom(payload)
}

/**
 * Would a fire of this trigger wake on one stored webhook event, and which filter rules the event meets. Nothing is
 * recorded, no fire is started and no payload comes back.
 */
export async function dryRunEventTrigger(triggerId: string, webhookEventId: string): Promise<EventTriggerDryRun> {
  const payload = await requestJson<unknown>(`${triggerPath(triggerId)}/dry-run`, {
    method: 'POST',
    body: JSON.stringify({ webhook_event_id: webhookEventId.trim() }),
  })
  return dryRunFrom(payload)
}
