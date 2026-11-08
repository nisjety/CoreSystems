import type { JSX } from '@solidjs/web'
import { Show } from 'solid-js'
import type { BrowserNavigationFailure } from '@/features/browser/session/browser-navigation-error'
import { useI18n } from '@/shared/i18n'
import './BrowserNavigationError.css'

/**
 * The error card shown in the browser chrome when a navigation did not load (WS9.B).
 * It shows the typed message and the URL that was asked for. Retry loads that URL
 * again; Dismiss removes the card. It never shows a raw server message or code.
 */
export function BrowserNavigationError(props: {
  failure: BrowserNavigationFailure
  busy?: boolean
  onRetry: (url: string) => void
  onDismiss: () => void
}): JSX.Element {
  const i18n = useI18n()
  return (
    <div class="browser-navigation-error" role="alert" data-reason={props.failure.reason}>
      <div class="browser-navigation-error__body">
        <p class="browser-navigation-error__title">{i18n.tr('Siden kunne ikke åpnes', 'The page could not be opened')}</p>
        <p class="browser-navigation-error__message">{props.failure.sentence}</p>
        <p class="browser-navigation-error__url" title={props.failure.url}>
          {props.failure.url}
        </p>
      </div>
      <div class="browser-navigation-error__actions">
        <button type="button" disabled={props.busy} onClick={() => props.onRetry(props.failure.url)}>
          {i18n.tr('Prøv igjen', 'Try again')}
        </button>
        <Show when={props.failure.openOutsideUrl}>
          {(url) => (
            <a href={url()} target="_blank" rel="noopener noreferrer">
              {i18n.tr('Åpne utenfor Verevon', 'Open outside Verevon')}
            </a>
          )}
        </Show>
        <button type="button" onClick={() => props.onDismiss()}>
          {i18n.tr('Lukk', 'Dismiss')}
        </button>
      </div>
    </div>
  )
}
