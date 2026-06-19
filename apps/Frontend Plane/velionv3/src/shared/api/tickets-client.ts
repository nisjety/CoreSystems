import { requestJson } from '@/shared/api/http'
import type { ConversationSummary } from '@/shared/api/inbox-client'

export type TicketStatus =
  | 'suggested'
  | 'open'
  | 'waiting_customer'
  | 'waiting_team'
  | 'snoozed'
  | 'escalated'
  | 'resolved'
  | 'solved'
  | 'closed'

export type SupportTicket = {
  id: string
  org_id: string
  conversation_id: string
  ticket_key: string
  status: TicketStatus | string
  priority: 'low' | 'normal' | 'high' | 'urgent' | string
  severity: 'low' | 'medium' | 'high' | 'critical' | string
  category?: string
  intent?: string
  assignee_user_id?: string
  assignee_name?: string
  team_id?: string
  team_name?: string
  due_at?: string | null
  source: 'manual' | 'ai' | string
  ai_confidence?: number
  ai_reason?: string
  created_by?: string
  waiting_since?: string | null
  last_customer_reply_at?: string | null
  first_response_at?: string | null
  resolved_at?: string | null
  snoozed_until?: string | null
  sla_policy_id?: string
  escalation_at?: string | null
  labels?: string[]
  sla_state?: 'ok' | 'risk' | 'breached' | string
  conversation?: ConversationSummary
  linked_resources?: TicketLinkedResource[]
  checklists?: TicketChecklist[]
  created_at: string
  updated_at: string
}

export type TicketClassification = {
  id: string
  org_id: string
  conversation_id: string
  outcome: 'no_ticket' | 'suggest_ticket' | 'auto_ticket' | string
  confidence: number
  reason: string
  payload: Record<string, unknown>
  ticket?: SupportTicket
  created_at: string
}

export type TicketLinkedResource = {
  id: string
  org_id: string
  ticket_id: string
  conversation_id: string
  link_type?: 'normal' | 'parent' | 'child' | 'related' | 'external' | string
  resource_kind: string
  resource_id?: string
  resource_url?: string
  label?: string
  metadata?: Record<string, unknown>
  created_by_user_id?: string
  created_at: string
}

export type TicketView = {
  id: string
  org_id: string
  name: string
  scope: 'org' | 'user' | 'team' | string
  owner_user_id?: string
  team_id?: string
  visibility: 'sidebar' | 'hidden' | string
  filter: Record<string, unknown>
  sort: Record<string, unknown>
  group_by?: string
  sidebar_order: number
  created_at: string
  updated_at: string
}

export type TicketMacro = {
  id: string
  org_id: string
  name: string
  description?: string
  visibility: 'personal' | 'team' | 'org' | string
  team_id?: string
  active: boolean
  actions: Record<string, unknown>
  conditions: Record<string, unknown>
  created_at: string
  updated_at: string
}

export type TicketMacroRunResult = {
  ticket: SupportTicket
  macro: TicketMacro
}

export type TicketAutomationRule = {
  id: string
  org_id: string
  name: string
  event_name: string
  active: boolean
  conditions: Record<string, unknown>
  actions: Record<string, unknown>
  created_at: string
  updated_at: string
}

export type SlaPolicy = {
  id: string
  org_id: string
  name: string
  active: boolean
  conditions: Record<string, unknown>
  calendar_ref?: string
  first_response_minutes: number
  next_response_minutes: number
  resolution_minutes: number
  created_at: string
  updated_at: string
}

export type TicketChecklist = {
  id: string
  org_id: string
  ticket_id: string
  name: string
  template_id?: string
  created_by_user_id?: string
  items: TicketChecklistItem[]
  created_at: string
  updated_at: string
}

export type TicketChecklistItem = {
  id: string
  org_id: string
  checklist_id: string
  label: string
  completed: boolean
  position: number
  created_at: string
  updated_at: string
}

export type ListTicketsParams = {
  queue?: string
  status?: string
  assigned?: string
  team?: string
  label?: string
  priority?: string
  severity?: string
  sla_state?: string
  q?: string
  limit?: number
}

