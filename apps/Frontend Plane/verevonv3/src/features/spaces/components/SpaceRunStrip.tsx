import { createEffect, createMemo, createSignal, For, onCleanup, Show } from 'solid-js'

import type { SpaceThread } from '@/shared/api/spaces-client'
import { useI18n } from '@/shared/i18n'
import { runHeadline } from '../lib/room-run-grammar'
import { advanceStrip, EMPTY_STRIP, type StripLine, type StripObservation, type StripState } from '../lib/space-run-strip'
import { spaceRunStore, trackLiveRuns } from '../lib/space-run-store'
import { threadTitle } from '../lib/space-thread-presentation'
import './space-run.css'

/** Lines named at once; the rest fold into "and N more". */
const MAX_LINES = 2

export interface SpaceRunStripProps {
  readonly spaceRef: string
  /** Other members' live threads in this room (the composer already subtracts this browser's own). */
  readonly threads: () => readonly SpaceThread[]
  /** Click-through: open the run's thread (pane) or bring its post into view. */
  readonly onOpenRun?: (threadId: string) => void
}

/**
 * The line above the composer that says which agents are working in the room (WS09-S6). It replaces the
 * flag-off "Verevon is working in ..." sentence. What it shows, when and how fast it changes is decided
 * by `lib/space-run-strip.ts`; this component only attaches the live views, keeps the clock and draws.
 *
 * A run whose live view is not available (expired, refused, waiting for an attach slot) still counts as
 * committed with the plain "Working" headline: the room listing says it is working, which is all the
 * legacy line ever said.
 */
export function SpaceRunStrip(props: SpaceRunStripProps) {
  const i18n = useI18n()

  const runs = createMemo(() =>
    props
      .threads()
      .map((thread) => ({ thread, runId: thread.latest_run_id?.trim() ?? '' }))
      .filter((entry) => entry.runId !== ''),
  )
  trackLiveRuns(
    () => props.spaceRef,
    () => runs().map((entry) => entry.runId),
    () => runs().length > 0,
  )

  const observations = createMemo<readonly StripObservation[]>(() =>
    runs().map(({ thread, runId }) => {
      const view = spaceRunStore.view(runId)
      const degraded = view !== undefined && (view.status === 'unavailable' || view.status === 'waiting')
      const committed = degraded
        || (view !== undefined
          && (view.steps.length > 0 || view.stepsDropped > 0 || view.text.length > 0 || view.text.progressChars > 0))
      return {
        runId,
        threadId: thread.thread_id,
        committed,
        finished: view?.finished != null,
        headline: view && !degraded ? runHeadline(view, i18n.tr) : i18n.tr('Jobber', 'Working'),
      }
    }),
  )

  const [lines, setLines] = createSignal<readonly StripLine[]>([])
  const [tick, setTick] = createSignal(0)
  let state: StripState = EMPTY_STRIP
  let timer: ReturnType<typeof setTimeout> | undefined
  const titles = createMemo(() => new Map(props.threads().map((thread) => [thread.thread_id, thread])))

  createEffect(
    () => ({ seen: observations(), wake: tick() }),
    ({ seen }) => {
      if (timer !== undefined) clearTimeout(timer)
      timer = undefined
      const now = Date.now()
      const step = advanceStrip(state, seen, now)
      state = step.state
      setLines(step.lines)
      if (step.nextWakeAt !== null) {
        timer = setTimeout(() => setTick((value) => value + 1), Math.max(0, step.nextWakeAt - now))
      }
    },
  )
  onCleanup(() => {
    if (timer !== undefined) clearTimeout(timer)
  })

  const titleOf = (threadId: string) => {
    const thread = titles().get(threadId)
    return thread ? threadTitle(thread, i18n.tr) : ''
  }
  const shown = () => lines().slice(0, MAX_LINES)
  const overflow = () => Math.max(0, lines().length - MAX_LINES)

  return (
    <Show when={lines().length > 0}>
      <div class="verevon-space-room-composer__working verevon-run-strip" role="status" aria-live="polite">
        <For each={shown()}>
          {(line) => (
            <button
              type="button"
              class="verevon-run-strip__line"
              disabled={!props.onOpenRun}
              onClick={() => props.onOpenRun?.(line.threadId)}
              title={i18n.tr('Åpne samtalen', 'Open the conversation')}
            >
              <span class="verevon-space-room-composer__working-dot verevon-run-strip__dot" aria-hidden="true" />
              <span class="verevon-run-strip__headline">{line.headline}</span>
              <Show when={titleOf(line.threadId)}>
                {(title) => <span class="verevon-run-strip__where">{` · «${title()}»`}</span>}
              </Show>
            </button>
          )}
        </For>
        <Show when={overflow() > 0}>
          <span class="verevon-run-strip__more">
            {i18n.tr(`og ${overflow()} til`, `and ${overflow()} more`)}
          </span>
        </Show>
      </div>
    </Show>
  )
}
