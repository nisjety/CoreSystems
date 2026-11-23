import { createMemo, createSignal, For, Show } from 'solid-js'
import { Dynamic } from '@solidjs/web'

import {
  buildSpaceActivity,
  type SpaceActivityItem,
  type ThreadLikeActivitySource,
} from '@/features/spaces/lib/activity-grammar'
import { liveRunIdOfRow } from '@/features/spaces/lib/room-run-grammar'
import { COPY, say } from '@/features/spaces/lib/space-copy'
import {
  RUN_STOP_TIMEOUT_MS,
  RunStopTimeout,
  runStopCanBePressed,
  runStopStateForCode,
  type RunStopState,
} from '@/features/spaces/lib/space-run-stop'
import { formatWhenWithTime } from '@/features/spaces/lib/space-thread-presentation'
import { ApiError } from '@/shared/api/api-error'
import { stopSpaceRun } from '@/shared/api/spaces-client/run-stop'
import { useI18n } from '@/shared/i18n'
import './space-run.css'

/**
 * A Space's activity feed, rendered with the Verb/Object/Outcome grammar.
 *
 * Standalone and data-agnostic on purpose. It takes already-fetched threads
 * rather than fetching them, so whoever owns the Space cockpit decides when and
 * how the data arrives, and this stays a pure rendering surface with no opinion
 * about transport.
 *
 * It asks the BFF for nothing of its own, except the manager's Stop on a live run when its parent offers it (WS09-S8).
 * The classification lives in `../lib/activity-grammar`, in the browser, over what the owning plane already
 * returned through the gateway's existing proxy route — no new endpoint and no
 * activity semantics pushed into the BFF.
 *
 * Styling is semantic classes only; this project has no Tailwind.
 */

interface SpaceActivityFeedProps {
  readonly threads: readonly ThreadLikeActivitySource[]
  /**
   * Already-built items, for a caller whose source is not threads.
   *
   * The Work tab composes runs and schedules through the same grammar and hands
   * the result here, so the two tabs cannot drift into describing one run with
   * two different vocabularies. When present this WINS over `threads` rather
   * than merging with it: a feed built from two sources at once would order
   * them together and make it impossible to say which question the list is
   * answering.
   */
  readonly items?: readonly SpaceActivityItem[]
  /** Rendered instead of the list when there is genuinely nothing yet. */
  readonly emptyLabel?: string
  /**
   * Offers the manager's Stop on each live run row (WS09-S8). Present only when the listing says manager Stop is on AND
   * the reader is the room's manager or owner: the gate the timeline applies. Presentation only; Control decides and
   * Session Core verifies on every stop. Absent means no Stop. The feed takes no lifecycle input, so a stop stays
   * available in an archived room (D22aj).
   */
  readonly stop?: { readonly spaceRef: string }
}

function toneClass(item: SpaceActivityItem): string {
  return `verevon-activity-outcome verevon-activity-outcome--${item.outcome.tone}`
}

function rowClass(item: SpaceActivityItem): string {
  return [
    'verevon-activity-row',
    `verevon-activity-row--${item.salience}`,
    item.outcome.live ? 'verevon-activity-row--live' : '',
  ]
    .filter(Boolean)
    .join(' ')
}

/**
 * The stop attempts of one feed, by run. A row is rebuilt whenever the feed's items change, which is on every live
 * frame, so an attempt cannot live in its row: the row would lose its "sent", and its key, on the next frame. One run
 * keeps one key for every attempt, so a retry after a failure is the same attempt to the server, as the run card's is.
 */
function createRunStops() {
  const keys = new Map<string, string>()
  const [states, setStates] = createSignal<Readonly<Record<string, RunStopState>>>({})
  const stateOf = (runId: string): RunStopState => states()[runId] ?? 'idle'
  const setState = (runId: string, state: RunStopState) => {
    setStates((current) => ({ ...current, [runId]: state }))
  }
  const keyFor = (runId: string): string => {
    const held = keys.get(runId)
    if (held) return held
    const fresh = `stop-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`
    keys.set(runId, fresh)
    return fresh
  }
  const request = async (spaceRef: string, runId: string) => {
    if (!runStopCanBePressed(stateOf(runId))) return
    setState(runId, 'pending')
    // A stop that has not answered in time is "not confirmed yet", not a failure: the reader may send it again
    // under the same key, and the server de-duplicates (as the run card does).
    let timer: ReturnType<typeof setTimeout> | undefined
    const timedOut = new Promise<never>((_, reject) => {
      timer = setTimeout(() => reject(new RunStopTimeout()), RUN_STOP_TIMEOUT_MS)
    })
    try {
      await Promise.race([stopSpaceRun(spaceRef, runId, keyFor(runId)), timedOut])
      // The run's end arrives with the room's next read; until then the row says the stop was sent.
      setState(runId, 'sent')
    } catch (error) {
      if (error instanceof RunStopTimeout) setState(runId, 'timeout')
      else setState(runId, runStopStateForCode(error instanceof ApiError ? error.code : ''))
    } finally {
      clearTimeout(timer)
    }
  }
  return { stateOf, request }
}

