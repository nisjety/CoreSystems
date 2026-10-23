/**
 * Pure presentation helpers for receipts and egress decisions (WS9.D). No network and no
 * Solid imports, so the mapping is testable on its own.
 */

export type Tr = (noText: string, enText: string) => string

export type DecisionTone = 'allow' | 'block' | 'neutral'

/** Tone of an egress decision. Anything other than `allow` or `block` is shown neutral. */
export function decisionTone(decision: string | null | undefined): DecisionTone {
  if (decision === 'allow') return 'allow'
  if (decision === 'block') return 'block'
  return 'neutral'
}

/** Short label for an egress decision. */
export function decisionLabel(decision: string | null | undefined, tr: Tr): string {
  const tone = decisionTone(decision)
  if (tone === 'allow') return tr('Tillatt', 'Allowed')
  if (tone === 'block') return tr('Blokkert', 'Blocked')
  return tr('Ukjent', 'Unknown')
}

const UNKNOWN_OUTCOME_COPY = { no: 'Ukjent', en: 'Unknown' }

const OUTCOME_COPY: Record<string, { no: string; en: string }> = {
  ok: { no: 'Fullført', en: 'Done' },
  completed: { no: 'Fullført', en: 'Done' },
  succeeded: { no: 'Fullført', en: 'Done' },
  failed: { no: 'Feilet', en: 'Failed' },
  error: { no: 'Feilet', en: 'Failed' },
  blocked: { no: 'Blokkert', en: 'Blocked' },
  denied: { no: 'Avvist', en: 'Denied' },
  unknown: { no: 'Ukjent', en: 'Unknown' },
}

/**
 * Label for a receipt outcome status. Unknown statuses show as "Unknown", never as the raw
 * server string, so a new status cannot leak an internal code into the page.
 */
export function outcomeLabel(status: string | null | undefined, tr: Tr): string {
  const key = (status ?? '').trim().toLowerCase()
  const copy = OUTCOME_COPY[key] ?? UNKNOWN_OUTCOME_COPY
  return tr(copy.no, copy.en)
}

/** Clock time of an egress decision (HH:MM:SS, local time). Invalid input gives an empty string. */
export function formatEvidenceTime(timestampMs: number | null | undefined): string {
  if (typeof timestampMs !== 'number' || !Number.isFinite(timestampMs)) return ''
  const date = new Date(timestampMs)
  if (Number.isNaN(date.getTime())) return ''
  return date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' })
}

/**
 * The action label of a procedure step for display. A typed text arrives already replaced by
 * `{ redacted: true, length }`; this shows the length, never a value.
 */
export function procedureActionSummary(action: Record<string, unknown>, tr: Tr): { type: string; hidden: string | null } {
  const type = typeof action.type === 'string' && action.type.trim() ? action.type.trim() : 'action'
  const text = action.text
  if (text && typeof text === 'object' && (text as { redacted?: unknown }).redacted === true) {
    const length = (text as { length?: unknown }).length
    const count = typeof length === 'number' && Number.isFinite(length) ? String(length) : '?'
    return { type, hidden: tr(`skjult tekst (${count} tegn)`, `hidden text (${count} characters)`) }
  }
  return { type, hidden: null }
}
