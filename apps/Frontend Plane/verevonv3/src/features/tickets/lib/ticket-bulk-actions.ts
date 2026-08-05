import type { ActionActor } from '@/shared/actions/types'
import type { SupportTicket } from '@/shared/api/tickets-client'
import { executeTicketPatch } from './ticket-actions'

/** The first bulk surface is intentionally limited to active-work states.
 * Resolution, closure, snooze, assignment, and macros remain individual,
 * reviewable operations until they have their own explicit batch policy. */
export const bulkTicketStatuses = ['open', 'waiting_customer', 'waiting_team', 'escalated'] as const
export type BulkTicketStatus = (typeof bulkTicketStatuses)[number]

export type BulkTicketStatusResult = {
  failed: Array<{ error: unknown; ticket: Pick<SupportTicket, 'id' | 'ticket_key'> }>
  updated: SupportTicket[]
}

type TicketPatcher = typeof executeTicketPatch

/** Applies one already-confirmed status to an explicit ticket set. Each ticket
 * still uses the standard audited action and canonical reread. Work proceeds
 * sequentially so a failure is reported precisely and does not hide a partial
 * outcome behind an all-or-nothing-looking toast. */
export async function executeBulkTicketStatusUpdate(
  actor: ActionActor,
  tickets: Array<Pick<SupportTicket, 'id' | 'ticket_key'>>,
  status: BulkTicketStatus,
  patcher: TicketPatcher = executeTicketPatch,
): Promise<BulkTicketStatusResult> {
  const updated: SupportTicket[] = []
  const failed: BulkTicketStatusResult['failed'] = []
  for (const ticket of tickets) {
    try {
      updated.push(await patcher(actor, ticket, { status }))
    } catch (error) {
      failed.push({ ticket, error })
    }
  }
  return { updated, failed }
}
