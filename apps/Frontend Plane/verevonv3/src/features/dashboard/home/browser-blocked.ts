import type { BrowserChallenge } from '@/shared/api/browser-client'
import { BLOCK_REASON_COPY, ERROR_COPY } from '@/shared/i18n/errors'

/**
 * When the browser chrome tells the person that a site blocks automation.
 *
 * The signal is Quarry's `challenge` on an observation. Only a `strong` one is a
 * verdict that a bot wall stands in front of the page (WS3.J): a login form, a
 * cookie banner, a paywall, a rate limit or a bare 403 arrive as `weak` or
 * `non_signal` and show nothing, so a person is never told "this site blocks
 * automation" about a page that merely asks them to sign in. A signal with no
 * `strength` at all is not read as strong here either: the notice offers to hand
 * the browser over or leave the site, and that has to be earned by a typed verdict.
 */
export type BrowserBlockedState = {
  /** Why the wall is up, when Quarry named a reason from its closed set. */
  reason: string | null
}

const hasOwn = (record: object, key: string): boolean => Object.prototype.hasOwnProperty.call(record, key)

export function blockedStateFor(challenge: BrowserChallenge | null | undefined): BrowserBlockedState | null {
  if (!challenge || typeof challenge !== 'object') return null
  if (challenge.strength !== 'strong') return null
  const reason = typeof challenge.block_reason === 'string' && hasOwn(BLOCK_REASON_COPY, challenge.block_reason)
    ? challenge.block_reason
    : null
  return { reason }
}

/** The sentence for the notice: the reason's own copy, else the general one. It
 * is written from the closed `block_reason` only, never from page text or from
 * the signal's `evidence` strings. */
export function blockedSentence(
  state: BrowserBlockedState,
  tr: (noText: string, enText: string) => string,
): string {
  const copy = (state.reason !== null ? BLOCK_REASON_COPY[state.reason] : undefined) ?? ERROR_COPY.browser_site_blocked
  if (!copy) return tr('Nettstedet stopper automatisk nettlesing.', 'The site blocks automated browsing.')
  return tr(copy.no, copy.en)
}

/**
 * Whether "Try another source" closed the session but could not stop the AI run
 * that was using it, because run controls are off for the organization (zero data
 * retention). The gateway's action result says so with `runControlsDisabled`;
 * it is read from the loose `result` of the action receipt, and only a literal
 * `true` counts.
 */
export function leftWithRunControlsDisabled(execution: unknown): boolean {
  if (!execution || typeof execution !== 'object') return false
  const result = (execution as { result?: unknown }).result
  if (!result || typeof result !== 'object') return false
  return (result as { runControlsDisabled?: unknown }).runControlsDisabled === true
}

/** What the person is told after such a leave: the site is left, the run was not stopped by us. */
export function runControlsDisabledNotice(tr: (noText: string, enText: string) => string): string {
  return tr(
    'Nettleserøkten er lukket, men AI-kjøringen kunne ikke stoppes herfra fordi kjøringskontroller er slått av for organisasjonen (ingen datalagring). Den kan fortsatt gi en feil før den avsluttes.',
    'The browser session is closed, but the AI run could not be stopped from here because run controls are turned off for your organization (zero data retention). It may still report an error before it ends.',
  )
}
