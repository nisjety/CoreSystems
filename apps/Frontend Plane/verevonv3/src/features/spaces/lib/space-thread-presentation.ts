import type { SpaceThread } from '@/shared/api/spaces-client'
import { isQueuedRunStatus, isStoppedRunStatus, legacyStateFromRunStatus } from '@/shared/chat/thread-status'

/**
 * Presentation vocabulary for a Space thread, shared by the room timeline,
 * the page header, and the pulse rail so a status never renders with two
 * different words in two corners of the same room.
 */

// The run-status sets live in the thread-status module, so the Space classifies a run the same way
// as the sidebar and the console; they stay exported here for the room's own consumers.
export { ACTIVE_RUN_STATUSES, AWAITING_APPROVAL_RUN_STATUS, FAILED_RUN_STATUSES, LIVE_RUN_STATUSES } from '@/shared/chat/thread-status'

export function threadTitle(thread: SpaceThread, tr: (no: string, en: string) => string): string {
  return thread.title?.trim() || thread.preview?.trim() || tr('Samtale uten tittel', 'Untitled conversation')
}

export function threadStatus(thread: SpaceThread, tr: (no: string, en: string) => string): string {
  const status = thread.latest_run_status
  if (!status) return tr('Samtale åpen', 'Conversation open')
  // A recorded stop. Session Core writes `cancelled` when a member presses Stop
  // (model-gateway cancels the direct-inference run rather than letting it
  // finish), and until this line existed the room rendered that as the
  // untranslated enum token "Cancelled" — so a stop the room itself caused read
  // as a foreign word.
  if (isStoppedRunStatus(status)) return tr('Stoppet', 'Stopped')
  // The attention vocabulary reads a queued run as working; the room keeps the
  // word "queued" so a run waiting for a worker does not look busy.
  if (isQueuedRunStatus(status)) return tr('I kø', 'Queued')
  const state = legacyStateFromRunStatus(status)
  if (state === 'needs_approval') return tr('Venter på godkjenning', 'Needs approval')
  if (state === 'working') return tr('Arbeider', 'Working')
  if (state === 'completed') return tr('Fullført', 'Completed')
  if (state === 'failed') return tr('Feilet', 'Failed')
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
