'use client'

import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'react'
import { useRouter } from 'next/navigation'
import { useQuery } from '@tanstack/react-query'
import {
  CheckCheck,
  ChevronDown,
  ExternalLink,
  Image,
  Mail,
  MessageCircle,
  MoreHorizontal,
  Paperclip,
  PenLine,
  Plus,
  RefreshCw,
  Search,
  SlidersHorizontal,
  Sparkles,
  Tag,
  X,
} from 'lucide-react'

import { AiDraftButton } from './AiDraftButton'
import { MacrosPanel } from './MacrosPanel'
import { SentimentBadge } from './SentimentBadge'
import { useAiDraft } from './useAiDraft'
import { emit } from '@/lib/telemetry/client'

// ─── Types ───────────────────────────────────────────────────────────────────

interface ZammadTicket {
  id: number
  number: string
  title: string
  state: { id: number; name: string }
  priority: { id: number; name: string }
  group: { id: number; name: string }
  owner: { id: number; firstname: string; lastname: string; email: string } | null
  customer: { id: number; firstname: string; lastname: string; email: string } | null
  tags?: string[]
  created_at: string
  updated_at: string
  article_count?: number
}

interface ZammadArticle {
  id: number
  ticket_id: number
  type: string
  internal: boolean
  body: string
  from?: string
  sender: string
  created_at: string
}

interface Agent {
  id: number
  firstname: string
  lastname: string
  email: string
}

interface Group {
  id: number
  name: string
}

interface ShopifyOrder {
  id: string | number
  name?: string
  order_number?: string | number
  financial_status?: string
  fulfillment_status?: string | null
  created_at?: string
  total_price?: string
}

interface StripeData {
  subscription?: {
    status?: string
    plan?: string
  }
}

interface CustomerContext {
  shopify?: {
    orders?: ShopifyOrder[]
  } | null
  stripe?: StripeData | null
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

function formatTime(iso: string): string {
  const d = new Date(iso)
  const now = new Date()
  const diffMs = now.getTime() - d.getTime()
  const diffMins = Math.floor(diffMs / 60_000)
  if (diffMins < 60) return `${diffMins}m`
  const diffHrs = Math.floor(diffMins / 60)
  if (diffHrs < 24) return `${diffHrs}h`
  return `${Math.floor(diffHrs / 24)}d`
}

function formatTimestamp(iso: string): string {
  return new Date(iso).toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit' })
}

function customerInitials(ticket: ZammadTicket): string {
  if (!ticket.customer) return '?'
  return `${ticket.customer.firstname[0] ?? ''}${ticket.customer.lastname[0] ?? ''}`.toUpperCase()
}

function customerFullName(ticket: ZammadTicket): string {
  if (!ticket.customer) return 'Unknown'
  return `${ticket.customer.firstname} ${ticket.customer.lastname}`
}

function priorityDotClass(priorityName: string): string {
  const name = priorityName.toLowerCase()
  if (name.includes('high') || name === '3') return 'bg-red-500'
  if (name.includes('normal') || name === '2') return 'bg-orange-400'
  return 'bg-gray-400'
}

const FILTER_TABS = ['all', 'open', 'pending', 'solved'] as const
type FilterTab = (typeof FILTER_TABS)[number]

const FILTER_TAB_LABELS: Record<FilterTab, string> = {
  all: 'All',
  open: 'Open',
  pending: 'Pending',
  solved: 'Solved',
}

const STATE_OPTIONS: { id: number; label: string }[] = [
  { id: 1, label: 'New' },
  { id: 2, label: 'Open' },
  { id: 4, label: 'Closed' },
  { id: 6, label: 'Pending reminder' },
]

const PRIORITY_OPTIONS: { id: number; label: string }[] = [
  { id: 1, label: 'Low' },
  { id: 2, label: 'Normal' },
  { id: 3, label: 'High' },
]

interface InboxRouteFilter {
  activeFilter: FilterTab
  queue?: string
  channel?: string
  assigned?: 'mine' | 'unassigned' | 'all'
  aiState?: 'all' | 'resolved' | 'routed' | 'abandoned'
  label: string
}

function resolveInboxRouteFilter(slug: string[] = []): InboxRouteFilter {
  const [section, value] = slug

  if (!section) {
    return { activeFilter: 'open', assigned: 'mine', label: 'Your inbox' }
  }

  if (section === 'all') {
    return { activeFilter: 'all', assigned: 'all', label: 'All conversations' }
  }

  if (section === 'unassigned') {
    return { activeFilter: 'open', assigned: 'unassigned', label: 'Unassigned' }
  }

  if (section === 'channels' && value) {
    return {
      activeFilter: value === 'all' ? 'all' : 'open',
      channel: value,
      label: value === 'all' ? 'All channels' : `${value[0]?.toUpperCase() ?? ''}${value.slice(1)}`,
    }
  }

  if (section === 'ai') {
    const aiState = value === 'resolved' || value === 'routed' || value === 'abandoned'
      ? value
      : 'all'
    return { activeFilter: 'all', aiState, label: `AI ${aiState}` }
  }

  if (section === 'created-by-you') {
    return { activeFilter: 'all', queue: 'created-by-you', label: 'Created by you' }
  }

  return { activeFilter: 'open', queue: section, label: section.replace(/-/g, ' ') }
}

function createClientId(prefix: string): string {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
    return `${prefix}-${crypto.randomUUID()}`
  }

  return `${prefix}-${Date.now()}-${Math.random().toString(36).slice(2)}`
}

function resolveMacroVariables(template: string, ticket: ZammadTicket | null): string {
  if (!ticket) return template
  const firstName = ticket.customer?.firstname ?? ''
  const lastName = ticket.customer?.lastname ?? ''
  const fullName = customerFullName(ticket)

  return template
    .replace(/\{\{\s*(ticket\.)?customer\.first_?name\s*\}\}/gi, firstName)
    .replace(/\{\{\s*(ticket\.)?customer\.last_?name\s*\}\}/gi, lastName)
    .replace(/\{\{\s*(ticket\.)?customer\.name\s*\}\}/gi, fullName)
    .replace(/\{\{\s*ticket\.number\s*\}\}/gi, ticket.number)
}

