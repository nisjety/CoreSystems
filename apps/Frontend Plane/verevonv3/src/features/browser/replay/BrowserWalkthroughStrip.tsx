import { createSignal, For, Show } from 'solid-js'
import type { JSX } from '@solidjs/web'
import { cn } from '@/shared/lib/cn'
import { CameraOff, ImageOff } from '@/shared/icons'
import { useI18n } from '@/shared/i18n'
import type { ProofBrowserWalkthrough, ProofBrowserWalkthroughStep } from '@/shared/api/run-console-client'
import { runArtifactUrl } from '@/features/chat/lib/chat-run-watch'
import {
  browserActionHost,
  browserActionPill,
  browserActionTitle,
  emptyBrowserActionFacts,
  type BrowserActionFacts,
} from './browser-action-brief'
import './browser-walkthrough-strip.css'

/**
 * Build-time switch for the browser walkthrough in the run proof bundle (F-t3-050, W4-R3-b). Only the
 * literal `true` turns it on; unset keeps the proof bundle as it was.
 */
export function runProofWalkthroughEnabled(): boolean {
  return import.meta.env.VITE_RUN_PROOF_WALKTHROUGH === 'true'
}

/**
 * The facts a walkthrough step's copy reads, built from what the bundle carries. A step with no status has
 * no observation yet, so it reads as sent rather than as a result.
 */
function stepFacts(step: ProofBrowserWalkthroughStep): BrowserActionFacts {
  return {
    ...emptyBrowserActionFacts(step.actionId),
    actionType: step.actionType ?? '',
    observed: step.status !== undefined,
    outcome: step.status ?? '',
    pageTitle: step.pageTitle ?? '',
    pageUrl: step.pageUrl ?? '',
    screenshotRef: step.screenshotRef ?? '',
    url: step.pageUrl ?? '',
  }
}

/** One frame of the filmstrip: the step's screenshot, then its action, host and status. */
function WalkthroughFrame(props: { runId: string; step: ProofBrowserWalkthroughStep }) {
  const i18n = useI18n()
  const tr = (noText: string, enText: string) => i18n.tr(noText, enText)
  const [broken, setBroken] = createSignal(false)
  const facts = () => stepFacts(props.step)
  const host = () => browserActionHost(props.step.pageUrl)
  // The chat run's artifact helper turns the reference into its fetch URL. A reference it cannot
  // turn is a failed read, not an absence.
  const src = () => (broken() ? undefined : runArtifactUrl(props.runId, props.step.screenshotRef))
  const title = () => browserActionTitle(facts(), tr)
  const pill = () => browserActionPill(facts(), tr)

  return (
    <li class="browser-walkthrough-strip__step">
      <figure class="browser-walkthrough-strip__frame">
        <Show
          when={props.step.screenshotRef}
          fallback={(
            <div class="browser-walkthrough-strip__empty">
              <CameraOff size={16} aria-hidden="true" />
              <span>{i18n.tr('Ingen skjermbilde', 'No screenshot')}</span>
            </div>
          )}
        >
          <Show
            when={src()}
            fallback={(
              <div class="browser-walkthrough-strip__empty">
                <ImageOff size={16} aria-hidden="true" />
                <span>{i18n.tr('Kunne ikke laste skjermbildet.', 'Could not load the screenshot.')}</span>
              </div>
            )}
          >
            {(url) => (
              <img
                class="browser-walkthrough-strip__shot"
                src={url()}
                alt={host()
                  ? i18n.tr(`Skjermbilde av ${host()}`, `Screenshot of ${host()}`)
                  : i18n.tr('Skjermbilde fra nettleserkjøringen', 'Screenshot from the browser run')}
                loading="lazy"
                decoding="async"
                onError={() => setBroken(true)}
              />
            )}
          </Show>
        </Show>
        <figcaption class="browser-walkthrough-strip__caption">
          <span class="browser-walkthrough-strip__title">{title()}</span>
          <span class={cn('browser-walkthrough-strip__pill', `browser-walkthrough-strip__pill--${pill().tone}`)}>
            {pill().label}
          </span>
        </figcaption>
      </figure>
    </li>
  )
}

/**
 * The run proof's browser walkthrough as a horizontal filmstrip (F-t3-050, W4-R3-b): one frame per browser
 * action, in dispatch order. A frame shows the screenshot the run already recorded, read through the chat
 * run's artifact helper, with the action, its host and its status. The strip reads nothing else, fetches
 * nothing beyond those screenshot reads, and stores nothing.
 */
export function BrowserWalkthroughStrip(props: {
  /** The run the walkthrough belongs to. Its screenshots are read under this run id. */
  runId: string
  walkthrough: ProofBrowserWalkthrough
}): JSX.Element {
  const i18n = useI18n()
  let scroller: HTMLDivElement | undefined

  // Arrow keys, Home and End move the track, so the strip scrolls from the keyboard as well as by pointer.
  const onKeyDown = (event: KeyboardEvent) => {
    if (!scroller) return
    const page = scroller.clientWidth * 0.8
    switch (event.key) {
      case 'ArrowRight':
        scroller.scrollBy({ left: page })
        break
      case 'ArrowLeft':
        scroller.scrollBy({ left: -page })
        break
      case 'Home':
        scroller.scrollTo({ left: 0 })
        break
      case 'End':
        scroller.scrollTo({ left: scroller.scrollWidth })
        break
      default:
        return
    }
    event.preventDefault()
  }

  return (
    <div class="browser-walkthrough-strip">
      <Show
        when={props.walkthrough.steps.length > 0}
        fallback={(
          <p class="browser-walkthrough-strip__note">
            {i18n.tr(
              'Ingen nettleserhandlinger er registrert for denne kjøringen.',
              'No browser actions were recorded for this run.',
            )}
          </p>
        )}
      >
        <div
          ref={scroller}
          class="browser-walkthrough-strip__scroller"
          role="region"
          tabindex={0}
          aria-label={i18n.tr('Nettleserforløp, steg for steg', 'Browser walkthrough, step by step')}
          onKeyDown={onKeyDown}
        >
          <ol class="browser-walkthrough-strip__steps">
            <For each={props.walkthrough.steps}>
              {(step) => <WalkthroughFrame runId={props.runId} step={step} />}
            </For>
          </ol>
        </div>
      </Show>
      <Show when={props.walkthrough.truncated}>
        <p class="browser-walkthrough-strip__note">
          {i18n.tr(
            'Forløpet er ufullstendig: senere steg er ikke med.',
            'The walkthrough is incomplete: later steps are not shown.',
          )}
        </p>
      </Show>
    </div>
  )
}