/**
 * The manager's Stop on one live run row. Its attempt belongs to the feed (see `createRunStops`), so the row can be
 * rebuilt on a live frame without losing it. The copy is the run card's. A zero-retention refusal holds the control:
 * the organisation's policy does not change between two clicks, so a retry would be refused the same way; so does a
 * refusal for the organisation's own setting. A timeout or a failure leaves the control open for a retry.
 */
function RunStopButton(props: { readonly object: string; readonly state: RunStopState; readonly onStop: () => void }) {
  const i18n = useI18n()
  const label = () => i18n.tr('Stopp kjøringen', 'Stop this run')
  return (
    <div class="verevon-run-card__stop">
      <button
        type="button"
        class="verevon-run-card__stop-button"
        aria-label={`${label()}: ${props.object}`}
        disabled={!runStopCanBePressed(props.state)}
        onClick={() => props.onStop()}
      >
        {label()}
      </button>
      <Show when={props.state === 'sent'}>
        <span role="status">{i18n.tr('Stopp sendt …', 'Stop sent…')}</span>
      </Show>
      <Show when={props.state === 'forbidden'}>
        <span role="alert">
          {i18n.tr(
            'Organisasjonen din har ikke slått på stopping av andre medlemmers kjøringer.',
            "Your organisation has not enabled stopping other members' runs.",
          )}
        </span>
      </Show>
      <Show when={props.state === 'zdr'}>
        <span role="alert">
          {i18n.tr(
            'Med nulllagring av data kan kjøringen ikke stoppes herfra ennå. Ingenting er endret.',
            'With zero data retention the run cannot be stopped from here yet. Nothing was changed.',
          )}
        </span>
      </Show>
      <Show when={props.state === 'timeout'}>
        <span role="alert">
          {i18n.tr('Stoppen er ikke bekreftet ennå. Prøv igjen.', 'The stop is not confirmed yet. Try again.')}
        </span>
      </Show>
      <Show when={props.state === 'failed'}>
        <span role="alert">{i18n.tr('Kunne ikke stoppe kjøringen. Prøv igjen.', 'Could not stop the run. Try again.')}</span>
      </Show>
    </div>
  )
}

export function SpaceActivityFeed(props: SpaceActivityFeedProps) {
  const i18n = useI18n()
  const items = createMemo(() => props.items ?? buildSpaceActivity(props.threads, i18n.tr))
  const stops = createRunStops()

  return (
    <Show
      when={items().length > 0}
      fallback={
        <p class="verevon-activity-empty">
          {props.emptyLabel ?? say(i18n.tr, COPY.feed.empty)}
        </p>
      }
    >
      <ul class="verevon-activity-feed">
        <For each={items()}>
          {(item) => (
            <li class={rowClass(item)}>
              {/* An item whose owning Space is unknown carries no href (the
                  `/chat` adoption fallback was removed). Render it as a span
                  rather than an anchor with no destination: an `<a>` without
                  href is announced as a link and focusable, promising a
                  navigation that does not exist. */}
              <Dynamic
                component={item.href ? 'a' : 'span'}
                class="verevon-activity-link"
                href={item.href}
                link={item.href ? true : undefined}
              >
                <span class="verevon-activity-verb">{item.verb}</span>
                {': '}
                <span class="verevon-activity-object">{item.object}</span>
                {' → '}
                <span
                  class={toneClass(item)}
                  /* "Never go dark": a moving row announces itself to assistive
                     tech as it changes, rather than silently mutating. */
                  aria-live={item.outcome.live ? 'polite' : undefined}
                >
                  {item.outcome.label}
                </span>
              </Dynamic>
              <Show when={item.at}>
                {(at) => <span class="verevon-activity-when">{formatWhenWithTime(at(), { locale: i18n.locale() })}</span>}
              </Show>
              {/* Progressive disclosure without a second component: the
                  sentence stays one line and the evidence a supervisor asks
                  for next — token counts, the ticket an effect produced, who
                  granted an authority — sits directly beneath it. Outside the
                  link, because these are facts about the row rather than part
                  of its destination. */}
              <Show when={item.detail?.length}>
                <ul class="verevon-activity-detail">
                  <For each={item.detail ?? []}>{(line) => <li>{line}</li>}</For>
                </ul>
              </Show>
              {/* The manager's Stop on a live run (WS09-S8), offered only when the feed was given the gate. */}
              <Show when={props.stop ? liveRunIdOfRow(item) : undefined}>
                {(runId) => (
                  <RunStopButton
                    object={item.object}
                    state={stops.stateOf(runId())}
                    onStop={() => {
                      const target = props.stop
                      if (target) void stops.request(target.spaceRef, runId())
                    }}
                  />
                )}
              </Show>
            </li>
          )}
        </For>
      </ul>
    </Show>
  )
}
