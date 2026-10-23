/**
 * Pure helpers of `browser-grants-prod-contract.spec.ts` (no Playwright import, so
 * `scripts/tests/browser-grant-refusal.test.mjs` can run them under plain node).
 *
 * Legs B and C of the production-mode grant contract fail until WS1.B is on the
 * stack. They may only be read as "the known failure" when the refusal IS the known
 * one: the gateway's session start answered, the run reached Quarry, and Quarry
 * refused it for the missing grant. Any other refusal, above all an authentication
 * one (a 401, a signed-out cookie, a token that did not mint), says the leg never
 * got as far as the grant and must stay a failure.
 */

/** One HTTP answer a leg saw: where, with what status, with what error message. */
export interface SeenRefusal {
  step: string
  status: number
  message: string
}

/** What the pre-WS1.B gateway is known to answer at the session start. */
export interface KnownRefusal {
  statuses: number[]
  messages: string[]
}

/** The refusal Quarry gives a production start that carries no grant (lifecycle.rs). */
export const NO_GRANT_MESSAGE = 'browser grant_id is required'

/**
 * Messages that mean "who are you", never "you have no grant". A refusal matching
 * this is not the known failure whatever the status and whatever the override says.
 */
export const AUTHENTICATION_REFUSAL =
  /authenticat|unauthori[sz]ed|not signed|sign[- ]?in|log[- ]?in|credential|token|session (?:is |has )?(?:expired|invalid)|csrf|origin/i

/** The defaults, overridable with CONTRACT_BC_EXPECTED_STATUSES / CONTRACT_BC_EXPECTED_MESSAGES. */
export function knownRefusalFromEnv(env: Record<string, string | undefined>): KnownRefusal {
  const statuses = (env.CONTRACT_BC_EXPECTED_STATUSES || '403')
    .split(',')
    .map((s) => Number(s.trim()))
    .filter((n) => Number.isInteger(n))
  const messages = (env.CONTRACT_BC_EXPECTED_MESSAGES || NO_GRANT_MESSAGE)
    .split('|')
    .map((s) => s.trim())
    .filter((s) => s.length > 0)
  return { statuses, messages }
}

export type Classification = { known: true } | { known: false; reason: string }

/**
 * The known failure is exactly: the answer was recorded at the `session` step (the
 * session start is where the missing grant bites), the status is a listed one, the
 * message is one of the listed messages word for word, and neither the status nor
 * the message is an authentication refusal.
 */
export function classifyKnownRefusal(seen: SeenRefusal | undefined, known: KnownRefusal): Classification {
  if (!seen) return { known: false, reason: 'no HTTP answer was recorded' }
  if (seen.status === 401) return { known: false, reason: `an authentication refusal (status 401) at ${seen.step}` }
  if (AUTHENTICATION_REFUSAL.test(seen.message)) {
    return { known: false, reason: `an authentication refusal at ${seen.step} (status ${seen.status}: ${seen.message})` }
  }
  if (seen.step !== 'session') {
    return { known: false, reason: `the refusal came at step ${seen.step}, not at the session start` }
  }
  if (!known.statuses.includes(seen.status)) {
    return { known: false, reason: `status ${seen.status} at ${seen.step} is not one of ${known.statuses.join(',')}` }
  }
  if (!known.messages.includes(seen.message)) {
    return { known: false, reason: `the message at ${seen.step} (status ${seen.status}: ${seen.message}) is not the missing-grant refusal` }
  }
  return { known: true }
}

/** What Quarry says when a step navigates outside the run's hosts (admission.rs). */
export const OUTSIDE_DOMAINS_MESSAGE = "browser navigation is outside the run's allowed domains"
/** What Quarry says for a grant it cannot use: forged, revoked, expired, unknown to the caller's org. */
export const GRANT_INACTIVE_MESSAGE = 'browser grant is inactive or expired'
/** What Quarry says when the grant's owner is not the caller (lifecycle.rs). */
export const GRANT_OTHER_USER_MESSAGE = 'browser grant belongs to another user'
