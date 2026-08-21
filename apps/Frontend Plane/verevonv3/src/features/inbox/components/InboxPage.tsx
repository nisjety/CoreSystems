import { useLocation, useNavigate } from '@solidjs/router'
import { createEffect, createMemo, createSignal } from 'solid-js'
import { createResource } from '@/shared/lib/create-resource-compat'
import { ConversationPanel } from '@/features/inbox/components/ConversationPanel'
import { InboxAside, type RecentConversationRef } from '@/features/inbox/components/InboxAside'
import { InboxWorkModal, type InboxModalRequest, type InboxTicketLinkRequest } from '@/features/inbox/components/InboxWorkModal'
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
import { buildInboxAssistContext } from '@/features/inbox/lib/inbox-ai-context'
import type { SupportAIMode } from '@/shared/api/organization-client'
import { createInboxLayout } from '@/features/inbox/lib/inbox-layout'
import { deriveConnectedEmailAccounts, deriveConnectedInboxSources, isDiscordInboxChannelAwaitingSetup, isMetaInboxChannelAwaitingAssetProvision } from '@/features/inbox/lib/inbox-sources'
import { getAuthSession, getSessionContext } from '@/shared/api/auth-client'
import { extendInboxHistory, getSyncJob, listConnections, startConnectSession, triggerInboxSync, type IntegrationConnection, type SyncJob } from '@/shared/api/integrations-client'
import { runDirectOauthWindow } from '@/shared/integrations/provider-auth-window'
import {
  addConversationTag,
  createInternalNoteProposal,
  createDraftReplyProposal,
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
import {
  getInboxWorkspaceState,
  setInboxConversationPinned,
  setInboxConversationRead,
  type InboxWorkspaceState,
} from '@/shared/api/inbox-workspace-client'
import { listTicketTeams, listTickets, type SupportTicket, type TicketTeam } from '@/shared/api/tickets-client'
import { executeTicketCreate, executeTicketPatch, executeTicketResourceLink } from '@/features/tickets/lib/ticket-actions'
import { getSession } from '@/shared/session/session-store'
import { ApiError } from '@/shared/api/http'
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

function safeConnectionStatusMessage(reason: unknown): string | null {
  if (!(reason instanceof ApiError)) return null
  if (!['integration_error', 'integration_auth_unavailable', 'upstream_unavailable'].includes(reason.code ?? '')) return null
  const message = reason.message.trim()
  if (!message || message.length > 180 || /https?:\/\//i.test(message)) return null
  return message
}

async function waitForInboxSyncJob(orgId: string, jobID: string): Promise<SyncJob['status'] | 'waiting'> {
	const deadline = Date.now() + 30_000
	while (Date.now() < deadline) {
		const job = await getSyncJob(orgId, jobID)
		if (job.status === 'completed' || job.status === 'failed' || job.status === 'cancelled') return job.status
		await new Promise<void>((resolve) => window.setTimeout(resolve, 750))
	}
	return 'waiting'
}

export default function InboxPage() {
  const i18n = useI18n()
  const location = useLocation()
  const navigate = useNavigate()
  const routeFilter = createMemo(() => resolveInboxRouteFilter(new URLSearchParams(location.search)))
  const requestedConversationId = createMemo(() => new URLSearchParams(location.search).get('conversation_id')?.trim() ?? '')
  const [routeKey, setRouteKey] = createSignal<string | null>(null)
  const [activeTab, setActiveTab] = createSignal<InboxTab>('open')
  const [articles, setArticles] = createSignal<ZammadArticle[]>([])
  const [articlesLoading, setArticlesLoading] = createSignal(false)
  const [modal, setModal] = createSignal<InboxModalRequest | InboxTicketLinkRequest | null>(null)
  const [notice, setNotice] = createSignal<string | null>(null)
  const [replySending, setReplySending] = createSignal(false)
  const [replyText, setReplyText] = createSignal('')
  const [replyInternal, setReplyInternal] = createSignal(false)
  const [deliveryRefreshKey, setDeliveryRefreshKey] = createSignal(0)
  const [aiProposalRefreshKey, setAiProposalRefreshKey] = createSignal(0)
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
  const [connectingInboxProvider, setConnectingInboxProvider] = createSignal<'google' | 'microsoft' | null>(null)
	const [refreshingInbox, setRefreshingInbox] = createSignal(false)
	const [inboxRefreshNotice, setInboxRefreshNotice] = createSignal<string | null>(null)

  const session = getSession()
  const [ctx] = createResource(loadInboxContext)
	const [inboxWorkspace, { mutate: mutateInboxWorkspace }] = createResource(
		() => ctx()?.userId ?? '',
		(userId) => userId ? getInboxWorkspaceState() : Promise.resolve({ pinnedConversationIds: [], readConversationIds: [] } satisfies InboxWorkspaceState),
	)
  const orgId = createMemo(() => session.activeOrg?.id ?? ctx()?.orgId ?? '')
  const layout = createInboxLayout()

  // Channel is part of the server query so a provider lane cannot disappear
  // merely because its conversations fall outside a global 50-item window.
  const inboxQuery = createMemo(() => ({ orgId: orgId(), channel: routeFilter().channel, connectionId: routeFilter().connectionId }))
  const inboxQueryKey = createMemo(() => `${inboxQuery().orgId}:${inboxQuery().channel ?? 'all'}:${inboxQuery().connectionId ?? 'all'}`)
  const [olderTickets, setOlderTickets] = createSignal<LiveTicket[]>([])
  const [nextConversationCursor, setNextConversationCursor] = createSignal<{ updated: string; id: string } | null>(null)
  const [olderTicketsLoading, setOlderTicketsLoading] = createSignal(false)
  let refreshRequestVersion = 0
  let paginationRequestVersion = 0
  const [ticketsRes, { mutate: mutateTickets, refetch: refetchTickets }] = createResource(inboxQuery, ({ orgId: id, channel, connectionId }) =>
    id
      ? listConversations(id, { limit: 100, channel, connectionId }).then((result) => {
        if (olderTickets().length === 0) setNextConversationCursor(result.nextCursor)
        return result.tickets
      })
      : Promise.resolve([] as LiveTicket[]),
  )
  createEffect(
    () => inboxQueryKey(),
    () => {
      refreshRequestVersion += 1
      paginationRequestVersion += 1
      setOlderTickets([])
      setNextConversationCursor(null)
      setOlderTicketsLoading(false)
    },
  )
  const baseTickets = () => {
    const seen = new Set<string>()
    return [...(ticketsRes() ?? []), ...olderTickets()].filter((ticket) => {
      if (seen.has(ticket.conversationId)) return false
      seen.add(ticket.conversationId)
      return true
    })
  }
  const [supportTicketsRes, { mutate: mutateSupportTickets, refetch: refetchSupportTickets }] = createResource(orgId, (id) =>
    id ? listTickets(id, { limit: 100 }) : Promise.resolve([] as SupportTicket[]),
  )
	// Routing metadata augments an AI proposal, but it must never stop the
	// operator review ledger when the optional Ticketing-team directory is
	// temporarily unavailable during a rolling cross-plane deployment.
	const [ticketTeamsRes] = createResource(orgId, (id) =>
		id ? listTicketTeams(id).catch(() => [] as TicketTeam[]) : Promise.resolve([] as TicketTeam[]),
	)
	const ticketTeams = () => ticketTeamsRes() ?? []
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
          connectionId: query.connectionId,
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
  const supportTicketByConversation = createMemo(() => {
    const byConversation = new Map<string, SupportTicket>()
    for (const ticket of supportTickets()) {
      byConversation.set(ticket.conversation_id, ticket)
      for (const resource of ticket.linked_resources ?? []) {
        if (resource.resource_kind === 'conversation_source' && resource.resource_id) {
          byConversation.set(resource.resource_id, ticket)
        }
      }
    }
    return byConversation
  })
  const tickets = () => baseTickets().map((ticket) => ({
    ...ticket,
    supportTicket: supportTicketByConversation().get(ticket.conversationId) ?? null,
  }))

  const [groupsRes, { mutate: mutateGroups }] = createResource(
    orgId,
    (id) => (id ? listInboxesAsGroups(id) : Promise.resolve([])),
  )
  const groups = () => groupsRes() ?? []
  const [inboxConnectionsRes, { mutate: mutateInboxConnections, refetch: refetchInboxConnections }] = createResource(
    orgId,
    async (id) => {
      if (!id) return { connections: [] as IntegrationConnection[], unavailable: false, message: null }
      try {
        return { connections: await listConnections(id), unavailable: false, message: null }
      } catch (reason) {
        return { connections: [] as IntegrationConnection[], unavailable: true, message: safeConnectionStatusMessage(reason) }
      }
    },
  )
  const inboxConnections = () => inboxConnectionsRes()?.connections ?? []
  const inboxConnectionsUnavailable = () => inboxConnectionsRes()?.unavailable === true
  const inboxConnectionsStatusMessage = () => inboxConnectionsRes()?.message ?? null
  const inboxSources = createMemo(() => deriveConnectedInboxSources(inboxConnections()))
  const metaSetupRequired = createMemo<'instagram' | 'messenger' | 'whatsapp' | null>(() => {
    const channel = routeFilter().channel
    if (channel !== 'instagram' && channel !== 'messenger' && channel !== 'whatsapp') return null
    return isMetaInboxChannelAwaitingAssetProvision(inboxConnections(), channel) ? channel : null
  })
  const discordSetupRequired = createMemo(() => routeFilter().channel === 'discord' && isDiscordInboxChannelAwaitingSetup(inboxConnections()))
  const activeEmailAccount = createMemo(() => {
    const connectionId = routeFilter().connectionId
    return connectionId ? deriveConnectedEmailAccounts(inboxConnections()).find((account) => account.id === connectionId) ?? null : null
  })

  const clearActiveEmailAccount = () => {
    const params = new URLSearchParams(location.search)
    params.delete('connection_id')
    const search = params.toString()
    navigate(`${location.pathname}${search ? `?${search}` : ''}`)
  }

  const connectInbox = async (provider: 'google' | 'microsoft') => {
    const id = orgId()
    if (!id || connectingInboxProvider()) return
    setConnectingInboxProvider(provider)
    setNotice(null)
    try {
      // This is the scoped, user-initiated full inbox grant. OAuth ownership
      // remains in integration-core; Inbox never receives a provider token.
      const session = await startConnectSession(id, provider, { bundles: ['full'] })
      const connectUrl = session.connectUrl || session.redirectUrl
      const sessionToken = session.sessionToken || session.id
      if (!connectUrl || !sessionToken) throw new Error(i18n.tr('Tilkoblingen kunne ikke startes.', 'The connection could not be started.'))
      await runDirectOauthWindow({ connectUrl, sessionToken })
      await Promise.all([refetchInboxConnections(), refetchTickets()])
      setNotice(i18n.tr(
        provider === 'google'
          ? 'Gmail er tilkoblet. Nye samtaler vises her når innhentingen leverer dem.'
          : 'Outlook er tilkoblet. Nye samtaler vises her når innhentingen leverer dem.',
        provider === 'google'
          ? 'Gmail connected. New conversations appear here when ingestion delivers them.'
          : 'Outlook connected. New conversations appear here when ingestion delivers them.',
      ))
    } catch (reason) {
      setNotice(translateApiError(reason, i18n.tr, { no: 'Tilkoblingen kunne ikke fullføres.', en: 'The connection could not be completed.' }))
    } finally {
      setConnectingInboxProvider(null)
    }
  }

	const refreshInboxFromProvider = async (channel: 'email' | 'teams' | 'slack', connectionIDs: string[]) => {
		const id = orgId()
		const uniqueConnectionIDs = [...new Set(connectionIDs.filter(Boolean))]
		if (!id || uniqueConnectionIDs.length === 0 || refreshingInbox()) return
		setRefreshingInbox(true)
		setInboxRefreshNotice(null)
		try {
			const queued = await Promise.allSettled(uniqueConnectionIDs.map((connectionID) => triggerInboxSync(id, connectionID, channel)))
			const jobIDs = queued.flatMap((result) => result.status === 'fulfilled' && result.value.syncJob?.id ? [result.value.syncJob.id] : [])
			if (jobIDs.length === 0) {
				const failure = queued.find((result): result is PromiseRejectedResult => result.status === 'rejected')
				throw failure?.reason ?? new Error(i18n.tr('Oppdateringen kunne ikke settes i kø.', 'The refresh could not be queued.'))
			}
			const outcomes = await Promise.all(jobIDs.map((jobID) => waitForInboxSyncJob(id, jobID).catch(() => 'waiting' as const)))
			if (id !== orgId()) return
			await Promise.all([refetchInboxConnections(), refetchTickets()])
			const hasProviderFailure = outcomes.some((outcome) => outcome === 'failed' || outcome === 'cancelled')
			const hasCompletedProvider = outcomes.some((outcome) => outcome === 'completed')
			if (hasProviderFailure && hasCompletedProvider) {
				setInboxRefreshNotice(i18n.tr(
					'Noen leverandører ble oppdatert, men minst én mislyktes. Nye samtaler kan vises; kontroller tilkoblingsstatusen for kontoene som feilet.',
					'Some providers refreshed, but at least one failed. New conversations may appear; check the connection status for the failed accounts.',
				))
			} else if (hasProviderFailure) {
				setInboxRefreshNotice(i18n.tr(
					'Oppdateringen fra leverandøren mislyktes. Eksisterende samtaler er uendret; kontroller tilkoblingsstatusen før du prøver igjen.',
					'The provider refresh failed. Existing conversations are unchanged; check the connection status before trying again.',
				))
			} else if (outcomes.some((outcome) => outcome === 'waiting')) {
				setInboxRefreshNotice(i18n.tr(
					'Oppdateringen er fortsatt i kø hos leverandøren. Samtalelisten oppdateres automatisk når innhentingen er ferdig.',
					'The provider refresh is still queued. The conversation list updates automatically when ingestion finishes.',
				))
			} else {
				setInboxRefreshNotice(i18n.tr(
					'Oppdateringen fra leverandøren er fullført. Nye samtaler vises når den kanoniske køen er oppdatert.',
					'The provider refresh completed. New conversations appear when the canonical queue updates.',
				))
			}
		} catch (reason) {
			if (id === orgId()) {
				setInboxRefreshNotice(translateApiError(reason, i18n.tr, { no: 'Oppdatering fra leverandøren kunne ikke startes.', en: 'The provider refresh could not be started.' }))
			}
		} finally {
			if (id === orgId()) setRefreshingInbox(false)
		}
	}

  let loadedOrgId = ''
  let orgGeneration = 0
  let detailRequestVersion = 0
  createEffect(
    () => orgId(),
    (nextOrgId) => {
      if (!nextOrgId || nextOrgId === loadedOrgId) return
      orgGeneration += 1
      detailRequestVersion += 1

      if (loadedOrgId) {
        // Never retain or render a prior tenant's resource values while the new
        // organization is loading. createResource ignores stale async results.
        mutateTickets([])
        mutateSupportTickets([])
        mutateGroups([])
        mutateInboxConnections({ connections: [], unavailable: false, message: null })
        setSelectedTicket(null)
        setArticles([])
        setArticlesLoading(false)
        setSentiment(null)
        setReplyText('')
		setReplyInternal(false)
        setReplySending(false)
        setPendingReplyIntent(null)
        setSuggesting(false)
        setNotice(null)
		setInboxRefreshNotice(null)
      }
      loadedOrgId = nextOrgId
    },
  )

  const agents = createMemo<Agent[]>(() => {
    const profile = ctx()
    if (!profile) return []
    const [firstname, ...rest] = (profile.name || profile.email || 'Verevon Agent').split(' ')
    return [{
      id: 1,
      firstname: firstname || 'Verevon',
      lastname: rest.join(' ') || 'Agent',
      email: profile.email || 'agent@verevon.local',
    }]
  })

  createEffect(
    () => ({ current: routeKey(), search: location.search, activeTab: routeFilter().activeTab }),
    ({ current, search, activeTab }) => {
      if (current === search) return
      detailRequestVersion += 1
      setRouteKey(search)
      setActiveTab(activeTab)
      setNotice(null)
      setReplyText('')
		setReplyInternal(false)
      setSelectedTicket(null)
      setSentiment(null)
      setArticles([])
      setArticlesLoading(false)
    },
  )

  const filteredTickets = createMemo(() => {
    const filter = routeFilter()
    const query = searchQuery().trim().toLowerCase()

    const currentUserId = ctx()?.userId ?? ''
    return tickets().filter((ticket) => {
      if (activeTab() !== 'all' && !ticketMatchesTab(ticket, activeTab())) return false
      // "Your inbox" is the actionable queue: conversations assigned to you PLUS
      // unassigned incoming ones you can pick up. Only conversations owned by a
      // *different* agent are hidden. Without this, a fresh support inbox whose
      // provider messages (email/Teams/etc.) all arrive unassigned looks empty.
      if (filter.assigned === 'mine' && ticket.owner && ticket.assigneeUserId !== currentUserId) return false
      if (filter.assigned === 'unassigned' && ticket.owner) return false
      if (filter.channel && filter.channel !== 'all' && ticket.channel !== filter.channel) return false
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
		setReplyInternal(false)
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

  let openedConversationKey = ''
  createEffect(
    // compute: gather every tracked read up front, including the `listed`
    // lookup (a pure derivation over baseTickets()), so the effect below is
    // free to run untracked.
    () => {
      const conversationId = requestedConversationId()
      const requestOrgId = orgId()
      const listed = conversationId && requestOrgId
        ? baseTickets().find((ticket) => ticket.conversationId === conversationId)
        : undefined
      return { conversationId, requestOrgId, listed }
    },
    ({ conversationId, requestOrgId, listed }) => {
      if (!conversationId || !requestOrgId) {
        openedConversationKey = ''
        return
      }
      const key = `${requestOrgId}:${conversationId}`
      if (openedConversationKey === key) return
      openedConversationKey = key

      if (listed) {
        void loadTicketDetails(listed)
        return
      }

      // A global review item may refer to a conversation outside the current list
      // page. Hydrate only that canonical record rather than pretending it is
      // absent or fetching unrelated customer content.
      void getConversationDetail(requestOrgId, conversationId).then((detail) => {
        if (requestedConversationId() !== conversationId || requestOrgId !== orgId()) return
        const ticket = {
          ...detail.ticket,
          supportTicket: supportTicketByConversation().get(detail.ticket.conversationId) ?? null,
        }
        setSelectedTicket(ticket)
        setArticles(detail.articles)
        setArticlesLoading(false)
        replaceTicketInPages(ticket)
      }).catch((reason) => {
        if (requestedConversationId() !== conversationId || requestOrgId !== orgId()) return
        setNotice(translateApiError(reason, i18n.tr, { no: 'Samtalen fra gjennomgangskøen kunne ikke lastes.', en: 'The review-queue conversation could not be loaded.' }))
      })
    },
  )

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
    void listConversations(query.orgId, {
      limit: 100,
      channel: query.channel,
      connectionId: query.connectionId,
    }).then((result) => {
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

  createEffect(
    () => undefined,
    // Two-phase effects run their effect function outside any owner context
    // (it fires from the queue drain, not the tracked compute), so onCleanup()
    // here would silently no-op (NO_OWNER_CLEANUP) and leak these listeners
    // across every mount. Returning the cleanup function is the mechanism v2
    // actually wires up: runEffect stores the return value and invokes it
    // before the next run / at disposal.
    () => {
    const interval = window.setInterval(refreshInbox, 15_000)
    const handleFocus = () => refreshInbox()
    const handleVisibility = () => {
      if (document.visibilityState === 'visible') refreshInbox()
    }
		const handleQueueNavigation = (event: KeyboardEvent) => {
			const target = event.target as HTMLElement | null
			const isTyping = target?.tagName === 'INPUT' || target?.tagName === 'TEXTAREA' || target?.isContentEditable
			if (isTyping || event.metaKey || event.ctrlKey || event.altKey || document.querySelector('[role="dialog"]')) return
			if (event.key === '/') {
				const search = document.getElementById('verevon-inbox-search')
				if (search instanceof HTMLInputElement) {
					event.preventDefault()
					search.focus()
				}
				return
			}
			if (event.key !== 'j' && event.key !== 'k') return
			const queue = filteredTickets()
			if (queue.length === 0) return
			event.preventDefault()
			const currentIndex = queue.findIndex((ticket) => ticket.id === selectedTicket()?.id)
			const nextIndex = event.key === 'j'
				? Math.min(currentIndex < 0 ? 0 : currentIndex + 1, queue.length - 1)
				: Math.max(currentIndex < 0 ? queue.length - 1 : currentIndex - 1, 0)
			void loadTicketDetails(queue[nextIndex]!)
		}
    window.addEventListener('focus', handleFocus)
    document.addEventListener('visibilitychange', handleVisibility)
		window.addEventListener('keydown', handleQueueNavigation)
    return () => {
      window.clearInterval(interval)
      window.removeEventListener('focus', handleFocus)
      document.removeEventListener('visibilitychange', handleVisibility)
		window.removeEventListener('keydown', handleQueueNavigation)
    }
    },
  )

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

  const refreshSelectedSupportTicket = async () => {
    const requestedOrgId = orgId()
    const conversationId = selectedTicket()?.conversationId
    if (!requestedOrgId || !conversationId) return
    const refreshed = await refetchSupportTickets()
    if (requestedOrgId !== orgId() || selectedTicket()?.conversationId !== conversationId) return
    const canonicalTicket = (refreshed ?? []).find((ticket) => ticket.conversation_id === conversationId)
    if (!canonicalTicket) return
    setSelectedTicket((current) => current?.conversationId === conversationId
      ? { ...current, supportTicket: canonicalTicket }
      : current)
  }

  const patchSelectedTicket = async (patch: Record<string, unknown>) => {
    const current = selectedTicket()
    if (!current) return
    const requestOrgId = orgId()
    const requestGeneration = orgGeneration
    // Selection guard: snapshot the current conversation's request epoch so a
    // result that resolves after the agent has opened a different conversation
    // (or the route/org changed) is dropped instead of writing onto the newly
    // selected one — e.g. an AI draft, a status/tag patch, or a sent reply
    // landing on the wrong customer. loadTicketDetails bumps detailRequestVersion
    // on every selection change; these in-conversation actions only read it.
    const requestVersion = detailRequestVersion
    const isCurrentRequest = () => requestOrgId === orgId()
      && requestGeneration === orgGeneration
      && requestVersion === detailRequestVersion

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

  const sendReply = async (text: string, internal: boolean): Promise<boolean> => {
    const body = text.trim()
    const ticket = selectedTicket()
    if (!body || !ticket || replySending()) return false
    const requestOrgId = orgId()
    const requestGeneration = orgGeneration
    // Selection guard: snapshot the current conversation's request epoch so a
    // result that resolves after the agent has opened a different conversation
    // (or the route/org changed) is dropped instead of writing onto the newly
    // selected one — e.g. an AI draft, a status/tag patch, or a sent reply
    // landing on the wrong customer. loadTicketDetails bumps detailRequestVersion
    // on every selection change; these in-conversation actions only read it.
    const requestVersion = detailRequestVersion
    const isCurrentRequest = () => requestOrgId === orgId()
      && requestGeneration === orgGeneration
      && requestVersion === detailRequestVersion
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
		setReplyInternal(false)

    try {
      const saved = await postReply(requestOrgId, ticket.conversationId, body, internal, intent.key)
      if (!isCurrentRequest()) return false
      setArticles((current) => current.map((article) => (article.id === optimistic.id ? saved : article)))
      setDeliveryRefreshKey((current) => current + 1)
      setPendingReplyIntent(null)
      setNotice(internal ? i18n.tr('Internt notat lagt til.', 'Internal note added.') : i18n.tr('Svar sendt.', 'Reply submitted.'))
      return true
    } catch (reason) {
      if (!isCurrentRequest()) return false
      setArticles((current) => current.filter((article) => article.id !== optimistic.id))
      setReplyText(body)
      setNotice(translateApiError(reason, i18n.tr, { no: 'Svaret kunne ikke sendes.', en: 'Reply could not be sent.' }))
      return false
    } finally {
      if (isCurrentRequest()) setReplySending(false)
    }
  }

  const queueDraftReply = async (ticket: ZammadTicket, body: string, zdr: boolean, supportAiMode: SupportAIMode = 'review', proposalGroupId?: string): Promise<boolean> => {
    const conversationId = (ticket as ZammadTicket & { conversationId?: string }).conversationId
    const text = body.trim()
    if (!conversationId || !text) {
      setNotice(i18n.tr('Verevon returnerte et tomt utkast.', 'Verevon returned an empty draft.'))
      return false
    }
    if (zdr) {
		setReplyInternal(false)
      setReplyText(text)
      setNotice(i18n.tr(
        'ZDR er aktiv: utkastet finnes bare i dette svarfeltet og blir ikke lagret i AI-gjennomgangskøen.',
        'ZDR is active: this draft exists only in this reply field and is not retained in the AI review queue.',
      ))
      return true
    }
    if (supportAiMode !== 'review') {
		setReplyInternal(false)
      setReplyText(text)
      setNotice(i18n.tr(
        'Assistentmodus er aktiv: utkastet er satt inn i svarfeltet, men ikke lagret i AI-gjennomgangskøen.',
        'Assist mode is active: the draft was inserted into the reply field but was not retained in the AI review queue.',
      ))
      return true
    }
    try {
      await createDraftReplyProposal({ conversationId, bodyText: text, proposalGroupId })
      setAiProposalRefreshKey((current) => current + 1)
      setNotice(i18n.tr(
        'AI-svarforslaget er lagret for gjennomgang. Bekreft den nøyaktige teksten over før det kan sendes.',
        'The AI reply proposal is saved for review. Confirm the exact text above before it can be sent.',
      ))
      return true
    } catch (reason) {
      setNotice(translateApiError(reason, i18n.tr, {
        no: 'Kunne ikke lagre AI-svarforslaget for gjennomgang.',
        en: 'Could not save the AI reply proposal for review.',
      }))
      return false
    }
  }

  const queueInternalNote = async (ticket: ZammadTicket, body: string, zdr: boolean, supportAiMode: SupportAIMode = 'review', proposalGroupId?: string): Promise<boolean> => {
    const conversationId = (ticket as ZammadTicket & { conversationId?: string }).conversationId
    const text = body.trim()
    if (!conversationId || !text || zdr) {
      setNotice(i18n.tr('ZDR eller manglende innhold hindrer lagring av et AI-notat.', 'ZDR or missing content prevents retaining an AI note.'))
      return false
    }
    if (supportAiMode !== 'review') {
		setReplyInternal(true)
      setReplyText(text)
      setNotice(i18n.tr(
        'Assistentmodus er aktiv: notatutkastet er satt inn i skrivefeltet, men ikke lagret i AI-gjennomgangskøen.',
        'Assist mode is active: the note draft was inserted into the composer but was not retained in the AI review queue.',
      ))
      return true
    }
    try {
      await createInternalNoteProposal({ conversationId, bodyText: text, proposalGroupId })
      setAiProposalRefreshKey((current) => current + 1)
      setNotice(i18n.tr('AI-notatet er lagret for gjennomgang. Det forblir internt etter godkjenning.', 'The AI note is saved for review. It remains internal after approval.'))
      return true
    } catch (reason) {
      setNotice(translateApiError(reason, i18n.tr, { no: 'Kunne ikke lagre AI-notatet for gjennomgang.', en: 'Could not save the AI note for review.' }))
      return false
    }
  }

  const suggestReply = async (instruction?: string): Promise<boolean> => {
    const ticket = selectedTicket()
    if (!ticket || !orgId() || suggesting()) return false
    const requestOrgId = orgId()
    const requestGeneration = orgGeneration
    // Selection guard: snapshot the current conversation's request epoch so a
    // result that resolves after the agent has opened a different conversation
    // (or the route/org changed) is dropped instead of writing onto the newly
    // selected one — e.g. an AI draft, a status/tag patch, or a sent reply
    // landing on the wrong customer. loadTicketDetails bumps detailRequestVersion
    // on every selection change; these in-conversation actions only read it.
    const requestVersion = detailRequestVersion
    const isCurrentRequest = () => requestOrgId === orgId()
      && requestGeneration === orgGeneration
      && requestVersion === detailRequestVersion
    setSuggesting(true)
    try {
      const messages = articles().map((a) => ({
        agent: a.sender?.toLowerCase() === 'agent',
        from: a.from,
        body: a.bodyText || a.body || '',
      }))
      const res = await runAssist(requestOrgId, 'draft', messages, {
        customer: customerName(ticket),
        instruction: instruction?.trim() || undefined,
        contextPack: buildInboxAssistContext({
          selectedTicket: ticket,
          visibleTickets: filteredTickets(),
          filter: routeFilter(),
          draftInput: replyText(),
          orgId: requestOrgId,
        }),
      })
      if (!isCurrentRequest()) return false
      if (res.text) {
        return queueDraftReply(ticket, res.text, res.zdr, res.supportAiMode)
      }
      setNotice(i18n.tr('Verevon returnerte et tomt utkast.', 'Verevon returned an empty draft.'))
      return false
    } catch {
      if (!isCurrentRequest()) return false
      setNotice(i18n.tr('Verevon kunne ikke utarbeide et svar. Prøv igjen.', 'Verevon could not draft a reply. Try again.'))
      return false
    } finally {
      if (isCurrentRequest()) setSuggesting(false)
    }
  }

  const createSocialFollowUp = async () => {
    const ticket = selectedTicket()
    if (!ticket || !orgId()) return
    const requestOrgId = orgId()
    const requestGeneration = orgGeneration
    // Selection guard: snapshot the current conversation's request epoch so a
    // result that resolves after the agent has opened a different conversation
    // (or the route/org changed) is dropped instead of writing onto the newly
    // selected one — e.g. an AI draft, a status/tag patch, or a sent reply
    // landing on the wrong customer. loadTicketDetails bumps detailRequestVersion
    // on every selection change; these in-conversation actions only read it.
    const requestVersion = detailRequestVersion
    const isCurrentRequest = () => requestOrgId === orgId()
      && requestGeneration === orgGeneration
      && requestVersion === detailRequestVersion
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
      window.sessionStorage.setItem('verevon.social.pendingDraft', JSON.stringify(result.post))
      navigate(`/social/calendar?source=inbox&ticketId=${ticket.id}`)
    } catch (reason) {
      if (!isCurrentRequest()) return
      setNotice(translateApiError(reason, i18n.tr, { no: 'Sosialt utkast kunne ikke opprettes.', en: 'Social draft could not be created.' }))
    }
  }

  const createSupportTicket = async () => {
    const ticket = selectedTicket()
    const userId = ctx()?.userId
    if (!ticket || !orgId() || !userId) return
    const requestOrgId = orgId()
    const requestGeneration = orgGeneration
    // Selection guard: snapshot the current conversation's request epoch so a
    // result that resolves after the agent has opened a different conversation
    // (or the route/org changed) is dropped instead of writing onto the newly
    // selected one — e.g. an AI draft, a status/tag patch, or a sent reply
    // landing on the wrong customer. loadTicketDetails bumps detailRequestVersion
    // on every selection change; these in-conversation actions only read it.
    const requestVersion = detailRequestVersion
    const isCurrentRequest = () => requestOrgId === orgId()
      && requestGeneration === orgGeneration
      && requestVersion === detailRequestVersion
    if (ticket.supportTicket) {
      navigate(ticketDetailHref(ticket.supportTicket.id))
      return
    }
    setNotice(null)
    try {
      const supportTicket = await executeTicketCreate({
        type: 'human',
        orgId: requestOrgId,
        userId,
      }, {
        conversation_id: ticket.conversationId,
        priority: ticket.priority?.name ?? 'normal',
        severity: ticket.priority?.name === 'high' ? 'high' : 'medium',
        category: ticket.tags?.[0] ?? '',
        intent: 'customer_follow_up',
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
    navigate(ticketDetailHref(supportTicket.id))
  }

  const ticketDetailHref = (ticketId: string) => location.pathname.startsWith('/support')
    ? `/support?surface=tickets&ticketId=${encodeURIComponent(ticketId)}`
    : `/tickets?ticketId=${encodeURIComponent(ticketId)}`

  const snoozeConversationTicket = async (ticket: ZammadTicket) => {
    const supportTicket = ticket.supportTicket
    const userId = ctx()?.userId
    if (!supportTicket || !orgId() || !userId) {
      setNotice(i18n.tr('Opprett eller åpne en sak før samtalen kan utsettes.', 'Create or open a ticket before this conversation can be snoozed.'))
      return
    }
    try {
      const updated = await executeTicketPatch(
        { type: 'human', orgId: orgId(), userId },
        supportTicket,
        { status: 'snoozed', snoozed_until: new Date(Date.now() + 24 * 60 * 60 * 1000).toISOString() },
      )
      replaceSupportTicket(updated)
      setNotice(i18n.tr('Saken er utsatt i 24 timer.', 'Ticket snoozed for 24 hours.'))
    } catch (reason) {
      setNotice(translateApiError(reason, i18n.tr, { no: 'Saken kunne ikke utsettes.', en: 'The ticket could not be snoozed.' }))
    }
  }

  const saveInboxWorkspace = async (
    ticket: ZammadTicket,
    operation: (conversationId: string, enabled: boolean) => Promise<InboxWorkspaceState>,
    enabled: boolean,
    successMessage: string,
    failureMessage: string,
  ) => {
    const conversationId = ticket.conversationId
    if (!conversationId) {
      setNotice(i18n.tr('Samtalen mangler en verifiserbar ID; ingen personlig innboksinnstilling ble endret.', 'This conversation has no verifiable ID, so no personal Inbox preference was changed.'))
      return
    }
    try {
      mutateInboxWorkspace(await operation(conversationId, enabled))
      setNotice(successMessage)
    } catch (reason) {
      setNotice(translateApiError(reason, i18n.tr, { no: failureMessage, en: failureMessage }))
    }
  }

  const toggleConversationPinned = async (ticket: ZammadTicket) => {
    const conversationId = ticket.conversationId
    const enabled = conversationId ? !((inboxWorkspace()?.pinnedConversationIds ?? []).includes(conversationId)) : false
    await saveInboxWorkspace(
      ticket,
      setInboxConversationPinned,
      enabled,
      enabled
        ? i18n.tr('Samtalen er festet i din personlige innboksvisning.', 'Conversation pinned in your personal Inbox view.')
        : i18n.tr('Samtalen er løsnet fra din personlige innboksvisning.', 'Conversation unpinned from your personal Inbox view.'),
      i18n.tr('Den personlige festingen kunne ikke lagres.', 'The personal pin could not be saved.'),
    )
  }

  const markConversationRead = async (ticket: ZammadTicket) => {
    const conversationId = ticket.conversationId
    if (!conversationId || (inboxWorkspace()?.readConversationIds ?? []).includes(conversationId)) return
    await saveInboxWorkspace(
      ticket,
      setInboxConversationRead,
      true,
      i18n.tr('Samtalen er markert som lest i din personlige innboksvisning.', 'Conversation marked read in your personal Inbox view.'),
      i18n.tr('Lesestatusen kunne ikke lagres.', 'The read state could not be saved.'),
    )
  }

  const resolveSelectedSupportTicket = async () => {
    const ticket = selectedTicket()
    const supportTicket = ticket?.supportTicket
    const userId = ctx()?.userId
    if (!supportTicket || !orgId() || !userId) return
    try {
      const updated = await executeTicketPatch(
        { type: 'human', orgId: orgId(), userId },
        supportTicket,
        { status: 'resolved' },
      )
      replaceSupportTicket(updated)
      setNotice(i18n.tr('Saken er løst og verifisert på nytt.', 'Ticket resolved and reread from the source of truth.'))
    } catch (reason) {
      // executeTicketPatch can fail (e.g. a transient 502) after the status
      // change was already durably recorded server-side. Re-fetch the real
      // ticket state before asserting failure, instead of trusting the
      // network error alone — otherwise a user sees a false "could not be
      // resolved" error for a resolution that already went through.
      let reconciled = true
      try {
        await refreshSelectedSupportTicket()
      } catch {
        reconciled = false
      }
      const latestStatus = selectedTicket()?.supportTicket?.status
      if (reconciled && latestStatus === 'resolved') {
        setNotice(i18n.tr('Saken er løst og verifisert på nytt.', 'Ticket resolved and reread from the source of truth.'))
      } else if (reconciled && latestStatus) {
        setNotice(translateApiError(reason, i18n.tr, { no: 'Saken kunne ikke løses.', en: 'The ticket could not be resolved.' }))
      } else {
        setNotice(i18n.tr(
          'Vi fikk ikke bekreftet om saken ble løst. Vent litt før du prøver på nytt.',
          "We couldn't confirm whether the ticket was resolved. Please wait a moment before trying again.",
        ))
      }
    }
  }

  const linkSelectedConversationToTicket = async (target: SupportTicket): Promise<boolean> => {
    const source = selectedTicket()
    const userId = ctx()?.userId
    if (!source?.conversationId || !orgId() || !userId) {
      setNotice(i18n.tr('Samtalen mangler en verifiserbar ID; ingen sak ble koblet.', 'The conversation has no verifiable ID, so no ticket was attached.'))
      return false
    }
    try {
      const updated = await executeTicketResourceLink(
        { type: 'human', orgId: orgId(), userId },
        target,
        {
          resource_kind: 'conversation_source',
          resource_id: source.conversationId,
          label: source.title,
        },
      )
      mutateSupportTickets((current) => (current ?? []).map((ticket) => ticket.id === updated.id ? updated : ticket))
      setModal(null)
      setNotice(i18n.tr('Samtalen er koblet til saken og verifisert på nytt.', 'Conversation attached to the ticket and reread from the source of truth.'))
      return true
    } catch (reason) {
      setNotice(translateApiError(reason, i18n.tr, { no: 'Samtalen kunne ikke kobles til saken.', en: 'Conversation could not be attached to the ticket.' }))
      return false
    }
  }

  return (
    <div class="verevon-inbox-page">
      <div class="verevon-inbox-workspace" style={{ '--inbox-list-w': `${layout.listWidth()}px` }}>
        <div
          class="verevon-inbox-resize-handle verevon-inbox-resize-handle--list"
          role="separator"
          aria-orientation="vertical"
          aria-label={i18n.tr('Endre størrelse på samtalelisten', 'Resize conversation list')}
          onPointerDown={layout.startListResize}
          onDblClick={layout.resetWidths}
        />
        <TicketQueue
          activeTab={activeTab()}
          activeChannel={routeFilter().channel ?? null}
          activeEmailAccount={activeEmailAccount()}
          connectingInboxProvider={connectingInboxProvider()}
          connectionStatusMessage={inboxConnectionsStatusMessage()}
          connectionStatusUnavailable={inboxConnectionsUnavailable()}
          connectedSources={inboxSources()}
          error={baseTickets().length === 0 && ticketsRes.error ? translateApiError(ticketsRes.error, i18n.tr, { no: 'Samtalene kunne ikke lastes.', en: 'Conversations could not be loaded.' }) : null}
          label={routeFilter().label}
          loading={ticketsRes.loading && baseTickets().length === 0}
          metaSetupRequired={metaSetupRequired()}
          discordSetupRequired={discordSetupRequired()}
          hasMore={Boolean(nextConversationCursor()) || (routeFilter().channel === 'teams' && inboxConnections().some(isTeamsInboxConnection))}
          loadingMore={olderTicketsLoading()}
          onActiveTabChange={setActiveTab}
          onAddSharedMailbox={() => navigate('/settings/integrations#shared-mailboxes')}
          onClearActiveEmailAccount={clearActiveEmailAccount}
          onConnectInbox={orgId() ? (provider) => void connectInbox(provider) : undefined}
			onRefreshInbox={orgId() ? (channel, connectionIDs) => void refreshInboxFromProvider(channel, connectionIDs) : undefined}
          onRetryConnectionStatus={() => void refetchInboxConnections()}
          onLoadMore={() => void loadOlderConversations()}
          onMarkRead={(ticket) => void markConversationRead(ticket)}
          onSearchChange={setSearchQuery}
          onSelectTicket={(ticket) => void loadTicketDetails(ticket)}
          onSnoozeTicket={(ticket) => void snoozeConversationTicket(ticket)}
          onTogglePinned={(ticket) => void toggleConversationPinned(ticket)}
          pinnedConversationIds={inboxWorkspace()?.pinnedConversationIds ?? []}
			providerRefreshNotice={inboxRefreshNotice()}
			refreshingInbox={refreshingInbox()}
          readConversationIds={inboxWorkspace()?.readConversationIds ?? []}
          searchQuery={searchQuery()}
          selectedTicketId={selectedTicket()?.id ?? null}
          tickets={filteredTickets()}
        />

        <div class="verevon-inbox-detail-grid" style={{ '--inbox-aside-w': `${layout.asideWidth()}px` }}>
          <div
            class="verevon-inbox-resize-handle verevon-inbox-resize-handle--aside"
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
            aiProposalRefreshKey={aiProposalRefreshKey()}
            deliveryRefreshKey={deliveryRefreshKey()}
            groups={groups()}
            isPinned={Boolean(selectedTicket()?.conversationId && (inboxWorkspace()?.pinnedConversationIds ?? []).includes(selectedTicket()!.conversationId!))}
            notice={notice()}
            orgId={orgId()}
            userId={ctx()?.userId ?? ''}
            onAddTag={addTag}
            onCreateTicket={() => void createSupportTicket()}
            onOpenModal={setModal}
            onPatchTicket={(patch) => void patchSelectedTicket(patch)}
            onTicketActionVerified={() => void refreshSelectedSupportTicket()}
            onTogglePinned={() => {
              const ticket = selectedTicket()
              if (ticket) void toggleConversationPinned(ticket)
            }}
            onLinkExistingTicket={() => setModal({
              type: 'link-ticket',
              conversationId: selectedTicket()?.conversationId ?? '',
              title: i18n.tr('Koble til eksisterende sak', 'Link existing ticket'),
            })}
            onRemoveTag={removeTag}
            onResolveTicket={() => void resolveSelectedSupportTicket()}
            onSendReply={sendReply}
            onCreateSocialFollowUp={() => void createSocialFollowUp()}
            onSuggestReply={suggestReply}
            onViewTicket={viewSupportTicket}
            replyText={replyText()}
			isInternal={replyInternal()}
            replySending={replySending()}
            selectedTicket={selectedTicket()}
            sentiment={sentiment()}
            setReplyText={setReplyText}
			setIsInternal={setReplyInternal}
			ticketTeams={ticketTeams()}
          />
          <InboxAside
            orgId={orgId()}
            articles={articles()}
            recent={recentConversations()}
            onSelectRecent={selectRecentConversation}
            onQueueDraftReply={(text, zdr, supportAiMode, proposalGroupId) => {
              const ticket = selectedTicket()
              return ticket ? queueDraftReply(ticket, text, zdr, supportAiMode, proposalGroupId) : Promise.resolve(false)
            }}
            onQueueInternalNote={(text, zdr, supportAiMode, proposalGroupId) => {
              const ticket = selectedTicket()
              return ticket ? queueInternalNote(ticket, text, zdr, supportAiMode, proposalGroupId) : Promise.resolve(false)
            }}
            onMacroExecuted={(ticket) => {
              replaceSupportTicket(ticket)
              setNotice(i18n.tr('Makroen ble brukt og saken ble verifisert på nytt.', 'Macro applied and ticket reread from the source of truth.'))
            }}
            onOpenModal={setModal}
            onTriageProposed={() => setAiProposalRefreshKey((current) => current + 1)}
            filter={routeFilter()}
            selectedTicket={selectedTicket()}
			ticketTeams={ticketTeams()}
            userId={ctx()?.userId ?? ''}
            visibleTickets={filteredTickets()}
          />
        </div>
      </div>
      <InboxWorkModal
        modal={modal()}
        onClose={() => setModal(null)}
        onLinkExistingTicket={linkSelectedConversationToTicket}
        selectedTicket={selectedTicket()}
        ticketCandidates={supportTickets()}
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

function priorityById(id: number) {
  if (id === 1) return { id, name: 'low' }
  if (id === 3) return { id, name: 'high' }
  return { id: 2, name: 'normal' }
}
