// Role-aware limit notices (F-t3-078): what a budget, quota or rate refusal says to the reader who sees it.
//
// An organisation's spend ceiling is the organisation's own figure. The Model Plane's 402 `budget_exceeded` body
// carries the spend and the cap, and the server's message repeats them, so a member is given the sentence and an
// action, never a figure. An owner or admin of the active organisation is pointed at the ceiling and, when the
// caller holds the server's amounts, sees them. A rate limit carries no spend, so it waits for the server's
// Retry-After hint and reads the same for every role.
//
// Dark behind VITE_CHAT_LIMIT_NOTICES, which is on only for the literal "true". The module stores, logs and sends
// nothing, so a zero-retention turn needs no skip. Copy goes through the caller's `tr(no, en)`.

import { localeIntl } from '@/shared/i18n/locales'

const LIMIT_CODES = ['budget_exceeded', 'quota_exceeded', 'run_ceiling_reached', 'rate_limited'] as const

/** The refusal codes this module words. Any other code keeps the presentation it already has. */
export type LimitCode = (typeof LIMIT_CODES)[number]

/**
 * What the reader can do. `contact_admin` and `open_billing` name a person or a page, and neither is a retry: the
 * same request is refused again until the ceiling changes.
 */
export type LimitAction = 'wait' | 'retry' | 'contact_admin' | 'open_billing' | 'none'

/** What the caller knows about a refusal beyond its code. Every field is optional. */
export type LimitHints = {
  /** Whole seconds until a retry can succeed: a `Retry-After` header, or a frame's `retry_after_ms` rounded up. */
  retryAfterSec?: number | null
  /** The organisation's spend so far, in USD, as the server sent it. Shown to an owner or admin only. */
  currentCostUsd?: number | null
  /** The organisation's spend ceiling, in USD, as the server sent it. Shown to an owner or admin only. */
  capCostUsd?: number | null
  /** The organisation's tokens so far, as the server sent them. Shown to an owner or admin only. */
  currentTokens?: number | null
  /** The organisation's token ceiling, as the server sent it. Shown to an owner or admin only. */
  capTokens?: number | null
}

export type LimitNotice = {
  /** What happened and what can be done, in the reader's language. */
  message: string
  action: LimitAction
  /** With `action: 'wait'`: whole seconds until a retry can succeed. */
  retryAfterSec?: number
}

/**
 * The translator, held as an object with a `tr` method (the shape `feedbackReasonLabels` takes), so each sentence here
 * is an `i18n.tr(no, en)` call, which the UI copy gate accepts in a new file.
 */
export type LimitCopy = { readonly tr: (noText: string, enText: string) => string }

/** Whether a limit notice is a code this module words. */
export function isLimitCode(code: unknown): code is LimitCode {
  return typeof code === 'string' && (LIMIT_CODES as readonly string[]).includes(code)
}

/**
 * Whether a role may change the organisation's ceiling: `owner` or `admin`. Read the way `access.ts` reads a role
 * list, since the session's role may carry several comma-separated roles.
 */
export function mayChangeLimits(role: string | null | undefined): boolean {
  return (role ?? '')
    .split(',')
    .map((part) => part.trim().toLowerCase())
    .some((part) => part === 'owner' || part === 'admin')
}

/** Whether limit notices are on. Only the literal `true` turns them on. */
export const chatLimitNoticesEnabled = (): boolean => import.meta.env.VITE_CHAT_LIMIT_NOTICES === 'true'

/** A figure the server sent: a finite, non-negative number. */
function isAmount(value: number | null | undefined): value is number {
  return typeof value === 'number' && Number.isFinite(value) && value >= 0
}

/** A USD amount, worded in Norwegian and in English so that the caller's `tr` picks one. */
function usd(value: number): { no: string; en: string } {
  return {
    no: new Intl.NumberFormat(localeIntl('no'), { style: 'currency', currency: 'USD' }).format(value),
    en: new Intl.NumberFormat(localeIntl('en'), { style: 'currency', currency: 'USD' }).format(value),
  }
}

/** A count of tokens, worded in Norwegian and in English. */
function tokenCount(value: number): { no: string; en: string } {
  return {
    no: new Intl.NumberFormat(localeIntl('no')).format(value),
    en: new Intl.NumberFormat(localeIntl('en')).format(value),
  }
}

function joinSentences(parts: string[]): string {
  return parts.filter((part) => part.length > 0).join(' ')
}

/**
 * The organisation's spend, from the amounts the server sent, as sentences in both languages. Only an owner or an
 * admin is given it; a member's words never carry a figure, so this is never called for one.
 */
