export type InboxTab = 'all' | 'open' | 'pending' | 'solved'

// FEEDBACK_TAG mirrors conversation-core-go's conversation.FeedbackTag --
// applied to every conversation created by the "Send feedback" widget
// (submitter's own org) and to its mirrored copy in the team's monitored org
// (see Service.mirrorFeedback). Drives the default "Feedback" saved view
// below so mirrored pilot feedback surfaces automatically, without anyone
// needing to know to filter for it manually.
export const FEEDBACK_TAG = 'pilot-feedback'

export type InboxSidebarView =
  | 'mine'
  | 'created-by-you'
  | 'all'
  | 'unassigned'
  | 'spam'
  | 'dashboard'
  | 'ai-all'
  | 'ai-resolved'
  | 'ai-routed'
  | 'ai-abandoned'
  | 'team-admin-support'
  | 'view-messenger'
  | 'view-email'
  | 'view-social'
  | 'view-feedback'
  | 'manage'

export type SupportTicketReference = {
  id: string
  ticket_key: string
  status: string
  follow_up_at?: string | null
  due_at?: string | null
  sla_state?: 'ok' | 'risk' | 'breached' | string
  source?: string
  ai_confidence?: number
  ai_reason?: string
}

export type ZammadTicket = {
	conversationId?: string
	id: number
  number: string
  title: string
  state?: { id: number; name: string }
  priority?: { id: number; name: string }
  group?: { id: number; name: string }
  owner?: { id: number; firstname: string; lastname: string; email: string } | null
  customer?: { id: number; firstname: string; lastname: string; email: string } | null
  tags?: string[]
  created_at: string
  updated_at: string
  article_count?: number
  channel?: string
  provider?: string
  lastMessagePreview?: string
  agentState?: string
  supportTicket?: SupportTicketReference | null
}

export type ZammadArticle = {
  id: number
  ticket_id?: number
  type?: string
  internal?: boolean
  body?: string
  bodyHtml?: string
  bodyText?: string
  from?: string
  fromEmail?: string
  sender?: string
  /** Channel submission receipt; never interpreted as customer delivery/read. */
  provider?: string
  providerMessageId?: string
	/** Safe attachment metadata only; downloads require a separate authority. */
	attachments?: InboxAttachment[]
  created_at: string
}

export type InboxAttachment = {
  id: string
  filename: string
  mimeType?: string
  sizeBytes: number
}

export type Agent = {
  id: number
  firstname: string
  lastname: string
  email: string
}

export type Group = {
  id: number
  name: string
}

export type Macro = {
  id: number
  name: string
}

export type CustomerContext = {
  shopify?: {
    orders?: Array<{
      id: string | number
      name?: string
      order_number?: string | number
      fulfillment_status?: string | null
      created_at?: string
      total_price?: string
    }>
  } | null
  stripe?: {
    customer?: {
      id?: string
      email?: string
    } | null
    subscription?: {
      status?: string
      plan?: string
    } | null
  } | null
}

export type TicketSentiment = {
  sentiment: string
  score: number
}

export type CalendarEvent = {
  id: string
  title: string
  start: string
  end: string
  type: string
  status: string
  createdAt: string
}

export type CalendarNote = {
  id: string
  text: string
  date: string
  createdAt: string
}

export type InboxRouteFilter = {
  activeTab: InboxTab
  assigned?: 'mine' | 'unassigned' | 'all'
  channel?: string
  connectionId?: string
  queue?: string
  agentState?: string
  label: string
}

