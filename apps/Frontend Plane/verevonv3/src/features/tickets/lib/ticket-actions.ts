import { executeAction } from '@/shared/actions/action-client'
import type { ActionActor } from '@/shared/actions/types'
import {
  getTicket,
  type CreateTicketInput,
  type SupportTicket,
  type UpdateTicketInput,
} from '@/shared/api/tickets-client'

type TicketPatchAction = {
  actionId: 'tickets.assign' | 'tickets.resolve' | 'tickets.update'
  input: Record<string, unknown>
}

type TicketCreateInput = Pick<
  CreateTicketInput,
  'conversation_id' | 'priority' | 'severity' | 'category' | 'intent'
>

/**
 * Creates a case through the same action gateway used by every other Ticketing
 * lifecycle change, then reads the canonical conversation-core representation.
 */
export async function executeTicketCreate(
  actor: ActionActor,
  input: TicketCreateInput,
): Promise<SupportTicket> {
  const execution = await executeAction('tickets.create', actor, {
    conversationId: input.conversation_id,
    priority: input.priority,
    severity: input.severity,
    category: input.category,
    intent: input.intent,
  })
  if (!execution.ticketId) {
    throw new Error('tickets.create completed without a durable ticket identifier.')
  }
  return getTicket(actor.orgId, execution.ticketId)
}

/**
 * Converts a ticket mutation into exactly one shared action contract.
 *
 * A lifecycle transition and a reassignment are deliberately not combined:
 * the operator gets one auditable receipt per state change and no partial
 * mutation can be hidden behind an apparently-successful compound request.
 */
export function ticketActionForPatch(ticketId: string, patch: UpdateTicketInput): TicketPatchAction {
  const assignment = {
    assigneeUserId: patch.assignee_user_id,
    assigneeName: patch.assignee_name,
    teamId: patch.team_id,
    teamName: patch.team_name,
  }
  const hasAssignment = Object.values(assignment).some((value) => value !== undefined)
  const update = {
    status: patch.status,
    workType: patch.work_type,
    priority: patch.priority,
    severity: patch.severity,
    category: patch.category,
    intent: patch.intent,
    dueAt: patch.due_at,
    followUpAt: patch.follow_up_at,
    snoozedUntil: patch.snoozed_until,
  }
  const hasUpdate = Object.values(update).some((value) => value !== undefined)

  if (hasAssignment && hasUpdate) {
    throw new Error('Ticket assignment and lifecycle changes must be executed one action at a time.')
  }

  if (hasAssignment) {
    return { actionId: 'tickets.assign', input: { ticketId, ...assignment } }
  }

  if (patch.status === 'resolved' && Object.values(update).filter((value) => value !== undefined).length === 1) {
    return { actionId: 'tickets.resolve', input: { ticketId } }
  }

  if (!hasUpdate) {
    throw new Error('Ticket update requires at least one change.')
  }

  return { actionId: 'tickets.update', input: { ticketId, ...update } }
}

export async function executeTicketPatch(
  actor: ActionActor,
  ticket: Pick<SupportTicket, 'id'>,
  patch: UpdateTicketInput,
): Promise<SupportTicket> {
  const action = ticketActionForPatch(ticket.id, patch)
  await executeAction(action.actionId, actor, action.input)
  return getTicket(actor.orgId, ticket.id)
}

export async function executeTicketResourceLink(
  actor: ActionActor,
  ticket: SupportTicket,
  input: {
    link_type?: 'normal' | 'parent' | 'child' | 'related' | 'external'
    resource_kind: 'conversation_source' | 'social_post' | 'campaign' | 'order' | 'document' | 'ticket' | 'external'
    resource_id?: string
    resource_url?: string
    label?: string
    metadata?: Record<string, unknown>
  },
): Promise<SupportTicket> {
  await executeAction('tickets.link_resource', actor, {
    ticketId: ticket.id,
    linkType: input.link_type,
    resourceKind: input.resource_kind,
    resourceId: input.resource_id,
    resourceUrl: input.resource_url,
    label: input.label,
    metadata: input.metadata,
  })
  return getTicket(actor.orgId, ticket.id)
}

export async function executeTicketMacro(
  actor: ActionActor,
  ticket: Pick<SupportTicket, 'id'>,
  macroId: string,
  expectedMacroUpdatedAt?: string,
): Promise<SupportTicket> {
  await executeAction('tickets.run_macro', actor, { ticketId: ticket.id, macroId, expectedMacroUpdatedAt })
  return getTicket(actor.orgId, ticket.id)
}

export async function executeTicketChecklistCreate(
  actor: ActionActor,
  ticket: SupportTicket,
  input: { name: string; template_id?: string; items?: string[] },
): Promise<SupportTicket> {
  await executeAction('tickets.create_checklist', actor, {
    ticketId: ticket.id,
    name: input.name,
    templateId: input.template_id,
    items: input.items ?? [],
  })
  return getTicket(actor.orgId, ticket.id)
}

export async function executeTicketChecklistItemUpdate(
  actor: ActionActor,
  ticket: SupportTicket,
  input: { checklistId: string; itemId: string; completed: boolean },
): Promise<SupportTicket> {
  await executeAction('tickets.update_checklist_item', actor, { ticketId: ticket.id, ...input })
  return getTicket(actor.orgId, ticket.id)
}

export async function executeTicketSideConversationCreate(
  actor: ActionActor,
  ticket: Pick<SupportTicket, 'id'>,
  input: { subject: string; body_text: string },
): Promise<SupportTicket> {
  await executeAction('tickets.create_side_conversation', actor, {
    ticketId: ticket.id,
    subject: input.subject,
    bodyText: input.body_text,
  })
  return getTicket(actor.orgId, ticket.id)
}

export async function executeTicketSideConversationMessage(
  actor: ActionActor,
  ticket: Pick<SupportTicket, 'id'>,
  input: { sideConversationId: string; body_text: string },
): Promise<SupportTicket> {
  await executeAction('tickets.add_side_conversation_message', actor, {
    ticketId: ticket.id,
    sideConversationId: input.sideConversationId,
    bodyText: input.body_text,
  })
  return getTicket(actor.orgId, ticket.id)
}

export async function executeTicketSideConversationStatus(
  actor: ActionActor,
  ticket: Pick<SupportTicket, 'id'>,
  input: { sideConversationId: string; status: 'open' | 'closed' },
): Promise<SupportTicket> {
  await executeAction('tickets.update_side_conversation', actor, {
    ticketId: ticket.id,
    sideConversationId: input.sideConversationId,
    status: input.status,
  })
  return getTicket(actor.orgId, ticket.id)
}
