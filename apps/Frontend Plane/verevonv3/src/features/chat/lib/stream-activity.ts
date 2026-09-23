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
import type { ChatToolCall } from '@/features/chat/components/chat-types'

export type StreamActivity =
  /** A tool this turn opened has not reported back yet. */
  | { kind: 'tool-running'; tool: string }
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
const isRunning = (call: ChatToolCall) => !call.status || call.status === 'running'

export function deriveStreamActivity(
  calls: readonly ChatToolCall[] | undefined,
): StreamActivity | null {
  if (!calls || calls.length === 0) return null
  // Scanned newest-first: several tools can be out at once, and the one that
  // started last is the thing that just happened. The totals are the pill's
  // job, not this line's.
  for (let index = calls.length - 1; index >= 0; index -= 1) {
    const call = calls[index]
    if (call && isRunning(call)) {
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

/** Norwegian, matching the indicator's own copy — see the module note. */
export function streamActivityLabel(activity: StreamActivity): string {
  if (activity.kind === 'tool-running') return `Kjører ${activity.tool}`
  return activity.failed
    ? `${activity.tool} feilet – venter på svar`
    : `${activity.tool} fullført – venter på svar`
}