// ─── Sub-components ──────────────────────────────────────────────────────────

function LoadingSkeleton() {
  return (
    <div className="space-y-2 px-2 pt-2">
      {[1, 2, 3, 4].map((i) => (
        <div key={i} className="rounded-xl p-3 bg-gray-100 animate-pulse">
          <div className="flex items-start gap-3">
            <div className="size-8 rounded-full bg-gray-200 shrink-0" />
            <div className="flex-1 space-y-2">
              <div className="h-3 bg-gray-200 rounded w-3/4" />
              <div className="h-2 bg-gray-200 rounded w-full" />
              <div className="h-2 bg-gray-200 rounded w-1/2" />
            </div>
          </div>
        </div>
      ))}
    </div>
  )
}

interface TagEditorProps {
  tags: string[]
  onAdd: (tag: string) => void
  onRemove: (tag: string) => void
}

function TagEditor({ tags, onAdd, onRemove }: TagEditorProps) {
  const [inputValue, setInputValue] = useState('')
  const [isAdding, setIsAdding] = useState(false)

  const handleAdd = () => {
    const tag = inputValue.trim()
    if (!tag || tags.includes(tag)) {
      setInputValue('')
      setIsAdding(false)
      return
    }
    onAdd(tag)
    setInputValue('')
    setIsAdding(false)
  }

  return (
    <div className="flex flex-wrap items-center gap-1.5">
      {tags.map((tag) => (
        <span
          key={tag}
          className="inline-flex items-center gap-1 rounded-full bg-blue-50 border border-blue-200 px-2 py-0.5 text-[11px] font-medium text-blue-700"
        >
          {tag}
          <button
            type="button"
            onClick={() => onRemove(tag)}
            className="hover:text-blue-900 transition-colors"
            aria-label={`Remove tag ${tag}`}
          >
            <X className="size-2.5" />
          </button>
        </span>
      ))}
      {isAdding ? (
        <input
          autoFocus
          type="text"
          value={inputValue}
          onChange={(e) => setInputValue(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') handleAdd()
            if (e.key === 'Escape') { setIsAdding(false); setInputValue('') }
          }}
          onBlur={handleAdd}
          className="w-20 rounded-full border border-blue-300 bg-white px-2 py-0.5 text-[11px] outline-none focus:ring-1 focus:ring-blue-400"
          placeholder="tag…"
        />
      ) : (
        <button
          type="button"
          onClick={() => setIsAdding(true)}
          className="inline-flex items-center gap-0.5 rounded-full border border-dashed border-gray-300 px-2 py-0.5 text-[11px] text-gray-400 hover:border-gray-400 hover:text-gray-600 transition-colors"
        >
          <Plus className="size-2.5" /> Add tag
        </button>
      )}
    </div>
  )
}

interface AiPanelProps {
  ticketId: number | null
  onInsertQuickReply: (text: string) => void
}

function AiPanel({ ticketId, onInsertQuickReply }: AiPanelProps) {
  const [quickReplies, setQuickReplies] = useState<string[]>([])
  const [quickLoading, setQuickLoading] = useState(false)

  const [summary, setSummary] = useState<string | null>(null)
  const [summaryLoading, setSummaryLoading] = useState(false)
  const [summaryExpanded, setSummaryExpanded] = useState(false)

  const generateQuickReplies = useCallback(async () => {
    if (!ticketId || quickLoading) return
    setQuickLoading(true)
    setQuickReplies([])
    try {
      const res = await fetch(`/api/support/tickets/${ticketId}/quick-replies`, {
        method: 'POST',
      })
      if (!res.ok) throw new Error(`Failed: ${res.status}`)
      const data = (await res.json()) as { options: string[] }
      setQuickReplies(data.options ?? [])
    } catch {
      // silent — user can retry
    } finally {
      setQuickLoading(false)
    }
  }, [ticketId, quickLoading])

  const generateSummary = useCallback(async () => {
    if (!ticketId || summaryLoading) return
    setSummaryLoading(true)
    setSummary(null)
    try {
      const res = await fetch(`/api/support/tickets/${ticketId}/summarize`, {
        method: 'POST',
      })
      if (!res.ok) throw new Error(`Failed: ${res.status}`)
      const data = (await res.json()) as { summary: string }
      setSummary(data.summary ?? null)
    } catch {
      setSummary('Unable to generate summary.')
    } finally {
      setSummaryLoading(false)
    }
  }, [ticketId, summaryLoading])

  // Reset when ticket changes
  useEffect(() => {
    setQuickReplies([])
    setSummary(null)
    setSummaryExpanded(false)
  }, [ticketId])

  return (
    <div className="space-y-3">
      {/* Quick Replies Card */}
      <div className="rounded-lg border border-gray-200 bg-white p-4">
        <div className="flex items-center justify-between mb-3">
          <div className="flex items-center gap-1.5">
            <Sparkles className="size-3.5 text-blue-500" />
            <h4 className="text-[13px] font-semibold text-gray-900">Quick Replies</h4>
          </div>
          <button
            type="button"
            disabled={!ticketId || quickLoading}
            onClick={generateQuickReplies}
            className="text-[11px] font-medium text-blue-600 hover:text-blue-800 disabled:opacity-40 disabled:cursor-not-allowed flex items-center gap-1"
          >
            <RefreshCw className={`size-3 ${quickLoading ? 'animate-spin' : ''}`} />
            Generate options
          </button>
        </div>

        {quickLoading && (
          <div className="space-y-2">
            {[1, 2, 3].map((i) => (
              <div key={i} className="h-10 bg-gray-100 rounded-lg animate-pulse" />
            ))}
          </div>
        )}

        {!quickLoading && quickReplies.length === 0 && (
          <p className="text-[12px] text-gray-400 italic">
            {ticketId ? 'Click "Generate options" to get AI suggestions.' : 'Select a ticket to generate quick replies.'}
          </p>
        )}

        {!quickLoading && quickReplies.length > 0 && (
          <div className="space-y-2">
            {quickReplies.map((reply, i) => (
              <button
                key={i}
                type="button"
                onClick={() => onInsertQuickReply(reply)}
                className="w-full text-left rounded-lg border border-gray-200 bg-gray-50 hover:bg-blue-50 hover:border-blue-200 px-3 py-2 text-[12px] text-gray-700 hover:text-blue-800 transition-colors"
              >
                {reply}
              </button>
            ))}
          </div>
        )}
      </div>

      {/* Summarize Card */}
      <div className="rounded-lg border border-gray-200 bg-white overflow-hidden">
        <button
          type="button"
          onClick={() => setSummaryExpanded((v) => !v)}
          className="w-full flex items-center justify-between px-4 py-3 hover:bg-gray-50 transition-colors"
        >
          <div className="flex items-center gap-1.5">
            <Sparkles className="size-3.5 text-purple-500" />
            <span className="text-[13px] font-semibold text-gray-900">Summarize</span>
          </div>
          <ChevronDown
            className={`size-4 text-gray-400 transition-transform ${summaryExpanded ? 'rotate-180' : ''}`}
          />
        </button>

        {summaryExpanded && (
          <div className="border-t border-gray-200 px-4 py-3">
            {!summary && !summaryLoading && (
              <button
                type="button"
                disabled={!ticketId}
                onClick={generateSummary}
                className="w-full rounded-lg bg-purple-50 border border-purple-200 text-[12px] font-medium text-purple-700 hover:bg-purple-100 px-3 py-2 transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
              >
                Summarize conversation
              </button>
            )}

            {summaryLoading && (
              <div className="space-y-2">
                {[1, 2, 3].map((i) => (
                  <div key={i} className="h-3 bg-gray-100 rounded animate-pulse" style={{ width: `${60 + i * 15}%` }} />
                ))}
              </div>
            )}

            {summary && !summaryLoading && (
              <ul className="space-y-1.5">
                {summary.split(/\n|•/).filter(Boolean).map((line, i) => (
                  <li key={i} className="flex items-start gap-2 text-[12px] text-gray-700">
                    <span className="mt-1.5 size-1 rounded-full bg-purple-400 shrink-0" />
                    {line.trim()}
                  </li>
                ))}
              </ul>
            )}
          </div>
        )}
      </div>
    </div>
  )
}

