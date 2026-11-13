import { describeBrowserError, type BrowserAction } from '@/shared/api/browser-client'
import { ApiError } from '@/shared/api/http'

/**
 * A navigation that did not load. The error card keeps the URL the person asked
 * for, so Retry (and a soft reload) can try the same address again (WS9.B).
 */
export type BrowserNavigationFailure = {
  /** The URL the person asked for. Only navigate actions produce one. */
  url: string
  /** A typed reason for the card. Never a raw message from the server. */
  reason: 'invalid_url' | 'blocked' | 'outside_scope' | 'unreachable' | 'http_response' | 'failed'
  /** The user-facing sentence, already chosen by the caller's `tr` (no raw error text). */
  sentence: string
  openOutsideUrl?: string
}

export type NavigationCopy = (noText: string, enText: string) => string

const FALLBACK_COPY = {
  no: 'Siden kunne ikke åpnes.',
  en: 'The page could not be opened.',
}

/**
 * Turns a failed browser action into an error card, or `null` when the failure
 * is not a navigation (those keep the inline form error). The URL is the one the
 * action carried; nothing else from the error is shown.
 */
export function navigationFailureFromError(
  action: BrowserAction,
  error: unknown,
  tr: NavigationCopy,
): BrowserNavigationFailure | null {
  if (action.type !== 'navigate') return null
  if (
    error instanceof ApiError
    && error.code === 'DRIVER_FAILED'
    && error.blockReason === null
    && !Array.isArray(error.details)
    && error.details?.reason === 'navigation_http_response'
    && error.details.http_status !== 407
  ) {
    return navigationHttpResponseFailure(action.url, error.details.http_status, tr, error.details.redirect_count)
  }
  return {
    url: action.url,
    reason: navigationReasonOf(error),
    sentence: describeBrowserError(error, tr, FALLBACK_COPY).message,
  }
}

/** Keeps a retained tab recoverable after the site returned an HTTP error. */
export function navigationHttpResponseFailure(
  url: string,
  httpStatus: unknown,
  tr: NavigationCopy,
  redirectCount?: unknown,
): BrowserNavigationFailure {
  const hasStatus = typeof httpStatus === 'number'
    && Number.isInteger(httpStatus) && httpStatus >= 400 && httpStatus <= 599 && httpStatus !== 407
  const statusSentence = hasStatus
    ? tr(`Nettstedet svarte med HTTP ${httpStatus} da Verevon prøvde å åpne siden.`, `The site returned HTTP ${httpStatus} when Verevon tried to open the page.`)
    : tr('Nettstedet svarte med en feil da Verevon prøvde å åpne siden.', 'The site returned an error when Verevon tried to open the page.')
  const redirectSentence = redirectCount === 1
    ? tr('Nettstedet videresendte forespørselen én gang.', 'The site redirected the request once.')
    : typeof redirectCount === 'number' && Number.isSafeInteger(redirectCount) && redirectCount > 1
      ? tr(`Nettstedet videresendte forespørselen ${redirectCount} ganger.`, `The site redirected the request ${redirectCount} times.`)
      : null
  const sentence = redirectSentence ? `${statusSentence} ${redirectSentence}` : statusSentence
  const address = URL.canParse(url) ? new URL(url) : null
  const openOutsideUrl = address && (address.protocol === 'http:' || address.protocol === 'https:')
    && !address.username && !address.password
    ? address.href
    : undefined
  return { url, reason: 'http_response', sentence, openOutsideUrl }
}

/** The URL a Retry or a soft reload should load. */
export function retryUrlOf(failure: BrowserNavigationFailure | null | undefined): string | null {
  return failure ? failure.url : null
}

function navigationReasonOf(error: unknown): BrowserNavigationFailure['reason'] {
  if (!(error instanceof ApiError)) return 'failed'
  if (error.code === 'invalid_url') return 'invalid_url'
  // The gateway's typed code for a navigation outside the run's allowed domains (D2-046).
  if (error.code === 'browser_navigation_outside_scope') return 'outside_scope'
  if (error.blockReason !== null) return 'blocked'
  if (error.status === 0 || error.status >= 500) return 'unreachable'
  return 'failed'
}
