import { useLocation, useNavigate } from '@solidjs/router'
import { createEffect, createMemo, createResource, createSignal } from 'solid-js'
import { ConversationPanel } from '@/features/inbox/components/ConversationPanel'
import { InboxAside } from '@/features/inbox/components/InboxAside'
import { InboxWorkModal, type InboxModalRequest } from '@/features/inbox/components/InboxWorkModal'
import { TicketQueue } from '@/features/inbox/components/TicketQueue'
import { demoQuickReplies } from '@/features/inbox/lib/inbox-demo-data'
import {
  customerName,
  resolveInboxRouteFilter,
  type Agent,
  type InboxTab,
  type TicketSentiment,
  type ZammadArticle,
  type ZammadTicket,
} from '@/features/inbox/lib/inbox-model'
import { getAuthSession, getSessionContext } from '@/shared/api/auth-client'
import {
  addConversationTag,
  getConversationDetail,
  listConversations,
  listInboxesAsGroups,
  removeConversationTag,
  sendReply as postReply,
  setConversationAssignment,
  setConversationStatus,
  statusFromStateId,
  type LiveTicket,
} from '@/shared/api/inbox-client'
import { createSocialDraftFromInbox } from '@/shared/api/social-client'

async function loadInboxContext() {
  const [session, ctx] = await Promise.all([getAuthSession(), getSessionContext()])
  return {
    email: session?.user.email ?? '',
    name: session?.user.name ?? '',
    orgId: ctx.orgs[0]?.id ?? '',
    userId: session?.user.id ?? '',
  }
}

