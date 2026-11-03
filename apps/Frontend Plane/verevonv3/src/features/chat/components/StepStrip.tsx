/**
 * The docked step strip (EP-14 S3): one line under a streaming turn, saying which step the run
 * is on and what that step is doing.
 *
 * The count comes from the turn's own tool calls and, when the turn carries a plan, the plan's
 * step count. The sentence is the narration the tool rows use, so the strip and the rows cannot
 * describe one call two ways. The line is a live region that changes when a step starts, ends or
 * retries, never on a token. It takes no focus.
 *
 * Behind `VITE_CHAT_STEP_STRIP`, read through `stepStripEnabled`: off, the page does not mount the
 * strip and the tool rows keep their names.
 */

import { createMemo } from 'solid-js'
import type { ChatToolCall, LifecycleNotice } from './chat-types'
import { narrate, narrationState, type NarrationState, type Translator } from '@/shared/activity/narration'
import { useI18n } from '@/shared/i18n'

/** Only the literal `true` enables the strip, as for the other chat switches. */
export function isStepStripEnabled(raw: string | undefined): boolean {
  return raw === 'true'
}

/** Fixed per build, like every `VITE_` switch: read once, at load. */
export const stepStripEnabled = isStepStripEnabled(import.meta.env.VITE_CHAT_STEP_STRIP)

export type StepProgress = {
  /** The step the run is on: the calls that have settled, plus one while a call is still out. */
  current: number
  /** The plan's step count, while the run has not gone past it. Absent otherwise, never estimated. */
  total?: number
  /** The sentence for the call the run is on, or for the last call that settled. */
  sentence: string
}

const SETTLED: ReadonlySet<NarrationState> = new Set(['done', 'failed'])

/**
 * Where the run is, from the turn's calls. A failed call counts as a settled step. With more calls
 * than plan steps the two no longer line up, so the total is dropped rather than shown wrong.
 */
export function stepProgress(
  calls: readonly ChatToolCall[],
  planSteps: number | undefined,
  notices: readonly LifecycleNotice[] | undefined,
  i18n: Translator,
): StepProgress | undefined {
  if (calls.length === 0) return undefined
  const states = calls.map((call) => narrationState(call, notices))
  const out = states.findLastIndex((state) => !SETTLED.has(state))
  const settled = states.filter((state) => SETTLED.has(state)).length
  const focus = calls[out === -1 ? calls.length - 1 : out]
  if (!focus) return undefined
  const current = settled + (out === -1 ? 0 : 1)
  const total = planSteps !== undefined && planSteps > 0 && current <= planSteps ? planSteps : undefined
  return { current, total, sentence: narrate(focus, i18n, notices) }
}

/**
 * The strip. It mounts under the answer of a streaming turn and stays mounted for the turn, so its
 * live region exists before the first step is announced. Empty until the turn has a tool call.
 */
export function StepStrip(props: {
  calls: readonly ChatToolCall[]
  /** The step count of the turn's plan, when it has one. */
  planSteps?: number
  notices?: readonly LifecycleNotice[]
}) {
  const i18n = useI18n()
  const progress = createMemo(() => stepProgress(props.calls, props.planSteps, props.notices, i18n))
  const text = () => {
    const step = progress()
    if (!step) return ''
    const position =
      step.total === undefined
        ? i18n.tr(`Steg ${step.current}`, `Step ${step.current}`)
        : i18n.tr(`Steg ${step.current} av ${step.total}`, `Step ${step.current} of ${step.total}`)
    return `${position} - ${step.sentence}`
  }
  return (
    <div class="verevon-chat-step-strip" aria-live="polite" aria-atomic="true">
      {text()}
    </div>
  )
}
