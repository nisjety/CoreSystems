import { requestJson } from './http'
import type { Group, ZammadArticle, ZammadTicket } from '@/features/inbox/lib/inbox-model'

// ── conversation-core-go wire types ───────────────────────────────────────────

export interface ConversationSummary {
  id: string
  org_id: string
  inbox_id: string
  title: string
  status: string
  priority: string
  channel: string
  provider?: string
  assignee_user_id?: string
  assignee_name?: string
  last_message_preview?: string
  last_message_at?: string
  contact?: { id?: string; name?: string; email?: string }
  tags?: string[]
  created_at: string
  updated_at: string
}

export interface ConversationMessage {
  id: string
  conversation_id: string
  direction: string
  sender_type: string
  sender_name?: string
  sender_email?: string
  body_text: string
  body_html?: string
  internal: boolean
  /** Present only after conversation-core has persisted the channel's accepted
   * outbound submission. This is an acceptance receipt, not a delivery/read
   * receipt from the customer. */
  provider?: string
  provider_message_id?: string
	attachments?: ConversationAttachment[]
  occurred_at: string
  created_at: string
}

/** Attachment metadata is intentionally display-only. The API never exposes
 * provider or storage references to the browser without a separately scoped
 * download contract. */
export interface ConversationAttachment {
  id: string
  filename: string
  mime_type?: string
  size_bytes: number
}

export interface ConversationDetail extends ConversationSummary {
  messages: ConversationMessage[]
}

export type DraftLease = { org_id: string; conversation_id: string; user_id: string; expires_at: string; updated_at: string }

/** A content-free, personal preference. It does not claim notification delivery. */
export type ConversationFollow = { org_id: string; conversation_id: string; user_id: string; created_at: string }
export type ConversationCSATPreference = { org_id: string; conversation_id: string; contact_id: string; opted_in: boolean; updated_by?: string; updated_at?: string }

/** Content-free provider submission/outcome receipt. `submitted` means a
 * provider accepted a submission; it is never customer delivery or read proof. */
export type OutboundIntent = {
  id: string
  conversation_id: string
  status: 'sending' | 'retryable' | 'submitted' | 'failed' | 'unknown'
  provider?: string
  provider_message_id?: string
  /** Later provider callback evidence. It is distinct from `submitted`, which
   * only means the provider accepted the outbound request. */
  delivery_status?: 'unconfirmed' | 'delivered' | 'read' | 'failed'
  delivery_occurred_at?: string
  delivery_error_code?: string
  error_code?: string
  created_at: string
  updated_at: string
}

export type OrganizationOutboundIntentFilter = {
  status?: OutboundIntent['status']
  provider?: string
  deliveryStatus?: NonNullable<OutboundIntent['delivery_status']>
  limit?: number
}

/** A private, operator-owned recovery record for unsent Inbox text. This is
 * intentionally distinct from shared notes and reviewable AI proposals. */
export type ConversationDraft = {
  org_id: string
  conversation_id: string
  user_id: string
  body_text: string
  internal: boolean
  updated_at: string
}

/** A payload-redacted, canonical work event for the selected conversation.
 * Customer messages and arbitrary audit metadata remain in their respective
 * protected reads; this compact timeline is only lifecycle visibility. */
