import { For, Show, createSignal, onCleanup, onMount } from 'solid-js'

import { spaceBrowserEvidenceUrl, type SpaceBrowserRun } from '@/shared/api/spaces-client/browser-runs'
import { useI18n } from '@/shared/i18n'
import {
  createSpaceBrowserRunFeed,
  lastSpaceBrowserActions,
  latestSpaceBrowserEvidenceId,
  spaceBrowserEventLabel,
  spaceBrowserPhaseLabel,
} from '../../lib/space-browser'
import './space-browser.css'

export interface SpaceBrowserCardProps {
  readonly spaceRef: string
  /** The run as the room's list states it (R2): the header, the hosts and the step count. */
  readonly run: SpaceBrowserRun
  readonly viewerSubjectId: string
  /** The starter's display name, resolved by the room. A person who left the room reads as a former member. */
  readonly startedByName: string
  readonly onOpen: (runId: string) => void
}

/**
 * The browser run's card in a thread (WS20-S5). Everyone in the room sees the phase, who started the run, the hosts,
 * the step count, the last steps and the control state. Only the starter sees a screenshot (SD-23): for anyone else the
 * card carries receipts only, and no image is requested. It polls its timeline while visible and the run is live.
 */
export function SpaceBrowserCard(props: SpaceBrowserCardProps) {
  const i18n = useI18n()
  let root: HTMLElement | undefined
  const [visible, setVisible] = createSignal(true)
  const feed = createSpaceBrowserRunFeed({
    spaceRef: () => props.spaceRef,
    runId: () => props.run.runId,
    visible,
  })

  onMount(() => {
    if (typeof IntersectionObserver === 'undefined' || !root) return
    const observer = new IntersectionObserver((entries) => {
      setVisible(entries.some((entry) => entry.isIntersecting))
    })
    observer.observe(root)
    onCleanup(() => observer.disconnect())
  })

  const starter = () => props.viewerSubjectId === props.run.startedBySubjectId
  const phase = () => feed.header()?.phase ?? props.run.phase
  const steps = () => lastSpaceBrowserActions(feed.events(), 3)
  // The starter alone asks for a screenshot; for everyone else no image URL is built.
  const shot = () => (starter() ? latestSpaceBrowserEvidenceId(feed.events()) : undefined)

  return (
    <article
      ref={(element) => {
        root = element
      }}
      class="verevon-space-browser-card"
      aria-label={i18n.tr('Nettleserkjøring', 'Browser run')}
      data-run-id={props.run.runId}
    >
      <header class="verevon-space-browser-card__head">
        <span class="verevon-space-browser-card__chip">{i18n.tr('Nettleser', 'Browser')}</span>
        <span class="verevon-space-browser-card__phase" data-live={feed.live() ? 'true' : undefined}>
          {spaceBrowserPhaseLabel(phase(), i18n.tr)}
        </span>
      </header>
      <p class="verevon-space-browser-card__meta">
        {i18n.tr('Startet av', 'Started by')} {props.startedByName} ·{' '}
        {i18n.tr(`${props.run.stepCount} steg`, `${props.run.stepCount} steps`)}
      </p>
      <Show when={props.run.hosts.length > 0}>
        <p class="verevon-space-browser-card__hosts">{props.run.hosts.join(', ')}</p>
      </Show>
      <p class="verevon-space-browser-card__control">{i18n.tr('kontroll: agent', 'control: agent')}</p>
      <ol class="verevon-space-browser-card__steps" aria-label={i18n.tr('Siste steg', 'Latest steps')}>
        <For each={steps()}>{(event) => <li>{spaceBrowserEventLabel(event, i18n.tr)}</li>}</For>
      </ol>
      <Show when={shot()}>
        {(evidenceId) => (
          <img
            class="verevon-space-browser-card__shot"
            src={spaceBrowserEvidenceUrl(props.spaceRef, props.run.runId, evidenceId())}
            alt={i18n.tr('Siste skjermbilde fra kjøringen', 'Latest screenshot from the run')}
            loading="lazy"
          />
        )}
      </Show>
      <Show when={feed.failed()}>
        <p class="verevon-space-browser-card__status" role="status">
          {i18n.tr('Status kunne ikke hentes akkurat nå.', 'Status could not be read right now.')}
        </p>
      </Show>
      <button type="button" class="verevon-space-browser-card__open" onClick={() => props.onOpen(props.run.runId)}>
        {i18n.tr('Åpne', 'Open')}
      </button>
    </article>
  )
}