interface CustomerContextPanelProps {
  ticket: ZammadTicket | null
}

function CustomerContextPanel({ ticket }: CustomerContextPanelProps) {
  const query = useQuery({
    queryKey: ['support', 'customerContext', ticket?.customer?.id ?? null, ticket?.group?.id ?? null],
    enabled: Boolean(ticket?.customer?.id),
    staleTime: 5 * 60_000,
    gcTime: 24 * 60_000,
    queryFn: async () => {
      if (!ticket?.customer?.id) return null
      const params = new URLSearchParams()
      if (ticket.group?.id) params.set('orgId', String(ticket.group.id))

      const response = await fetch(`/api/support/customers/${ticket.customer.id}/context?${params}`)
      if (!response.ok) {
        throw new Error(`Customer context failed: ${response.status}`)
      }
      return (await response.json()) as CustomerContext
    },
  })

  if (!ticket) return null

  const context = query.data ?? null
  const loading = query.isPending || query.isFetching
  const customerName = customerFullName(ticket)
  const customerEmail = ticket.customer?.email ?? '—'

  return (
    <div className="rounded-lg border border-gray-200 bg-white overflow-hidden">
      <div className="px-4 py-3 border-b border-gray-100">
        <h4 className="text-[13px] font-semibold text-gray-900">Customer</h4>
      </div>
      <div className="px-4 py-3 space-y-2.5">
        <div className="flex items-center gap-2 text-[13px]">
          <Mail className="size-4 text-gray-400 shrink-0" />
          <span className="text-blue-600 truncate">{customerEmail}</span>
        </div>
        <div className="text-[13px] font-medium text-gray-800">{customerName}</div>
      </div>

      {/* Shopify */}
      <div className="border-t border-gray-100 px-4 py-3">
        <div className="flex items-center justify-between mb-2">
          <h5 className="text-[12px] font-semibold text-gray-700">Recent Orders (Shopify)</h5>
          {context?.shopify && <ExternalLink className="size-3.5 text-gray-400" />}
        </div>

        {loading && (
          <div className="space-y-2">
            {[1, 2].map((i) => (
              <div key={i} className="h-14 bg-gray-100 rounded-lg animate-pulse" />
            ))}
          </div>
        )}

        {!loading && context?.shopify === null && (
          <div className="text-center py-2">
            <p className="text-[12px] text-gray-500 mb-2">No Shopify connection found.</p>
            <a
              href="/settings/integrations"
              className="text-[11px] font-medium text-blue-600 hover:underline"
            >
              Connect Shopify →
            </a>
          </div>
        )}

        {!loading && context?.shopify?.orders && context.shopify.orders.length > 0 && (
          <div className="space-y-2">
            {context.shopify.orders.slice(0, 3).map((order) => (
              <div
                key={order.id}
                className="rounded-lg border border-gray-200 bg-gray-50 p-2.5"
              >
                <div className="flex items-center justify-between mb-1">
                  <span className="text-[12px] font-semibold text-gray-900">
                    {order.name ?? `#${order.order_number ?? order.id}`}
                  </span>
                  <span
                    className={`text-[11px] font-medium ${
                      order.fulfillment_status === 'fulfilled'
                        ? 'text-green-600'
                        : 'text-yellow-600'
                    }`}
                  >
                    {order.fulfillment_status
                      ? order.fulfillment_status.charAt(0).toUpperCase() + order.fulfillment_status.slice(1)
                      : 'Unfulfilled'}
                  </span>
                </div>
                {order.created_at && (
                  <p className="text-[11px] text-gray-500">
                    {new Date(order.created_at).toLocaleDateString('en-GB', {
                      day: 'numeric',
                      month: 'short',
                    })}
                    {order.total_price ? ` · $${order.total_price}` : ''}
                  </p>
                )}
              </div>
            ))}
          </div>
        )}

        {!loading && !context?.shopify && context !== null && (
          <div className="text-center py-2">
            <a
              href="/settings/integrations"
              className="text-[11px] font-medium text-blue-600 hover:underline"
            >
              Connect Shopify →
            </a>
          </div>
        )}
      </div>

      {/* Stripe */}
      {context?.stripe && (
        <div className="border-t border-gray-100 px-4 py-3">
          <h5 className="text-[12px] font-semibold text-gray-700 mb-2">Subscription (Stripe)</h5>
          <div className="flex items-center justify-between text-[12px]">
            <span className="text-gray-600">Status</span>
            <span
              className={`font-medium ${
                context.stripe.subscription?.status === 'active'
                  ? 'text-green-600'
                  : 'text-orange-600'
              }`}
            >
              {context.stripe.subscription?.status ?? 'Unknown'}
            </span>
          </div>
          {context.stripe.subscription?.plan && (
            <div className="flex items-center justify-between text-[12px] mt-1">
              <span className="text-gray-600">Plan</span>
              <span className="font-medium text-gray-800">{context.stripe.subscription.plan}</span>
            </div>
          )}
        </div>
      )}
    </div>
  )
}