export default function InboxPage() {
  const location = useLocation()
  const navigate = useNavigate()
  const routeFilter = createMemo(() => resolveInboxRouteFilter(new URLSearchParams(location.search)))
  const [routeKey, setRouteKey] = createSignal<string | null>(null)
  const [activeTab, setActiveTab] = createSignal<InboxTab>('open')
  const [articles, setArticles] = createSignal<ZammadArticle[]>([])
  const [articlesLoading, setArticlesLoading] = createSignal(false)
  const [modal, setModal] = createSignal<InboxModalRequest | null>(null)
  const [notice, setNotice] = createSignal<string | null>(null)
  const [replySending, setReplySending] = createSignal(false)
  const [replyText, setReplyText] = createSignal('')
  const [searchQuery, setSearchQuery] = createSignal('')
  const [selectedTicket, setSelectedTicket] = createSignal<LiveTicket | null>(null)
  const [sentiment, setSentiment] = createSignal<TicketSentiment | null>(null)

  const [ctx] = createResource(loadInboxContext)
  const orgId = createMemo(() => ctx()?.orgId ?? '')

  // Broad load (client-side filtering below handles tab/queue/channel/search).
  const [ticketsRes, { mutate: mutateTickets }] = createResource(orgId, (id) =>
    id ? listConversations(id, { limit: 50 }).then((result) => result.tickets) : Promise.resolve([] as LiveTicket[]),
  )
  const tickets = () => ticketsRes() ?? []

  const groupsRes = createResource(orgId, (id) => (id ? listInboxesAsGroups(id) : Promise.resolve([])))
  const groups = () => groupsRes[0]() ?? []

  const agents = createMemo<Agent[]>(() => {
    const profile = ctx()
    if (!profile) return []
    const [firstname, ...rest] = (profile.name || profile.email || 'Velion Agent').split(' ')
    return [{
      id: 1,
      firstname: firstname || 'Velion',
      lastname: rest.join(' ') || 'Agent',
      email: profile.email || 'agent@velion.local',
    }]
  })

  createEffect(() => {
    if (routeKey() === location.search) return
    setRouteKey(location.search)
    setActiveTab(routeFilter().activeTab)
    setNotice(null)
    setReplyText('')
    setSelectedTicket(null)
    setSentiment(null)
    setArticles([])
  })

  const filteredTickets = createMemo(() => {
    const filter = routeFilter()
    const query = searchQuery().trim().toLowerCase()

    return tickets().filter((ticket) => {
      if (activeTab() !== 'all' && !ticketMatchesTab(ticket, activeTab())) return false
      if (filter.assigned === 'mine' && !ticket.owner) return false
      if (filter.assigned === 'unassigned' && ticket.owner) return false
      if (filter.channel && filter.channel !== 'all' && ticket.channel !== filter.channel) return false
      if (filter.queue === 'mentions' && !ticketMatchesMentions(ticket)) return false
      if (filter.queue === 'Admin Support' && ticket.group?.name !== 'Admin Support') return false
      if (filter.queue === 'spam' && ticket.state?.name.toLowerCase() !== 'spam') return false
      if (filter.agentState && filter.agentState !== 'all' && ticket.agentState !== filter.agentState) return false
      if (!query) return true

      return (
        ticket.title.toLowerCase().includes(query) ||
        ticket.number.includes(query) ||
        customerName(ticket).toLowerCase().includes(query) ||
        Boolean(ticket.customer?.email?.toLowerCase().includes(query)) ||
        Boolean(ticket.tags?.join(' ').toLowerCase().includes(query))
      )
    })
  })

  const loadTicketDetails = async (ticket: ZammadTicket) => {
    const live = ticket as LiveTicket
    setArticlesLoading(true)
    setNotice(null)
    setReplyText('')
    setSelectedTicket(live)
    setSentiment(null)
    setArticles([])

    try {
      const detail = await getConversationDetail(orgId(), live.conversationId)
      setArticles(detail.articles)
      setSelectedTicket(detail.ticket)
      mutateTickets((items) => (items ?? []).map((item) => (item.id === detail.ticket.id ? detail.ticket : item)))
    } catch (reason) {
      setNotice(reason instanceof Error ? reason.message : 'Conversation could not be loaded.')
    } finally {
      setArticlesLoading(false)
    }
  }

  const replaceTicket = (updated: LiveTicket) => {
    mutateTickets((items) => (items ?? []).map((item) => (item.id === updated.id ? updated : item)))
    setSelectedTicket(updated)
  }

  const patchSelectedTicket = async (patch: Record<string, unknown>) => {
    const current = selectedTicket()
    if (!current) return

    try {
      let updated: LiveTicket = current

      if (typeof patch.state_id === 'number') {
        updated = await setConversationStatus(orgId(), current.conversationId, statusFromStateId(patch.state_id))
      }

      if (typeof patch.owner_id === 'number') {
        // conversation-core models a single acting agent; assign to the current user.
        updated = await setConversationAssignment(
          orgId(),
          current.conversationId,
          ctx()?.userId ?? '',
          ctx()?.name || ctx()?.email || 'You',
        )
      }

      if (Array.isArray(patch.tags)) {
        const wanted = patch.tags.filter((tag): tag is string => typeof tag === 'string')
        const before = new Set(current.tags ?? [])
        const after = new Set(wanted)
        for (const tag of after) {
          if (!before.has(tag)) await addConversationTag(orgId(), current.conversationId, tag)
        }
        for (const tag of before) {
          if (!after.has(tag)) await removeConversationTag(orgId(), current.conversationId, tag)
        }
        updated = { ...updated, tags: wanted }
      }

      // priority/group have no conversation-core equivalent yet — reflect locally.
      if (typeof patch.priority_id === 'number') {
        updated = { ...updated, priority: priorityById(patch.priority_id) }
      }
      if (typeof patch.group_id === 'number') {
        const group = groups().find((candidate) => candidate.id === patch.group_id)
        if (group) updated = { ...updated, group }
      }

      replaceTicket({ ...updated, updated_at: new Date().toISOString() })
      setNotice('Conversation details updated.')
    } catch (reason) {
      setNotice(reason instanceof Error ? reason.message : 'Update could not be saved.')
    }
  }

  const addTag = (tag: string) => {
    const ticket = selectedTicket()
    const normalized = tag.trim()
    if (!ticket || !normalized || ticket.tags?.includes(normalized)) return
    void patchSelectedTicket({ tags: [...(ticket.tags ?? []), normalized] })
  }

  const removeTag = (tag: string) => {
    const ticket = selectedTicket()
    if (!ticket) return
    void patchSelectedTicket({ tags: (ticket.tags ?? []).filter((current) => current !== tag) })
  }

  const sendReply = async (text: string, internal: boolean) => {
    const body = text.trim()
    const ticket = selectedTicket()
    if (!body || !ticket || replySending()) return

    const optimistic: ZammadArticle = {
      id: -Date.now(),
      ticket_id: ticket.id,
      body,
      internal,
      sender: 'Agent',
      from: 'You',
      created_at: new Date().toISOString(),
    }

    setReplySending(true)
    setArticles((current) => [...current, optimistic])
    setReplyText('')

    try {
      const saved = await postReply(orgId(), ticket.conversationId, body, internal)
      setArticles((current) => current.map((article) => (article.id === optimistic.id ? saved : article)))
      setNotice(internal ? 'Internal note added.' : 'Reply sent.')
    } catch (reason) {
      setArticles((current) => current.filter((article) => article.id !== optimistic.id))
      setReplyText(body)
      setNotice(reason instanceof Error ? reason.message : 'Reply could not be sent.')
    } finally {
      setReplySending(false)
    }
  }

  const suggestReply = () => {
    if (!selectedTicket()) return
    setReplyText(demoQuickReplies[0] ?? '')
  }

  const createSocialFollowUp = async () => {
    const ticket = selectedTicket()
    if (!ticket || !orgId()) return
    setNotice(null)
    try {
      const latestArticle = [...articles()].reverse().find((article) => !article.internal)
      const result = await createSocialDraftFromInbox(orgId(), {
        ticketId: String(ticket.id),
        ticketTitle: ticket.title,
        customerName: customerName(ticket),
        channel: ticket.channel,
        excerpt: latestArticle?.body,
      })
      window.sessionStorage.setItem('velion.social.pendingDraft', JSON.stringify(result.post))
      navigate(`/social/calendar?source=inbox&ticketId=${ticket.id}`)
    } catch (reason) {
      setNotice(reason instanceof Error ? reason.message : 'Social draft could not be created.')
    }
  }

  return (
    <div class="velion-inbox-page">
      <div class="velion-inbox-workspace">
        <TicketQueue
          activeTab={activeTab()}
          error={ticketsRes.error instanceof Error ? ticketsRes.error.message : null}
          label={routeFilter().label}
          loading={ticketsRes.loading}
          onActiveTabChange={setActiveTab}
          onOpenModal={setModal}
          onSearchChange={setSearchQuery}
          onSelectTicket={(ticket) => void loadTicketDetails(ticket)}
          searchQuery={searchQuery()}
          selectedTicketId={selectedTicket()?.id ?? null}
          tickets={filteredTickets()}
        />

        <div class="velion-inbox-detail-grid">
          <ConversationPanel
            agents={agents()}
            articles={articles()}
            articlesLoading={articlesLoading()}
            groups={groups()}
            notice={notice()}
            onAddTag={addTag}
            onOpenModal={setModal}
            onPatchTicket={(patch) => void patchSelectedTicket(patch)}
            onRemoveTag={removeTag}
            onSendReply={(text, internal) => void sendReply(text, internal)}
            onCreateSocialFollowUp={() => void createSocialFollowUp()}
            onSuggestReply={suggestReply}
            replyText={replyText()}
            replySending={replySending()}
            selectedTicket={selectedTicket()}
            sentiment={sentiment()}
            setReplyText={setReplyText}
          />
          <InboxAside
            onInsertQuickReply={setReplyText}
            onMacroExecuted={() => setNotice('Macro executed.')}
            onOpenModal={setModal}
            selectedTicket={selectedTicket()}
          />
        </div>
      </div>
      <InboxWorkModal
        agents={agents()}
        groups={groups()}
        modal={modal()}
        onClose={() => setModal(null)}
        onInsertReply={setReplyText}
        onPatchTicket={(patch) => void patchSelectedTicket(patch)}
        onRefreshTicket={() => undefined}
        onSendReply={(text, internal) => void sendReply(text, internal)}
        selectedTicket={selectedTicket()}
      />
    </div>
  )
}

function ticketMatchesTab(ticket: ZammadTicket, tab: InboxTab) {
  const state = ticket.state?.name.toLowerCase() ?? ''
  if (tab === 'open') return state.includes('open') || state.includes('new')
  if (tab === 'pending') return state.includes('pending')
  if (tab === 'solved') return state.includes('closed') || state.includes('solved')
  return true
}

function ticketMatchesMentions(ticket: ZammadTicket) {
  const text = `${ticket.title} ${ticket.tags?.join(' ') ?? ''}`.toLowerCase()
  return text.includes('@') || text.includes('mention') || text.includes('urgent') || text.includes('vip')
}

function priorityById(id: number) {
  if (id === 1) return { id, name: 'low' }
  if (id === 3) return { id, name: 'high' }
  return { id: 2, name: 'normal' }
}
