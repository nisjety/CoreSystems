import { writePendingChatLaunch } from '@/features/chat/lib/pending-chat-launch'
import type { AssistMessage } from '@/features/inbox/lib/inbox-ai'
import { getConversationDetail } from '@/shared/api/inbox-client'
import { executeAction } from '@/shared/actions/action-client'
import type { ActionActor } from '@/shared/actions/types'
import type { SupportTicket } from '@/shared/api/tickets-client'

/** Creates the user-visible starting prompt for an explicit Ticket → Chat handoff.
 * It contains only the case metadata needed to orient the assistant; Chat must
 * retrieve any sensitive or dynamic evidence through permission-aware APIs. */
function boundedMessageBody(value: string, max = 2400): string {
  return value.trim().slice(0, max)
}

export function buildTicketChatPrompt(ticket: SupportTicket, messages: readonly AssistMessage[] = []): string {
  const title = ticket.conversation?.title || ticket.intent || ticket.category || 'Support case'
  const fields = [
    `Ticket: ${ticket.ticket_key} (${ticket.id})`,
    `Conversation: ${ticket.conversation_id}`,
    `Title: ${title}`,
    `State: ${ticket.status}`,
    `Priority: ${ticket.priority}; severity: ${ticket.severity}`,
    ticket.category ? `Category: ${ticket.category}` : null,
    ticket.intent ? `Intent: ${ticket.intent}` : null,
    ticket.team_name || ticket.assignee_name ? `Owner: ${ticket.team_name || ticket.assignee_name}` : null,
    ticket.sla_state ? `SLA: ${ticket.sla_state}` : null,
  ].filter((value): value is string => Boolean(value))

  const conversationEvidence = messages
    .filter((message) => message.body.trim())
    .slice(-12)
    .map((message, index) => {
      const speaker = message.internal
        ? 'Internal note'
        : message.agent
          ? message.from || 'Support agent'
          : message.from || 'Customer'
      return `${index + 1}. ${speaker}: ${boundedMessageBody(message.body)}`
    })

  return [
    'Help resolve the selected Verevon support case.',
    ...fields,
    conversationEvidence.length
      ? `PERMISSION-SCOPED CONVERSATION EVIDENCE (last ${conversationEvidence.length} authorized messages; use only this evidence):\n${conversationEvidence.join('\n')}`
      : 'PERMISSION-SCOPED CONVERSATION EVIDENCE: unavailable in this handoff. Do not infer the customer message or claim that conversation history was reviewed.',
    'First explain the customer goal, unresolved facts, risk, and next best safe step. Use only permission-aware evidence. Do not claim an action or delivery succeeded without an authoritative receipt. Prepare any reply, ticket change, or business action as a clearly labelled text proposal in the Chat answer. This full Chat handoff has no ticket-side reply editor: never say that a side-panel proposal is ready, editable, staged, sent, or delivered. If you include reply or note text, label it as a draft in this Chat answer only. The operator may return to the Ticketing Verevon panel and use “Suggest next action” to prepare a governed review proposal; no action is executed automatically. Make no claim that anything was staged, sent, or executed unless an authoritative receipt proves it.',
  ].join('\n')
}

export async function launchTicketAssistant(ticket: SupportTicket, actor: ActionActor): Promise<void> {
  let messages: AssistMessage[] = []
  try {
    const detail = await getConversationDetail(actor.orgId, ticket.conversation_id)
    messages = detail.articles.map((article) => ({
      agent: article.sender?.toLowerCase() === 'agent',
      from: article.from,
      body: article.bodyText || article.body || '',
      internal: article.internal,
    }))
  } catch {
    // The handoff remains safe and useful with case metadata only. The prompt
    // explicitly tells Chat that the permission-aware transcript was absent.
  }
  await executeAction('tickets.record_chat_handoff', actor, { ticketId: ticket.id })
  await writePendingChatLaunch({
    startNewThread: true,
    supportHandoff: {
      conversationId: ticket.conversation_id,
      orgId: actor.orgId,
      userId: actor.userId,
    },
    text: buildTicketChatPrompt(ticket, messages),
    tools: ['reason'],
  })
}
