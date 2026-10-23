import type { JSX } from '@solidjs/web'
import type { BrowserNavigationFailure } from '@/features/browser/session/browser-navigation-error'
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
  return (
    <div class="browser-navigation-error" role="alert" data-reason={props.failure.reason}>
      <div class="browser-navigation-error__body">
        <p class="browser-navigation-error__title">Siden kunne ikke åpnes</p>
        <p class="browser-navigation-error__message">{props.failure.message}</p>
        <p class="browser-navigation-error__url" title={props.failure.url}>
          {props.failure.url}
        </p>
      </div>
      <div class="browser-navigation-error__actions">
        <button type="button" disabled={props.busy} onClick={() => props.onRetry(props.failure.url)}>
          Prøv igjen
        </button>
        <button type="button" onClick={() => props.onDismiss()}>
          Lukk
        </button>
      </div>
    </div>
  )
}