// ─── Main component ───────────────────────────────────────────────────────────

interface InboxWorkspacePageProps {
  liveCount: number
  agentId?: string
  routeSlug?: string[]
}

export function InboxWorkspacePage({ liveCount: _liveCount, agentId, routeSlug = [] }: InboxWorkspacePageProps) {
  const router = useRouter()
  const routeFilter = useMemo(() => resolveInboxRouteFilter(routeSlug), [routeSlug])
  // ── Ticket list state ──
  const [tickets, setTickets] = useState<ZammadTicket[]>([])
  const [ticketsLoading, setTicketsLoading] = useState(true)
  const [ticketsError, setTicketsError] = useState<string | null>(null)
  const [activeFilter, setActiveFilter] = useState<FilterTab>(routeFilter.activeFilter)
  const [searchQuery, setSearchQuery] = useState('')

  // ── Selected ticket state ──
  const [selectedTicket, setSelectedTicket] = useState<ZammadTicket | null>(null)
  const [articles, setArticles] = useState<ZammadArticle[]>([])
  const [articlesLoading, setArticlesLoading] = useState(false)
  const [sentiment, setSentiment] = useState<{ sentiment: string; score: number } | null>(null)

  // ── Reply state ──
  const [replyText, setReplyText] = useState('')
  const [isInternal, setIsInternal] = useState(false)
  const [isSending, setIsSending] = useState(false)
  const [replyError, setReplyError] = useState<string | null>(null)
  const [workspaceNotice, setWorkspaceNotice] = useState<string | null>(null)
  const [macroPaletteOpen, setMacroPaletteOpen] = useState(false)

  // ── Agent/group dropdowns ──
  const [agents, setAgents] = useState<Agent[]>([])
  const [groups, setGroups] = useState<Group[]>([])

  // ── Tag editing ──
  const [localTags, setLocalTags] = useState<string[]>([])

  const scrollRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    setActiveFilter(routeFilter.activeFilter)
  }, [routeFilter.activeFilter])

  // ─── Fetch ticket list ──────────────────────────────────────────────────────

  const fetchTickets = useCallback(async (filter: FilterTab, route: InboxRouteFilter) => {
    setTicketsLoading(true)
    setTicketsError(null)
    try {
      const state = filter === 'all' ? '' : filter
      const params = new URLSearchParams({ page: '1', limit: '50' })
      if (state) params.set('state', state)
      if (route.queue) params.set('queue', route.queue)
      if (route.channel) params.set('channel', route.channel)
      if (route.assigned) params.set('assigned', route.assigned)
      if (route.aiState) params.set('aiState', route.aiState)
      const url = `/api/support/tickets?${params}`
      const res = await fetch(url)
      if (!res.ok) throw new Error(`Failed to load tickets: ${res.status}`)
      const data = (await res.json()) as { tickets: ZammadTicket[]; total: number }
      setTickets(data.tickets ?? [])
    } catch (err: unknown) {
      setTicketsError(err instanceof Error ? err.message : 'Failed to load tickets')
    } finally {
      setTicketsLoading(false)
    }
  }, [])

  useEffect(() => {
    void fetchTickets(activeFilter, routeFilter)
  }, [activeFilter, fetchTickets, routeFilter])

  // ─── Fetch agents/groups once ───────────────────────────────────────────────

  useEffect(() => {
    fetch('/api/support/agents')
      .then((r) => r.ok ? (r.json() as Promise<Agent[]>) : Promise.reject())
      .then(setAgents)
      .catch(() => undefined)

    fetch('/api/support/groups')
      .then((r) => r.ok ? (r.json() as Promise<Group[]>) : Promise.reject())
      .then(setGroups)
      .catch(() => undefined)
  }, [])

  // ─── Select ticket ──────────────────────────────────────────────────────────

  const loadTicketDetails = useCallback(async (ticket: ZammadTicket) => {
    setSelectedTicket(ticket)
    setLocalTags(ticket.tags ?? [])
    setArticles([])
    setArticlesLoading(true)
    setSentiment(null)
    setReplyText('')
    setIsInternal(false)

    try {
      const [articlesRes, sentimentRes] = await Promise.allSettled([
        fetch(`/api/support/tickets/${ticket.id}/articles`).then((r) =>
          r.ok ? (r.json() as Promise<{ articles: ZammadArticle[] }>) : Promise.reject(r.status),
        ),
        fetch(`/api/support/tickets/${ticket.id}/sentiment`, { method: 'POST' }).then((r) =>
          r.ok
            ? (r.json() as Promise<{ sentiment: string; score: number }>)
            : Promise.reject(r.status),
        ),
      ])

      if (articlesRes.status === 'fulfilled') {
        setArticles(articlesRes.value.articles ?? [])
      }
      if (sentimentRes.status === 'fulfilled') {
        setSentiment(sentimentRes.value)
      }
    } finally {
      setArticlesLoading(false)
    }
  }, [])

  // Scroll to bottom after articles load
  useEffect(() => {
    if (!articlesLoading) {
      requestAnimationFrame(() => {
        scrollRef.current?.scrollTo({ top: scrollRef.current.scrollHeight, behavior: 'smooth' })
      })
    }
  }, [articlesLoading, articles.length])

  // ─── Refresh selected ticket ────────────────────────────────────────────────

  const refreshSelectedTicket = useCallback(async () => {
    if (!selectedTicket) return
    try {
      const res = await fetch(`/api/support/tickets/${selectedTicket.id}`)
      if (!res.ok) return
      const updated = (await res.json()) as ZammadTicket
      setSelectedTicket(updated)
      setLocalTags(updated.tags ?? [])
      // Also refresh in the list
      setTickets((prev) => prev.map((t) => (t.id === updated.id ? updated : t)))
    } catch {
      // best-effort
    }
  }, [selectedTicket])

  // ─── AI Draft integration ───────────────────────────────────────────────────

  const conversationHistory = useMemo(() => articles.map((a) => ({
    role: (a.sender?.toLowerCase() === 'agent' ? 'assistant' : 'user') as 'user' | 'assistant',
    content: a.body,
  })), [articles])

  const { suggestReply, isStreaming, wasAiGenerated, markEdited } = useAiDraft({
    conversationHistory,
    customerName: selectedTicket ? customerFullName(selectedTicket) : 'Customer',
    agentId,
    onDraftReady: (draft) => setReplyText(draft),
  })

  // ─── Send reply ─────────────────────────────────────────────────────────────

  const handleSend = useCallback(async () => {
    const text = replyText.trim()
    if (!text || !selectedTicket || isSending) return
    setIsSending(true)
    setReplyError(null)
    const clientId = createClientId('article')
    const optimisticArticle: ZammadArticle = {
      id: -Date.now(),
      ticket_id: selectedTicket.id,
      type: 'note',
      internal: isInternal,
      body: text,
      from: 'You',
      sender: 'Agent',
      created_at: new Date().toISOString(),
    }
    setArticles((prev) => [...prev, optimisticArticle])
    setReplyText('')
    try {
      const res = await fetch(`/api/support/tickets/${selectedTicket.id}/articles`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ body: text, internal: isInternal, clientId }),
      })
      if (!res.ok) throw new Error(`Send failed: ${res.status}`)
      const payload = (await res.json()) as ZammadArticle | { article?: ZammadArticle; clientId?: string; optimisticStatus?: string }
      const newArticle = 'article' in payload && payload.article ? payload.article : payload as ZammadArticle
      setArticles((prev) => prev.map((article) => (article.id === optimisticArticle.id ? newArticle : article)))
      if (wasAiGenerated && agentId) {
        void fetch('/api/inbox/draft/feedback', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({
            agentId,
            customerName: customerFullName(selectedTicket),
            customerMessage: articles.at(-1)?.body,
            approvedText: text,
          }),
        })
      }
      emit('support.outcome', {
        props: {
          outcome: isInternal ? 'internal_note_added' : 'agent_replied',
          ticket_id: selectedTicket.id,
          client_id: clientId,
        },
      })
      requestAnimationFrame(() => {
        scrollRef.current?.scrollTo({ top: scrollRef.current.scrollHeight, behavior: 'smooth' })
      })
    } catch (error) {
      setArticles((prev) => prev.filter((article) => article.id !== optimisticArticle.id))
      setReplyText(text)
      setReplyError(error instanceof Error ? error.message : 'Failed to send reply')
      emit('mutation.failed', {
        props: {
          mutation: 'support.article.create',
          ticket_id: selectedTicket.id,
          client_id: clientId,
        },
      })
    } finally {
      setIsSending(false)
    }
  }, [agentId, articles, isInternal, isSending, replyText, selectedTicket, wasAiGenerated])

  // ─── PATCH ticket fields ────────────────────────────────────────────────────

  const patchTicket = useCallback(
    async (patch: Record<string, unknown>) => {
      if (!selectedTicket) return
      try {
        const res = await fetch(`/api/support/tickets/${selectedTicket.id}`, {
          method: 'PATCH',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify(patch),
        })
        if (!res.ok) return
        const updated = (await res.json()) as ZammadTicket
        setSelectedTicket(updated)
        setLocalTags(updated.tags ?? [])
        setTickets((prev) => prev.map((t) => (t.id === updated.id ? updated : t)))
      } catch {
        setWorkspaceNotice('Ticket update failed. Your local changes may be stale.')
      }
    },
    [selectedTicket],
  )

  const handleAddTag = useCallback(
    (tag: string) => {
      const newTags = [...localTags, tag]
      setLocalTags(newTags)
      void patchTicket({ tags: newTags })
    },
    [localTags, patchTicket],
  )

  const handleRemoveTag = useCallback(
    (tag: string) => {
      const newTags = localTags.filter((t) => t !== tag)
      setLocalTags(newTags)
      void patchTicket({ tags: newTags })
    },
    [localTags, patchTicket],
  )

  const handleReplyChange = useCallback(
    (value: string) => {
      setReplyText(value)
      markEdited()
    },
    [markEdited],
  )

  // ─── Filtered ticket list ───────────────────────────────────────────────────

  const filteredTickets = tickets.filter((t) => {
    if (!searchQuery) return true
    const q = searchQuery.toLowerCase()
    return (
      t.title.toLowerCase().includes(q) ||
      customerFullName(t).toLowerCase().includes(q) ||
      t.number.includes(q)
    )
  })

  // ─── Render ─────────────────────────────────────────────────────────────────

  return (
    <div className="flex h-full min-h-0 overflow-hidden pr-3 md:pr-4 bg-white">
      <div className="grid h-full w-full min-w-0 xl:grid-cols-[280px_minmax(0,1fr)] xl:gap-4">

        {/* ── Left column: ticket list ── */}
        <section className="h-full overflow-y-auto border-r border-gray-200 bg-white flex flex-col pb-4">
          {/* Header */}
          <div className="px-4 py-4 border-b border-gray-200 bg-white sticky top-0 z-10">
            <div className="flex items-center justify-between mb-3">
              <div>
                <div className="text-[16px] font-semibold text-gray-900">Inbox</div>
                <div className="text-[11px] font-medium uppercase tracking-[0.16em] text-gray-400">{routeFilter.label}</div>
              </div>
              <button className="text-gray-500 hover:text-gray-900 p-1 rounded-md hover:bg-gray-100">
                <SlidersHorizontal className="size-4" />
              </button>
            </div>
            <div className="relative">
              <Search className="absolute left-2.5 top-2 size-4 text-gray-400" />
              <input
                type="text"
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                placeholder="Search tickets..."
                className="w-full rounded-md border border-gray-200 bg-gray-50 py-1.5 pl-8 pr-3 text-[13px] outline-none focus:border-blue-500 focus:ring-1 focus:ring-blue-500"
              />
            </div>
          </div>

          {/* Filter tabs */}
          <div className="flex items-center gap-0.5 px-3 py-2 border-b border-gray-200 bg-gray-50/50">
            {FILTER_TABS.map((tab) => (
              <button
                key={tab}
                type="button"
                onClick={() => setActiveFilter(tab)}
                className={`flex-1 rounded-md py-1 text-[12px] font-medium transition-colors ${
                  activeFilter === tab
                    ? 'bg-white shadow-sm text-gray-900 border border-gray-200'
                    : 'text-gray-500 hover:text-gray-800'
                }`}
              >
                {FILTER_TAB_LABELS[tab]}
              </button>
            ))}
          </div>

          {/* Ticket rows */}
          <div className="flex-1 overflow-y-auto px-2 pt-2">
            {ticketsLoading && <LoadingSkeleton />}

            {ticketsError && (
              <div className="px-4 py-4 text-[13px] text-red-600">{ticketsError}</div>
            )}

            {!ticketsLoading && !ticketsError && filteredTickets.length === 0 && (
              <div className="px-4 py-6 text-center text-[13px] text-gray-400">
                No tickets found.
              </div>
            )}

            {!ticketsLoading &&
              filteredTickets.map((ticket) => {
                const initials = customerInitials(ticket)
                const name = customerFullName(ticket)
                const isActive = selectedTicket?.id === ticket.id

                return (
                  <button
                    key={ticket.id}
                    type="button"
                    onClick={() => loadTicketDetails(ticket)}
                    className={`w-full flex items-start gap-3 p-3 rounded-xl text-left transition-colors mb-1 ${
                      isActive
                        ? 'bg-white shadow-[0_4px_12px_rgba(22,20,17,0.08)] ring-1 ring-gray-200'
                        : 'bg-transparent hover:bg-gray-50'
                    }`}
                  >
                    {/* Avatar */}
                    <div className="relative flex size-8 shrink-0 items-center justify-center rounded-full bg-blue-500 text-[12px] font-bold text-white">
                      {initials}
                      <div className="absolute -bottom-0.5 -right-0.5 rounded-full border-2 border-white bg-white">
                        <MessageCircle className="size-2.5 text-gray-600" />
                      </div>
                    </div>

                    <div className="min-w-0 flex-1">
                      <div className="flex items-center justify-between mb-0.5">
                        <span className="truncate text-[13px] font-semibold text-gray-900">{name}</span>
                        <div className="flex items-center gap-1 shrink-0 ml-1">
                          <span
                            className={`inline-block size-1.5 rounded-full ${priorityDotClass(ticket.priority?.name ?? '')}`}
                          />
                          <span className={`text-[11px] ${isActive ? 'text-blue-600 font-medium' : 'text-gray-400'}`}>
                            {formatTime(ticket.updated_at)}
                          </span>
                        </div>
                      </div>
                      <div className="truncate text-[12px] text-gray-700 mb-1">{ticket.title}</div>
                      <div className="flex items-center gap-1.5 flex-wrap">
                        <span className={`inline-flex items-center rounded-full px-1.5 py-0.5 text-[10px] font-medium ${
                          ticket.state?.name === 'closed'
                            ? 'bg-green-100 text-green-700'
                            : ticket.state?.name === 'pending reminder'
                            ? 'bg-yellow-100 text-yellow-700'
                            : 'bg-blue-100 text-blue-700'
                        }`}>
                          {ticket.state?.name ?? 'open'}
                        </span>
                        {ticket.tags?.slice(0, 2).map((tag) => (
                          <span
                            key={tag}
                            className="rounded-full bg-gray-100 px-1.5 py-0.5 text-[10px] text-gray-600"
                          >
                            {tag}
                          </span>
                        ))}
                      </div>
                    </div>
                  </button>
                )
              })}
          </div>
        </section>

        {/* ── Center + Right columns ── */}
        <div className="my-3 overflow-hidden rounded-[16px] border border-[#E9EBF2] bg-white shadow-[0_8px_24px_rgba(22,20,17,0.04)] md:my-4">
          <div className="grid min-h-full xl:grid-cols-[minmax(0,1fr)_320px]">

            {/* ── Center: conversation ── */}
            <main className="flex h-full flex-col border-r border-gray-200 bg-white min-h-0">
              {!selectedTicket ? (
                <div className="flex-1 flex items-center justify-center">
                  <div className="text-center text-gray-400">
                    <MessageCircle className="size-12 mx-auto mb-3 opacity-30" />
                    <p className="text-[14px]">Select a ticket to view the conversation</p>
                  </div>
                </div>
              ) : (
                <>
                  {/* Ticket header */}
                  <div className="border-b border-gray-200 px-5 py-3 bg-white">
                    <div className="flex items-start justify-between gap-3 mb-2">
                      <div className="flex-1 min-w-0">
                        <div className="flex items-center gap-2 mb-1">
                          <h2 className="text-[14px] font-semibold text-gray-900 truncate">
                            {selectedTicket.title}
                          </h2>
                          <span className="text-[11px] text-gray-400 shrink-0">#{selectedTicket.number}</span>
                        </div>
                        {sentiment && (
                          <SentimentBadge sentiment={sentiment.sentiment} />
                        )}
                      </div>
                      <div className="flex items-center gap-1.5 shrink-0">
                        <button
                          onClick={() => void patchTicket({ state_id: 4 })}
                          className="rounded-md border border-gray-200 px-3 py-1.5 text-[12px] font-medium text-gray-700 hover:bg-gray-50 flex items-center gap-1.5"
                        >
                          <CheckCheck className="size-3.5" /> Close
                        </button>
                        <button className="rounded-md border border-gray-200 p-1.5 text-gray-500 hover:bg-gray-50">
                          <MoreHorizontal className="size-4" />
                        </button>
                      </div>
                    </div>

                    {/* Controls row */}
                    <div className="flex flex-wrap items-center gap-2">
                      {/* Status */}
                      <div className="relative">
                        <select
                          value={selectedTicket.state?.id ?? 2}
                          onChange={(e) => void patchTicket({ state_id: Number(e.target.value) })}
                          className="rounded-md border border-gray-200 bg-white py-1 pl-2 pr-6 text-[12px] text-gray-700 outline-none focus:border-blue-400 appearance-none cursor-pointer"
                        >
                          {STATE_OPTIONS.map((s) => (
                            <option key={s.id} value={s.id}>{s.label}</option>
                          ))}
                        </select>
                        <ChevronDown className="absolute right-1.5 top-1/2 -translate-y-1/2 size-3 text-gray-400 pointer-events-none" />
                      </div>

                      {/* Priority */}
                      <div className="relative">
                        <select
                          value={selectedTicket.priority?.id ?? 2}
                          onChange={(e) => void patchTicket({ priority_id: Number(e.target.value) })}
                          className="rounded-md border border-gray-200 bg-white py-1 pl-2 pr-6 text-[12px] text-gray-700 outline-none focus:border-blue-400 appearance-none cursor-pointer"
                        >
                          {PRIORITY_OPTIONS.map((p) => (
                            <option key={p.id} value={p.id}>{p.label}</option>
                          ))}
                        </select>
                        <ChevronDown className="absolute right-1.5 top-1/2 -translate-y-1/2 size-3 text-gray-400 pointer-events-none" />
                      </div>

                      {/* Assign to agent */}
                      {agents.length > 0 && (
                        <div className="relative">
                          <select
                            value={selectedTicket.owner?.id ?? 0}
                            onChange={(e) => void patchTicket({ owner_id: Number(e.target.value) })}
                            className="rounded-md border border-gray-200 bg-white py-1 pl-2 pr-6 text-[12px] text-gray-700 outline-none focus:border-blue-400 appearance-none cursor-pointer"
                          >
                            <option value={0}>Unassigned</option>
                            {agents.map((a) => (
                              <option key={a.id} value={a.id}>
                                {a.firstname} {a.lastname}
                              </option>
                            ))}
                          </select>
                          <ChevronDown className="absolute right-1.5 top-1/2 -translate-y-1/2 size-3 text-gray-400 pointer-events-none" />
                        </div>
                      )}

                      {/* Group */}
                      {groups.length > 0 && (
                        <div className="relative">
                          <select
                            value={selectedTicket.group?.id ?? 0}
                            onChange={(e) => void patchTicket({ group_id: Number(e.target.value) })}
                            className="rounded-md border border-gray-200 bg-white py-1 pl-2 pr-6 text-[12px] text-gray-700 outline-none focus:border-blue-400 appearance-none cursor-pointer"
                          >
                            {groups.map((g) => (
                              <option key={g.id} value={g.id}>{g.name}</option>
                            ))}
                          </select>
                          <ChevronDown className="absolute right-1.5 top-1/2 -translate-y-1/2 size-3 text-gray-400 pointer-events-none" />
                        </div>
                      )}
                    </div>

                    {/* Tags row */}
                    <div className="mt-2 flex items-center gap-2">
                      <Tag className="size-3.5 text-gray-400 shrink-0" />
                      <TagEditor
                        tags={localTags}
                        onAdd={handleAddTag}
                        onRemove={handleRemoveTag}
                      />
                    </div>
                  </div>

                  {/* Articles thread */}
                  <div ref={scrollRef} className="flex-1 overflow-y-auto p-5 bg-white space-y-4 min-h-0">
                    <div className="flex justify-center">
                      <span className="text-[11px] font-medium uppercase tracking-wider text-gray-400 bg-gray-50 px-3 py-1 rounded-full">
                        {new Date(selectedTicket.created_at).toLocaleDateString('en-GB', {
                          day: 'numeric',
                          month: 'short',
                          year: 'numeric',
                        })}
                      </span>
                    </div>

                    {articlesLoading && (
                      <div className="space-y-3">
                        {[1, 2].map((i) => (
                          <div key={i} className="flex gap-3">
                            <div className="size-7 rounded-full bg-gray-200 animate-pulse shrink-0" />
                            <div className="flex-1 space-y-2">
                              <div className="h-16 bg-gray-100 rounded-2xl animate-pulse" />
                            </div>
                          </div>
                        ))}
                      </div>
                    )}

                    {!articlesLoading &&
                      articles.map((article) => {
                        const isAgentMessage = article.sender?.toLowerCase() === 'agent'
                        const isInternalNote = article.internal

                        return (
                          <div
                            key={article.id}
                            className={`flex gap-3 ${isAgentMessage ? 'flex-row-reverse max-w-[85%] ml-auto' : 'max-w-[85%]'}`}
                          >
                            {/* Avatar */}
                            <div
                              className={`mt-auto flex size-7 shrink-0 items-center justify-center rounded-full text-[11px] font-bold text-white ${
                                isAgentMessage ? 'bg-gray-500' : 'bg-blue-500'
                              }`}
                            >
                              {isAgentMessage
                                ? 'A'
                                : customerInitials(selectedTicket)}
                            </div>

                            <div className={`space-y-1 ${isAgentMessage ? 'items-end flex flex-col' : ''}`}>
                              <div
                                className={`rounded-2xl px-4 py-2.5 text-[14px] ${
                                  isInternalNote
                                    ? 'bg-amber-50 border border-amber-200 text-amber-900 rounded-br-sm'
                                    : isAgentMessage
                                    ? 'bg-blue-600 text-white rounded-br-sm'
                                    : 'bg-gray-100 text-gray-900 rounded-bl-sm'
                                }`}
                                // Article body may contain HTML from Zammad
                                dangerouslySetInnerHTML={{ __html: article.body }}
                              />
                              <div className="text-[11px] text-gray-400 pl-1 pt-0.5 flex items-center gap-1.5">
                                {isInternalNote && (
                                  <span className="rounded bg-amber-100 text-amber-600 px-1 py-0.5 text-[10px] font-medium">
                                    Internal
                                  </span>
                                )}
                                {article.from
                                  ? article.from.split('<')[0]?.trim()
                                  : isAgentMessage
                                  ? 'Agent'
                                  : customerFullName(selectedTicket)}
                                {' · '}
                                {formatTimestamp(article.created_at)}
                              </div>
                            </div>
                          </div>
                        )
                      })}
                  </div>

                  {/* Reply area */}
                  <div className="p-4 bg-white border-t border-gray-100">
                    <div className="rounded-xl border border-gray-200 bg-white shadow-sm overflow-hidden focus-within:border-blue-500 focus-within:ring-1 focus-within:ring-blue-500 transition-shadow">
                      {/* Mode toggle */}
                      <div className="flex items-center gap-4 bg-gray-50 px-4 py-2 border-b border-gray-100">
                        <button
                          type="button"
                          onClick={() => setIsInternal(false)}
                          className={`text-[12px] font-medium flex items-center gap-1.5 transition-colors ${
                            !isInternal ? 'text-blue-600' : 'text-gray-500 hover:text-gray-800'
                          }`}
                        >
                          <MessageCircle className="size-3.5" /> Public reply
                        </button>
                        <button
                          type="button"
                          onClick={() => setIsInternal(true)}
                          className={`text-[12px] font-medium flex items-center gap-1.5 transition-colors ${
                            isInternal ? 'text-amber-600' : 'text-gray-500 hover:text-gray-800'
                          }`}
                        >
                          <PenLine className="size-3.5" /> Internal note
                        </button>
                      </div>

                      <textarea
                        rows={3}
                        value={replyText}
                        onChange={(e) => handleReplyChange(e.target.value)}
                        onKeyDown={(e) => {
                          if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') {
                            e.preventDefault()
                            void handleSend()
                          }
                        }}
                        className={`w-full resize-none bg-transparent p-4 text-[14px] text-gray-900 outline-none placeholder:text-gray-400 ${
                          isInternal ? 'bg-amber-50/30' : ''
                        }`}
                        placeholder={
                          isInternal
                            ? 'Add an internal note…'
                            : `Reply to ${customerFullName(selectedTicket)}…`
                        }
                      />

                      <div className="flex items-center justify-between px-3 py-2 bg-gray-50/50 border-t border-gray-100">
                        <div className="flex items-center gap-1">
                          <button className="p-1.5 text-gray-400 hover:text-gray-600 hover:bg-gray-100 rounded-md">
                            <Paperclip className="size-4" />
                          </button>
                          <button className="p-1.5 text-gray-400 hover:text-gray-600 hover:bg-gray-100 rounded-md">
                            <Image className="size-4" />
                          </button>
                          <div className="w-px h-4 bg-gray-300 mx-1" />
                          <AiDraftButton onSuggest={suggestReply} isStreaming={isStreaming} />
                        </div>
                        <div className="flex items-center gap-2">
                          <span className="text-[11px] text-gray-400 hidden sm:inline">⌘Enter to send</span>
                          <button
                            type="button"
                            onClick={() => void handleSend()}
                            disabled={!replyText.trim() || isSending}
                            className="flex items-center gap-2 rounded-lg bg-blue-600 px-4 py-1.5 text-[13px] font-medium text-white hover:bg-blue-700 shadow-sm disabled:opacity-40 disabled:cursor-not-allowed"
                          >
                            {isSending ? 'Sending…' : 'Send'}
                          </button>
                        </div>
                      </div>
                    </div>
                  </div>
                </>
              )}
            </main>

            {/* ── Right column: AI + Customer context ── */}
            <aside className="h-full overflow-y-auto bg-[#F9FAFB] p-4 space-y-4">
              {/* AI Panel */}
              <AiPanel
                ticketId={selectedTicket?.id ?? null}
                onInsertQuickReply={(text) => {
                  setReplyText(text)
                  markEdited()
                }}
              />

              {/* Customer Context */}
              <CustomerContextPanel ticket={selectedTicket} />

              {/* Macros */}
              <MacrosPanel
                ticketId={selectedTicket?.id ?? null}
                onMacroExecuted={refreshSelectedTicket}
              />
            </aside>
          </div>
        </div>
      </div>
    </div>
  )
}
