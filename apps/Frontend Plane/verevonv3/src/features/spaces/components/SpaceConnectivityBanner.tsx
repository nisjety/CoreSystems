import { onSettled, Show } from 'solid-js'

import { useI18n } from '@/shared/i18n'
import {
  roomLastFreshAt,
  spaceConnectivity,
  spaceConnectivityBannerEnabled,
  watchBrowserConnectivity,
} from '../lib/space-connectivity'
import { formatTurnTime } from '../lib/space-thread-presentation'
import './space-connectivity.css'

/**
 * Says plainly when a send cannot reach the room (WS01-S8). Offline and
 * unreachable are different facts with different next steps, so they are
 * different sentences; `online` renders nothing, so a room with a good
 * connection looks exactly as it did before. The live region announces the
 * change once, not every render.
 *
 * With `spaceRef`, the sentence also says when the room last heard from its
 * server, in the reader's own clock, so what is on screen is dated honestly.
 */
export function SpaceConnectivityBanner(props: { readonly spaceRef?: string }) {
  const i18n = useI18n()
  if (!spaceConnectivityBannerEnabled()) return null
  // Started once settled, not in the component body: the watcher's first sync may write a signal.
  // The returned stop function is the cleanup.
  onSettled(() => watchBrowserConnectivity())
  const lastFresh = () => {
    const at = props.spaceRef ? roomLastFreshAt(props.spaceRef) : undefined
    return at === undefined ? undefined : formatTurnTime(new Date(at).toISOString(), { locale: i18n.locale() })
  }

  return (
    <Show when={spaceConnectivity() !== 'online'}>
      <p class="verevon-space-connectivity" role="status" aria-live="polite" data-state={spaceConnectivity()}>
        {spaceConnectivity() === 'offline'
          ? i18n.tr(
              'Du er frakoblet. Meldinger du sender nå kan ikke leveres før du er tilbake på nett.',
              'You are offline. Messages you send now cannot be delivered until you are back online.',
            )
          : i18n.tr(
              'Rommet kan ikke nås akkurat nå. Meldingen din er ikke tapt: bruk «Prøv igjen», så sendes den én gang.',
              'The room cannot be reached right now. Your message is not lost: use "Try again" and it is sent once.',
            )}
        <Show when={lastFresh()}>
          {(fresh) => (
            <>
              {' '}
              {i18n.tr('Sist oppdatert kl.', 'Last updated at')}{' '}
              <time datetime={fresh().iso}>{fresh().short}</time>
            </>
          )}
        </Show>
      </p>
    </Show>
  )
}
