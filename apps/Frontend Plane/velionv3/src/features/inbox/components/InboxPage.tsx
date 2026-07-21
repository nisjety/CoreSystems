import { useLocation, useNavigate } from '@solidjs/router'
import { createEffect, createMemo, createResource, createSignal, onCleanup, onMount } from 'solid-js'
import { ConversationPanel } from '@/features/inbox/components/ConversationPanel'
import { InboxAside, type RecentConversationRef } from '@/features/inbox/components/InboxAside'
import { InboxWorkModal, type InboxModalRequest } from '@/features/inbox/components/InboxWorkModal'
import { TicketQueue } from '@/features/inbox/components/TicketQueue'
import {
  customerName,
  FEEDBACK_TAG,
  resolveInboxRouteFilter,
  type Agent,
  type InboxTab,
  type TicketSentiment,
  type ZammadArticle,
  type ZammadTicket,
} from '@/features/inbox/lib/inbox-model'
import { runAssist } from '@/features/inbox/lib/inbox-ai'
import { createInboxLayout } from '@/features/inbox/lib/inbox-layout'
import { deriveConnectedInboxSources } from '@/features/inbox/lib/inbox-sources'
import { getAuthSession, getSessionContext } from '@/shared/api/auth-client'
import { extendInboxHistory, listConnections, type IntegrationConnection } from '@/shared/api/integrations-client'
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
import { createTicket, listTickets, type SupportTicket } from '@/shared/api/tickets-client'
import { getSession } from '@/shared/session/session-store'
import { translateApiError, useI18n } from '@/shared/i18n'

function mergeConversationPages(preferred: LiveTicket[], existing: LiveTicket[]): LiveTicket[] {
  const seen = new Set<string>()
  return [...preferred, ...existing].filter((ticket) => {
    if (seen.has(ticket.conversationId)) return false
    seen.add(ticket.conversationId)
    return true
  })
}

async function loadInboxContext() {
  const [session, ctx] = await Promise.all([getAuthSession(), getSessionContext()])
  return {
    email: session?.user.email ?? '',
    name: session?.user.name ?? '',
    orgId: ctx.orgId ?? ctx.orgs[0]?.id ?? '',
    userId: session?.user.id ?? '',
  }
}

function isTeamsInboxConnection(connection: IntegrationConnection): boolean {
  if (connection.providerKey !== 'microsoft' || !['active', 'needs_refresh'].includes(connection.status)) return false
  return connection.capabilities?.includes('teams.messages.read') === true
    || connection.scopes?.some((scope) => scope.toLowerCase() === 'channelmessage.read.all') === true
}

