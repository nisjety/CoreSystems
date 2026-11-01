import type { SpaceThread } from '@/shared/api/spaces-client'

/**
 * Presentation vocabulary for a Space thread, shared by the room timeline,
 * the page header, and the pulse rail so a status never renders with two
 * different words in two corners of the same room.
 */

/** Session Core's own token for a run paused on a human decision. Named once
 * so the pulse count, the post's approval surface, and the composer guard all
 * agree on what "waiting" means. */
export const AWAITING_APPROVAL_RUN_STATUS = 'awaiting_approval'

export const ACTIVE_RUN_STATUSES = new Set(['queued', 'running', AWAITING_APPROVAL_RUN_STATUS])

/**
 * Statuses under which an agent is actually producing output right now.
 *
 * Distinct from `ACTIVE_RUN_STATUSES` on purpose: a run paused for approval is
 * active in the sense that it has not finished, but nobody is working — a
 * person is being waited for. The "working" indicator (item 1b) must say the
 * first thing and not the second, or a room stalled on a decision would look
 * busy instead of blocked.
 */
export const LIVE_RUN_STATUSES = new Set(['queued', 'running'])

export const FAILED_RUN_STATUSES = new Set(['failed'])

export function threadTitle(thread: SpaceThread, tr: (no: string, en: string) => string): string {
  return thread.title?.trim() || thread.preview?.trim() || tr('Samtale uten tittel', 'Untitled conversation')
}

export function threadStatus(thread: SpaceThread, tr: (no: string, en: string) => string): string {
  const status = thread.latest_run_status
  if (!status) return tr('Samtale åpen', 'Conversation open')
  if (status === 'awaiting_approval') return tr('Venter på godkjenning', 'Needs approval')
  if (status === 'running') return tr('Arbeider', 'Working')
  if (status === 'queued') return tr('I kø', 'Queued')
  if (status === 'completed') return tr('Fullført', 'Completed')
  if (status === 'failed') return tr('Feilet', 'Failed')
  // A recorded stop. Session Core writes `cancelled` when a member presses Stop
  // (model-gateway cancels the direct-inference run rather than letting it
  // finish), and until this line existed the room rendered that as the
  // untranslated enum token "Cancelled" — so a stop the room itself caused read
  // as a foreign word.
  if (status === 'cancelled') return tr('Stoppet', 'Stopped')
  return formatLabel(status)
}

// Humanizes a raw server enum token that has no translated dictionary here.
export function formatLabel(value: string): string {
  return value
    .trim()
    .replace(/[_-]+/g, ' ')
    .replace(/\b\w/g, (letter) => letter.toUpperCase())
}

// Moved to shared (governed read-aloud builds on it); re-exported so the room
// timeline's import keeps working.
export { stripMarkdownPreview } from '@/shared/lib/markdown-plain-text'

/**
 * A post's date, for the room timeline: "6. sep.".
 *
 * Day precision on purpose — a post is placed in the conversation by the turns
 * around it, so the hour is noise there. The activity feed wants the opposite
 * (see [`formatWhenWithTime`]), and the two used to be separate functions with
 * the SAME name in different files, which is how one surface can quietly start
 * disagreeing with the other about what a timestamp means.
 */
export function formatWhen(value: string): string {
  try {
    return new Date(value).toLocaleDateString('nb-NO', { day: 'numeric', month: 'short' })
  } catch {
    return value
  }
}

/**
 * How long ago something happened, for a reply summary: "for 4 min. siden", "i går", and the plain
 * date from a week on. `now` is a parameter so the wording is testable. An unreadable value reads as
 * empty rather than as a wrong time.
 */
export function formatRelativeWhen(value: string | undefined, now: number = Date.now()): string {
  if (!value) return ''
  const at = new Date(value).getTime()
  if (!Number.isFinite(at)) return ''
  // A time ahead of this browser's clock (skew between the server and the reader) is "now", not "in 1 t".
  const seconds = Math.min(0, Math.round((at - now) / 1000))
  const abs = Math.abs(seconds)
  const rtf = new Intl.RelativeTimeFormat('nb-NO', { numeric: 'auto', style: 'short' })
  if (abs < 45) return rtf.format(0, 'second')
  if (abs < 3600) return rtf.format(Math.round(seconds / 60), 'minute')
  if (abs < 86_400) return rtf.format(Math.round(seconds / 3600), 'hour')
  if (abs < 7 * 86_400) return rtf.format(Math.round(seconds / 86_400), 'day')
  return formatWhen(value)
}

/**
 * A moment, for the activity feed: "06.09.2026, 14:32".
 *
 * The feed is ordered by consequence rather than recency, so a row has to carry
 * its own time — "which of these two failures came first" is not answerable
 * from the order. Falls back to the raw value rather than dropping it: an
 * unparseable timestamp is still evidence something happened then.
 */
export function formatWhenWithTime(value?: string): string {
  if (!value) return ''
  try {
    return new Date(value).toLocaleString('nb-NO', { dateStyle: 'short', timeStyle: 'short' })
  } catch {
    return value
  }
}

/**
 * A turn's time, for the header beside the author: "14:32" in the reader's local time, with the full
 * date and time for the tooltip and the machine-readable `datetime`. Undefined when the server sent no
 * time (or one that is not a time): a row then renders without one rather than with an invented one.
 */
export function formatTurnTime(value: string | undefined): { iso: string; short: string; full: string } | undefined {
  if (!value) return undefined
  const at = new Date(value)
  if (!Number.isFinite(at.getTime())) return undefined
  return {
    iso: at.toISOString(),
    short: at.toLocaleTimeString('nb-NO', { hour: '2-digit', minute: '2-digit' }),
    full: at.toLocaleString('nb-NO', { dateStyle: 'long', timeStyle: 'short' }),
  }
}

/** Consecutive turns by the same author within this window share one avatar and name (Slack's grouping). */
export const TURN_GROUP_WINDOW_MS = 5 * 60_000

/** The fields of a turn that decide who wrote it and when. */
export type GroupableTurn = {
  readonly role: 'user' | 'assistant'
  readonly agentName?: string
  readonly authorSubjectId?: string
  readonly createdAt?: string
}

/**
 * Whether `turn` continues the group that `previous` started: the same author, close enough in time.
 *
 * "Same author" is what the server recorded, never a guess: a member turn needs the same recorded
 * `authorSubjectId` (two unattributed turns are two unknown authors, not one), an agent turn needs the
 * same persona name (the room-level `fallbackAgentName` stands in for an unrecorded one). A member and
 * an agent never group. Both turns need a time and the gap must be 0 to 5 minutes, so a turn without a
 * time starts its own group: the grouping never claims something the record does not show.
 */
export function continuesTurnGroup(
  previous: GroupableTurn | undefined,
  turn: GroupableTurn,
  fallbackAgentName: string,
): boolean {
  if (!previous || previous.role !== turn.role) return false
  if (turn.role === 'assistant') {
    if ((previous.agentName ?? fallbackAgentName) !== (turn.agentName ?? fallbackAgentName)) return false
  } else if (!turn.authorSubjectId || previous.authorSubjectId !== turn.authorSubjectId) {
    return false
  }
  if (!previous.createdAt || !turn.createdAt) return false
  const gap = new Date(turn.createdAt).getTime() - new Date(previous.createdAt).getTime()
  return Number.isFinite(gap) && gap >= 0 && gap <= TURN_GROUP_WINDOW_MS
}