export type ConversationActivity = {
  id: string
  action:
    | 'conversation.created'
    | 'message.received'
    | 'message.sent'
    | 'message.submitted'
    | 'note.created'
    | 'outbound.delivery_recorded'
    | 'status.changed'
    | 'assignment.changed'
    | 'tag.added'
    | 'tag.removed'
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

/** The UI model keys tickets by a numeric id, but conversation-core uses string
 * ids. We carry the real `conversationId` for API calls and derive a stable
 * numeric surrogate for UI identity/selection. `assigneeUserId` is the raw
 * conversation-core assignee (empty when unassigned) so the queue filters can
 * tell "assigned to me" apart from "assigned to another agent" without relying
 * on the derived numeric owner id. */
export type LiveTicket = ZammadTicket & { conversationId: string; assigneeUserId?: string }

export interface InboxConversationsResult {
  tickets: LiveTicket[]
  total: number
  nextCursor: { updated: string; id: string } | null
}

export interface ConversationListParams {
  limit?: number
  state?: string
  assigned?: string
  channel?: string
  connectionId?: string
  cursorUpdated?: string
  cursorId?: string
}

function orgHeaders(orgId: string): Record<string, string> {
  return { 'x-verevon-org-id': orgId }
}

/** Deterministic string → positive int, so the same conversation/message always
 * maps to the same numeric UI id within and across loads. */
function hashToInt(value: string): number {
  let hash = 0
  for (let index = 0; index < value.length; index += 1) {
    hash = (Math.imul(31, hash) + value.charCodeAt(index)) | 0
  }
  return Math.abs(hash) || 1
}

function stateFromStatus(status: string): { id: number; name: string } {
  switch (status) {
    case 'pending':
      return { id: 6, name: 'pending' }
    case 'solved':
    case 'closed':
      return { id: 4, name: 'closed' }
    default:
      return { id: 2, name: 'open' }
  }
}

export function statusFromStateId(id: number): string {
  switch (id) {
    case 4:
      return 'solved'
    case 6:
      return 'pending'
    default:
      return 'open'
  }
}

function priorityFromValue(priority: string): { id: number; name: string } {
  switch (priority) {
    case 'high':
      return { id: 3, name: 'high' }
    case 'low':
      return { id: 1, name: 'low' }
    default:
      return { id: 2, name: 'normal' }
  }
}

function agentStateFromConversation(conversation: ConversationSummary): string {
  if (conversation.status === 'solved' || conversation.status === 'closed') return 'resolved'
  if (conversation.assignee_user_id) return 'routed'
  return 'all'
}

export function toLiveTicket(conversation: ConversationSummary | ConversationDetail): LiveTicket {
  const [firstname, ...rest] = (conversation.contact?.name || conversation.contact?.email || 'Unknown').split(' ')
  return {
    id: hashToInt(conversation.id),
    conversationId: conversation.id,
    assigneeUserId: conversation.assignee_user_id || undefined,
    number: conversation.id.replace(/^conv_/, '').slice(0, 12),
    title: conversation.title,
    state: stateFromStatus(conversation.status),
    priority: priorityFromValue(conversation.priority),
    group: { id: hashToInt(conversation.inbox_id || 'inbox'), name: conversation.channel || 'Inbox' },
    owner: conversation.assignee_user_id
      ? {
        id: hashToInt(conversation.assignee_user_id),
        firstname: conversation.assignee_name?.split(' ')[0] || 'Assigned',
        lastname: conversation.assignee_name?.split(' ').slice(1).join(' ') || 'Agent',
        email: '',
      }
      : null,
    customer: {
      id: hashToInt(conversation.contact?.id || conversation.contact?.email || 'unknown'),
      firstname: firstname || 'Unknown',
      lastname: rest.join(' '),
      email: conversation.contact?.email || '',
    },
    tags: conversation.tags ?? [],
    created_at: conversation.created_at,
    updated_at: conversation.updated_at,
    article_count: 'messages' in conversation ? conversation.messages.length : undefined,
    channel: conversation.channel,
    provider: conversation.provider,
    lastMessagePreview: conversation.last_message_preview,
    agentState: agentStateFromConversation(conversation),
  }
}

export function toArticle(message: ConversationMessage): ZammadArticle {
  const agentMessage = message.sender_type === 'agent' || message.direction === 'outbound'
  return {
    id: hashToInt(message.id),
    ticket_id: hashToInt(message.conversation_id),
    type: message.internal ? 'note' : 'email',
    internal: message.internal,
    body: message.body_html || message.body_text,
    bodyHtml: message.body_html,
    bodyText: message.body_text,
    from: message.sender_name || message.sender_email || (agentMessage ? 'Verevon Support' : 'Customer'),
    fromEmail: message.sender_email,
    sender: agentMessage ? 'Agent' : 'Customer',
    provider: message.provider,
    providerMessageId: message.provider_message_id,
		attachments: message.attachments?.map((attachment) => ({
			id: attachment.id,
			filename: attachment.filename,
			mimeType: attachment.mime_type,
			sizeBytes: attachment.size_bytes,
		})),
    created_at: message.occurred_at || message.created_at,
  }
}

// ── API ───────────────────────────────────────────────────────────────────────

export async function listConversations(
  orgId: string,
  params: ConversationListParams = {},
  signal?: AbortSignal,
): Promise<InboxConversationsResult> {
  const query = new URLSearchParams()
  query.set('limit', String(params.limit ?? 50))
  if (params.state) query.set('state', params.state)
  if (params.assigned) query.set('assigned', params.assigned)
  if (params.channel && params.channel !== 'all') query.set('channel', params.channel)
  if (params.connectionId) query.set('connection_id', params.connectionId)
  if (params.cursorUpdated) query.set('cursor_updated', params.cursorUpdated)
  if (params.cursorId) query.set('cursor_id', params.cursorId)

  const data = await requestJson<ConversationSummary[]>(`/api/v1/inbox/conversations?${query}`, {
    headers: orgHeaders(orgId),
    signal,
  })
  const list = Array.isArray(data) ? data : []
  const last = list.at(-1)
  const nextCursor = last && list.length === (params.limit ?? 50)
    ? { updated: last.updated_at, id: last.id }
    : null
  return { tickets: list.map(toLiveTicket), total: list.length, nextCursor }
}

export async function getConversationDetail(
  orgId: string,
  conversationId: string,
  signal?: AbortSignal,
): Promise<{ ticket: LiveTicket; articles: ZammadArticle[] }> {
  const detail = await requestJson<ConversationDetail>(
    `/api/v1/inbox/conversations/${encodeURIComponent(conversationId)}`,
    { headers: orgHeaders(orgId), signal },
  )
  return {
    ticket: toLiveTicket(detail),
    articles: Array.isArray(detail.messages) ? detail.messages.map(toArticle) : [],
  }
}

export function getDraftLease(orgId: string, conversationId: string): Promise<DraftLease> {
  return requestJson(`/api/v1/inbox/conversations/${encodeURIComponent(conversationId)}/draft-lease`, { headers: orgHeaders(orgId) })
}

export async function listConversationActivity(orgId: string, conversationId: string): Promise<ConversationActivity[]> {
  const activity = await requestJson<ConversationActivity[]>(
    `/api/v1/inbox/conversations/${encodeURIComponent(conversationId)}/activity?limit=20`,
    { headers: orgHeaders(orgId) },
  )
  return Array.isArray(activity) ? activity : []
}

export function claimDraftLease(orgId: string, conversationId: string): Promise<DraftLease> {
  return requestJson(`/api/v1/inbox/conversations/${encodeURIComponent(conversationId)}/draft-lease`, { method: 'POST', headers: orgHeaders(orgId) })
}

export function releaseDraftLease(orgId: string, conversationId: string): Promise<void> {
  return requestJson(`/api/v1/inbox/conversations/${encodeURIComponent(conversationId)}/draft-lease`, { method: 'DELETE', headers: orgHeaders(orgId) })
}

export function getConversationFollow(orgId: string, conversationId: string): Promise<ConversationFollow> {
  return requestJson(`/api/v1/inbox/conversations/${encodeURIComponent(conversationId)}/follow`, { headers: orgHeaders(orgId) })
}

export function getConversationCSATPreference(orgId: string, conversationId: string): Promise<ConversationCSATPreference> {
  return requestJson(`/api/v1/inbox/conversations/${encodeURIComponent(conversationId)}/csat-preference`, { headers: orgHeaders(orgId) })
}

export function getConversationDraft(orgId: string, conversationId: string): Promise<ConversationDraft> {
  return requestJson(`/api/v1/inbox/conversations/${encodeURIComponent(conversationId)}/draft`, { headers: orgHeaders(orgId) })
}

export function saveConversationDraft(
  orgId: string,
  conversationId: string,
  bodyText: string,
  internal: boolean,
): Promise<ConversationDraft> {
  return requestJson(`/api/v1/inbox/conversations/${encodeURIComponent(conversationId)}/draft`, {
    method: 'PUT',
    body: JSON.stringify({ body_text: bodyText, internal }),
    headers: orgHeaders(orgId),
  })
}

export function deleteConversationDraft(orgId: string, conversationId: string): Promise<void> {
  return requestJson(`/api/v1/inbox/conversations/${encodeURIComponent(conversationId)}/draft`, { method: 'DELETE', headers: orgHeaders(orgId) })
}

export function listOutboundIntents(orgId: string, conversationId: string): Promise<OutboundIntent[]> {
  return requestJson(`/api/v1/inbox/conversations/${encodeURIComponent(conversationId)}/outbound-intents`, { headers: orgHeaders(orgId) })
}

/** Organization-scoped, content-free reconciliation ledger. A `submitted`
 * intent records provider acceptance only; it never implies delivery or read. */
export function listOrganizationOutboundIntents(
  orgId: string,
  filter: OrganizationOutboundIntentFilter = {},
): Promise<OutboundIntent[]> {
  const query = new URLSearchParams()
  if (filter.status) query.set('status', filter.status)
  if (filter.provider) query.set('provider', filter.provider)
  if (filter.deliveryStatus) query.set('delivery_status', filter.deliveryStatus)
  if (filter.limit) query.set('limit', String(filter.limit))
  const suffix = query.size > 0 ? `?${query}` : ''
  return requestJson(`/api/v1/inbox/outbound-intents${suffix}`, { headers: orgHeaders(orgId) })
}

export async function sendReply(
  orgId: string,
  conversationId: string,
  body: string,
  internal: boolean,
  idempotencyKey: string,
): Promise<ZammadArticle> {
  const path = internal ? 'notes' : 'messages'
  const message = await requestJson<ConversationMessage>(
    `/api/v1/inbox/conversations/${encodeURIComponent(conversationId)}/${path}`,
    {
      method: 'POST',
      body: JSON.stringify({ body_text: body, internal, idempotency_key: idempotencyKey }),
      headers: orgHeaders(orgId),
    },
  )
  return toArticle(message)
}

export async function setConversationStatus(
  orgId: string,
  conversationId: string,
  status: string,
): Promise<LiveTicket> {
  const detail = await requestJson<ConversationDetail>(
    `/api/v1/inbox/conversations/${encodeURIComponent(conversationId)}/status`,
    { method: 'PATCH', body: JSON.stringify({ status }), headers: orgHeaders(orgId) },
  )
  return toLiveTicket(detail)
}

export async function setConversationAssignment(
  orgId: string,
  conversationId: string,
  assigneeUserId: string,
  assigneeName: string,
): Promise<LiveTicket> {
  const detail = await requestJson<ConversationDetail>(
    `/api/v1/inbox/conversations/${encodeURIComponent(conversationId)}/assignment`,
    {
      method: 'PATCH',
      body: JSON.stringify({ assignee_user_id: assigneeUserId, assignee_name: assigneeName }),
      headers: orgHeaders(orgId),
    },
  )
  return toLiveTicket(detail)
}

export async function addConversationTag(orgId: string, conversationId: string, tag: string): Promise<void> {
  await requestJson(`/api/v1/inbox/conversations/${encodeURIComponent(conversationId)}/tags`, {
    method: 'POST',
    body: JSON.stringify({ tag }),
    headers: orgHeaders(orgId),
  })
}

export async function removeConversationTag(orgId: string, conversationId: string, tag: string): Promise<void> {
  await requestJson(
    `/api/v1/inbox/conversations/${encodeURIComponent(conversationId)}/tags/${encodeURIComponent(tag)}`,
    { method: 'DELETE', headers: orgHeaders(orgId) },
  )
}

// ── AI-action HITL review queue ───────────────────────────────────────────────
//
// Model-proposed actions on a conversation awaiting a human decision. The org is
// resolved from the authenticated session at the gateway — these calls do NOT
// send any client org header (a forged one would be stripped at ingress anyway).

export interface AiAction {
  id: string
  org_id: string
  conversation_id: string
  proposal_group_id?: string
  kind: string
  status: string
  payload: Record<string, unknown>
  created_by: string
  reviewed_by?: string
  reviewed_at?: string
  created_at: string
  updated_at: string
}

/** Queue a generated reply for the durable, human-in-the-loop review surface.
 * This intentionally has no status/actor/org fields: the gateway derives them
 * from the authenticated session and admits only `draft.reply`. */
export async function createDraftReplyProposal(input: {
  conversationId: string
  bodyText: string
  proposalGroupId?: string
}): Promise<AiAction> {
  return requestJson<AiAction>('/api/v1/inbox/ai-actions', {
    method: 'POST',
    body: JSON.stringify({
      conversation_id: input.conversationId,
      body_text: input.bodyText,
      proposal_group_id: input.proposalGroupId,
    }),
  })
}

export async function createInternalNoteProposal(input: { conversationId: string; bodyText: string; proposalGroupId?: string }): Promise<AiAction> {
  return requestJson<AiAction>('/api/v1/inbox/ai-actions', {
    method: 'POST',
    body: JSON.stringify({ conversation_id: input.conversationId, body_text: input.bodyText, proposal_group_id: input.proposalGroupId, kind: 'internal.note' }),
  })
}

/** Queue a bounded AI suggestion for an existing Ticketing record. Active-work
 * status and a paired canonical team may be proposed; resolution, closure,
 * snooze, and ownership remain explicit human decisions. */
export async function createTicketUpdateProposal(input: {
  conversationId: string
  ticketId: string
  confidence: number
  reason: string
  evidenceMessageIds: string[]
  proposalGroupId?: string
  suggestedFields: Pick<AiActionFieldEdits, 'category' | 'intent' | 'work_type' | 'priority' | 'severity' | 'status' | 'team_id' | 'team_name'>
}): Promise<AiAction> {
  return requestJson<AiAction>('/api/v1/inbox/ai-actions', {
    method: 'POST',
    body: JSON.stringify({
      conversation_id: input.conversationId,
      proposal_group_id: input.proposalGroupId,
      ticket_id: input.ticketId,
      confidence: input.confidence,
      reason: input.reason,
      evidence_message_ids: input.evidenceMessageIds,
      suggested_fields: input.suggestedFields,
      kind: 'ticket.update',
    }),
  })
}

/** Queue a strictly review-gated incident declaration linked to an existing
 * ticket. This cannot set incident status, ownership, or mutate the ticket;
 * the approved core action creates only the incident and its affected-ticket
 * relationship. */
export async function createIncidentProposal(input: {
  conversationId: string
  ticketId: string
  title: string
  severity: 'low' | 'medium' | 'high' | 'critical'
  customerImpact: string
  confidence: number
  reason: string
  evidenceMessageIds: string[]
  proposalGroupId?: string
}): Promise<AiAction> {
  return requestJson<AiAction>('/api/v1/inbox/ai-actions', {
    method: 'POST',
    body: JSON.stringify({
      conversation_id: input.conversationId,
      proposal_group_id: input.proposalGroupId,
      ticket_id: input.ticketId,
      title: input.title,
      severity: input.severity,
      customer_impact: input.customerImpact,
      confidence: input.confidence,
      reason: input.reason,
      evidence_message_ids: input.evidenceMessageIds,
      kind: 'incident.create',
    }),
  })
}

/** Queue a proposed root-cause record. It cannot name an Incident, owner,
 * lifecycle state, or propagation rule: the approving reviewer creates only
 * an independent investigating Problem that can be associated later. */
export async function createProblemProposal(input: {
  conversationId: string
  title: string
  summary: string
  rootCause?: string
  confidence: number
  reason: string
  evidenceMessageIds: string[]
  proposalGroupId?: string
}): Promise<AiAction> {
  return requestJson<AiAction>('/api/v1/inbox/ai-actions', {
    method: 'POST',
    body: JSON.stringify({
      conversation_id: input.conversationId,
      proposal_group_id: input.proposalGroupId,
      title: input.title,
      summary: input.summary,
      root_cause: input.rootCause,
      confidence: input.confidence,
      reason: input.reason,
      evidence_message_ids: input.evidenceMessageIds,
      kind: 'problem.create',
    }),
  })
}

export async function listAiActions(
  params: { status?: string; conversationId?: string; limit?: number } = {},
  signal?: AbortSignal,
): Promise<AiAction[]> {
  const query = new URLSearchParams()
  // conversation-core deliberately defaults an omitted status to the legacy
  // `suggested` queue. Ask for its explicit `all` sentinel when this panel
  // needs both reviewable `suggest_ticket` records and durable execution
  // receipts from the same conversation ledger.
  if (params.status) query.set('status', params.status)
  if (params.conversationId) query.set('conversation_id', params.conversationId)
  query.set('limit', String(params.limit ?? 50))
  const data = await requestJson<AiAction[]>(`/api/v1/inbox/ai-actions?${query}`, { signal })
  return Array.isArray(data) ? data : []
}

/** The bounded values a reviewer may edit before approval. Ticket fields alter
 * the suggested ticket payload; body_text replaces the exact draft.reply or
 * internal.note payload that the executor will later use. Only non-empty
 * strings are sent. */
export interface AiActionFieldEdits {
  /** Exact human-reviewed copy for draft.reply and internal.note proposals. */
  body_text?: string
  category?: string
  intent?: string
  work_type?: 'customer_case' | 'internal_work' | 'incident'
  /** Reviewable active-work states only; terminal lifecycle states stay manual. */
  status?: 'open' | 'waiting_customer' | 'waiting_team' | 'escalated'
  priority?: string
  severity?: string
  team_id?: string
  team_name?: string
  /** Exact reviewer-approved fields for a proposed incident declaration. */
  title?: string
  customer_impact?: string
  /** Exact reviewer-approved values for a proposed root-cause Problem. */
  summary?: string
  root_cause?: string
}

export interface ReviewAiActionOptions {
  comment?: string
  /** Reviewer-edited payload values, sent only alongside an `approve` decision.
   * The executor applies these instead of the AI's original suggestion when
   * present — see conversation-core-go's ReviewAIAction. */
  editedFields?: AiActionFieldEdits
}

/** Record a human review decision on a model-proposed action. The gateway forces
 * the decision from the route (`approve` → approved, `reject` → rejected); the
 * optional comment and (on approve) edited fields are the only body the reviewer
 * supplies. Resolves only on a 2xx — a missing/foreign-org action id surfaces as
 * a thrown 404. */
export async function reviewAiAction(
  aiActionId: string,
  decision: 'approve' | 'reject',
  options: ReviewAiActionOptions = {},
): Promise<void> {
  const body: { comment?: string; edited_fields?: AiActionFieldEdits } = {}
  if (options.comment && options.comment.trim()) {
    body.comment = options.comment.trim()
  }
  if (decision === 'approve' && options.editedFields) {
    const cleaned = Object.fromEntries(
      Object.entries(options.editedFields).filter(
        ([, value]) => typeof value === 'string' && value.trim() !== '',
      ),
    ) as AiActionFieldEdits
    if (Object.keys(cleaned).length > 0) {
      body.edited_fields = cleaned
    }
  }
  await requestJson(`/api/v1/inbox/ai-actions/${encodeURIComponent(aiActionId)}/${decision}`, {
    method: 'POST',
    body: JSON.stringify(body),
  })
}

// ── pilot feedback ────────────────────────────────────────────────────────────
//
// The shell's persistent "Send feedback" control. Lands as a new conversation
// in the org's own Inbox, tagged "pilot-feedback" by conversation-core-go's
// Service.SubmitFeedback -- no separate feedback store. SubmitFeedback also
// mirrors the submission into the team's monitored org so feedback from an
// external pilot org's own isolated tenant stays visible to the team.

export interface SubmitFeedbackInput {
  bodyText: string
  fromName?: string
  fromEmail?: string
  // The route the submitter was on when they opened the widget (e.g.
  // "/inbox?view=mine"). Best-effort context only -- see
  // conversation-core-go's FeedbackInput.PageURL.
  pageUrl?: string
}

export async function submitFeedback(orgId: string, input: SubmitFeedbackInput): Promise<void> {
  await requestJson('/api/v1/inbox/feedback', {
    method: 'POST',
    // A fresh key per call: this is a new ticket every submission, not a
    // thread to append to -- see conversation-core-go's Service.SubmitFeedback,
    // which requires one (no provider event/message id to derive a fallback).
    body: JSON.stringify({
      body_text: input.bodyText,
      from_name: input.fromName,
      from_email: input.fromEmail,
      page_url: input.pageUrl,
      idempotency_key: crypto.randomUUID(),
    }),
    headers: orgHeaders(orgId),
  })
}

export async function listInboxesAsGroups(orgId: string, signal?: AbortSignal): Promise<Group[]> {
  const inboxes = await requestJson<Array<{ id: string; name: string }>>('/api/v1/inbox/inboxes', {
    headers: orgHeaders(orgId),
    signal,
  })
  const list = Array.isArray(inboxes) ? inboxes : []
  return list.map((inbox, index) => ({ id: index + 1, name: inbox.name }))
}
