import { createEffect, createSignal, Show } from 'solid-js'

import { useI18n } from '@/shared/i18n'
import type { SpaceStreamState } from '../lib/space-stream'
import './space-live-indicator.css'

/**
 * Says whether the room is updating live. `live` is deliberately quiet (a dot
 * and one word in the meta row); `degraded` says plainly that the room is
 * refreshing on a timer instead. States that are not a claim about the
 * connection (`idle`, `connecting`, `unavailable`) render nothing, so a room
 * that never had a stream looks exactly as it did before.
 *
 * A polite live region announces transitions only, never the steady state.
 */
export function SpaceLiveIndicator(props: { state: () => SpaceStreamState }) {
  const i18n = useI18n()
  const [announcement, setAnnouncement] = createSignal('')
  let previous: SpaceStreamState = 'idle'

  createEffect(
    () => props.state(),
    (next) => {
      const was = previous
      previous = next
      if (next === 'degraded' && was === 'live') {
        setAnnouncement(i18n.tr('Live-oppdateringer er satt på pause. Rommet oppdateres hvert par sekunder.', 'Live updates paused. The room is refreshing every few seconds.'))
      } else if (next === 'live' && was === 'degraded') {
        setAnnouncement(i18n.tr('Live-oppdateringer er tilbake.', 'Live updates are back.'))
      } else {
        setAnnouncement('')
      }
    },
  )

  return (
    <>
      <Show when={props.state() === 'live' || props.state() === 'degraded'}>
        <span class="verevon-space-live" data-state={props.state()}>
          <span class="verevon-space-live__dot" aria-hidden="true" />
          {props.state() === 'live'
            ? i18n.tr('Live', 'Live')
            : i18n.tr('Oppdateres hvert par sekunder', 'Refreshing every few seconds')}
        </span>
      </Show>
      <span class="verevon-space-live__announce" role="status" aria-live="polite" aria-atomic="true">
        {announcement()}
      </span>
    </>
  )
}
