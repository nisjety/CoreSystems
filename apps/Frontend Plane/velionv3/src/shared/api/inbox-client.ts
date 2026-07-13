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
  occurred_at: string
  created_at: string
}

export interface ConversationDetail extends ConversationSummary {
  messages: ConversationMessage[]
}

/** The UI model keys tickets by a numeric id, but conversation-core uses string
 * ids. We carry the real `conversationId` for API calls and derive a stable
 * numeric surrogate for UI identity/selection. */
export type LiveTicket = ZammadTicket & { conversationId: string }

export interface InboxConversationsResult {
  tickets: LiveTicket[]
  total: number
}

export interface ConversationListParams {
  limit?: number
  state?: string
  assigned?: string
  channel?: string
}

function orgHeaders(orgId: string): Record<string, string> {
  return { 'x-velion-org-id': orgId }
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
    from: message.sender_name || message.sender_email || (agentMessage ? 'Velion Support' : 'Customer'),
    sender: agentMessage ? 'Agent' : 'Customer',
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

  const data = await requestJson<ConversationSummary[]>(`/api/v1/inbox/conversations?${query}`, {
    headers: orgHeaders(orgId),
    signal,
  })
  const list = Array.isArray(data) ? data : []
  return { tickets: list.map(toLiveTicket), total: list.length }
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
  kind: string
  status: string
  payload: Record<string, unknown>
  created_by: string
  reviewed_by?: string
  reviewed_at?: string
  created_at: string
  updated_at: string
}

export async function listAiActions(
  params: { status?: string; conversationId?: string; limit?: number } = {},
  signal?: AbortSignal,
): Promise<AiAction[]> {
  const query = new URLSearchParams()
  if (params.status) query.set('status', params.status)
  if (params.conversationId) query.set('conversation_id', params.conversationId)
  query.set('limit', String(params.limit ?? 50))
  const data = await requestJson<AiAction[]>(`/api/v1/inbox/ai-actions?${query}`, { signal })
  return Array.isArray(data) ? data : []
}

/** Record a human review decision on a model-proposed action. The gateway forces
 * the decision from the route (`approve` → approved, `reject` → rejected); the
 * optional comment is the only body the reviewer supplies. Resolves only on a 2xx
 * — a missing/foreign-org action id surfaces as a thrown 404. */
export async function reviewAiAction(
  aiActionId: string,
  decision: 'approve' | 'reject',
  comment?: string,
): Promise<void> {
  await requestJson(`/api/v1/inbox/ai-actions/${encodeURIComponent(aiActionId)}/${decision}`, {
    method: 'POST',
    body: JSON.stringify(comment && comment.trim() ? { comment: comment.trim() } : {}),
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
