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

export type TicketWorkType = 'customer_case' | 'internal_work' | 'incident'

export type SupportTicket = {
  id: string
  org_id: string
  conversation_id: string
  ticket_key: string
  status: TicketStatus | string
  // Optional only while clients may still read a pre-migration response; the
  // canonical service defaults omitted values to customer_case.
  work_type?: TicketWorkType | string
  priority: 'low' | 'normal' | 'high' | 'urgent' | string
  severity: 'low' | 'medium' | 'high' | 'critical' | string
  category?: string
  intent?: string
  assignee_user_id?: string
  assignee_name?: string
  team_id?: string
  team_name?: string
  due_at?: string | null
  follow_up_at?: string | null
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
  side_conversations?: TicketSideConversation[]
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

export type TicketActivity = {
  id: string
  action:
    | 'ticket.created'
    | 'ticket.updated'
    | 'ticket.linked'
    | 'ticket.macro_run'
    | 'ticket.checklist_created'
    | 'ticket.checklist_item_updated'
    | string
  actor_user_id?: string
  resource_kind?: string
  created_at: string
}

/** A score actually recorded from a consented, resolved support case. */
export type TicketCSATOutcome = {
  org_id: string
  ticket_id: string
  conversation_id: string
  score: number
  recorded_by?: string
  recorded_at?: string
}

/** No response rate is exposed until a real delivery system provides a truthful denominator. */
export type CSATScorecard = {
  rated_tickets: number
  positive_ratings: number
  average_score?: number
  positive_rate?: number
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
  linked_ticket?: {
    id: string
    ticket_key: string
    status: string
    work_type: TicketWorkType
  }
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

export type TicketTeam = {
  id: string
  org_id: string
  name: string
  description?: string
  active: boolean
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

export type TicketSideConversation = {
  id: string
  org_id: string
  ticket_id: string
  subject: string
  status: 'open' | 'closed'
  created_by_user_id?: string
  messages: TicketSideConversationMessage[]
  created_at: string
  updated_at: string
}

export type TicketSideConversationMessage = {
  id: string
  org_id: string
  side_conversation_id: string
  body_text: string
  created_by_user_id?: string
  created_at: string
}

// Operational records are intentionally separate from ticket work type. A
// ticket can be linked to an incident, but resolving either entity never
// performs a hidden lifecycle transition on the other.
export type IncidentTicketLink = {
  id: string
  org_id: string
  incident_id: string
  ticket_id: string
  ticket_key?: string
  ticket_status?: string
  relationship: 'affected' | 'root_cause' | 'related' | string
  created_by_user_id?: string
  created_at: string
}

export type SupportIncident = {
  id: string
  org_id: string
  incident_key: string
  title: string
  status: 'declared' | 'investigating' | 'monitoring' | 'resolved' | string
  severity: 'low' | 'medium' | 'high' | 'critical' | string
  owner_user_id?: string
  owner_name?: string
  customer_impact?: string
  problem_id?: string
  declared_by_user_id?: string
  declared_at: string
  resolved_at?: string | null
  ticket_links?: IncidentTicketLink[]
  created_at: string
  updated_at: string
}

export type SupportProblem = {
  id: string
  org_id: string
  problem_key: string
  title: string
  status: 'investigating' | 'known_error' | 'resolved' | string
  owner_user_id?: string
  owner_name?: string
  summary?: string
  root_cause?: string
  created_by_user_id?: string
  resolved_at?: string | null
  created_at: string
  updated_at: string
}

export type CreateIncidentInput = {
  title: string
  status?: SupportIncident['status']
  severity?: SupportIncident['severity']
  owner_user_id?: string
  owner_name?: string
  customer_impact?: string
  problem_id?: string
}

export type UpdateIncidentInput = Partial<CreateIncidentInput>

export type CreateProblemInput = {
  title: string
  status?: SupportProblem['status']
  owner_user_id?: string
  owner_name?: string
  summary?: string
  root_cause?: string
}

export type UpdateProblemInput = Partial<CreateProblemInput>

export type ListTicketsParams = {
  queue?: string
  status?: string
  work_type?: TicketWorkType
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
  work_type?: TicketWorkType
  priority?: string
  severity?: string
  category?: string
  intent?: string
  assignee_user_id?: string
  assignee_name?: string
  team_id?: string
  team_name?: string
  due_at?: string
  follow_up_at?: string
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
  if (params.work_type) query.set('work_type', params.work_type)
  if (params.assigned) query.set('assigned', params.assigned)
  if (params.team) query.set('team', params.team)
  if (params.label) query.set('label', params.label)
  if (params.priority) query.set('priority', params.priority)
  if (params.severity) query.set('severity', params.severity)
  if (params.sla_state) query.set('sla_state', params.sla_state)
  if (params.q) query.set('q', params.q)

  return requestJson<SupportTicket[]>(`/api/v1/tickets?${query}`, {
    headers: { 'x-verevon-org-id': orgId },
    signal,
  })
}

export function createTicket(orgId: string, input: CreateTicketInput) {
  return requestJson<SupportTicket>('/api/v1/tickets', {
    method: 'POST',
    body: JSON.stringify(input),
    headers: { 'x-verevon-org-id': orgId },
  })
}

export function getTicket(orgId: string, ticketId: string, signal?: AbortSignal) {
  return requestJson<SupportTicket>(`/api/v1/tickets/${encodeURIComponent(ticketId)}`, {
    headers: { 'x-verevon-org-id': orgId },
    signal,
  })
}

export function listTicketActivity(orgId: string, ticketId: string, signal?: AbortSignal) {
  return requestJson<TicketActivity[]>(`/api/v1/tickets/${encodeURIComponent(ticketId)}/activity?limit=20`, {
    headers: { 'x-verevon-org-id': orgId },
    signal,
  })
}

export function getTicketCSATOutcome(orgId: string, ticketId: string, signal?: AbortSignal) {
  return requestJson<TicketCSATOutcome>(`/api/v1/tickets/${encodeURIComponent(ticketId)}/csat-outcome`, {
    headers: { 'x-verevon-org-id': orgId }, signal,
  })
}

/** A bounded piece of evidence: a ticket ID the caller is already authorized
 * to see. Deliberately nothing more -- no score, no inferred relationship. */
export type SupportRecurrenceCandidate = { ticket_id: string }

/** Mirrors conversation-core-go's SupportRecurrenceResult -- a
 * "similarity candidate" claim, never a shared-cause or incident claim. */
export type SupportRecurrenceResult = {
  status: 'candidate_found' | 'no_candidate' | 'unavailable'
  candidates: SupportRecurrenceCandidate[]
  algorithm_version?: string
  corpus_window_start?: string
  similarity_threshold?: number
}

export function getSupportRecurrenceCandidates(orgId: string, ticketId: string, signal?: AbortSignal) {
  return requestJson<SupportRecurrenceResult>(
    `/api/v1/tickets/${encodeURIComponent(ticketId)}/support-recurrence-candidates`,
    { headers: { 'x-verevon-org-id': orgId }, signal },
  )
}

export function getCSATScorecard(orgId: string, signal?: AbortSignal) {
  return requestJson<CSATScorecard>('/api/v1/tickets/csat-scorecard', {
    headers: { 'x-verevon-org-id': orgId }, signal,
  })
}

export function listIncidents(orgId: string, signal?: AbortSignal) {
  return requestJson<SupportIncident[]>('/api/v1/incidents', {
    headers: { 'x-verevon-org-id': orgId },
    signal,
  })
}

export function createIncident(orgId: string, input: CreateIncidentInput) {
  return requestJson<SupportIncident>('/api/v1/incidents', {
    method: 'POST', body: JSON.stringify(input), headers: { 'x-verevon-org-id': orgId },
  })
}

export function updateIncident(orgId: string, incidentId: string, input: UpdateIncidentInput) {
  return requestJson<SupportIncident>(`/api/v1/incidents/${encodeURIComponent(incidentId)}`, {
    method: 'PATCH', body: JSON.stringify(input), headers: { 'x-verevon-org-id': orgId },
  })
}

export function linkIncidentTicket(
  orgId: string,
  incidentId: string,
  input: Pick<IncidentTicketLink, 'ticket_id' | 'relationship'>,
) {
  return requestJson<IncidentTicketLink>(`/api/v1/incidents/${encodeURIComponent(incidentId)}/tickets`, {
    method: 'POST', body: JSON.stringify(input), headers: { 'x-verevon-org-id': orgId },
  })
}

export function listProblems(orgId: string, signal?: AbortSignal) {
  return requestJson<SupportProblem[]>('/api/v1/problems', {
    headers: { 'x-verevon-org-id': orgId }, signal,
  })
}

export function createProblem(orgId: string, input: CreateProblemInput) {
  return requestJson<SupportProblem>('/api/v1/problems', {
    method: 'POST', body: JSON.stringify(input), headers: { 'x-verevon-org-id': orgId },
  })
}

export function updateProblem(orgId: string, problemId: string, input: UpdateProblemInput) {
  return requestJson<SupportProblem>(`/api/v1/problems/${encodeURIComponent(problemId)}`, {
    method: 'PATCH', body: JSON.stringify(input), headers: { 'x-verevon-org-id': orgId },
  })
}

export function updateTicket(orgId: string, ticketId: string, input: UpdateTicketInput) {
  return requestJson<SupportTicket>(`/api/v1/tickets/${encodeURIComponent(ticketId)}`, {
    method: 'PATCH',
    body: JSON.stringify(input),
    headers: { 'x-verevon-org-id': orgId },
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
    headers: { 'x-verevon-org-id': orgId },
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
      headers: { 'x-verevon-org-id': orgId },
    },
  )
}

export function listTicketTeams(orgId: string, signal?: AbortSignal) {
  return requestJson<TicketTeam[]>('/api/v1/ticket-teams', {
    headers: { 'x-verevon-org-id': orgId },
    signal,
  })
}

export function createTicketTeam(
  orgId: string,
  input: Pick<TicketTeam, 'name'> & Partial<Pick<TicketTeam, 'description' | 'active'>>,
) {
  return requestJson<TicketTeam>('/api/v1/ticket-teams', {
    method: 'POST',
    body: JSON.stringify(input),
    headers: { 'x-verevon-org-id': orgId },
  })
}

export function updateTicketTeam(
  orgId: string,
  teamId: string,
  input: Partial<Pick<TicketTeam, 'name' | 'description' | 'active'>>,
) {
  return requestJson<TicketTeam>(`/api/v1/ticket-teams/${encodeURIComponent(teamId)}`, {
    method: 'PATCH',
    body: JSON.stringify(input),
    headers: { 'x-verevon-org-id': orgId },
  })
}

export function listTicketViews(orgId: string, signal?: AbortSignal) {
  return requestJson<TicketView[]>('/api/v1/ticket-views', {
    headers: { 'x-verevon-org-id': orgId },
    signal,
  })
}

export function createTicketView(orgId: string, input: Partial<Omit<TicketView, 'id' | 'org_id' | 'created_at' | 'updated_at'>>) {
  return requestJson<TicketView>('/api/v1/ticket-views', {
    method: 'POST',
    body: JSON.stringify(input),
    headers: { 'x-verevon-org-id': orgId },
  })
}

export function updateTicketView(orgId: string, viewId: string, input: Partial<TicketView>) {
  return requestJson<TicketView>(`/api/v1/ticket-views/${encodeURIComponent(viewId)}`, {
    method: 'PATCH',
    body: JSON.stringify(input),
    headers: { 'x-verevon-org-id': orgId },
  })
}

export function listTicketMacros(orgId: string, signal?: AbortSignal) {
  return requestJson<TicketMacro[]>('/api/v1/ticket-macros', {
    headers: { 'x-verevon-org-id': orgId },
    signal,
  })
}

export function createTicketMacro(orgId: string, input: Partial<Omit<TicketMacro, 'id' | 'org_id' | 'created_at' | 'updated_at'>>) {
  return requestJson<TicketMacro>('/api/v1/ticket-macros', {
    method: 'POST',
    body: JSON.stringify(input),
    headers: { 'x-verevon-org-id': orgId },
  })
}

export function updateTicketMacro(orgId: string, macroId: string, input: Partial<TicketMacro>) {
  return requestJson<TicketMacro>(`/api/v1/ticket-macros/${encodeURIComponent(macroId)}`, {
    method: 'PATCH',
    body: JSON.stringify(input),
    headers: { 'x-verevon-org-id': orgId },
  })
}

export function runTicketMacro(orgId: string, ticketId: string, macroId: string) {
  return requestJson<TicketMacroRunResult>(
    `/api/v1/tickets/${encodeURIComponent(ticketId)}/macros/${encodeURIComponent(macroId)}/run`,
    {
      method: 'POST',
      headers: { 'x-verevon-org-id': orgId },
    },
  )
}

export function listTicketAutomationRules(orgId: string, signal?: AbortSignal) {
  return requestJson<TicketAutomationRule[]>('/api/v1/ticket-automation-rules', {
    headers: { 'x-verevon-org-id': orgId },
    signal,
  })
}

export function createTicketAutomationRule(orgId: string, input: Partial<Omit<TicketAutomationRule, 'id' | 'org_id' | 'created_at' | 'updated_at'>>) {
  return requestJson<TicketAutomationRule>('/api/v1/ticket-automation-rules', {
    method: 'POST',
    body: JSON.stringify(input),
    headers: { 'x-verevon-org-id': orgId },
  })
}

export function updateTicketAutomationRule(orgId: string, ruleId: string, input: Partial<TicketAutomationRule>) {
  return requestJson<TicketAutomationRule>(`/api/v1/ticket-automation-rules/${encodeURIComponent(ruleId)}`, {
    method: 'PATCH',
    body: JSON.stringify(input),
    headers: { 'x-verevon-org-id': orgId },
  })
}

export function listSlaPolicies(orgId: string, signal?: AbortSignal) {
  return requestJson<SlaPolicy[]>('/api/v1/sla-policies', {
    headers: { 'x-verevon-org-id': orgId },
    signal,
  })
}

export function createSlaPolicy(orgId: string, input: Partial<Omit<SlaPolicy, 'id' | 'org_id' | 'created_at' | 'updated_at'>>) {
  return requestJson<SlaPolicy>('/api/v1/sla-policies', {
    method: 'POST',
    body: JSON.stringify(input),
    headers: { 'x-verevon-org-id': orgId },
  })
}

export function updateSlaPolicy(orgId: string, policyId: string, input: Partial<SlaPolicy>) {
  return requestJson<SlaPolicy>(`/api/v1/sla-policies/${encodeURIComponent(policyId)}`, {
    method: 'PATCH',
    body: JSON.stringify(input),
    headers: { 'x-verevon-org-id': orgId },
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
    headers: { 'x-verevon-org-id': orgId },
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
      headers: { 'x-verevon-org-id': orgId },
    },
  )
}
