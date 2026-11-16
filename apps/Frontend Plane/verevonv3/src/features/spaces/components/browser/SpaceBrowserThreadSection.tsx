import { For, Show, createEffect, createSignal, onCleanup, untrack } from 'solid-js'

import { listSpaceBrowserRuns, type SpaceBrowserRun } from '@/shared/api/spaces-client/browser-runs'
import { useI18n } from '@/shared/i18n'
import { spaceRoomFeature } from '../../lib/space-room-features'
import { SpaceBrowserCard } from './SpaceBrowserCard'
import { SpaceBrowserStartPanel } from './SpaceBrowserStartPanel'
import './space-browser.css'

/** How often the thread's run list is read while the thread is open (the card polls each run on its own). */
const LIST_POLL_MS = 6000

export interface SpaceBrowserThreadSectionProps {
  readonly spaceRef: string
  readonly threadId: string
  /** Whether this viewer may start a run in this thread: an editor, manager or owner in a room that is not archived. */
  readonly canStart: boolean
  readonly disabledReason?: string
  readonly viewerSubjectId: string
  readonly nameOf: (subjectId: string) => string
  /** Opens a run's pane (the `browser_run` parameter). */
  readonly onOpen: (runId: string) => void
}

/**
 * The browser runs of one thread, above its composer (WS20-S5): the start entry and the run cards. Behind
 * `features.browser_run` (the start) and `features.browser_card` (the cards) from the room's context read; with both
 * off it renders nothing. The gateway decides every read and every start.
 */
export function SpaceBrowserThreadSection(props: SpaceBrowserThreadSectionProps) {
  const i18n = useI18n()
  const [runs, setRuns] = createSignal<readonly SpaceBrowserRun[]>([])
  const [composing, setComposing] = createSignal(false)
  const [listFailed, setListFailed] = createSignal(false)
  const startOn = () => spaceRoomFeature('browser_run')
  const cardsOn = () => spaceRoomFeature('browser_card')
  let inFlight = false

  const refresh = async () => {
    if (inFlight) return
    inFlight = true
    try {
      const page = await listSpaceBrowserRuns(props.spaceRef, { threadId: props.threadId, limit: 5 })
      setRuns(page.runs)
      setListFailed(false)
    } catch {
      setListFailed(true)
    } finally {
      inFlight = false
    }
  }

  // A different thread never shows the previous thread's runs. The list is read now and then every few seconds.
  createEffect(() => {
    const threadId = props.threadId
    if (!cardsOn() || !threadId) return
    untrack(() => {
      setRuns([])
      void refresh()
    })
    const timer = setInterval(() => void refresh(), LIST_POLL_MS)
    onCleanup(() => clearInterval(timer))
  })

  return (
    <Show when={startOn() || cardsOn()}>
      <section class="verevon-space-browser-thread" aria-label={i18n.tr('Nettleserkjøringer i denne tråden', 'Browser runs in this thread')}>
        <Show when={cardsOn() && runs().length > 0}>
          <For each={runs()}>
            {(run) => (
              <SpaceBrowserCard
                spaceRef={props.spaceRef}
                run={run}
                viewerSubjectId={props.viewerSubjectId}
                startedByName={props.nameOf(run.startedBySubjectId)}
                onOpen={props.onOpen}
              />
            )}
          </For>
        </Show>
        <Show when={cardsOn() && listFailed()}>
          <p class="verevon-space-browser-thread__status" role="status">
            {i18n.tr('Nettleserkjøringene kunne ikke hentes akkurat nå.', 'The browser runs could not be read right now.')}
          </p>
        </Show>
        <Show when={startOn()}>
          <Show
            when={composing()}
            fallback={
              <div class="verevon-space-browser-thread__entry">
                <button
                  type="button"
                  class="verevon-space-browser-thread__browse"
                  aria-expanded="false"
                  disabled={!props.canStart}
                  onClick={() => setComposing(true)}
                >
                  {i18n.tr('Surf med agenten', 'Browse with the agent')}
                </button>
                <Show when={!props.canStart && props.disabledReason}>
                  <p class="verevon-space-browser-thread__reason">{props.disabledReason}</p>
                </Show>
              </div>
            }
          >
            <SpaceBrowserStartPanel
              spaceRef={props.spaceRef}
              threadId={props.threadId}
              canStart={props.canStart}
              disabledReason={props.disabledReason}
              onStarted={(receipt) => {
                setComposing(false)
                void refresh()
                props.onOpen(receipt.runId)
              }}
              onCancel={() => setComposing(false)}
            />
          </Show>
        </Show>
      </section>
    </Show>
  )
}
