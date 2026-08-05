import type { InboxRouteFilter, ZammadTicket } from '@/features/inbox/lib/inbox-model'
import { buildModelContextPack, type ModelContextPack, type SupportAssistantContext, type VisibleItem } from '@/shared/context-packs/context-pack'

/**
 * Builds the non-conversational operating state that accompanies an Inbox
 * assist request. This intentionally contains identifiers and lifecycle
 * summaries only: the transcript is the source of customer facts, while this
 * pack tells the model which work record and queue the agent is operating in.
 */
export function buildInboxAssistContext(input: {
  selectedTicket: ZammadTicket | null
  visibleTickets?: readonly ZammadTicket[]
  filter?: InboxRouteFilter
  draftInput?: string
  orgId?: string
  permissions?: readonly string[]
}): ModelContextPack {
  const itemFor = (ticket: ZammadTicket): VisibleItem => ({
    type: 'ticket',
    id: ticket.supportTicket?.id ?? `inbox:${ticket.id}`,
    // Never put a customer name, email address, or ticket title in the pack.
    // The number/key is sufficient for an operator and for a model to refer to
    // work safely alongside the separate, explicit transcript.
    label: ticket.supportTicket?.ticket_key ?? `conversation-${ticket.number}`,
    status: ticket.supportTicket?.status ?? ticket.state?.name ?? 'open',
  })

  const selectedEntity = input.selectedTicket ? itemFor(input.selectedTicket) : undefined
  const visibleItems = (input.visibleTickets ?? (input.selectedTicket ? [input.selectedTicket] : []))
    .slice(0, 25)
    .map(itemFor)
  const filter = input.filter
  const filters = filter
    ? Object.fromEntries(([
      ['status', filter.activeTab],
      ['assignment', filter.assigned],
      ['channel', filter.channel],
      ['queue', filter.queue],
      ['agent_state', filter.agentState],
    ] as Array<[string, string | undefined]>).filter((entry): entry is [string, string] => Boolean(entry[1])))
    : undefined

  const selected = input.selectedTicket
  const support: SupportAssistantContext | undefined = selected
    ? {
      organization: input.orgId ? { id: input.orgId } : undefined,
      conversation: {
        id: selected.conversationId ?? String(selected.id),
        title: selected.title,
        channel: selected.channel,
        status: selected.state?.name,
        customer: selected.customer
          ? {
            id: String(selected.customer.id),
            name: `${selected.customer.firstname} ${selected.customer.lastname}`.trim() || undefined,
            email: selected.customer.email || undefined,
          }
          : undefined,
      },
      ticket: selected.supportTicket
        ? {
          id: selected.supportTicket.id,
          key: selected.supportTicket.ticket_key,
          status: selected.supportTicket.status,
          slaState: selected.supportTicket.sla_state,
          priority: selected.priority?.name,
          category: selected.supportTicket.source,
        }
        : undefined,
      relatedConversations: (input.visibleTickets ?? [])
        .filter((ticket) => ticket.conversationId !== selected.conversationId)
        .slice(0, 8)
        .map((ticket) => ({
          id: ticket.conversationId ?? String(ticket.id),
          title: ticket.title,
          channel: ticket.channel,
          status: ticket.state?.name,
        })),
      availableActions: ['support.draft_reply', 'support.summarize', 'support.triage', 'support.resolve_review'],
      permissions: input.permissions ?? ['support.read'],
    }
    : undefined

  return buildModelContextPack({
    route: '/inbox',
    selectedEntity,
    visibleItems,
    filters,
    draftInput: input.draftInput?.trim() || undefined,
    support,
  })
}