export type CreateTicketInput = {
  conversation_id: string
  status?: string
  priority?: string
  severity?: string
  category?: string
  intent?: string
  assignee_user_id?: string
  assignee_name?: string
  team_id?: string
  team_name?: string
  due_at?: string
  source?: string
  ai_confidence?: number
  ai_reason?: string
  created_by?: string
  waiting_since?: string
  last_customer_reply_at?: string
  first_response_at?: string
  resolved_at?: string
  snoozed_until?: string
  sla_policy_id?: string
  escalation_at?: string
  labels?: string[]
}

export type UpdateTicketInput = Partial<Omit<CreateTicketInput, 'conversation_id' | 'created_by'>>

export type TicketClassificationInput = {
  outcome?: 'no_ticket' | 'suggest_ticket' | 'auto_ticket'
  confidence: number
  reason: string
  suggested_fields?: Record<string, unknown>
  evidence_message_ids?: string[]
}

export function listTickets(orgId: string, params: ListTicketsParams = {}, signal?: AbortSignal) {
  const query = new URLSearchParams()
  query.set('limit', String(params.limit ?? 50))
  if (params.queue) query.set('queue', params.queue)
  if (params.status) query.set('status', params.status)
  if (params.assigned) query.set('assigned', params.assigned)
  if (params.team) query.set('team', params.team)
  if (params.label) query.set('label', params.label)
  if (params.priority) query.set('priority', params.priority)
  if (params.severity) query.set('severity', params.severity)
  if (params.sla_state) query.set('sla_state', params.sla_state)
  if (params.q) query.set('q', params.q)

  return requestJson<SupportTicket[]>(`/api/v1/tickets?${query}`, {
    headers: { 'x-velion-org-id': orgId },
    signal,
  })
}

export function createTicket(orgId: string, input: CreateTicketInput) {
  return requestJson<SupportTicket>('/api/v1/tickets', {
    method: 'POST',
    body: JSON.stringify(input),
    headers: { 'x-velion-org-id': orgId },
  })
}

export function getTicket(orgId: string, ticketId: string, signal?: AbortSignal) {
  return requestJson<SupportTicket>(`/api/v1/tickets/${encodeURIComponent(ticketId)}`, {
    headers: { 'x-velion-org-id': orgId },
    signal,
  })
}

export function updateTicket(orgId: string, ticketId: string, input: UpdateTicketInput) {
  return requestJson<SupportTicket>(`/api/v1/tickets/${encodeURIComponent(ticketId)}`, {
    method: 'PATCH',
    body: JSON.stringify(input),
    headers: { 'x-velion-org-id': orgId },
  })
}

export function linkTicketResource(
  orgId: string,
  ticketId: string,
  input: {
    link_type?: string
    resource_kind: string
    resource_id?: string
    resource_url?: string
    label?: string
    metadata?: Record<string, unknown>
  },
) {
  return requestJson<TicketLinkedResource>(`/api/v1/tickets/${encodeURIComponent(ticketId)}/links`, {
    method: 'POST',
    body: JSON.stringify(input),
    headers: { 'x-velion-org-id': orgId },
  })
}

export function classifyConversationForTicket(
  orgId: string,
  conversationId: string,
  input: TicketClassificationInput,
) {
  return requestJson<TicketClassification>(
    `/api/v1/tickets/conversations/${encodeURIComponent(conversationId)}/classifications`,
    {
      method: 'POST',
      body: JSON.stringify(input),
      headers: { 'x-velion-org-id': orgId },
    },
  )
}

export function listTicketViews(orgId: string, signal?: AbortSignal) {
  return requestJson<TicketView[]>('/api/v1/ticket-views', {
    headers: { 'x-velion-org-id': orgId },
    signal,
  })
}

export function createTicketView(orgId: string, input: Partial<Omit<TicketView, 'id' | 'org_id' | 'created_at' | 'updated_at'>>) {
  return requestJson<TicketView>('/api/v1/ticket-views', {
    method: 'POST',
    body: JSON.stringify(input),
    headers: { 'x-velion-org-id': orgId },
  })
}

export function updateTicketView(orgId: string, viewId: string, input: Partial<TicketView>) {
  return requestJson<TicketView>(`/api/v1/ticket-views/${encodeURIComponent(viewId)}`, {
    method: 'PATCH',
    body: JSON.stringify(input),
    headers: { 'x-velion-org-id': orgId },
  })
}