export default function InboxPage() {
  const i18n = useI18n()
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
  const [pendingReplyIntent, setPendingReplyIntent] = createSignal<{
    conversationId: string
    body: string
    internal: boolean
    key: string
  } | null>(null)
  const [searchQuery, setSearchQuery] = createSignal('')
  const [selectedTicket, setSelectedTicket] = createSignal<LiveTicket | null>(null)
  const [sentiment, setSentiment] = createSignal<TicketSentiment | null>(null)
  const [suggesting, setSuggesting] = createSignal(false)

  const session = getSession()
  const [ctx] = createResource(loadInboxContext)
  const orgId = createMemo(() => session.activeOrg?.id ?? ctx()?.orgId ?? '')
  const layout = createInboxLayout()

  // Channel is part of the server query so a provider lane cannot disappear
  // merely because its conversations fall outside a global 50-item window.
  const inboxQuery = createMemo(() => ({ orgId: orgId(), channel: routeFilter().channel }))
  const inboxQueryKey = createMemo(() => `${inboxQuery().orgId}:${inboxQuery().channel ?? 'all'}`)
  const [olderTickets, setOlderTickets] = createSignal<LiveTicket[]>([])
  const [nextConversationCursor, setNextConversationCursor] = createSignal<{ updated: string; id: string } | null>(null)
  const [olderTicketsLoading, setOlderTicketsLoading] = createSignal(false)
  let refreshRequestVersion = 0
  let paginationRequestVersion = 0
  const [ticketsRes, { mutate: mutateTickets }] = createResource(inboxQuery, ({ orgId: id, channel }) =>
    id
      ? listConversations(id, { limit: 100, channel }).then((result) => {
        if (olderTickets().length === 0) setNextConversationCursor(result.nextCursor)
        return result.tickets
      })
      : Promise.resolve([] as LiveTicket[]),
  )
  createEffect(() => {
    inboxQueryKey()
    refreshRequestVersion += 1
    paginationRequestVersion += 1
    setOlderTickets([])
    setNextConversationCursor(null)
    setOlderTicketsLoading(false)
  })
  const baseTickets = () => {
    const seen = new Set<string>()
    return [...(ticketsRes() ?? []), ...olderTickets()].filter((ticket) => {
      if (seen.has(ticket.conversationId)) return false
      seen.add(ticket.conversationId)
      return true
    })
  }
  const [supportTicketsRes, { mutate: mutateSupportTickets }] = createResource(orgId, (id) =>
    id ? listTickets(id, { limit: 100 }) : Promise.resolve([] as SupportTicket[]),
  )
  const supportTickets = () => {
    const value = supportTicketsRes()
    return Array.isArray(value) ? value : []
  }

  const loadOlderConversations = async () => {
    const query = inboxQuery()
    const teamsCandidates = inboxConnections().filter(isTeamsInboxConnection)
    const teamsConnection = teamsCandidates.find((connection) => connection.userId === ctx()?.userId)
      ?? teamsCandidates.find((connection) => !connection.userId)
    const shouldExtendTeamsHistory = query.channel === 'teams' && Boolean(teamsConnection)
    const tail = baseTickets().at(-1)
    const cursor = nextConversationCursor() ?? (shouldExtendTeamsHistory && tail
      ? { updated: tail.updated_at, id: tail.conversationId }
      : null)
    if ((!cursor && !shouldExtendTeamsHistory) || !query.orgId || olderTicketsLoading()) return
    const requestKey = inboxQueryKey()
    const requestVersion = ++paginationRequestVersion
    setOlderTicketsLoading(true)
    try {
      const paginationRequest = cursor
        ? listConversations(query.orgId, {
          limit: 100,
          channel: query.channel,
          cursorUpdated: cursor.updated,
          cursorId: cursor.id,
        })
        : Promise.resolve(null)
      const historyRequest = shouldExtendTeamsHistory && teamsConnection
        ? extendInboxHistory(query.orgId, teamsConnection.id)
        : Promise.resolve(null)
      const [pagination, history] = await Promise.allSettled([paginationRequest, historyRequest])
      if (requestKey !== inboxQueryKey() || requestVersion !== paginationRequestVersion) return

      if (pagination.status === 'fulfilled' && pagination.value) {
        const page = pagination.value
        setOlderTickets((current) => [...current, ...page.tickets])
        setNextConversationCursor(page.nextCursor)
      }
      if (history.status === 'fulfilled' && history.value) {
        const paginationWarning = pagination.status === 'rejected'
          ? i18n.tr(' Lagrede samtaler kunne ikke sideinndeles, men historikkforespørselen er aktiv.', ' Stored conversations could not be paged, but the history request is active.')
          : ''
        setNotice(i18n.tr(
          `Laster Microsoft Teams-samtaler fra de siste ${history.value.historyDays} dagene. Nye resultater vises automatisk.${paginationWarning}`,
          `Loading Microsoft Teams conversations from the last ${history.value.historyDays} days. New results will appear automatically.${paginationWarning}`,
        ))
      } else if (history.status === 'rejected') {
        setNotice(translateApiError(history.reason, i18n.tr, { no: 'Det neste Teams-historikkvinduet kunne ikke settes i kø.', en: 'The next Teams history window could not be queued.' }))
      } else if (pagination.status === 'rejected') {
        setNotice(translateApiError(pagination.reason, i18n.tr, { no: 'Eldre samtaler kunne ikke lastes.', en: 'Older conversations could not be loaded.' }))
      }
    } catch (reason) {
      if (requestKey === inboxQueryKey() && requestVersion === paginationRequestVersion) {
        setNotice(translateApiError(reason, i18n.tr, { no: 'Eldre samtaler kunne ikke lastes.', en: 'Older conversations could not be loaded.' }))
      }
    } finally {
      if (requestKey === inboxQueryKey() && requestVersion === paginationRequestVersion) setOlderTicketsLoading(false)
    }
  }
  const supportTicketByConversation = createMemo(() => new Map(supportTickets().map((ticket) => [ticket.conversation_id, ticket])))
  const tickets = () => baseTickets().map((ticket) => ({
    ...ticket,
    supportTicket: supportTicketByConversation().get(ticket.conversationId) ?? null,
  }))

  const [groupsRes, { mutate: mutateGroups }] = createResource(
    orgId,
    (id) => (id ? listInboxesAsGroups(id) : Promise.resolve([])),
  )
  const groups = () => groupsRes() ?? []
  const [inboxConnectionsRes, { mutate: mutateInboxConnections }] = createResource(
    orgId,
    (id) => (id ? listConnections(id) : Promise.resolve([] as IntegrationConnection[])),
  )
  const inboxConnections = () => inboxConnectionsRes() ?? []
  const inboxSources = createMemo(() => deriveConnectedInboxSources(inboxConnections()))

  let loadedOrgId = ''
  let orgGeneration = 0
  let detailRequestVersion = 0
  createEffect(() => {
    const nextOrgId = orgId()
    if (!nextOrgId || nextOrgId === loadedOrgId) return
    orgGeneration += 1
    detailRequestVersion += 1

    if (loadedOrgId) {
      // Never retain or render a prior tenant's resource values while the new
      // organization is loading. createResource ignores stale async results.
      mutateTickets([])
      mutateSupportTickets([])
      mutateGroups([])
      mutateInboxConnections([])
      setSelectedTicket(null)
      setArticles([])
      setArticlesLoading(false)
      setSentiment(null)
      setReplyText('')
      setReplySending(false)
      setPendingReplyIntent(null)
      setSuggesting(false)
      setNotice(null)
    }
    loadedOrgId = nextOrgId
  })

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
    detailRequestVersion += 1
    setRouteKey(location.search)
    setActiveTab(routeFilter().activeTab)
    setNotice(null)
    setReplyText('')
    setSelectedTicket(null)
    setSentiment(null)
    setArticles([])
    setArticlesLoading(false)
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
      if (filter.queue === 'feedback' && !ticket.tags?.includes(FEEDBACK_TAG)) return false
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
    const requestOrgId = orgId()
    const requestGeneration = orgGeneration
    const requestVersion = ++detailRequestVersion
    const isCurrentRequest = () => requestOrgId === orgId()
      && requestGeneration === orgGeneration
      && requestVersion === detailRequestVersion
    setArticlesLoading(true)
    setNotice(null)
    setReplyText('')
    setSelectedTicket(live)
    setSentiment(null)
    setArticles([])

    try {
      const detail = await getConversationDetail(requestOrgId, live.conversationId)
      if (!isCurrentRequest()) return
      const updatedTicket = {
        ...detail.ticket,
        supportTicket: supportTicketByConversation().get(detail.ticket.conversationId) ?? null,
      }
      setArticles(detail.articles)
      setSelectedTicket(updatedTicket)
      replaceTicketInPages(updatedTicket)
    } catch (reason) {
      if (!isCurrentRequest()) return
      setNotice(translateApiError(reason, i18n.tr, { no: 'Samtalen kunne ikke lastes.', en: 'Conversation could not be loaded.' }))
    } finally {
      if (isCurrentRequest()) setArticlesLoading(false)
    }
  }

  const replaceTicket = (updated: LiveTicket) => {
    // A local mutation must win over any list refresh that was launched before
    // the server acknowledged the action.
    refreshRequestVersion += 1
    replaceTicketInPages(updated)
    setSelectedTicket(updated)
  }

  const replaceTicketInPages = (updated: LiveTicket) => {
    const replace = (items: LiveTicket[]) => items.map((item) => (item.id === updated.id ? updated : item))
    mutateTickets((items) => replace(items ?? []))
    setOlderTickets(replace)
  }

  const refreshInbox = () => {
    if (!orgId() || document.visibilityState === 'hidden') return
    const query = inboxQuery()
    const requestKey = inboxQueryKey()
    const listRefreshVersion = ++refreshRequestVersion
    void listConversations(query.orgId, { limit: 100, channel: query.channel }).then((result) => {
      if (requestKey !== inboxQueryKey() || listRefreshVersion !== refreshRequestVersion) return
      if (olderTickets().length === 0 && !olderTicketsLoading()) {
        mutateTickets(result.tickets)
        setNextConversationCursor(result.nextCursor)
        return
      }
      // Preserve the already contiguous page chain. Replacing page one would
      // drop rows displaced by newly arrived mail while the older cursor still
      // points beyond them.
      mutateTickets((current) => mergeConversationPages(result.tickets, current ?? []))
    }).catch(() => {
      // Keep the last successful queue visible; the next focus/timer retries.
    })

    const current = selectedTicket()
    if (!current) return
    const requestOrgId = orgId()
    const requestGeneration = orgGeneration
    const requestVersion = ++detailRequestVersion
    void getConversationDetail(requestOrgId, current.conversationId).then((detail) => {
      if (
        requestOrgId !== orgId()
        || requestGeneration !== orgGeneration
        || requestVersion !== detailRequestVersion
        || selectedTicket()?.conversationId !== current.conversationId
      ) return
      const updatedTicket = {
        ...detail.ticket,
        supportTicket: supportTicketByConversation().get(detail.ticket.conversationId) ?? null,
      }
      setArticles(detail.articles)
      setSelectedTicket(updatedTicket)
      replaceTicketInPages(updatedTicket)
    }).catch(() => {
      // Background refresh is best-effort; the foreground loader owns user-facing errors.
    })
  }

  onMount(() => {
    const interval = window.setInterval(refreshInbox, 15_000)
    const handleFocus = () => refreshInbox()
    const handleVisibility = () => {
      if (document.visibilityState === 'visible') refreshInbox()
    }
    window.addEventListener('focus', handleFocus)
    document.addEventListener('visibilitychange', handleVisibility)
    onCleanup(() => {
      window.clearInterval(interval)
      window.removeEventListener('focus', handleFocus)
      document.removeEventListener('visibilitychange', handleVisibility)
    })
  })

  // Other conversations from the same contact (within the loaded window).
  const recentConversations = createMemo<RecentConversationRef[]>(() => {
    const current = selectedTicket()
    if (!current) return []
    const email = current.customer?.email?.toLowerCase()
    if (!email) return []
    return baseTickets()
      .filter((t) => t.conversationId !== current.conversationId && t.customer?.email?.toLowerCase() === email)
      .slice(0, 6)
      .map((t) => ({
        conversationId: t.conversationId,
        title: t.title,
        channel: t.channel ?? 'email',
        createdAt: t.created_at,
      }))
  })

  const selectRecentConversation = (conversationId: string) => {
    const target = baseTickets().find((t) => t.conversationId === conversationId)
    if (target) void loadTicketDetails(target)
  }

  const replaceSupportTicket = (ticket: SupportTicket) => {
    mutateSupportTickets((items) => {
      const current = items ?? []
      if (current.some((item) => item.id === ticket.id)) {
        return current.map((item) => (item.id === ticket.id ? ticket : item))
      }
      return [ticket, ...current]
    })
    setSelectedTicket((current) => current && current.conversationId === ticket.conversation_id
      ? { ...current, supportTicket: ticket }
      : current)
  }

  const patchSelectedTicket = async (patch: Record<string, unknown>) => {
    const current = selectedTicket()
    if (!current) return
    const requestOrgId = orgId()
    const requestGeneration = orgGeneration
    const isCurrentRequest = () => requestOrgId === orgId() && requestGeneration === orgGeneration

    try {
      let updated: LiveTicket = current

      if (typeof patch.state_id === 'number') {
        updated = await setConversationStatus(requestOrgId, current.conversationId, statusFromStateId(patch.state_id))
      }

      if (typeof patch.owner_id === 'number') {
        // conversation-core models a single acting agent; assign to the current user.
        updated = await setConversationAssignment(
          requestOrgId,
          current.conversationId,
          ctx()?.userId ?? '',
          ctx()?.name || ctx()?.email || i18n.tr('Deg', 'You'),
        )
      }

      if (Array.isArray(patch.tags)) {
        const wanted = patch.tags.filter((tag): tag is string => typeof tag === 'string')
        const before = new Set(current.tags ?? [])
        const after = new Set(wanted)
        for (const tag of after) {
          if (!before.has(tag)) await addConversationTag(requestOrgId, current.conversationId, tag)
        }
        for (const tag of before) {
          if (!after.has(tag)) await removeConversationTag(requestOrgId, current.conversationId, tag)
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

      if (!isCurrentRequest()) return
      replaceTicket({
        ...updated,
        supportTicket: current.supportTicket ?? supportTicketByConversation().get(current.conversationId) ?? null,
        updated_at: new Date().toISOString(),
      })
      setNotice(i18n.tr('Samtaledetaljene er oppdatert.', 'Conversation details updated.'))
    } catch (reason) {
      if (!isCurrentRequest()) return
      setNotice(translateApiError(reason, i18n.tr, { no: 'Oppdateringen kunne ikke lagres.', en: 'Update could not be saved.' }))
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
    const requestOrgId = orgId()
    const requestGeneration = orgGeneration
    const isCurrentRequest = () => requestOrgId === orgId() && requestGeneration === orgGeneration
    const pending = pendingReplyIntent()
    const intent = pending
      && pending.conversationId === ticket.conversationId
      && pending.body === body
      && pending.internal === internal
      ? pending
      : {
        conversationId: ticket.conversationId,
        body,
        internal,
        key: crypto.randomUUID(),
      }
    setPendingReplyIntent(intent)

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
      const saved = await postReply(requestOrgId, ticket.conversationId, body, internal, intent.key)
      if (!isCurrentRequest()) return
      setArticles((current) => current.map((article) => (article.id === optimistic.id ? saved : article)))
      setPendingReplyIntent(null)
      setNotice(internal ? i18n.tr('Internt notat lagt til.', 'Internal note added.') : i18n.tr('Svar sendt.', 'Reply submitted.'))
    } catch (reason) {
      if (!isCurrentRequest()) return
      setArticles((current) => current.filter((article) => article.id !== optimistic.id))
      setReplyText(body)
      setNotice(translateApiError(reason, i18n.tr, { no: 'Svaret kunne ikke sendes.', en: 'Reply could not be sent.' }))
    } finally {
      if (isCurrentRequest()) setReplySending(false)
    }
  }

  const suggestReply = async () => {
    const ticket = selectedTicket()
    if (!ticket || !orgId() || suggesting()) return
    const requestOrgId = orgId()
    const requestGeneration = orgGeneration
    const isCurrentRequest = () => requestOrgId === orgId() && requestGeneration === orgGeneration
    setSuggesting(true)
    try {
      const messages = articles().map((a) => ({
        agent: a.sender?.toLowerCase() === 'agent',
        from: a.from,
        body: a.bodyText || a.body || '',
      }))
      const res = await runAssist(requestOrgId, 'draft', messages, { customer: customerName(ticket) })
      if (!isCurrentRequest()) return
      if (res.text) setReplyText(res.text)
      else setNotice(i18n.tr('Velion returnerte et tomt utkast.', 'Velion returned an empty draft.'))
    } catch {
      if (!isCurrentRequest()) return
      setNotice(i18n.tr('Velion kunne ikke utarbeide et svar. Prøv igjen.', 'Velion could not draft a reply. Try again.'))
    } finally {
      if (isCurrentRequest()) setSuggesting(false)
    }
  }

  const createSocialFollowUp = async () => {
    const ticket = selectedTicket()
    if (!ticket || !orgId()) return
    const requestOrgId = orgId()
    const requestGeneration = orgGeneration
    const isCurrentRequest = () => requestOrgId === orgId() && requestGeneration === orgGeneration
    setNotice(null)
    try {
      const latestArticle = [...articles()].reverse().find((article) => !article.internal)
      const result = await createSocialDraftFromInbox(requestOrgId, {
        ticketId: String(ticket.id),
        ticketTitle: ticket.title,
        supportTicketId: ticket.supportTicket?.id,
        conversationId: ticket.conversationId,
        customerName: customerName(ticket),
        channel: ticket.channel,
        excerpt: latestArticle?.body,
      })
      if (!isCurrentRequest()) return
      window.sessionStorage.setItem('velion.social.pendingDraft', JSON.stringify(result.post))
      navigate(`/social/calendar?source=inbox&ticketId=${ticket.id}`)
    } catch (reason) {
      if (!isCurrentRequest()) return
      setNotice(translateApiError(reason, i18n.tr, { no: 'Sosialt utkast kunne ikke opprettes.', en: 'Social draft could not be created.' }))
    }
  }

  const createSupportTicket = async () => {
    const ticket = selectedTicket()
    if (!ticket || !orgId()) return
    const requestOrgId = orgId()
    const requestGeneration = orgGeneration
    const isCurrentRequest = () => requestOrgId === orgId() && requestGeneration === orgGeneration
    if (ticket.supportTicket) {
      navigate(`/tickets?ticketId=${ticket.supportTicket.id}`)
      return
    }
    setNotice(null)
    try {
      const supportTicket = await createTicket(requestOrgId, {
        conversation_id: ticket.conversationId,
        priority: ticket.priority?.name ?? 'normal',
        severity: ticket.priority?.name === 'high' ? 'high' : 'medium',
        category: ticket.tags?.[0] ?? '',
        intent: 'customer_follow_up',
        source: 'manual',
        created_by: ctx()?.userId ?? '',
      })
      if (!isCurrentRequest()) return
      replaceSupportTicket(supportTicket)
      setNotice(i18n.tr('Sak opprettet.', 'Ticket created.'))
    } catch (reason) {
      if (!isCurrentRequest()) return
      setNotice(translateApiError(reason, i18n.tr, { no: 'Saken kunne ikke opprettes.', en: 'Ticket could not be created.' }))
    }
  }

  const viewSupportTicket = () => {
    const supportTicket = selectedTicket()?.supportTicket
    if (!supportTicket) return
    navigate(`/tickets?ticketId=${supportTicket.id}`)
  }

  return (
    <div class="velion-inbox-page">
      <div class="velion-inbox-workspace" style={{ '--inbox-list-w': `${layout.listWidth()}px` }}>
        <div
          class="velion-inbox-resize-handle velion-inbox-resize-handle--list"
          role="separator"
          aria-orientation="vertical"
          aria-label={i18n.tr('Endre størrelse på samtalelisten', 'Resize conversation list')}
          onPointerDown={layout.startListResize}
          onDblClick={layout.resetWidths}
        />
        <TicketQueue
          activeTab={activeTab()}
          activeChannel={routeFilter().channel ?? null}
          connectedSources={inboxSources()}
          error={baseTickets().length === 0 && ticketsRes.error ? translateApiError(ticketsRes.error, i18n.tr, { no: 'Samtalene kunne ikke lastes.', en: 'Conversations could not be loaded.' }) : null}
          label={routeFilter().label}
          loading={ticketsRes.loading && baseTickets().length === 0}
          hasMore={Boolean(nextConversationCursor()) || (routeFilter().channel === 'teams' && inboxConnections().some(isTeamsInboxConnection))}
          loadingMore={olderTicketsLoading()}
          onActiveTabChange={setActiveTab}
          onOpenModal={setModal}
          onLoadMore={() => void loadOlderConversations()}
          onSearchChange={setSearchQuery}
          onSelectTicket={(ticket) => void loadTicketDetails(ticket)}
          searchQuery={searchQuery()}
          selectedTicketId={selectedTicket()?.id ?? null}
          tickets={filteredTickets()}
        />

        <div class="velion-inbox-detail-grid" style={{ '--inbox-aside-w': `${layout.asideWidth()}px` }}>
          <div
            class="velion-inbox-resize-handle velion-inbox-resize-handle--aside"
            role="separator"
            aria-orientation="vertical"
            aria-label={i18n.tr('Endre størrelse på kontekstpanelet', 'Resize context panel')}
            onPointerDown={layout.startAsideResize}
            onDblClick={layout.resetWidths}
          />
          <ConversationPanel
            agents={agents()}
            articles={articles()}
            articlesLoading={articlesLoading()}
            groups={groups()}
            notice={notice()}
            onAddTag={addTag}
            onCreateTicket={() => void createSupportTicket()}
            onOpenModal={setModal}
            onPatchTicket={(patch) => void patchSelectedTicket(patch)}
            onLinkExistingTicket={() => setModal({
              type: 'work',
              title: i18n.tr('Koble til eksisterende sak', 'Link existing ticket'),
              description: i18n.tr('Koble denne samtalen til en eksisterende sak og behold samtalen som meldingskilde.', 'Connect this conversation to an existing ticket and preserve the conversation as the message source.'),
              primaryAction: i18n.tr('Koble til sak', 'Link ticket'),
            })}
            onRemoveTag={removeTag}
            onSendReply={(text, internal) => void sendReply(text, internal)}
            onCreateSocialFollowUp={() => void createSocialFollowUp()}
            onSuggestReply={suggestReply}
            onViewTicket={viewSupportTicket}
            replyText={replyText()}
            replySending={replySending()}
            selectedTicket={selectedTicket()}
            sentiment={sentiment()}
            setReplyText={setReplyText}
          />
          <InboxAside
            orgId={orgId()}
            articles={articles()}
            recent={recentConversations()}
            onSelectRecent={selectRecentConversation}
            onInsertQuickReply={setReplyText}
            onMacroExecuted={() => setNotice(i18n.tr('Makro brukt på svaret.', 'Macro applied to reply.'))}
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
