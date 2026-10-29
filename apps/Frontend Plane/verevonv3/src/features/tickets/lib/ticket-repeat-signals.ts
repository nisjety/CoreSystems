import type { SupportTicket, TicketWorkType } from '@/shared/api/tickets-client'

export type TicketRepeatSignal = {
  category: string
  intent: string
  workType: TicketWorkType | string
  count: number
  /** Bounded identifiers let an operator inspect the supporting tickets without
   * turning this lightweight queue signal into a second ticket index. */
  ticketKeys: string[]
  /** Same bounded set and order as ticketKeys, but the actual ticket id --
   * needed to anchor a similarity-candidates lookup, which the display key
   * alone can't do. */
  ticketIds: string[]
}

const terminalStatuses = new Set(['resolved', 'solved', 'closed'])
const maxEvidenceTickets = 3

function canonicalTaxonomy(value: string | undefined): string {
  return value?.trim().replace(/\s+/g, ' ').toLowerCase() ?? ''
}

function signalKey(category: string, intent: string, workType: string): string {
  return `${workType}\u0000${category}\u0000${intent}`
}

/**
 * Derives a repeat signal from the exact ticket taxonomy already present in a
 * loaded, organization-scoped queue. It intentionally does not infer semantic
 * similarity, cross an organization boundary, include terminal work, or create
 * an Incident/Problem. Those require separate evidence and an explicit review.
 */
export function deriveTicketRepeatSignals(tickets: readonly SupportTicket[]): TicketRepeatSignal[] {
  const grouped = tickets.reduce<Readonly<Record<string, TicketRepeatSignal>>>((signals, ticket) => {
    if (terminalStatuses.has(ticket.status.trim().toLowerCase())) return signals

    const category = canonicalTaxonomy(ticket.category)
    const intent = canonicalTaxonomy(ticket.intent)
    if (!category || !intent) return signals

    const workType = ticket.work_type?.trim() || 'customer_case'
    const key = signalKey(category, intent, workType)
    const current = signals[key]
    const belowEvidenceCap = !current || current.ticketKeys.length < maxEvidenceTickets
    const ticketKeys = belowEvidenceCap
      ? [...(current?.ticketKeys ?? []), ticket.ticket_key]
      : current.ticketKeys
    const ticketIds = belowEvidenceCap
      ? [...(current?.ticketIds ?? []), ticket.id]
      : current.ticketIds
    const next: TicketRepeatSignal = current
      ? { ...current, count: current.count + 1, ticketKeys, ticketIds }
      : { category, intent, workType, count: 1, ticketKeys, ticketIds }

    return { ...signals, [key]: next }
  }, {})

  return Object.values(grouped)
    .filter((signal) => signal.count >= 2)
    .sort((left, right) => right.count - left.count
      || left.category.localeCompare(right.category)
      || left.intent.localeCompare(right.intent)
      || left.workType.localeCompare(right.workType))
}