function spendLine(hints: LimitHints, i18n: LimitCopy): string {
  const spent = isAmount(hints.currentCostUsd) ? usd(hints.currentCostUsd) : undefined
  const ceiling = isAmount(hints.capCostUsd) ? usd(hints.capCostUsd) : undefined
  const used = isAmount(hints.currentTokens) ? tokenCount(hints.currentTokens) : undefined
  const tokenCeiling = isAmount(hints.capTokens) ? tokenCount(hints.capTokens) : undefined
  const lines: string[] = []
  if (spent) {
    lines.push(ceiling
      ? i18n.tr(`Forbruket er ${spent.no} av taket på ${ceiling.no}.`, `Spend is ${spent.en} of the ${ceiling.en} ceiling.`)
      : i18n.tr(`Forbruket er ${spent.no}.`, `Spend is ${spent.en}.`))
  }
  if (used) {
    lines.push(tokenCeiling
      ? i18n.tr(`Det er brukt ${used.no} av ${tokenCeiling.no} tokens.`, `${used.en} of ${tokenCeiling.en} tokens are used.`)
      : i18n.tr(`Det er brukt ${used.no} tokens.`, `${used.en} tokens are used.`))
  }
  return lines.join(' ')
}

function budgetNotice(role: string | null | undefined, hints: LimitHints, i18n: LimitCopy): LimitNotice {
  if (!mayChangeLimits(role)) {
    return {
      message: i18n.tr(
        'Organisasjonens forbrukstak er nådd, så agenten startet ikke. Be en administrator om å endre taket.',
        "The organisation's spend ceiling has been reached, so the agent did not start. Ask an administrator to change it.",
      ),
      action: 'contact_admin',
    }
  }
  return {
    message: joinSentences([
      i18n.tr(
        'Forbrukstaket er nådd, så agenten startet ikke. Du kan endre taket under Innstillinger › Forbrukstak.',
        'The spend ceiling has been reached, so the agent did not start. You can change the ceiling under Settings › Spend ceiling.',
      ),
      spendLine(hints, i18n),
    ]),
    action: 'open_billing',
  }
}

function quotaNotice(role: string | null | undefined, hints: LimitHints, i18n: LimitCopy): LimitNotice {
  if (!mayChangeLimits(role)) {
    return {
      message: i18n.tr(
        'Organisasjonens kvote er brukt opp, så forespørselen ble ikke utført. Be en administrator om å endre kvoten.',
        "The organisation's quota is used up, so the request was not carried out. Ask an administrator to change it.",
      ),
      action: 'contact_admin',
    }
  }
  return {
    message: joinSentences([
      i18n.tr(
        'Organisasjonens kvote er brukt opp, så forespørselen ble ikke utført. Du kan endre kvoten under Innstillinger › Forbrukstak.',
        "The organisation's quota is used up, so the request was not carried out. You can change the quota under Settings › Spend ceiling.",
      ),
      spendLine(hints, i18n),
    ]),
    action: 'open_billing',
  }
}

function runCeilingNotice(role: string | null | undefined, hints: LimitHints, i18n: LimitCopy): LimitNotice {
  if (!mayChangeLimits(role)) {
    return {
      message: i18n.tr(
        'Denne kjøringen har nådd grensen. Be en administrator om å endre den.',
        'This run has reached its limit. Ask an administrator to change it.',
      ),
      action: 'contact_admin',
    }
  }
  return {
    message: joinSentences([
      i18n.tr(
        'Denne kjøringen har nådd grensen. Du kan endre grensen under Innstillinger › Forbrukstak.',
        'This run has reached its limit. You can change the limit under Settings › Spend ceiling.',
      ),
      spendLine(hints, i18n),
    ]),
    action: 'open_billing',
  }
}

/**
 * A rate limit reads the same for every role. With the server's Retry-After hint the reader is given a wait; without
 * one, a retry is offered.
 */
function rateNotice(hints: LimitHints, i18n: LimitCopy): LimitNotice {
  const delay = hints.retryAfterSec
  if (!isAmount(delay) || delay < 1) {
    return {
      message: i18n.tr(
        'Det kommer for mange forespørsler akkurat nå. Prøv igjen om litt.',
        'There are too many requests right now. Try again shortly.',
      ),
      action: 'retry',
    }
  }
  const seconds = Math.ceil(delay)
  return {
    message: seconds === 1
      ? i18n.tr(
          'Det kommer for mange forespørsler akkurat nå. Prøv igjen om 1 sekund.',
          'There are too many requests right now. Try again in 1 second.',
        )
      : i18n.tr(
          `Det kommer for mange forespørsler akkurat nå. Prøv igjen om ${seconds} sekunder.`,
          `There are too many requests right now. Try again in ${seconds} seconds.`,
        ),
    action: 'wait',
    retryAfterSec: seconds,
  }
}

/**
 * The words and the action a limit refusal gives this reader. `role` is the active organisation's role, which the
 * caller reads from the session. The server's own message is never an input here: for a budget refusal it repeats
 * the organisation's spend and cap.
 */
export function limitNotice(
  code: LimitCode,
  role: string | null | undefined,
  hints: LimitHints,
  i18n: LimitCopy,
): LimitNotice {
  switch (code) {
    case 'budget_exceeded':
      return budgetNotice(role, hints, i18n)
    case 'quota_exceeded':
      return quotaNotice(role, hints, i18n)
    case 'run_ceiling_reached':
      return runCeilingNotice(role, hints, i18n)
    case 'rate_limited':
      return rateNotice(hints, i18n)
  }
}
