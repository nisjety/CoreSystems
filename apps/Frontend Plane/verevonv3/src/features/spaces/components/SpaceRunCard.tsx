import { createMemo, createSignal, For, onCleanup, Show } from 'solid-js'

import { ApiError } from '@/shared/api/api-error'
import { stopSpaceRun } from '@/shared/api/spaces-client/run-stop'
import type { SpaceRunWork } from '@/shared/api/spaces-client/run-work'
import { useI18n } from '@/shared/i18n'
import {
  durationLabel,
  runHeadline,
  stepLine,
  summedStepSeconds,
  workSummaryLine,
} from '../lib/room-run-grammar'
import { elapsedSeconds, totalSteps, type SpaceRunView } from '../lib/space-run-view'
import './space-run.css'

/** How many of the newest steps the card lists before "N more". */
const VISIBLE_STEPS = 3

export interface SpaceRunCardProps {
  readonly view: SpaceRunView
  /** Who started the run, already resolved to a name (roster match, "former member", or unnamed). */
  readonly startedByName: string
  /**
   * Offers Stop (WS09-S8). Present only when the listing says manager Stop is on AND the reader's room
   * role is manager or owner; presentation only, since Control decides and Session Core verifies on
   * every call. Absent means no button.
   */
  readonly stop?: { readonly spaceRef: string; readonly runId: string }
}

/**
 * What every other member sees while someone's agent works in this post (WS09-S5). It replaces the
 * polled "is working" line while the live view is attached; the legacy row stays as the fallback.
 *
 * It shows the KIND of each step, the elapsed time, and the answer text only when the deployment
 * streams text to members (`space_run_live_text`); otherwise it says how much has been written. It never
 * shows tool input or output: the server does not send them. After the run it collapses to
 * "Used N tools in X s". A member cannot steer here (that is the run's owner, S2); a room manager or owner
 * gets Stop when `stop` is set (S8).
 *
 * Accessibility: the card is a labelled group, the headline is the one polite live region, and the
 * streaming text is `aria-live="off"` because the finished turn is announced by the transcript itself.
 */
