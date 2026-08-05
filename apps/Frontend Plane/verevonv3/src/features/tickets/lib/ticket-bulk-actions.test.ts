import { describe, expect, it, vi } from 'vitest'
import { executeBulkTicketStatusUpdate } from './ticket-bulk-actions'

describe('executeBulkTicketStatusUpdate', () => {
  const actor = { type: 'human' as const, orgId: 'org-1', userId: 'user-1' }
  const tickets = [{ id: 'ticket-1', ticket_key: 'TCK-1' }, { id: 'ticket-2', ticket_key: 'TCK-2' }]

  it('uses the ordinary audited patch and canonical reread path for every selected ticket', async () => {
    const patcher = vi.fn(async (_actor, ticket, patch) => ({ ...ticket, org_id: 'org-1', conversation_id: 'conv-1', status: patch.status, priority: 'normal', severity: 'medium', source: 'manual', created_at: '', updated_at: '' }))
    const result = await executeBulkTicketStatusUpdate(actor, tickets, 'waiting_team', patcher)

    expect(patcher).toHaveBeenCalledTimes(2)
    expect(patcher).toHaveBeenNthCalledWith(1, actor, tickets[0], { status: 'waiting_team' })
    expect(patcher).toHaveBeenNthCalledWith(2, actor, tickets[1], { status: 'waiting_team' })
    expect(result.updated.map((ticket) => ticket.id)).toEqual(['ticket-1', 'ticket-2'])
    expect(result.failed).toEqual([])
  })

  it('preserves successful authoritative updates and reports each failed ticket explicitly', async () => {
    const patcher = vi.fn()
      .mockResolvedValueOnce({ ...tickets[0], org_id: 'org-1', conversation_id: 'conv-1', status: 'open', priority: 'normal', severity: 'medium', source: 'manual', created_at: '', updated_at: '' })
      .mockRejectedValueOnce(new Error('conflict'))
    const result = await executeBulkTicketStatusUpdate(actor, tickets, 'open', patcher)

    expect(result.updated.map((ticket) => ticket.ticket_key)).toEqual(['TCK-1'])
    expect(result.failed.map(({ ticket }) => ticket.ticket_key)).toEqual(['TCK-2'])
  })
})
