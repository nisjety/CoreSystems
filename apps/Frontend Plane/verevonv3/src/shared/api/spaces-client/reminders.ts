// WS11-S11: reminders for a post or message of a room.
//
// Wire (gateway: apps/gateway/src/domains/spaces/reminders.rs; flag `space_reminders_api`):
//   POST   /api/v1/spaces/{ref}/reminders  {thread_id, message_id?, remind_at, idempotency_key}
//                                          -> 201 { data: { reminder_id, remind_at, deduplicated } }  (200 when an identical retry)
//   GET    /api/v1/spaces/reminders?state=pending -> { data: { reminders: [{reminder_id, space_ref, thread_id, message_id, remind_at, state, created_at}] } }
//   DELETE /api/v1/spaces/reminders/{id}   -> { data: { reminder_id, cancelled } }
//
// Errors arrive as an ApiError carrying the code: 404 `space_reminders_disabled` (flag off, hide the
// control), 400 `invalid_reminder_time`, 429 `too_many_reminders` (details.max_pending), 404
// `reminder_not_found`, 403 `active_membership_required`, 412 `zdr_durable_mutation_forbidden`
// (creating is refused under Zero Data Retention; cancelling is always allowed).
// A reminder carries ids and a time only: there is no note. `spaces.create_reminder` and
// `spaces.cancel_reminder` are the action contracts (shared/actions/domains/spaces/ws11.ts).
import { requestJson } from '../http'

/** The server accepts a time at least five seconds and at most 366 days ahead; the UI offers less. */
export const SPACE_REMINDER_MAX_PENDING = 50

export type SpaceReminder = {
  readonly reminderId: string
  readonly spaceRef: string
  readonly threadId: string
  readonly messageId: string | null
  /** Epoch milliseconds. */
  readonly remindAt: number
  readonly createdAt: number | null
}

export type CreatedSpaceReminder = {
  readonly reminderId: string
  readonly remindAt: number
  /** True when an identical pending reminder already existed (nothing new was scheduled). */
  readonly deduplicated: boolean
}

const isRecord = (value: unknown): value is Record<string, unknown> =>
  !!value && typeof value === 'object' && !Array.isArray(value)

const text = (value: unknown): string => (typeof value === 'string' ? value.trim() : '')
const millis = (value: unknown): number | null =>
  typeof value === 'number' && Number.isFinite(value) ? value : null

function idempotencyKey(): string {
  const random =
    typeof crypto !== 'undefined' && 'randomUUID' in crypto ? crypto.randomUUID() : `${Date.now()}-${Math.random()}`
  return `remind-${random}`.slice(0, 64)
}

/** One wire reminder; anything not well formed reads as absent (a row is never guessed at). */
export function spaceReminderFrom(raw: unknown): SpaceReminder | null {
  if (!isRecord(raw)) return null
  const reminderId = text(raw.reminder_id)
  const spaceRef = text(raw.space_ref)
  const threadId = text(raw.thread_id)
  const remindAt = millis(raw.remind_at)
  if (!reminderId || !spaceRef || !threadId || remindAt === null) return null
  return {
    reminderId,
    spaceRef,
    threadId,
    messageId: text(raw.message_id) || null,
    remindAt,
    createdAt: millis(raw.created_at),
  }
}

export async function createSpaceReminder(input: {
  readonly spaceRef: string
  readonly threadId: string
  readonly messageId?: string
  /** Epoch milliseconds. */
  readonly remindAt: number
}): Promise<CreatedSpaceReminder> {
  const wire = await requestJson<unknown>(`/api/v1/spaces/${encodeURIComponent(input.spaceRef)}/reminders`, {
    method: 'POST',
    body: JSON.stringify({
      thread_id: input.threadId,
      ...(input.messageId ? { message_id: input.messageId } : {}),
      remind_at: Math.floor(input.remindAt),
      idempotency_key: idempotencyKey(),
    }),
  })
  const body = isRecord(wire) ? wire : {}
  return {
    reminderId: text(body.reminder_id),
    remindAt: millis(body.remind_at) ?? Math.floor(input.remindAt),
    deduplicated: body.deduplicated === true,
  }
}

/** The caller's own pending reminders, soonest first. */
export async function listPendingSpaceReminders(): Promise<readonly SpaceReminder[]> {
  const wire = await requestJson<unknown>('/api/v1/spaces/reminders?state=pending')
  const rows = isRecord(wire) && Array.isArray(wire.reminders) ? wire.reminders : []
  return rows
    .map(spaceReminderFrom)
    .filter((row): row is SpaceReminder => row !== null)
    .sort((a, b) => a.remindAt - b.remindAt)
}

export async function cancelSpaceReminder(reminderId: string): Promise<void> {
  await requestJson<unknown>(`/api/v1/spaces/reminders/${encodeURIComponent(reminderId)}`, { method: 'DELETE' })
}