export function SpaceRunCard(props: SpaceRunCardProps) {
  const i18n = useI18n()
  const [now, setNow] = createSignal(Date.now())
  const timer = setInterval(() => setNow(Date.now()), 1000)
  onCleanup(() => clearInterval(timer))
  const [showAll, setShowAll] = createSignal(false)
  // One stop attempt = one key; a retry after a failure reuses it so the server de-duplicates.
  const stopKey = `stop-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`
  const [stopState, setStopState] = createSignal<'idle' | 'pending' | 'sent' | 'forbidden' | 'failed'>('idle')

  const requestStop = async () => {
    const target = props.stop
    if (!target || stopState() === 'pending' || stopState() === 'sent') return
    setStopState('pending')
    try {
      await stopSpaceRun(target.spaceRef, target.runId, stopKey)
      // The run's end arrives as a `finished` frame; until then the card says the stop was sent.
      setStopState('sent')
    } catch (error) {
      const code = error instanceof ApiError ? error.code : ''
      setStopState(code === 'space_run_control_not_authorized' ? 'forbidden' : 'failed')
    }
  }

  const ended = () => props.view.finished !== null
  const steps = () => props.view.steps
  const shown = createMemo(() => (showAll() ? steps() : steps().slice(-VISIBLE_STEPS)))
  const hiddenCount = () => Math.max(0, totalSteps(props.view) - shown().length)
  const seconds = () => elapsedSeconds(props.view, now())
  const asWork = (): Pick<SpaceRunWork, 'steps' | 'steer'> => ({
    steps: steps()
      .filter((step) => step.state !== 'running')
      .map((step) => ({ id: step.id, class: step.class, state: step.state as 'ok' | 'error' | 'denied' })),
    steer: props.view.steer,
  })

  return (
    <div
      class="verevon-run-card"
      data-ended={ended() ? 'true' : 'false'}
      role="group"
      aria-label={i18n.tr('Agentens arbeid', 'Agent work')}
    >
      <Show
        when={!ended()}
        fallback={
          <details class="verevon-run-work">
            <summary>
              {/* The time is the sum of recorded step times; the live path carries none today, so this reads
                  "Used N tools" until the server stamps steps (a clock here would keep running after the end). */}
              {workSummaryLine(asWork(), i18n.tr, summedStepSeconds(asWork()))}
            </summary>
            <ul class="verevon-run-work__steps">
              <For each={steps()}>
                {(step) => (
                  <li class="verevon-run-work__step" data-state={step.state}>
                    {stepLine(step, i18n.tr)}
                  </li>
                )}
              </For>
            </ul>
          </details>
        }
      >
        <p class="verevon-run-card__headline" aria-live="polite">
          <span class="verevon-run-card__dots" aria-hidden="true" />
          {runHeadline(props.view, i18n.tr)}
        </p>
        <div class="verevon-run-card__head">
          <Show when={props.startedByName}>
            <span>{i18n.tr(`Startet av ${props.startedByName}`, `Started by ${props.startedByName}`)}</span>
          </Show>
          <Show when={seconds() > 0}>
            <span>{durationLabel(seconds(), i18n.tr)}</span>
          </Show>
          <Show when={props.view.steer > 0}>
            <span>
              {props.view.steer === 1
                ? i18n.tr('1 styring fra eieren', '1 steer from the owner')
                : i18n.tr(`${props.view.steer} styringer fra eieren`, `${props.view.steer} steers from the owner`)}
            </span>
          </Show>
        </div>
        <Show when={props.stop}>
          <div class="verevon-run-card__stop">
            <button
              type="button"
              class="verevon-run-card__stop-button"
              disabled={stopState() === 'pending' || stopState() === 'sent'}
              onClick={() => { void requestStop() }}
            >
              {i18n.tr('Stopp kjøringen', 'Stop this run')}
            </button>
            <Show when={stopState() === 'sent'}>
              <span role="status">{i18n.tr('Stopp sendt …', 'Stop sent…')}</span>
            </Show>
            <Show when={stopState() === 'forbidden'}>
              <span role="alert">
                {i18n.tr(
                  'Organisasjonen din har ikke slått på stopping av andre medlemmers kjøringer.',
                  "Your organisation has not enabled stopping other members' runs.",
                )}
              </span>
            </Show>
            <Show when={stopState() === 'failed'}>
              <span role="alert">
                {i18n.tr('Kunne ikke stoppe kjøringen. Prøv igjen.', 'Could not stop the run. Try again.')}
              </span>
            </Show>
          </div>
        </Show>
        <Show when={shown().length > 0}>
          <ul class="verevon-run-card__steps" aria-label={i18n.tr('Siste steg', 'Latest steps')}>
            <For each={shown()}>
              {(step) => (
                <li class="verevon-run-card__step" data-state={step.state}>
                  {stepLine(step, i18n.tr)}
                </li>
              )}
            </For>
          </ul>
          <Show when={hiddenCount() > 0 || showAll()}>
            <button
              type="button"
              class="verevon-run-card__more"
              aria-expanded={showAll() ? 'true' : 'false'}
              onClick={() => setShowAll(!showAll())}
            >
              {showAll()
                ? i18n.tr('Vis færre', 'Show fewer')
                : hiddenCount() === 1
                  ? i18n.tr('1 steg til', '1 more step')
                  : i18n.tr(`${hiddenCount()} steg til`, `${hiddenCount()} more steps`)}
            </button>
          </Show>
        </Show>
        <Show
          when={props.view.text.visible && props.view.text.value}
          fallback={
            <Show when={props.view.text.progressChars > 0}>
              <p class="verevon-run-card__progress">
                {i18n.tr(
                  `Skriver svaret … ${props.view.text.progressChars} tegn`,
                  `Writing the answer… ${props.view.text.progressChars} characters`,
                )}
              </p>
            </Show>
          }
        >
          <p class="verevon-run-card__text" aria-live="off">
            {props.view.text.value}
            <Show when={props.view.text.truncated}>{'…'}</Show>
          </p>
        </Show>
      </Show>
    </div>
  )
}

/**
 * The collapsed "Used N tools" line under a finished agent turn, from the stored `metadata.work`
 * summary (WS09-S3). Content-free: kinds and outcomes of steps only. Draws nothing when there is
 * nothing to say.
 */
export function SpaceRunWorkSummary(props: { readonly work: SpaceRunWork }) {
  const i18n = useI18n()
  const seconds = () => summedStepSeconds(props.work)
  return (
    <Show when={props.work.steps.length > 0 || props.work.steer > 0}>
      <details class="verevon-run-work">
        <summary>{workSummaryLine(props.work, i18n.tr, seconds())}</summary>
        <Show when={props.work.steps.length > 0}>
          <ul class="verevon-run-work__steps">
            <For each={props.work.steps}>
              {(step) => (
                <li class="verevon-run-work__step" data-state={step.state}>
                  {stepLine(step, i18n.tr)}
                </li>
              )}
            </For>
          </ul>
        </Show>
      </details>
    </Show>
  )
}
