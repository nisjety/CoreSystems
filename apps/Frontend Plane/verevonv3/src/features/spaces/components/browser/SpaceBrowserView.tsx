import { For, Show, createMemo } from 'solid-js'

import { spaceBrowserEvidenceUrl } from '@/shared/api/spaces-client/browser-runs'
import { useI18n } from '@/shared/i18n'
import {
  createSpaceBrowserRunFeed,
  spaceBrowserEventLabel,
  spaceBrowserEvidenceIds,
  spaceBrowserPhaseLabel,
} from '../../lib/space-browser'
import './space-browser.css'

/** The most screenshots the starter's view shows at once. */
const MAX_SHOTS = 6

export interface SpaceBrowserViewProps {
  readonly spaceRef: string
  /** The run the pane was opened for (the `browser_run` parameter). */
  readonly runId: () => string
  readonly viewerSubjectId: string
  /** The starter's display name, resolved by the room. */
  readonly nameOf: (subjectId: string) => string
}

/**
 * The browser pane of a room run (WS20-S5): the run's whole timeline as receipts. The starter also sees the
 * screenshots (SD-23); every other member sees the rows and the hosts only. The live view is not in this pane: it
 * belongs to WS20-S8, and it is the starter's alone.
 */
export function SpaceBrowserView(props: SpaceBrowserViewProps) {
  const i18n = useI18n()
  const feed = createSpaceBrowserRunFeed({
    spaceRef: () => props.spaceRef,
    runId: props.runId,
    visible: () => true,
  })
  const header = () => feed.header()
  const starter = createMemo(() => header()?.startedBySubjectId === props.viewerSubjectId)
  const shots = () => (starter() ? spaceBrowserEvidenceIds(feed.events(), MAX_SHOTS) : [])

  return (
    <div class="verevon-space-browser-view" data-run-id={props.runId()}>
      <Show
        when={header()}
        fallback={
          <p class="verevon-space-browser-view__status" role="status">
            {feed.failed()
              ? i18n.tr('Kjøringen kunne ikke åpnes akkurat nå.', 'The run could not be opened right now.')
              : i18n.tr('Henter kjøringen …', 'Loading the run …')}
          </p>
        }
      >
        {(run) => (
          <>
            <p class="verevon-space-browser-view__meta">
              <span class="verevon-space-browser-card__phase" data-live={feed.live() ? 'true' : undefined}>
                {spaceBrowserPhaseLabel(run().phase, i18n.tr)}
              </span>{' '}
              · {i18n.tr('Startet av', 'Started by')} {props.nameOf(run().startedBySubjectId)} ·{' '}
              {i18n.tr('kontroll: agent', 'control: agent')}
            </p>
            <Show
              when={starter()}
              fallback={
                <p class="verevon-space-browser-view__note">
                  {i18n.tr(
                    'Skjermbilder og live-visningen vises bare for den som startet kjøringen.',
                    'Screenshots and the live view are shown only to the person who started the run.',
                  )}
                </p>
              }
            >
              <Show when={shots().length > 0}>
                <div class="verevon-space-browser-view__shots">
                  <For each={shots()}>
                    {(evidenceId) => (
                      <img
                        class="verevon-space-browser-card__shot"
                        src={spaceBrowserEvidenceUrl(props.spaceRef, props.runId(), evidenceId)}
                        alt={i18n.tr('Skjermbilde fra kjøringen', 'Screenshot from the run')}
                        loading="lazy"
                      />
                    )}
                  </For>
                </div>
              </Show>
            </Show>
          </>
        )}
      </Show>
      <ol class="verevon-space-browser-view__rows" aria-label={i18n.tr('Hendelser i kjøringen', 'Events in the run')}>
        <For each={feed.events()}>{(event) => <li>{spaceBrowserEventLabel(event, i18n.tr)}</li>}</For>
      </ol>
      <Show when={feed.failed() && header() !== null}>
        <p class="verevon-space-browser-view__status" role="status">
          {i18n.tr('Status kunne ikke hentes akkurat nå.', 'Status could not be read right now.')}
        </p>
      </Show>
    </div>
  )
}