export function listTicketMacros(orgId: string, signal?: AbortSignal) {
  return requestJson<TicketMacro[]>('/api/v1/ticket-macros', {
    headers: { 'x-velion-org-id': orgId },
    signal,
  })
}

export function createTicketMacro(orgId: string, input: Partial<Omit<TicketMacro, 'id' | 'org_id' | 'created_at' | 'updated_at'>>) {
  return requestJson<TicketMacro>('/api/v1/ticket-macros', {
    method: 'POST',
    body: JSON.stringify(input),
    headers: { 'x-velion-org-id': orgId },
  })
}

export function updateTicketMacro(orgId: string, macroId: string, input: Partial<TicketMacro>) {
  return requestJson<TicketMacro>(`/api/v1/ticket-macros/${encodeURIComponent(macroId)}`, {
    method: 'PATCH',
    body: JSON.stringify(input),
    headers: { 'x-velion-org-id': orgId },
  })
}

export function runTicketMacro(orgId: string, ticketId: string, macroId: string) {
  return requestJson<TicketMacroRunResult>(
    `/api/v1/tickets/${encodeURIComponent(ticketId)}/macros/${encodeURIComponent(macroId)}/run`,
    {
      method: 'POST',
      headers: { 'x-velion-org-id': orgId },
    },
  )
}

export function listTicketAutomationRules(orgId: string, signal?: AbortSignal) {
  return requestJson<TicketAutomationRule[]>('/api/v1/ticket-automation-rules', {
    headers: { 'x-velion-org-id': orgId },
    signal,
  })
}

export function createTicketAutomationRule(orgId: string, input: Partial<Omit<TicketAutomationRule, 'id' | 'org_id' | 'created_at' | 'updated_at'>>) {
  return requestJson<TicketAutomationRule>('/api/v1/ticket-automation-rules', {
    method: 'POST',
    body: JSON.stringify(input),
    headers: { 'x-velion-org-id': orgId },
  })
}

export function updateTicketAutomationRule(orgId: string, ruleId: string, input: Partial<TicketAutomationRule>) {
  return requestJson<TicketAutomationRule>(`/api/v1/ticket-automation-rules/${encodeURIComponent(ruleId)}`, {
    method: 'PATCH',
    body: JSON.stringify(input),
    headers: { 'x-velion-org-id': orgId },
  })
}

export function listSlaPolicies(orgId: string, signal?: AbortSignal) {
  return requestJson<SlaPolicy[]>('/api/v1/sla-policies', {
    headers: { 'x-velion-org-id': orgId },
    signal,
  })
}

export function createSlaPolicy(orgId: string, input: Partial<Omit<SlaPolicy, 'id' | 'org_id' | 'created_at' | 'updated_at'>>) {
  return requestJson<SlaPolicy>('/api/v1/sla-policies', {
    method: 'POST',
    body: JSON.stringify(input),
    headers: { 'x-velion-org-id': orgId },
  })
}

export function updateSlaPolicy(orgId: string, policyId: string, input: Partial<SlaPolicy>) {
  return requestJson<SlaPolicy>(`/api/v1/sla-policies/${encodeURIComponent(policyId)}`, {
    method: 'PATCH',
    body: JSON.stringify(input),
    headers: { 'x-velion-org-id': orgId },
  })
}

export function createTicketChecklist(
  orgId: string,
  ticketId: string,
  input: { name?: string; template_id?: string; items?: string[] },
) {
  return requestJson<TicketChecklist>(`/api/v1/tickets/${encodeURIComponent(ticketId)}/checklists`, {
    method: 'POST',
    body: JSON.stringify(input),
    headers: { 'x-velion-org-id': orgId },
  })
}

export function updateTicketChecklistItem(
  orgId: string,
  ticketId: string,
  checklistId: string,
  itemId: string,
  input: { completed: boolean },
) {
  return requestJson<TicketChecklist>(
    `/api/v1/tickets/${encodeURIComponent(ticketId)}/checklists/${encodeURIComponent(checklistId)}/items/${encodeURIComponent(itemId)}`,
    {
      method: 'PATCH',
      body: JSON.stringify(input),
      headers: { 'x-velion-org-id': orgId },
    },
  )
}