export function resolveInboxRouteFilter(searchParams: URLSearchParams): InboxRouteFilter {
  const view = (searchParams.get('view') || 'mine') as InboxSidebarView
  const channel = searchParams.get('channel') || undefined
  const connectionId = searchParams.get('connection_id')?.trim() || undefined
  const requestedStatus = searchParams.get('status')
  const status: InboxTab | undefined = requestedStatus === 'all' || requestedStatus === 'open' || requestedStatus === 'pending' || requestedStatus === 'solved'
    ? requestedStatus
    : undefined

  if (channel) {
    // Channel lanes show every conversation in the channel, not only those
    // assigned to the current user. Incoming provider messages (email, Slack,
    // Teams, WhatsApp, …) arrive UNASSIGNED, so filtering to 'mine' here hid
    // the entire shared queue — a fresh workspace saw an empty inbox on every
    // lane despite real messages. This matches the equivalent 'view-*' channel
    // views below, which already use assigned: 'all'. Personal scoping lives in
    // the dedicated 'Uten eier' (unassigned) / assignment filters.
    return {
      activeTab: status ?? (channel === 'all' ? 'all' : 'open'),
      assigned: 'all',
      channel,
      connectionId,
      label: channel === 'all' ? 'All messages' : capitalize(channel.replace(/-/g, ' ')),
    }
  }

  switch (view) {
    case 'created-by-you':
      return { activeTab: 'all', queue: 'created-by-you', label: 'Created by you' }
    case 'all':
      return { activeTab: status ?? 'all', assigned: 'all', label: 'All conversations' }
    case 'unassigned':
      return { activeTab: 'open', assigned: 'unassigned', label: 'Unassigned' }
    case 'spam':
      return { activeTab: 'all', queue: 'spam', label: 'Spam' }
    case 'dashboard':
      return { activeTab: 'all', queue: 'dashboard', label: 'Dashboard' }
    case 'ai-all':
      return { activeTab: 'all', agentState: 'all', label: 'Agent conversations' }
    case 'ai-resolved':
      return { activeTab: 'solved', agentState: 'resolved', label: 'Solved' }
    case 'ai-routed':
      return { activeTab: 'all', agentState: 'routed', label: 'Routed' }
    case 'ai-abandoned':
      return { activeTab: 'all', agentState: 'abandoned', label: 'Abandoned' }
    case 'team-admin-support':
      return { activeTab: 'open', queue: 'Admin Support', label: 'Admin Support' }
    case 'view-feedback':
      // Default saved view for mirrored + own-org pilot feedback (see
      // FEEDBACK_TAG above). Shows every status, like Mentions -- a feedback
      // note is not itself "open/pending/solved" support work.
      return { activeTab: 'all', assigned: 'all', queue: 'feedback', label: 'Feedback' }
    case 'view-messenger':
      return { activeTab: 'open', assigned: 'all', channel: 'messenger', label: 'Messenger' }
    case 'view-email':
      return { activeTab: 'open', assigned: 'all', channel: 'email', label: 'Email' }
    case 'view-social':
      return { activeTab: 'open', assigned: 'all', channel: 'whatsapp', label: 'WhatsApp & Social' }
    case 'manage':
      return { activeTab: 'all', queue: 'manage', label: 'Manage' }
    case 'mine':
    default:
      // "Your inbox" is the actionable queue: conversations assigned to the
      // current agent PLUS unassigned incoming ones anyone can pick up.
      // InboxPage's filteredTickets treats assigned:'mine' as "not claimed by
      // someone else" (an unassigned ticket has no owner, so it passes) --
      // only a conversation a *different* agent already owns is hidden.
      return { activeTab: 'all', assigned: 'mine', channel, connectionId, label: 'Your inbox' }
  }
}

export function searchParamsFromInboxSlug(slug: string[] = []) {
  const [section, value] = slug
  const params = new URLSearchParams()

  if (!section) {
    params.set('view', 'mine')
    return params
  }

  if (section === 'channels') {
    params.set('view', 'mine')
    if (value) params.set('channel', value)
    return params
  }

  // Previous UI releases exposed "mentions" by guessing from ticket title
  // and tags. That is not a user-scoped mention signal, so legacy URLs land
  // in the actionable personal queue until a canonical mention read model exists.
  if (section === 'mentions') {
    params.set('view', 'mine')
    return params
  }

  if (section === 'ai') {
    const aiViewByValue: Record<string, InboxSidebarView> = {
      all: 'ai-all',
      resolved: 'ai-resolved',
      solved: 'ai-resolved',
      routed: 'ai-routed',
      forwarded: 'ai-routed',
      abandoned: 'ai-abandoned',
      cancelled: 'ai-abandoned',
    }
    params.set('view', aiViewByValue[value ?? 'all'] ?? 'ai-all')
    return params
  }

  if (section === 'teams') {
    params.set('view', 'team-admin-support')
    return params
  }

  if (section === 'views') {
    if (value === 'email') params.set('view', 'view-email')
    else if (value === 'social' || value === 'whatsapp-social') params.set('view', 'view-social')
    else params.set('view', 'view-messenger')
    return params
  }

  params.set('view', section)
  return params
}

export function customerName(ticket: ZammadTicket) {
  if (!ticket.customer) return 'Unknown'
  return `${ticket.customer.firstname} ${ticket.customer.lastname}`.trim() || ticket.customer.email || 'Unknown'
}

export function customerInitials(ticket: ZammadTicket) {
  if (!ticket.customer) return '?'
  return `${ticket.customer.firstname.charAt(0)}${ticket.customer.lastname.charAt(0)}`.toUpperCase() || '?'
}

export function formatRelativeTime(iso: string) {
  const diffMs = Date.now() - new Date(iso).getTime()
  const mins = Math.max(0, Math.floor(diffMs / 60_000))
  if (mins < 60) return `${mins}m`
  const hours = Math.floor(mins / 60)
  if (hours < 24) return `${hours}h`
  return `${Math.floor(hours / 24)}d`
}

export function formatDate(iso: string) {
  return new Date(iso).toLocaleDateString('en-GB', {
    day: 'numeric',
    month: 'short',
    year: 'numeric',
  })
}

export function formatTimestamp(iso: string) {
  return new Date(iso).toLocaleTimeString('en-GB', {
    hour: '2-digit',
    minute: '2-digit',
  })
}

export function formatDateKey(date: Date) {
  const year = date.getFullYear()
  const month = String(date.getMonth() + 1).padStart(2, '0')
  const day = String(date.getDate()).padStart(2, '0')
  return `${year}-${month}-${day}`
}

export function stripHtml(value = '') {
  return value.replace(/<[^>]*>/g, ' ').replace(/\s+/g, ' ').trim()
}

export function titleCase(value: string) {
  return value
    .split(/[\s_-]+/)
    .filter(Boolean)
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1).toLowerCase())
    .join(' ')
}

function capitalize(value: string) {
  return value.charAt(0).toUpperCase() + value.slice(1)
}
