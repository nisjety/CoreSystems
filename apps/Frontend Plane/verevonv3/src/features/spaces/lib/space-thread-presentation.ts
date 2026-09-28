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
