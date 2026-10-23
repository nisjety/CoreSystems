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
  reason: 'invalid_url' | 'blocked' | 'unreachable' | 'failed'
  /** The user-facing sentence, already chosen by the caller's `tr` (no raw error text). */
  message: string
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
  return {
    url: action.url,
    reason: navigationReasonOf(error),
    message: describeBrowserError(error, tr, FALLBACK_COPY).message,
  }
}

/** The URL a Retry or a soft reload should load. */
export function retryUrlOf(failure: BrowserNavigationFailure | null | undefined): string | null {
  return failure ? failure.url : null
}

function navigationReasonOf(error: unknown): BrowserNavigationFailure['reason'] {
  if (!(error instanceof ApiError)) return 'failed'
  if (error.code === 'invalid_url') return 'invalid_url'
  if (error.blockReason !== null) return 'blocked'
  if (error.status === 0 || error.status >= 500) return 'unreachable'
  return 'failed'
}
