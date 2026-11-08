/**
 * What the run is DOING while the answer is still pending.
 *
 * F-07: a subscription-route turn took ~90 seconds showing nothing but
 * "Tenker", and a wait that never accounts for itself reads as a hung page.
 * The elapsed counter beside the indicator answers *how long*; this answers
 * *at what*.
 *
 * # Only real state
 *
 * Derived from the turn's own tool calls and nothing else. There is no
 * synthetic phase, no estimated stage, no rotating reassurance: when the stream
 * has reported no activity — the plain subscription route emits no tool calls
 * at all, which is exactly the turn F-07 was filed against — this returns
 * `null` and the indicator is left with elapsed time, because elapsed time is
 * then the only thing actually known. That is the same standard the snippet
 * fallbacks and the "ikke lest" source labels are held to: say what happened,
 * or say nothing.
 *
 * Copy lives in `streamActivityLabel` rather than in the caller so the visible
 * line and the screen-reader announcement are formatted by one function and
 * cannot describe the same wait differently.
 */

import { humanizeToolName } from '@/features/chat/components/chat-normalizers'
import type { ChatToolCall, LifecycleNotice } from '@/features/chat/components/chat-types'

export type StreamActivity =
  /** A tool this turn opened has not reported back yet. */
  | { kind: 'tool-running'; tool: string }
  /**
   * That tool is out and is being retried after a transient failure (WS15): `attempt` is the try about to run,
   * of `max`. Read from the turn's `retry_scheduled` notice, never guessed from elapsed time.
   */
  | { kind: 'retrying'; tool: string; attempt: number; max: number }
  /**
   * Every tool has settled and no answer token has arrived: the last thing that
   * demonstrably happened is this call finishing. Deliberately NOT reported as
   * "writing the answer" — no token has been seen, and claiming one would be
   * the invented progress this module exists to avoid.
   */
  | { kind: 'tool-settled'; tool: string; failed: boolean }

/**
 * The same running test `StepsPill` applies, on purpose: the pill and this line
 * sit inches apart on the same turn and must not be able to disagree about
 * whether a tool is still out. A call arrives with `status: 'running'` and is
 * rewritten by the tool result, which may carry any status string the backend
 * chose — so "running" is the absence of a result, not a list of end states.
 */
export const isToolCallRunning = (call: ChatToolCall) => !call.status || call.status === 'running'

export function deriveStreamActivity(
  calls: readonly ChatToolCall[] | undefined,
  notices?: readonly LifecycleNotice[],
): StreamActivity | null {
  if (!calls || calls.length === 0) return null
  // Scanned newest-first: several tools can be out at once, and the one that
  // started last is the thing that just happened. The totals are the pill's
  // job, not this line's.
  for (let index = calls.length - 1; index >= 0; index -= 1) {
    const call = calls[index]
    if (call && isToolCallRunning(call)) {
      // A retry names the call it was matched to, so a later call of the same tool is not read as retrying.
      const retry = notices?.findLast((notice) => notice.kind === 'retry' && notice.callId === call.id)
      if (retry?.kind === 'retry') {
        return {
          kind: 'retrying',
          tool: humanizeToolName(call.name),
          attempt: retry.attempt,
          max: retry.maxAttempts,
        }
      }
      return { kind: 'tool-running', tool: humanizeToolName(call.name) }
    }
  }
  const last = calls[calls.length - 1]
  if (!last) return null
  return {
    kind: 'tool-settled',
    tool: humanizeToolName(last.name),
    // A failure is part of what happened. Swallowing it here would leave the
    // status line implying progress while the steps surface one click away
    // reports "1 feilet".
    failed: Boolean(last.error) || last.status === 'error' || last.status === 'failed',
  }
}

/**
 * The app's translator, `useI18n().tr`. Passed in rather than read here, so the visible line and the
 * screen-reader sentence are worded by one function in whichever locale is active.
 */
export type Translator = { tr: (noText: string, enText: string) => string }

export function streamActivityLabel(activity: StreamActivity, i18n: Translator): string {
  if (activity.kind === 'tool-running') return i18n.tr(`Kjører ${activity.tool}`, `Running ${activity.tool}`)
  if (activity.kind === 'retrying') {
    return i18n.tr(
      `Prøver ${activity.tool} på nytt (forsøk ${activity.attempt} av ${activity.max})`,
      `Retrying ${activity.tool} (attempt ${activity.attempt} of ${activity.max})`,
    )
  }
  return activity.failed
    ? i18n.tr(`${activity.tool} feilet – venter på svar`, `${activity.tool} failed – waiting for answer`)
    : i18n.tr(`${activity.tool} fullført – venter på svar`, `${activity.tool} done – waiting for answer`)
}
