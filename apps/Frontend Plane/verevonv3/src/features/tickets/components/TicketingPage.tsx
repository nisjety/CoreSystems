import { useLocation, useNavigate } from '@solidjs/router'
import {
  AlertTriangle,
  Bot,
  CheckCheck,
  Clock3,
  ExternalLink,
  FileText,
  Inbox,
  Link2,
  List,
  ListChecks,
  MessageSquare,
  Search,
  ShieldAlert,
  Sparkles,
  Tag,
  TimerReset,
  UserRound,
  UsersRound,
  Zap,
} from 'lucide-solid'
import type { LucideProps } from 'lucide-solid'
import { createEffect, createMemo, createResource, createSignal, For, Show, type Component, type JSX } from 'solid-js'
import { Dynamic } from 'solid-js/web'
import { formatRelativeTime } from '@/features/inbox/lib/inbox-model'
import type { AssistMessage } from '@/features/inbox/lib/inbox-ai'
import { SupportVerevonComposer } from '@/features/support/components/SupportVerevonComposer'
import { launchTicketAssistant } from '@/features/tickets/lib/ticket-chat-launch'
import { deriveTicketRepeatSignals } from '@/features/tickets/lib/ticket-repeat-signals'
import { TicketRepeatSignals } from '@/features/tickets/components/TicketRepeatSignals'
import { getConversationDetail } from '@/shared/api/inbox-client'
import {
  executeTicketChecklistCreate,
  executeTicketChecklistItemUpdate,
  executeTicketMacro,
  executeTicketPatch,
  executeTicketResourceLink,
  executeTicketSideConversationCreate,
  executeTicketSideConversationMessage,
  executeTicketSideConversationStatus,
} from '@/features/tickets/lib/ticket-actions'
import {
  bulkTicketStatuses,
  executeBulkTicketStatusUpdate,
  type BulkTicketStatus,
  type BulkTicketStatusResult,
} from '@/features/tickets/lib/ticket-bulk-actions'
import { safeTicketResourceUrl } from '@/features/tickets/lib/ticket-resource-links'
import { getAuthSession, getSessionContext } from '@/shared/api/auth-client'
import { createSocialDraftFromInbox } from '@/shared/api/social-client'
import {
	createTicketTeam,
	createTicketAutomationRule,
  createIncident,
  createProblem,
  linkIncidentTicket,
  listIncidents,
  listProblems,
  updateIncident,
  updateProblem,
  listSlaPolicies,
  listTicketAutomationRules,
  listTicketMacros,
  listTicketTeams,
	listTicketActivity,
  listTickets,
	listTicketViews,
  type SlaPolicy,
	type TicketActivity,
  type SupportIncident,
  type SupportProblem,
  type SupportTicket,
  type TicketAutomationRule,
  type TicketChecklist,
  type TicketMacro,
  type TicketTeam,
  type TicketWorkType,
  type TicketView,
  type UpdateTicketInput,
	updateTicketAutomationRule,
	updateTicketTeam,
} from '@/shared/api/tickets-client'
import { executeAction } from '@/shared/actions/action-client'
import { buildModelContextPack, type SupportAssistantContext } from '@/shared/context-packs/context-pack'
import { cn } from '@/shared/lib/cn'
import { translateApiError, useI18n } from '@/shared/i18n'
import { handleTabKeyDown } from '@/shared/ui/tab-keyboard'

const ticketQueues = [
  { id: 'all', label: 'All tickets', labelNo: 'Alle saker', icon: List },
  { id: 'suggested', label: 'Suggested by AI', labelNo: 'Foreslått av AI', icon: Bot },
  { id: 'my', label: 'My tickets', labelNo: 'Mine saker', icon: UserRound },
  { id: 'unassigned', label: 'Unassigned', labelNo: 'Ikke tildelt', icon: Inbox },
  { id: 'sla-risk', label: 'SLA risk', labelNo: 'SLA-risiko', icon: AlertTriangle },
  { id: 'escalated', label: 'Escalated', labelNo: 'Eskalert', icon: ShieldAlert },
  { id: 'waiting-customer', label: 'Waiting on customer', labelNo: 'Venter på kunde', icon: Clock3 },
  { id: 'waiting-team', label: 'Waiting on team', labelNo: 'Venter på team', icon: UsersRound },
  { id: 'resolved', label: 'Resolved', labelNo: 'Løst', icon: CheckCheck },
  { id: 'rules', label: 'Rules / queues', labelNo: 'Regler / køer', icon: ListChecks },
] as const

type TicketQueueId = (typeof ticketQueues)[number]['id']
type TicketCenterTab = 'ticket' | 'conversation' | 'related' | 'activity'
type TicketContextTab = 'details' | 'verevon' | 'actions' | 'audit'

type TicketingContext = {
  email: string
  name: string
  orgId: string
  role: string
  userId: string
}

type TicketListState = {
  error: unknown | null
  tickets: SupportTicket[]
}

type TrFn = (noText: string, enText: string) => string

function ticketQueueShortLabel(queue: TicketQueueId, tr: TrFn) {
  const match = ticketQueues.find((item) => item.id === queue)
  return match ? tr(match.labelNo, match.label) : tr('Mine saker', 'My tickets')
}

function ticketQueueSummary(queue: TicketQueueId, count: number, tr: TrFn) {
  if (queue === 'rules') return tr('Rutingarbeidsområde', 'Routing workspace')
  if (count === 1) return tr('1 sak', '1 ticket')
  return tr(`${count} saker`, `${count} tickets`)
}

function ticketWorkTypeLabel(workType: TicketWorkType, tr: TrFn) {
  switch (workType) {
    case 'customer_case':
      return tr('Kundesaker', 'Customer cases')
    case 'internal_work':
      return tr('Internt arbeid', 'Internal work')
    case 'incident':
      return tr('Hendelser', 'Incidents')
  }
}

function isTerminalTicketStatus(status: string): boolean {
  return status === 'resolved' || status === 'solved' || status === 'closed'
}

async function loadTicketingContext(): Promise<TicketingContext> {
  const [session, ctx] = await Promise.all([getAuthSession(), getSessionContext()])
  return {
    email: session?.user.email ?? '',
    name: session?.user.name ?? '',
    orgId: ctx.orgs[0]?.id ?? '',
    role: ctx.orgs[0]?.role ?? '',
    userId: session?.user.id ?? '',
  }
}

async function loadTicketList(
  orgId: string,
  params: Parameters<typeof listTickets>[1],
): Promise<TicketListState> {
  try {
    return { tickets: await listTickets(orgId, params), error: null }
  } catch (error) {
    // A list failure is a first-class UI state. Returning it deliberately keeps
    // the last view honest instead of rendering a plausible empty queue.
    return { tickets: [], error }
  }
}

// Saved views are durable Ticketing configuration, while the active list stays
// URL-driven and therefore shareable and source-of-truth backed. Apply only the
// ticket-list contract's allow-listed scalar filters; arbitrary view JSON never
// becomes a client-side predicate or an unbounded query parameter.
const TICKET_VIEW_FILTER_KEYS = ['queue', 'status', 'work_type', 'team', 'label', 'priority', 'severity', 'sla_state', 'assigned'] as const

function ticketViewHref(pathname: string, search: string, view: TicketView): string {
  const params = new URLSearchParams(search)
  for (const key of TICKET_VIEW_FILTER_KEYS) params.delete(key)
  for (const key of TICKET_VIEW_FILTER_KEYS) {
    const value = view.filter?.[key]
    if (typeof value === 'string' && value.trim()) params.set(key, value.trim())
  }
  params.set('view', view.id)
  if (pathname.startsWith('/support')) params.set('surface', 'tickets')
  return `${pathname.startsWith('/support') ? '/support' : '/tickets'}?${params.toString()}`
}

export default function TicketingPage() {
  const i18n = useI18n()
  const location = useLocation()
  const navigate = useNavigate()
  const [ctx] = createResource(loadTicketingContext)
  const [selectedId, setSelectedId] = createSignal<string | null>(null)
  const [activeTicketTab, setActiveTicketTab] = createSignal<TicketCenterTab>('ticket')
  const [notice, setNotice] = createSignal<string | null>(null)
  const [searchQuery, setSearchQuery] = createSignal('')
  const [bulkSelectedIds, setBulkSelectedIds] = createSignal<Set<string>>(new Set())
  const [bulkStatus, setBulkStatus] = createSignal<BulkTicketStatus>('waiting_team')
  const [bulkPreview, setBulkPreview] = createSignal<{ status: BulkTicketStatus; tickets: SupportTicket[] } | null>(null)
  const [bulkExecuting, setBulkExecuting] = createSignal(false)
  const [bulkResult, setBulkResult] = createSignal<BulkTicketStatusResult | null>(null)
  const [macroPreview, setMacroPreview] = createSignal<{ macro: TicketMacro; ticket: SupportTicket } | null>(null)
  const [dependencyResolutionPreview, setDependencyResolutionPreview] = createSignal<{ ticket: SupportTicket; children: TicketDependencyTarget[] } | null>(null)

  const activeQueue = createMemo<TicketQueueId>(() => {
    const value = new URLSearchParams(location.search).get('queue') as TicketQueueId | null
    // Default to the "All tickets" queue so the Ticketing page opens populated
    // rather than on an empty "My tickets" (assigned-to-me) view.
    return ticketQueues.some((queue) => queue.id === value) ? value! : 'all'
  })
  const activeParams = createMemo(() => new URLSearchParams(location.search))
  const activeStatus = createMemo(() => activeParams().get('status') ?? undefined)
  const activeWorkType = createMemo<TicketWorkType | undefined>(() => {
    const value = activeParams().get('work_type')
    return value === 'customer_case' || value === 'internal_work' || value === 'incident' ? value : undefined
  })
  const activeTeam = createMemo(() => activeParams().get('team') ?? undefined)
  const activeLabel = createMemo(() => activeParams().get('label') ?? undefined)
  const activePriority = createMemo(() => activeParams().get('priority') ?? undefined)
  const activeSeverity = createMemo(() => activeParams().get('severity') ?? undefined)
  const activeSlaState = createMemo(() => activeParams().get('sla_state') ?? undefined)
  const activeView = createMemo(() => activeParams().get('view') ?? undefined)
  const requestedTicketId = createMemo(() => activeParams().get('ticketId'))

  const [viewsRes] = createResource(() => ctx()?.orgId ?? '', (orgId) => (orgId ? loadOptionalTicketMetadata(() => listTicketViews(orgId), [] as TicketView[]) : Promise.resolve([] as TicketView[])))
	const [teamsRes, { refetch: refetchTicketTeams }] = createResource(() => ctx()?.orgId ?? '', (orgId) => (orgId ? loadOptionalTicketMetadata(() => listTicketTeams(orgId), [] as TicketTeam[]) : Promise.resolve([] as TicketTeam[])))
  const [macrosRes, { refetch: refetchMacros }] = createResource(() => ctx()?.orgId ?? '', (orgId) => (orgId ? loadOptionalTicketMetadata(() => listTicketMacros(orgId), [] as TicketMacro[]) : Promise.resolve([] as TicketMacro[])))
  const [slaRes] = createResource(() => ctx()?.orgId ?? '', (orgId) => (orgId ? loadOptionalTicketMetadata(() => listSlaPolicies(orgId), [] as SlaPolicy[]) : Promise.resolve([] as SlaPolicy[])))
  const [rulesRes, { refetch: refetchAutomationRules }] = createResource(() => ctx()?.orgId ?? '', (orgId) => (orgId ? loadOptionalTicketMetadata(() => listTicketAutomationRules(orgId), [] as TicketAutomationRule[]) : Promise.resolve([] as TicketAutomationRule[])))

  const savedViews = () => viewsRes() ?? []
  const ticketTeams = () => teamsRes() ?? []
  const activeTicketTeams = () => ticketTeams().filter((team) => team.active)
  const macros = () => (macrosRes() ?? []).filter((macro) => macro.active)
  const slaPolicies = () => slaRes() ?? []
  const automationRules = () => rulesRes() ?? []

  const [ticketsRes, { mutate }] = createResource(
    () => ({
      orgId: ctx()?.orgId ?? '',
      queue: activeQueue(),
      userId: ctx()?.userId ?? '',
      status: activeStatus(),
      workType: activeWorkType(),
      team: activeTeam(),
      label: activeLabel(),
      priority: activePriority(),
      severity: activeSeverity(),
      slaState: activeSlaState(),
      q: searchQuery().trim(),
    }),
    (source) => {
      if (!source.orgId || source.queue === 'rules') return Promise.resolve({ tickets: [], error: null } satisfies TicketListState)
      return loadTicketList(source.orgId, {
        // 'all' sends no queue filter (backend $4='' returns every ticket).
        queue: source.queue === 'all' ? undefined : source.queue,
        assigned: source.queue === 'my' ? source.userId : undefined,
        status: source.status,
        work_type: source.workType,
        team: source.team,
        label: source.label,
        priority: source.priority,
        severity: source.severity,
        sla_state: source.slaState,
        q: source.q,
        limit: 100,
      })
    },
  )

  const tickets = () => ticketsRes()?.tickets ?? []
  const ticketsError = () => ticketsRes()?.error ?? null
  const normalizedSearch = () => searchQuery().trim().toLowerCase()
  const filteredTickets = createMemo(() => {
    const query = normalizedSearch()
    if (!query) return tickets()
    return tickets().filter((ticket) => [
      ticket.ticket_key,
      ticket.status,
      ticket.priority,
      ticket.severity,
      ticket.sla_state ?? '',
      ticket.category ?? '',
      ticket.intent ?? '',
      ticket.conversation?.title ?? '',
      ticket.conversation?.contact?.name ?? '',
      ticket.conversation?.contact?.email ?? '',
      ...(ticket.labels ?? []),
    ].some((value) => value.toLowerCase().includes(query)))
  })
  const repeatSignals = createMemo(() => deriveTicketRepeatSignals(tickets()))
  const activeQueueMeta = createMemo(() => ticketQueues.find((queue) => queue.id === activeQueue()) ?? ticketQueues[1])
  const activeListTitle = createMemo(() => activeWorkType()
    ? ticketWorkTypeLabel(activeWorkType()!, i18n.tr)
    : i18n.tr(activeQueueMeta().labelNo, activeQueueMeta().label))
  const selectedTicket = createMemo(() => filteredTickets().find((ticket) => ticket.id === selectedId()) ?? filteredTickets()[0] ?? null)
  const bulkSelectedTickets = createMemo(() => filteredTickets().filter((ticket) => bulkSelectedIds().has(ticket.id)))
	const [ticketActivityRes] = createResource(
		() => {
			const orgId = ctx()?.orgId
			const ticketId = selectedTicket()?.id
			return orgId && ticketId ? { orgId, ticketId } : null
		},
		(source) => source ? loadOptionalTicketMetadata(() => listTicketActivity(source.orgId, source.ticketId), [] as TicketActivity[]) : Promise.resolve([] as TicketActivity[]),
	)
	const ticketActivity = () => ticketActivityRes() ?? []

  createEffect(() => {
    void selectedTicket()?.id
    setActiveTicketTab('ticket')
  })

  createEffect(() => {
    const visibleIDs = new Set(filteredTickets().map((ticket) => ticket.id))
    setBulkSelectedIds((previous) => {
      const retained = new Set([...previous].filter((id) => visibleIDs.has(id)))
      return retained.size === previous.size ? previous : retained
    })
  })

  // Operational records are loaded only for an incident work item. This keeps
  // ordinary customer-case Ticketing fast while still putting incident and
  // problem authority in the unified work surface when it is relevant.
  const [incidentsRes, { refetch: refetchIncidents }] = createResource(
    () => selectedTicket()?.work_type === 'incident' ? ctx()?.orgId ?? '' : '',
    (orgId) => orgId ? loadOptionalTicketMetadata(() => listIncidents(orgId), [] as SupportIncident[]) : Promise.resolve([] as SupportIncident[]),
  )
  const [problemsRes, { refetch: refetchProblems }] = createResource(
    () => selectedTicket()?.work_type === 'incident' ? ctx()?.orgId ?? '' : '',
    (orgId) => orgId ? loadOptionalTicketMetadata(() => listProblems(orgId), [] as SupportProblem[]) : Promise.resolve([] as SupportProblem[]),
  )
  const incidents = () => incidentsRes() ?? []
  const problems = () => problemsRes() ?? []

  createEffect(() => {
    const requested = requestedTicketId()
    if (requested && tickets().some((ticket) => ticket.id === requested)) {
      setSelectedId(requested)
      return
    }
    if (!selectedId() || !filteredTickets().some((ticket) => ticket.id === selectedId())) {
      setSelectedId(filteredTickets()[0]?.id ?? null)
    }
  })

  const replaceTicket = (updated: SupportTicket) => {
    mutate((current) => current && {
      ...current,
      tickets: current.tickets.map((item) => (item.id === updated.id ? updated : item)),
    })
  }

  const patchTicket = async (ticket: SupportTicket, patch: UpdateTicketInput, message: string) => {
    const orgId = ctx()?.orgId
    const userId = ctx()?.userId
    if (!orgId || !userId) return
    setNotice(null)
    try {
      const updated = await executeTicketPatch({ type: 'human', orgId, userId }, ticket, patch)
      replaceTicket(updated)
      setNotice(message)
    } catch (reason) {
      setNotice(translateApiError(reason, i18n.tr, { no: 'Saken kunne ikke oppdateres.', en: 'Ticket could not be updated.' }))
    }
  }

  const toggleBulkSelection = (ticketID: string, selected: boolean) => {
    setBulkResult(null)
    setBulkSelectedIds((previous) => {
      const next = new Set(previous)
      if (selected) next.add(ticketID)
      else next.delete(ticketID)
      return next
    })
  }

  const toggleVisibleBulkSelection = (selected: boolean) => {
    setBulkResult(null)
    setBulkSelectedIds(selected ? new Set(filteredTickets().map((ticket) => ticket.id)) : new Set<string>())
  }

  const applyBulkStatus = async (ticketsToUpdate: SupportTicket[], status: BulkTicketStatus) => {
    const orgId = ctx()?.orgId
    const userId = ctx()?.userId
    if (!orgId || !userId || ticketsToUpdate.length === 0 || bulkExecuting()) return
    setBulkExecuting(true)
    setBulkResult(null)
    try {
      const result = await executeBulkTicketStatusUpdate(
        { type: 'human', orgId, userId },
        ticketsToUpdate,
        status,
      )
      result.updated.forEach(replaceTicket)
      setBulkSelectedIds(new Set(result.failed.map(({ ticket }) => ticket.id)))
      setBulkResult(result)
    } finally {
      setBulkExecuting(false)
    }
  }

  const runMacroAction = async (ticket: SupportTicket, macro: TicketMacro) => {
    const orgId = ctx()?.orgId
    const userId = ctx()?.userId
    if (!orgId || !userId) return
    setNotice(null)
    try {
      const updated = await executeTicketMacro({ type: 'human', orgId, userId }, ticket, macro.id, macro.updated_at)
      replaceTicket(updated)
      setNotice(i18n.tr(`Makroen "${macro.name}" ble kjørt.`, `Macro "${macro.name}" applied.`))
    } catch (reason) {
      setNotice(translateApiError(reason, i18n.tr, { no: 'Makroen kunne ikke kjøres.', en: 'Macro could not be applied.' }))
    }
  }

  const createMacro = async (input: { name: string; description: string; status: string }) => {
    const orgId = ctx()?.orgId
    if (!orgId || !input.name.trim()) return false
    try {
      await executeAction('tickets.create_macro', {
        type: 'human',
        orgId,
        userId: ctx()?.userId ?? '',
      }, {
        name: input.name.trim(),
        description: input.description.trim(),
        visibility: 'team',
        status: input.status,
      })
      await refetchMacros()
      setNotice(i18n.tr('Makroen ble opprettet.', 'Macro created.'))
      return true
    } catch (reason) {
      setNotice(translateApiError(reason, i18n.tr, { no: 'Makroen kunne ikke opprettes.', en: 'Macro could not be created.' }))
      return false
    }
  }

  const createChecklist = async (ticket: SupportTicket) => {
    const orgId = ctx()?.orgId
    const userId = ctx()?.userId
    if (!orgId || !userId) return
    setNotice(null)
    try {
      const updated = await executeTicketChecklistCreate({ type: 'human', orgId, userId }, ticket, {
        name: i18n.tr('Løsningssjekkliste', 'Resolution checklist'),
        items: [
          i18n.tr('Bekreft eier', 'Confirm owner'),
          i18n.tr('Dokumenter kundepåvirkning', 'Document customer impact'),
          i18n.tr('Send kundeoppdatering', 'Send customer update'),
        ],
      })
      replaceTicket(updated)
      setNotice(i18n.tr('Sjekkliste lagt til.', 'Checklist added.'))
    } catch (reason) {
      setNotice(translateApiError(reason, i18n.tr, { no: 'Sjekklisten kunne ikke legges til.', en: 'Checklist could not be added.' }))
    }
  }

  const toggleChecklistItem = async (ticket: SupportTicket, checklist: TicketChecklist, itemId: string, completed: boolean) => {
    const orgId = ctx()?.orgId
    const userId = ctx()?.userId
    if (!orgId || !userId) return
    setNotice(null)
    try {
      const updated = await executeTicketChecklistItemUpdate(
        { type: 'human', orgId, userId },
        ticket,
        { checklistId: checklist.id, itemId, completed },
      )
      replaceTicket(updated)
    } catch (reason) {
      setNotice(translateApiError(reason, i18n.tr, { no: 'Sjekklistepunktet kunne ikke oppdateres.', en: 'Checklist item could not be updated.' }))
    }
  }

  const createSideConversation = async (ticket: SupportTicket, input: { subject: string; body_text: string }) => {
    const orgId = ctx()?.orgId
    const userId = ctx()?.userId
    if (!orgId || !userId) return
    setNotice(null)
    try {
      replaceTicket(await executeTicketSideConversationCreate({ type: 'human', orgId, userId }, ticket, input))
      setNotice(i18n.tr('Intern koordineringssamtale startet.', 'Internal coordination conversation started.'))
    } catch (reason) {
      setNotice(translateApiError(reason, i18n.tr, { no: 'Den interne samtalen kunne ikke startes.', en: 'The internal conversation could not be started.' }))
    }
  }

  const addSideConversationMessage = async (ticket: SupportTicket, input: { sideConversationId: string; body_text: string }) => {
    const orgId = ctx()?.orgId
    const userId = ctx()?.userId
    if (!orgId || !userId) return
    setNotice(null)
    try {
      replaceTicket(await executeTicketSideConversationMessage({ type: 'human', orgId, userId }, ticket, input))
    } catch (reason) {
      setNotice(translateApiError(reason, i18n.tr, { no: 'Det interne svaret kunne ikke sendes.', en: 'The internal reply could not be added.' }))
    }
  }

  const updateSideConversationStatus = async (ticket: SupportTicket, input: { sideConversationId: string; status: 'open' | 'closed' }) => {
    const orgId = ctx()?.orgId
    const userId = ctx()?.userId
    if (!orgId || !userId) return
    setNotice(null)
    try {
      replaceTicket(await executeTicketSideConversationStatus({ type: 'human', orgId, userId }, ticket, input))
    } catch (reason) {
      setNotice(translateApiError(reason, i18n.tr, { no: 'Samtalestatusen kunne ikke oppdateres.', en: 'The conversation status could not be updated.' }))
    }
  }

  const linkConversationSource = async (ticket: SupportTicket) => {
    const orgId = ctx()?.orgId
    const userId = ctx()?.userId
    if (!orgId || !userId) return
    setNotice(null)
    try {
      const updated = await executeTicketResourceLink({ type: 'human', orgId, userId }, ticket, {
        link_type: 'related',
        resource_kind: 'conversation_source',
        resource_id: ticket.conversation_id,
        label: ticket.conversation?.provider ? i18n.tr(`${ticket.conversation.provider}-tråd`, `${ticket.conversation.provider} thread`) : i18n.tr('Samtalekilde', 'Conversation source'),
        metadata: { channel: ticket.conversation?.channel, provider: ticket.conversation?.provider },
      })
      replaceTicket(updated)
      setNotice(i18n.tr('Kilde lenket til saken.', 'Source linked to ticket.'))
    } catch (reason) {
      setNotice(translateApiError(reason, i18n.tr, { no: 'Ressursen kunne ikke lenkes.', en: 'Resource could not be linked.' }))
    }
  }

  const linkTicketDependency = async (
    ticket: SupportTicket,
    target: SupportTicket,
    linkType: 'parent' | 'child' | 'related',
  ) => {
    const orgId = ctx()?.orgId
    const userId = ctx()?.userId
    if (!orgId || !userId || ticket.id === target.id) return
    setNotice(null)
    try {
      const updated = await executeTicketResourceLink({ type: 'human', orgId, userId }, ticket, {
        link_type: linkType,
        resource_kind: 'ticket',
        resource_id: target.id,
        label: target.ticket_key,
        metadata: {
          ticket_key: target.ticket_key,
          target_status: target.status,
          relationship: linkType,
        },
      })
      replaceTicket(updated)
      setNotice(i18n.tr(`Saksforhold til ${target.ticket_key} ble lagt til.`, `Ticket relationship to ${target.ticket_key} was added.`))
    } catch (reason) {
      setNotice(translateApiError(reason, i18n.tr, { no: 'Saksforholdet kunne ikke legges til.', en: 'Could not add the ticket relationship.' }))
    }
  }

  const askVerevon = async (ticket: SupportTicket) => {
	const orgId = ctx()?.orgId
	const userId = ctx()?.userId
	if (!orgId || !userId) return
    setNotice(null)
    try {
      await launchTicketAssistant(ticket, { type: 'human', orgId, userId })
      navigate('/chat')
    } catch (reason) {
      setNotice(translateApiError(reason, i18n.tr, { no: 'Saken kunne ikke åpnes i Verevon Chat.', en: 'The ticket could not be opened in Verevon Chat.' }))
    }
  }

  const createSocialFollowUp = async (ticket: SupportTicket) => {
    const orgId = ctx()?.orgId
    if (!orgId) return
    setNotice(null)
    try {
      await createSocialDraftFromInbox(orgId, {
        ticketId: ticket.conversation_id,
        ticketTitle: ticket.conversation?.title || ticket.ticket_key,
        supportTicketId: ticket.id,
        conversationId: ticket.conversation_id,
        customerName: ticketCustomerLabel(ticket, i18n.tr),
        channel: ticket.conversation?.channel,
        excerpt: ticket.conversation?.last_message_preview,
      })
      setNotice(i18n.tr('Utkast til sosial oppfølging opprettet og lenket.', 'Social follow-up draft created and linked.'))
      navigate('/social/drafts')
    } catch (reason) {
      setNotice(translateApiError(reason, i18n.tr, { no: 'Sosial oppfølging kunne ikke opprettes.', en: 'Social follow-up could not be created.' }))
    }
  }

  const snoozeTicket = (ticket: SupportTicket) => {
    const until = new Date(Date.now() + 24 * 60 * 60 * 1000).toISOString()
    void patchTicket(ticket, { status: 'snoozed', snoozed_until: until }, i18n.tr('Saken er utsatt i 24 timer.', 'Ticket snoozed for 24 hours.'))
  }

  const requestTicketResolution = (ticket: SupportTicket) => {
    const children = unresolvedChildDependencies(ticket)
    if (children.length > 0) {
      setDependencyResolutionPreview({ ticket, children })
      return
    }
    void patchTicket(ticket, { status: 'resolved' }, i18n.tr('Saken er løst.', 'Ticket resolved.'))
  }

  const scheduleTeamFollowUp = (ticket: SupportTicket) => {
    const followUpAt = new Date(Date.now() + 24 * 60 * 60 * 1000).toISOString()
    void patchTicket(ticket, { follow_up_at: followUpAt }, i18n.tr('Teamoppfølgingen er satt til i morgen.', 'Team follow-up is due tomorrow.'))
  }

  const declareIncidentForTicket = async (ticket: SupportTicket, title: string, problemId?: string) => {
    const orgId = ctx()?.orgId
    if (!orgId || !title.trim()) return false
    try {
      const incident = await createIncident(orgId, {
        title: title.trim(), severity: ticket.severity, owner_user_id: ctx()?.userId, owner_name: ctx()?.name, customer_impact: ticket.conversation?.last_message_preview ?? '', problem_id: problemId || undefined,
      })
      await linkIncidentTicket(orgId, incident.id, { ticket_id: ticket.id, relationship: 'affected' })
      await Promise.all([refetchIncidents(), refetchProblems()])
      setNotice(i18n.tr(`Hendelsen ${incident.incident_key} er erklært og saken er lenket.`, `Incident ${incident.incident_key} declared and ticket linked.`))
      return true
    } catch (reason) {
      setNotice(translateApiError(reason, i18n.tr, { no: 'Hendelsen kunne ikke erklæres.', en: 'Incident could not be declared.' }))
      return false
    }
  }

  const createProblemForIncident = async (title: string) => {
    const orgId = ctx()?.orgId
    if (!orgId || !title.trim()) return false
    try {
      await createProblem(orgId, { title: title.trim(), owner_user_id: ctx()?.userId, owner_name: ctx()?.name, summary: i18n.tr('Opprettet fra Ticketing for årsaksanalyse.', 'Created from Ticketing for root-cause analysis.') })
      await refetchProblems()
      setNotice(i18n.tr('Problem registrert.', 'Problem recorded.'))
      return true
    } catch (reason) {
      setNotice(translateApiError(reason, i18n.tr, { no: 'Problemet kunne ikke registreres.', en: 'Problem could not be recorded.' }))
      return false
    }
  }

  const patchIncidentStatus = async (incident: SupportIncident, status: string) => {
    const orgId = ctx()?.orgId
    if (!orgId || status === incident.status) return
    try {
      await updateIncident(orgId, incident.id, { status })
      await refetchIncidents()
      setNotice(i18n.tr(`Hendelsen ${incident.incident_key} er oppdatert.`, `Incident ${incident.incident_key} updated.`))
    } catch (reason) {
      setNotice(translateApiError(reason, i18n.tr, { no: 'Hendelsen kunne ikke oppdateres.', en: 'Incident could not be updated.' }))
    }
  }

  const patchProblemStatus = async (problem: SupportProblem, status: string) => {
    const orgId = ctx()?.orgId
    if (!orgId || status === problem.status) return
    try {
      await updateProblem(orgId, problem.id, { status })
      await refetchProblems()
      setNotice(i18n.tr(`Problemet ${problem.problem_key} er oppdatert.`, `Problem ${problem.problem_key} updated.`))
    } catch (reason) {
      setNotice(translateApiError(reason, i18n.tr, { no: 'Problemet kunne ikke oppdateres.', en: 'Problem could not be updated.' }))
    }
  }

	const createTeam = async (name: string, description: string): Promise<boolean> => {
		const orgId = ctx()?.orgId
		if (!orgId || !name.trim()) return false
		setNotice(null)
		try {
			await createTicketTeam(orgId, { name: name.trim(), description: description.trim() || undefined, active: true })
			await refetchTicketTeams()
			setNotice(i18n.tr(`Teamet ${name.trim()} er opprettet.`, `Team ${name.trim()} created.`))
			return true
		} catch (reason) {
			setNotice(translateApiError(reason, i18n.tr, { no: 'Teamet kunne ikke opprettes.', en: 'The Ticketing team could not be created.' }))
			return false
		}
	}

	const setTeamActive = async (team: TicketTeam, active: boolean): Promise<void> => {
		const orgId = ctx()?.orgId
		if (!orgId) return
		setNotice(null)
		try {
			await updateTicketTeam(orgId, team.id, { active })
			await refetchTicketTeams()
			setNotice(active
				? i18n.tr(`${team.name} er aktivert.`, `${team.name} activated.`)
				: i18n.tr(`${team.name} er deaktivert.`, `${team.name} deactivated.`))
		} catch (reason) {
			setNotice(translateApiError(reason, i18n.tr, { no: 'Teamet kunne ikke oppdateres.', en: 'The Ticketing team could not be updated.' }))
		}
	}

	const setAutomationRuleActive = async (rule: TicketAutomationRule, active: boolean): Promise<void> => {
		const orgId = ctx()?.orgId
		if (!orgId) return
		setNotice(null)
		try {
			await updateTicketAutomationRule(orgId, rule.id, { active })
			await refetchAutomationRules()
			setNotice(active
				? i18n.tr(`Regelen ${rule.name} er aktivert.`, `Rule ${rule.name} is active.`)
				: i18n.tr(`Regelen ${rule.name} er satt på pause.`, `Rule ${rule.name} is paused.`))
		} catch (reason) {
			setNotice(translateApiError(reason, i18n.tr, { no: 'Regelen kunne ikke oppdateres.', en: 'The rule could not be updated.' }))
		}
	}

	const createAutomationRule = async (input: { name: string; eventName: string; conditionKey: string; conditionValue: string; actionKey: string; actionValue: string }): Promise<boolean> => {
		const orgId = ctx()?.orgId
		if (!orgId || !input.name.trim() || !input.conditionValue.trim() || !input.actionValue.trim()) return false
		setNotice(null)
		try {
			const actionValue = input.actionKey === 'labels'
				? input.actionValue.split(',').map((value) => value.trim()).filter(Boolean)
				: input.actionValue.trim()
			await createTicketAutomationRule(orgId, {
				name: input.name.trim(), event_name: input.eventName, active: true,
				conditions: { [input.conditionKey]: input.conditionValue.trim() },
				actions: { [input.actionKey]: actionValue },
			})
			await refetchAutomationRules()
			setNotice(i18n.tr(`Regelen ${input.name.trim()} er opprettet.`, `Rule ${input.name.trim()} created.`))
			return true
		} catch (reason) {
			setNotice(translateApiError(reason, i18n.tr, { no: 'Regelen kunne ikke opprettes.', en: 'The rule could not be created.' }))
			return false
		}
	}

  return (
    <main class="verevon-ticketing-page">
      <section class="verevon-ticketing-list" aria-label={i18n.tr('Saker', 'Tickets')}>
        <div class="verevon-ticketing-list__header">
          <div>
            <span>{i18n.tr('Kø', 'Queue')}</span>
            <h2>{activeListTitle()}</h2>
          </div>
          <small>{ticketQueueSummary(activeQueue(), tickets().length, i18n.tr)}</small>
        </div>
        <div class="verevon-ticketing-list__tools">
          <label class="verevon-ticketing-search">
            <Search class="size-4" />
            <input
              value={searchQuery()}
              onInput={(event) => setSearchQuery(event.currentTarget.value)}
              placeholder={i18n.tr('Søk i saker', 'Search tickets')}
            />
          </label>
          <div class="verevon-ticketing-filter-strip" aria-label={i18n.tr('Sakfiltre', 'Ticket filters')}>
            <a href={withTicketParam(location.pathname, location.search, 'sla_state', 'risk')}>{i18n.tr('SLA-risiko', 'SLA risk')}</a>
            <a href={withTicketParam(location.pathname, location.search, 'priority', 'urgent')}>{i18n.tr('Haster', 'Urgent')}</a>
            <a href={withTicketParam(location.pathname, location.search, 'label', 'refund')}>{i18n.tr('Refusjon', 'Refund')}</a>
          </div>
          <Show when={savedViews().length > 0}>
            <label class="verevon-ticketing-saved-view">
              <span>{i18n.tr('Lagret visning', 'Saved view')}</span>
              <select
                aria-label={i18n.tr('Lagret sakvisning', 'Saved ticket view')}
                value={activeView() ?? ''}
                onChange={(event) => {
                  const view = savedViews().find((candidate) => candidate.id === event.currentTarget.value)
                  if (view) navigate(ticketViewHref(location.pathname, location.search, view))
                }}
              >
                <option value="">{i18n.tr('Velg en visning', 'Choose a view')}</option>
                <For each={savedViews()}>{(view) => <option value={view.id}>{view.name}</option>}</For>
              </select>
            </label>
          </Show>
        </div>
        <TicketRepeatSignals orgId={ctx()?.orgId ?? ''} signals={repeatSignals()} />
        <Show when={activeQueue() !== 'rules' && filteredTickets().length > 0}>
          <div class="verevon-ticketing-bulk-toolbar" aria-label={i18n.tr('Massehandlinger for saker', 'Bulk ticket actions')}>
            <label>
              <input
                type="checkbox"
                checked={bulkSelectedTickets().length === filteredTickets().length}
                aria-label={i18n.tr('Velg alle synlige saker', 'Select all visible tickets')}
                onChange={(event) => toggleVisibleBulkSelection(event.currentTarget.checked)}
              />
              <span>{i18n.tr('Velg synlige', 'Select visible')}</span>
            </label>
            <span>{i18n.tr(`${bulkSelectedTickets().length} valgt`, `${bulkSelectedTickets().length} selected`)}</span>
            <select
              aria-label={i18n.tr('Målstatus for valgte saker', 'Target status for selected tickets')}
              value={bulkStatus()}
              disabled={bulkSelectedTickets().length === 0 || bulkExecuting()}
              onChange={(event) => setBulkStatus(event.currentTarget.value as BulkTicketStatus)}
            >
              <For each={bulkTicketStatuses}>
                {(status) => <option value={status}>{bulkStatusLabel(status, i18n.tr)}</option>}
              </For>
            </select>
            <button
              type="button"
              disabled={bulkSelectedTickets().length === 0 || bulkExecuting()}
              onClick={() => setBulkPreview({ status: bulkStatus(), tickets: bulkSelectedTickets() })}
            >
              {i18n.tr('Gjennomgå endring', 'Review change')}
            </button>
          </div>
        </Show>
        <Show when={activeQueue() !== 'rules' && bulkResult()}>
          {(result) => (
            <p class={cn('verevon-ticketing-bulk-result', result().failed.length > 0 && 'verevon-ticketing-bulk-result--partial')} role="status">
              {result().failed.length === 0
                ? i18n.tr(`Status ble oppdatert for ${result().updated.length} saker.`, `Status updated for ${result().updated.length} tickets.`)
                : i18n.tr(
                  `${result().updated.length} saker oppdatert. Kunne ikke oppdatere: ${result().failed.map(({ ticket }) => ticket.ticket_key).join(', ')}.`,
                  `${result().updated.length} tickets updated. Could not update: ${result().failed.map(({ ticket }) => ticket.ticket_key).join(', ')}.`,
                )}
            </p>
          )}
        </Show>
        <Show when={activeQueue() === 'rules' && notice()}>
          <p class="verevon-ticketing-notice">{notice()}</p>
        </Show>
        <Show when={activeQueue() !== 'rules'} fallback={
          <RulesWorkspace
            activeView={activeView()}
            automationRules={automationRules()}
            macros={macros()}
            slaPolicies={slaPolicies()}
            views={savedViews()}
            onSelectView={(view) => navigate(ticketViewHref(location.pathname, location.search, view))}
            onCreateMacro={createMacro}
			onSetAutomationRuleActive={setAutomationRuleActive}
			onCreateAutomationRule={createAutomationRule}
			canManageAutomation={['owner', 'admin'].includes(ctx()?.role.trim().toLowerCase() ?? '')}
          />
        }>
          <Show when={ticketsRes.loading}>
            <div class="verevon-ticketing-empty">{i18n.tr('Laster saker …', 'Loading tickets...')}</div>
          </Show>
          <Show when={!ticketsRes.loading && ticketsError()}>
            <div class="verevon-ticketing-empty verevon-ticketing-empty--error">{translateApiError(ticketsError(), i18n.tr, { no: 'Saksbehandlingen er utilgjengelig akkurat nå.', en: 'Ticketing is unavailable.' })}</div>
          </Show>
          <Show when={!ticketsRes.loading && !ticketsError() && filteredTickets().length === 0}>
            <div class="verevon-ticketing-empty">{i18n.tr('Ingen saker i denne køen.', 'No tickets in this queue.')}</div>
          </Show>
          <Show when={!ticketsRes.loading && !ticketsError() && filteredTickets().length > 0}>
            <ul class="verevon-ticketing-ticket-list">
              <For each={filteredTickets()}>
                {(ticket) => (
                  <li class="verevon-ticketing-ticket-list__item">
                    <label class="verevon-ticketing-ticket-select">
                      <input
                        type="checkbox"
                        checked={bulkSelectedIds().has(ticket.id)}
                        aria-label={i18n.tr(`Velg ${ticket.ticket_key}`, `Select ${ticket.ticket_key}`)}
                        onClick={(event) => event.stopPropagation()}
                        onChange={(event) => toggleBulkSelection(ticket.id, event.currentTarget.checked)}
                      />
                    </label>
                    <button
                      type="button"
                      onClick={() => setSelectedId(ticket.id)}
                      class={cn('verevon-ticketing-ticket-row', selectedTicket()?.id === ticket.id && 'verevon-ticketing-ticket-row--active')}
                    >
                      <div class="verevon-ticketing-ticket-row__top">
                        <strong>{ticket.ticket_key}</strong>
                        <TicketStatus status={ticket.status} />
                      </div>
                      <span>{(ticket.conversation?.title ?? ticket.intent) || ticket.category || i18n.tr('Support-sak', 'Support ticket')}</span>
                      <small>{ticketCustomerLabel(ticket, i18n.tr)} · {ticket.priority} {i18n.tr('prioritet', 'priority')}</small>
                      <TicketLabels labels={ticket.labels ?? []} />
                      <div class="verevon-ticketing-ticket-row__meta">
                        <span class={cn(ticket.sla_state === 'breached' && 'verevon-ticketing-danger-text')}>{ticket.sla_state || 'ok'} SLA</span>
                        <span>{ticket.team_name || ticket.assignee_name || i18n.tr('Ikke tildelt', 'Unassigned')}</span>
                      </div>
                    </button>
                  </li>
                )}
              </For>
            </ul>
          </Show>
        </Show>
      </section>

      <section class="verevon-ticketing-detail" aria-label={i18n.tr('Sakdetaljer', 'Ticket detail')}>
        <Show when={selectedTicket()} fallback={<div class="verevon-ticketing-empty">{i18n.tr('Velg en sak.', 'Select a ticket.')}</div>}>
          {(ticket) => (
            <>
              <div class="verevon-ticketing-detail__header">
                <div>
                  <span>{ticket().ticket_key}</span>
                  <h2>{(ticket().conversation?.title ?? ticket().intent) || i18n.tr('Support-sak', 'Support ticket')}</h2>
                </div>
                <TicketStatus status={ticket().status} />
              </div>

              <div class="verevon-ticketing-detail-tabs" role="tablist" aria-label={i18n.tr('Saksarbeidsområde', 'Ticket workspace')}>
                <TicketWorkspaceTab active={activeTicketTab() === 'ticket'} id="ticket" label={i18n.tr('Sak', 'Ticket')} onSelect={setActiveTicketTab} />
                <TicketWorkspaceTab active={activeTicketTab() === 'conversation'} id="conversation" label={i18n.tr('Samtale', 'Conversation')} onSelect={setActiveTicketTab} />
                <TicketWorkspaceTab active={activeTicketTab() === 'related'} id="related" label={i18n.tr('Relatert', 'Related')} onSelect={setActiveTicketTab} />
                <TicketWorkspaceTab active={activeTicketTab() === 'activity'} id="activity" label={i18n.tr('Aktivitet', 'Activity')} onSelect={setActiveTicketTab} />
              </div>

              <Show when={notice()}>
                <p class="verevon-ticketing-notice">{notice()}</p>
              </Show>

              <Show when={activeTicketTab() === 'ticket'}>
                <div id="ticket-workspace-panel-ticket" role="tabpanel" aria-labelledby="ticket-workspace-tab-ticket" class="verevon-ticketing-detail-tabpanel">
              <div class="verevon-ticketing-detail__quick-actions">
                <button type="button" onClick={() => patchTicket(ticket(), { assignee_user_id: ctx()?.userId, assignee_name: ctx()?.name }, i18n.tr('Saken er tildelt deg.', 'Ticket assigned to you.'))}>
                  <UserRound class="size-4" />
                  {i18n.tr('Tildel meg', 'Assign me')}
                </button>
                <button type="button" onClick={() => patchTicket(ticket(), { status: 'waiting_customer' }, i18n.tr('Saken venter på kunde.', 'Ticket is waiting on customer.'))}>
                  <Clock3 class="size-4" />
                  {i18n.tr('Vent på kunde', 'Wait customer')}
                </button>
                <button type="button" onClick={() => snoozeTicket(ticket())}>
                  <TimerReset class="size-4" />
                  {i18n.tr('Utsett', 'Snooze')}
                </button>
                <button type="button" onClick={() => scheduleTeamFollowUp(ticket())}>
                  <Clock3 class="size-4" />
                  {i18n.tr('Oppfølging i morgen', 'Follow up tomorrow')}
                </button>
                <Show
                  when={isTerminalTicketStatus(ticket().status)}
                  fallback={
                    <button type="button" onClick={() => requestTicketResolution(ticket())}>
                      <CheckCheck class="size-4" />
                      {i18n.tr('Løs', 'Resolve')}
                    </button>
                  }
                >
                  <button type="button" onClick={() => patchTicket(ticket(), { status: 'open' }, i18n.tr('Saken er åpnet på nytt.', 'Ticket reopened.'))}>
                    <CheckCheck class="size-4" />
                    {i18n.tr('Åpne på nytt', 'Reopen')}
                  </button>
                </Show>
              </div>

              <label class="verevon-ticketing-team-routing">
                <span>{i18n.tr('Rute til team', 'Route to team')}</span>
                <select
                  aria-label={i18n.tr('Rute saken til et kanonisk team', 'Route ticket to a canonical team')}
                  value={ticket().team_id ?? ''}
                  onChange={(event) => {
                    const team = activeTicketTeams().find((item) => item.id === event.currentTarget.value)
                    if (!team) {
                      void patchTicket(ticket(), { team_id: '', team_name: '' }, i18n.tr('Teamruting ble fjernet.', 'Team routing cleared.'))
                      return
                    }
                    void patchTicket(ticket(), { team_id: team.id, team_name: team.name }, i18n.tr(`Saken er rutet til ${team.name}.`, `Ticket routed to ${team.name}.`))
                  }}
                >
                  <option value="">{i18n.tr('Ikke tildelt team', 'No team assigned')}</option>
                  <For each={activeTicketTeams()}>{(team) => <option value={team.id}>{team.name}</option>}</For>
                </select>
                <small>{i18n.tr('Kun Ticketing-team vises; innboksgrupper er ikke rutingmyndighet.', 'Only Ticketing teams appear; Inbox groups are not routing authority.')}</small>
              </label>

              <div class="verevon-ticketing-ai-card">
                <Bot class="size-4" />
                <div>
                  <strong>{ticket().status === 'suggested' ? i18n.tr('Foreslått sak', 'Suggested ticket') : ticket().source === 'ai' ? i18n.tr('Auto-opprettet sak', 'Auto-created ticket') : i18n.tr('Manuell sak', 'Manual ticket')}</strong>
                  <p>{ticket().ai_reason || ticket().intent || i18n.tr('Kundeoppfølging spores som en varig sak.', 'Customer follow-up is tracked as a durable ticket.')}</p>
                </div>
                <Show when={ticket().ai_confidence}>
                  {(confidence) => <span>{Math.round(confidence() * 100)}%</span>}
                </Show>
              </div>

              <TicketSlaSnapshot ticket={ticket()} policies={slaPolicies()} />

              <div class="verevon-ticketing-field-grid">
                <label class="verevon-ticketing-field">
                  <span>{i18n.tr('Arbeidstype', 'Work type')}</span>
                  <select
                    aria-label={i18n.tr('Sakens arbeidstype', 'Ticket work type')}
                    value={ticket().work_type || 'customer_case'}
                    onChange={(event) => patchTicket(ticket(), { work_type: event.currentTarget.value as TicketWorkType }, i18n.tr('Arbeidstypen er oppdatert.', 'Work type updated.'))}
                  >
                    <option value="customer_case">{i18n.tr('Kundesak', 'Customer case')}</option>
                    <option value="internal_work">{i18n.tr('Internt arbeid', 'Internal work')}</option>
                    <option value="incident">{i18n.tr('Hendelse', 'Incident')}</option>
                  </select>
                </label>
                <TicketField label={i18n.tr('Prioritet', 'Priority')} value={ticket().priority} />
                <TicketField label={i18n.tr('Alvorlighetsgrad', 'Severity')} value={ticket().severity} />
                <TicketField label={i18n.tr('Kategori', 'Category')} value={ticket().category || i18n.tr('Ingen', 'None')} />
                <TicketField label={i18n.tr('Team', 'Team')} value={ticket().team_name || i18n.tr('Ikke tildelt', 'Unassigned')} />
                <TicketField label={i18n.tr('Eier', 'Owner')} value={ticket().assignee_name || i18n.tr('Ikke tildelt', 'Unassigned')} />
                <TicketField label={i18n.tr('Teamoppfølging', 'Team follow-up')} value={ticket().follow_up_at ? i18n.tr(`${formatRelativeTime(ticket().follow_up_at ?? '')} siden`, `${formatRelativeTime(ticket().follow_up_at ?? '')} from now`) : i18n.tr('Ikke planlagt', 'Not scheduled')} />
                <TicketField label={i18n.tr('Oppdatert', 'Updated')} value={i18n.tr(`${formatRelativeTime(ticket().updated_at)} siden`, `${formatRelativeTime(ticket().updated_at)} ago`)} />
              </div>

              <div class="verevon-ticketing-actions">
                <Show when={ticket().status === 'suggested'}>
                  <button type="button" class="verevon-inbox-button verevon-inbox-button--primary verevon-inbox-button--sm" onClick={() => patchTicket(ticket(), { status: 'open' }, i18n.tr('Saken er akseptert.', 'Ticket accepted.'))}>
                    {i18n.tr('Godta', 'Accept')}
                  </button>
                </Show>
                <button type="button" class="verevon-inbox-button verevon-inbox-button--secondary verevon-inbox-button--sm" onClick={() => createChecklist(ticket())}>
                  {i18n.tr('Legg til sjekkliste', 'Add checklist')}
                </button>
                <button type="button" class="verevon-inbox-button verevon-inbox-button--secondary verevon-inbox-button--sm" onClick={() => linkConversationSource(ticket())}>
                  {i18n.tr('Lenk kilde', 'Link source')}
                </button>
                <button type="button" class="verevon-inbox-button verevon-inbox-button--secondary verevon-inbox-button--sm" onClick={() => createSocialFollowUp(ticket())}>
                  {i18n.tr('Sosial oppfølging', 'Social follow-up')}
                </button>
                <button type="button" class="verevon-inbox-button verevon-inbox-button--primary verevon-inbox-button--sm" onClick={() => void askVerevon(ticket())}>
                  <Bot class="size-4" />
                  {i18n.tr('Åpne i Verevon Chat', 'Open in Verevon Chat')}
                </button>
                <Show when={ticket().conversation_id}>
                  <button type="button" class="verevon-inbox-button verevon-inbox-button--secondary verevon-inbox-button--sm" onClick={() => navigate(`/inbox?ticketId=${ticket().conversation_id}`)}>
                    {i18n.tr('Åpne innboks', 'Open inbox')}
                  </button>
                </Show>
              </div>
                </div>
              </Show>

              <Show when={activeTicketTab() === 'conversation'}>
                <div id="ticket-workspace-panel-conversation" role="tabpanel" aria-labelledby="ticket-workspace-tab-conversation" class="verevon-ticketing-detail-tabpanel">
                  <TicketConversationPanel
                    ticket={ticket()}
                    onOpenInbox={() => navigate(`/inbox?ticketId=${encodeURIComponent(ticket().conversation_id ?? '')}`)}
                  />
                </div>
              </Show>

              <Show when={activeTicketTab() === 'related'}>
                <div id="ticket-workspace-panel-related" role="tabpanel" aria-labelledby="ticket-workspace-tab-related" class="verevon-ticketing-detail-tabpanel">
                  <TicketLinkedResources ticket={ticket()} onOpenTicket={(ticketId) => navigate(`/tickets?ticketId=${encodeURIComponent(ticketId)}`)} />
                  <Show when={ticket().work_type === 'incident'}>
                    <IncidentProblemPanel
                      ticket={ticket()}
                      incidents={incidents()}
                      problems={problems()}
                      onDeclareIncident={declareIncidentForTicket}
                      onCreateProblem={createProblemForIncident}
                      onUpdateIncidentStatus={patchIncidentStatus}
                      onUpdateProblemStatus={patchProblemStatus}
                    />
                  </Show>
                  <TicketDependencyComposer
                    candidates={tickets()}
                    ticket={ticket()}
                    onLink={(target, linkType) => void linkTicketDependency(ticket(), target, linkType)}
                  />
                </div>
              </Show>

              <Show when={activeTicketTab() === 'activity'}>
                <div id="ticket-workspace-panel-activity" role="tabpanel" aria-labelledby="ticket-workspace-tab-activity" class="verevon-ticketing-detail-tabpanel">
			      <TicketTimeline activities={ticketActivity()} />
                </div>
              </Show>
            </>
          )}
        </Show>
      </section>

      <aside class="verevon-ticketing-context" aria-label={i18n.tr('Sakkontekst', 'Ticket context')}>
        <TicketingContextPanel
          orgId={ctx()?.orgId ?? ''}
          userId={ctx()?.userId ?? ''}
          role={ctx()?.role ?? ''}
          automationRules={automationRules()}
          macros={macros()}
		  teams={ticketTeams()}
		  onCreateTeam={createTeam}
		  onSetTeamActive={setTeamActive}
          onCreateChecklist={createChecklist}
          onMacroRun={(ticket, macro) => setMacroPreview({ ticket, macro })}
          onToggleChecklistItem={toggleChecklistItem}
		  onCreateSideConversation={createSideConversation}
		  onAddSideConversationMessage={addSideConversationMessage}
		  onUpdateSideConversationStatus={updateSideConversationStatus}
          queue={activeQueue()}
          slaPolicies={slaPolicies()}
          ticket={selectedTicket()}
          views={savedViews()}
        />
      </aside>
      <Show when={macroPreview()}>
        {(preview) => (
          <MacroPreviewDialog
            macro={preview().macro}
            ticket={preview().ticket}
            onCancel={() => setMacroPreview(null)}
            onConfirm={() => {
              const current = preview()
              setMacroPreview(null)
              void runMacroAction(current.ticket, current.macro)
            }}
          />
        )}
      </Show>
      <Show when={dependencyResolutionPreview()}>
        {(preview) => (
          <DependencyResolutionDialog
            ticket={preview().ticket}
            children={preview().children}
            onCancel={() => setDependencyResolutionPreview(null)}
            onConfirm={() => {
              const current = preview()
              setDependencyResolutionPreview(null)
              void patchTicket(current.ticket, { status: 'resolved' }, i18n.tr('Saken er løst.', 'Ticket resolved.'))
            }}
          />
        )}
      </Show>
      <Show when={bulkPreview()}>
        {(preview) => (
          <BulkStatusPreviewDialog
            status={preview().status}
            tickets={preview().tickets}
            busy={bulkExecuting()}
            onCancel={() => setBulkPreview(null)}
            onConfirm={() => {
              const current = preview()
              setBulkPreview(null)
              void applyBulkStatus(current.tickets, current.status)
            }}
          />
        )}
      </Show>
    </main>
  )
}

function bulkStatusLabel(status: BulkTicketStatus, tr: TrFn): string {
  switch (status) {
    case 'open': return tr('Åpen', 'Open')
    case 'waiting_customer': return tr('Venter på kunde', 'Waiting on customer')
    case 'waiting_team': return tr('Venter på team', 'Waiting on team')
    case 'escalated': return tr('Eskalert', 'Escalated')
  }
}

function BulkStatusPreviewDialog(props: {
  busy: boolean
  onCancel: () => void
  onConfirm: () => void
  status: BulkTicketStatus
  tickets: SupportTicket[]
}) {
  const i18n = useI18n()
  return (
    <div class="verevon-ticketing-macro-preview" role="dialog" aria-modal="true" aria-label={i18n.tr('Bekreft masseendring av status', 'Confirm bulk status change')}>
      <div class="verevon-ticketing-macro-preview__card">
        <h2>{i18n.tr('Gjennomgå masseendring', 'Review bulk change')}</h2>
        <p>{i18n.tr(
          `${props.tickets.length} saker settes til ${bulkStatusLabel(props.status, i18n.tr)}.`,
          `${props.tickets.length} tickets will be set to ${bulkStatusLabel(props.status, i18n.tr)}.`,
        )}</p>
        <p>{i18n.tr('Hver sak får sin egen reviderbare handling og kanoniske gjenlesing. Feil blir vist eksplisitt; ingen ufullstendig kjøring kalles vellykket.', 'Each ticket receives its own auditable action and canonical reread. Failures are shown explicitly; a partial run is never presented as successful.')}</p>
        <ul class="verevon-ticketing-bulk-preview-list">
          <For each={props.tickets}>{(ticket) => <li>{ticket.ticket_key}</li>}</For>
        </ul>
        <small>{i18n.tr('Løse, lukke, utsette, tildele eller kjøre makroer i bulk er ikke tilgjengelig i denne kontrollen.', 'Resolving, closing, snoozing, assigning, or running macros in bulk is not available in this control.')}</small>
        <div>
          <button type="button" disabled={props.busy} onClick={() => props.onCancel()}>{i18n.tr('Avbryt', 'Cancel')}</button>
          <button type="button" disabled={props.busy} onClick={() => props.onConfirm()}>{props.busy ? i18n.tr('Oppdaterer …', 'Updating…') : i18n.tr('Oppdater status', 'Update status')}</button>
        </div>
      </div>
    </div>
  )
}

function MacroPreviewDialog(props: { macro: TicketMacro; ticket: SupportTicket; onCancel: () => void; onConfirm: () => void }) {
  const i18n = useI18n()
  return (
    <div class="verevon-ticketing-macro-preview" role="dialog" aria-modal="true" aria-label={i18n.tr('Bekreft makro', 'Confirm macro')}>
      <div class="verevon-ticketing-macro-preview__card">
        <h2>{i18n.tr('Gjennomgå makro før kjøring', 'Review macro before running')}</h2>
        <p>{i18n.tr(`Makroen "${props.macro.name}" vil endre ${props.ticket.ticket_key}. Bekreft den kanoniske konfigurasjonen nedenfor.`, `The "${props.macro.name}" macro will change ${props.ticket.ticket_key}. Confirm the canonical configuration below.`)}</p>
        <h3>{i18n.tr('Handlinger', 'Actions')}</h3>
        <pre>{JSON.stringify(props.macro.actions, null, 2)}</pre>
        <Show when={Object.keys(props.macro.conditions).length > 0}>
          <h3>{i18n.tr('Betingelser', 'Conditions')}</h3>
          <pre>{JSON.stringify(props.macro.conditions, null, 2)}</pre>
        </Show>
        <small>{i18n.tr('Kjøring avvises dersom makroen endres etter denne gjennomgangen.', 'Execution is rejected if this macro changes after this review.')}</small>
        <div>
          <button type="button" onClick={() => props.onCancel()}>{i18n.tr('Avbryt', 'Cancel')}</button>
          <button type="button" onClick={() => props.onConfirm()}>{i18n.tr('Kjør makro', 'Run macro')}</button>
        </div>
      </div>
    </div>
  )
}

type TicketDependencyTarget = NonNullable<SupportTicket['linked_resources']>[number]['linked_ticket'] extends infer Target
  ? NonNullable<Target>
  : never

function unresolvedChildDependencies(ticket: SupportTicket): TicketDependencyTarget[] {
  return (ticket.linked_resources ?? [])
    .filter((link) => link.link_type === 'child' && link.linked_ticket && !isTerminalTicketStatus(link.linked_ticket.status))
    .map((link) => link.linked_ticket!)
}

function DependencyResolutionDialog(props: {
  ticket: SupportTicket
  children: TicketDependencyTarget[]
  onCancel: () => void
  onConfirm: () => void
}) {
  const i18n = useI18n()
  return (
    <div class="verevon-ticketing-macro-preview" role="dialog" aria-modal="true" aria-label={i18n.tr('Bekreft løsning med åpne underordnede saker', 'Confirm resolution with open child tickets')}>
      <div class="verevon-ticketing-macro-preview__card">
        <h2>{i18n.tr('Underordnede saker er fortsatt åpne', 'Child tickets are still open')}</h2>
        <p>{i18n.tr(
          `${props.ticket.ticket_key} har åpne underordnede saker. Å løse denne saken endrer ikke de underordnede sakene.`,
          `${props.ticket.ticket_key} has open child tickets. Resolving this ticket will not change those child tickets.`,
        )}</p>
        <ul>
          <For each={props.children}>
            {(child) => <li>{child.ticket_key} · {ticketStatusLabel(child.status, i18n.tr)} · {ticketWorkTypeLabel(child.work_type, i18n.tr)}</li>}
          </For>
        </ul>
        <small>{i18n.tr('Kontroller avhengighetene og fortsett bare hvis denne saken kan avsluttes uavhengig.', 'Review the dependencies and continue only if this ticket can be resolved independently.')}</small>
        <div>
          <button type="button" onClick={() => props.onCancel()}>{i18n.tr('Avbryt', 'Cancel')}</button>
          <button type="button" onClick={() => props.onConfirm()}>{i18n.tr('Løs likevel', 'Resolve anyway')}</button>
        </div>
      </div>
    </div>
  )
}

function TicketWorkspaceTab(props: {
  active: boolean
  id: TicketCenterTab
  label: string
  onSelect: (tab: TicketCenterTab) => void
}) {
  return (
    <button
      type="button"
      role="tab"
      id={`ticket-workspace-tab-${props.id}`}
      aria-controls={`ticket-workspace-panel-${props.id}`}
      aria-selected={props.active}
      tabIndex={props.active ? 0 : -1}
      onKeyDown={handleTabKeyDown}
      onClick={() => props.onSelect(props.id)}
    >
      {props.label}
    </button>
  )
}

function TicketingContextPanel(props: {
  orgId: string
  userId: string
  role: string
  automationRules: TicketAutomationRule[]
	macros: TicketMacro[]
	teams: TicketTeam[]
	onCreateTeam: (name: string, description: string) => Promise<boolean>
	onSetTeamActive: (team: TicketTeam, active: boolean) => Promise<void>
  onCreateChecklist: (ticket: SupportTicket) => void
  onMacroRun: (ticket: SupportTicket, macro: TicketMacro) => void
  onToggleChecklistItem: (ticket: SupportTicket, checklist: TicketChecklist, itemId: string, completed: boolean) => void
  onCreateSideConversation: (ticket: SupportTicket, input: { subject: string; body_text: string }) => Promise<void>
  onAddSideConversationMessage: (ticket: SupportTicket, input: { sideConversationId: string; body_text: string }) => Promise<void>
  onUpdateSideConversationStatus: (ticket: SupportTicket, input: { sideConversationId: string; status: 'open' | 'closed' }) => Promise<void>
  queue: TicketQueueId
  slaPolicies: SlaPolicy[]
  ticket: SupportTicket | null
  views: TicketView[]
}) {
  const i18n = useI18n()
  const [activeTab, setActiveTab] = createSignal<TicketContextTab>('details')
  const [conversationDetail] = createResource(
    () => {
      const ticket = props.ticket
      return activeTab() === 'verevon' && props.orgId && ticket?.conversation_id
        ? { conversationId: ticket.conversation_id, orgId: props.orgId }
        : undefined
    },
    ({ conversationId, orgId }) => getConversationDetail(orgId, conversationId),
  )
  const contextPack = createMemo(() => {
    const ticket = props.ticket
    const support: SupportAssistantContext | undefined = ticket
      ? {
        organization: props.orgId ? { id: props.orgId } : undefined,
        conversation: {
          id: ticket.conversation_id,
          title: ticket.conversation?.title,
          channel: ticket.conversation?.channel,
          status: ticket.conversation?.status ?? ticket.status,
          customer: ticket.conversation?.contact
            ? {
              id: ticket.conversation.contact.id,
              name: ticket.conversation.contact.name,
              email: ticket.conversation.contact.email,
            }
            : undefined,
        },
        ticket: {
          id: ticket.id,
          key: ticket.ticket_key,
          status: ticket.status,
          slaState: ticket.sla_state,
          priority: ticket.priority,
          severity: ticket.severity,
          category: ticket.category,
          intent: ticket.intent,
          assignee: ticket.assignee_name,
          team: ticket.team_name,
        },
        relatedConversations: (ticket.side_conversations ?? []).slice(0, 8).map((side) => ({
          id: side.id,
          title: side.subject,
          status: side.status,
        })),
        availableActions: [
          'support.suggest_next_action',
          'support.prepare_reply_proposal',
          'support.prepare_internal_note_proposal',
          'support.draft_reply',
          'support.summarize',
          'support.explain_escalation',
          'support.resolve_review',
        ],
        permissions: ['support.read'],
      }
      : undefined

    return buildModelContextPack({
      route: '/support/ticketing',
      selectedEntity: ticket ? {
        type: 'ticket',
        id: ticket.id,
        label: ticket.ticket_key,
        status: ticket.status,
      } : undefined,
      visibleItems: ticket ? [{
        type: 'ticket',
        id: ticket.id,
        label: ticket.ticket_key,
        status: ticket.status,
      }] : [],
      filters: { queue: props.queue },
      support,
    })
  })
  const messages = createMemo<AssistMessage[]>(() => {
    const loaded = conversationDetail()?.articles
    if (loaded?.length) {
      return loaded.map((article) => ({
        id: String(article.id),
        agent: article.sender?.toLowerCase() === 'agent',
        from: article.from,
        body: article.bodyText || article.body || '',
        internal: article.internal,
      })).filter((message) => message.body.trim())
    }
    const preview = props.ticket?.conversation?.last_message_preview?.trim()
    return preview ? [{
      agent: false,
      from: ticketCustomerLabel(props.ticket!, i18n.tr),
      body: preview,
    }] : []
  })

  createEffect(() => {
    void props.ticket?.id
    setActiveTab('details')
  })

  return (
    <div class="verevon-ticketing-context-shell">
      <div class="verevon-ticketing-context-tabs" role="tablist" aria-label={i18n.tr('Sakkontekst', 'Ticket context')}>
        <TicketContextTabButton active={activeTab() === 'details'} id="details" label={i18n.tr('Detaljer', 'Details')} onSelect={setActiveTab} />
        <TicketContextTabButton active={activeTab() === 'verevon'} id="verevon" label="Verevon" onSelect={setActiveTab} />
        <TicketContextTabButton active={activeTab() === 'actions'} id="actions" label={i18n.tr('Handlinger', 'Actions')} onSelect={setActiveTab} />
        <TicketContextTabButton active={activeTab() === 'audit'} id="audit" label={i18n.tr('Revisjon', 'Audit')} onSelect={setActiveTab} />
      </div>
      <div class="verevon-ticketing-context__scroll">
      <Show when={activeTab() === 'details'}>
        <div id="ticket-context-panel-details" role="tabpanel" aria-labelledby="ticket-context-tab-details">
      <TicketingPanel title={i18n.tr('Køhelse', 'Queue health')} icon={TimerReset}>
        <div class="verevon-ticketing-health-grid">
          <TicketMetric label={i18n.tr('Kø', 'Queue')} value={ticketQueueShortLabel(props.queue, i18n.tr)} />
          <TicketMetric label="SLA" value={props.ticket?.sla_state === 'breached' ? i18n.tr('Brutt', 'Breached') : props.ticket?.sla_state === 'risk' ? i18n.tr('I faresonen', 'At risk') : i18n.tr('Spores', 'Tracked')} tone={props.ticket?.sla_state === 'breached' ? 'danger' : undefined} />
          <TicketMetric label={i18n.tr('Visninger', 'Views')} value={String(props.views.length)} />
          <TicketMetric label={i18n.tr('Regler', 'Rules')} value={String(props.automationRules.filter((rule) => rule.active).length)} />
        </div>
      </TicketingPanel>

      <TicketingPanel title={i18n.tr('Kunde', 'Customer')} icon={UserRound}>
        <Show when={props.ticket} fallback={<p class="verevon-ticketing-panel-muted">{i18n.tr('Velg en sak for å laste kundekontekst.', 'Select a ticket to load customer context.')}</p>}>
          {(ticket) => (
            <div class="verevon-ticketing-customer">
              <strong>{ticketCustomerLabel(ticket(), i18n.tr)}</strong>
              <span>{ticket().conversation?.contact?.email || i18n.tr('Ingen e-post registrert', 'No email attached')}</span>
              <small>{ticket().conversation?.channel || i18n.tr('Samtale', 'Conversation')} · {ticket().conversation?.status || ticket().status}</small>
            </div>
          )}
        </Show>
      </TicketingPanel>
        </div>
      </Show>

      <Show when={activeTab() === 'verevon'}>
        <div id="ticket-context-panel-verevon" role="tabpanel" aria-labelledby="ticket-context-tab-verevon">
          <SupportVerevonComposer
            contextLabel={props.ticket ? i18n.tr(`Kontekst: ${props.ticket.ticket_key}`, `Context: ${props.ticket.ticket_key}`) : i18n.tr('Velg en sak', 'Select a ticket')}
            contextPack={contextPack()}
            conversationId={props.ticket?.conversation_id ?? ''}
            ticketId={props.ticket?.id}
            conversationLoading={conversationDetail.loading}
            conversationSource={conversationDetail.error ? (messages().length > 0 ? 'preview' : 'unavailable') : conversationDetail()?.articles ? 'authorized' : 'preview'}
            customer={props.ticket ? ticketCustomerLabel(props.ticket, i18n.tr) : undefined}
            messages={messages()}
            orgId={props.orgId}
            userId={props.userId}
          />
        </div>
      </Show>

      <Show when={activeTab() === 'actions'}>
        <div id="ticket-context-panel-actions" role="tabpanel" aria-labelledby="ticket-context-tab-actions">
		<TicketTeamsPanel teams={props.teams} onCreate={props.onCreateTeam} onSetActive={props.onSetTeamActive} />
      <TicketingPanel title={i18n.tr('Makroer', 'Macros')} icon={Zap}>
        <div class="verevon-ticketing-macro-list">
          <For each={props.macros.slice(0, 4)}>
            {(macro) => (
              <TicketMacroButton
                macro={macro}
                disabled={!props.ticket}
                onClick={() => props.ticket && props.onMacroRun(props.ticket, macro)}
              />
            )}
          </For>
        </div>
      </TicketingPanel>

      <TicketingPanel title={i18n.tr('Sjekkliste', 'Checklist')} icon={ListChecks}>
        <Show when={props.ticket} fallback={<p class="verevon-ticketing-panel-muted">{i18n.tr('Velg en sak for å administrere sjekklistearbeid.', 'Select a ticket to manage checklist work.')}</p>}>
          {(ticket) => (
            <TicketChecklistPanel
              ticket={ticket()}
              onCreateChecklist={() => props.onCreateChecklist(ticket())}
              onToggleItem={(checklist, itemId, completed) => props.onToggleChecklistItem(ticket(), checklist, itemId, completed)}
            />
          )}
        </Show>
      </TicketingPanel>
        </div>
      </Show>

      <Show when={activeTab() === 'audit'}>
        <div id="ticket-context-panel-audit" role="tabpanel" aria-labelledby="ticket-context-tab-audit">
      <TicketingPanel title={i18n.tr('Interne samtaler', 'Internal conversations')} icon={MessageSquare}>
        <Show when={props.ticket} fallback={<p class="verevon-ticketing-panel-muted">{i18n.tr('Velg en sak for intern koordinering.', 'Select a ticket for internal coordination.')}</p>}>
          {(ticket) => <TicketSideConversationPanel
            ticket={ticket()}
            onCreate={(input) => props.onCreateSideConversation(ticket(), input)}
            onReply={(input) => props.onAddSideConversationMessage(ticket(), input)}
            onStatus={(input) => props.onUpdateSideConversationStatus(ticket(), input)}
          />}
        </Show>
      </TicketingPanel>

      <TicketingPanel title={i18n.tr('Automatiseringssperrer', 'Automation guardrails')} icon={ShieldAlert}>
        <ul class="verevon-ticketing-policy-list">
          <For each={props.automationRules.slice(0, 3)}>
            {(rule) => (
              <li>
                <span>{rule.event_name.replace(/\./g, ' ')}</span>
                <strong>{rule.name}</strong>
              </li>
            )}
          </For>
        </ul>
      </TicketingPanel>
        </div>
      </Show>
      </div>
    </div>
  )
}

function TicketContextTabButton(props: {
  active: boolean
  id: TicketContextTab
  label: string
  onSelect: (tab: TicketContextTab) => void
}) {
  return (
    <button
      type="button"
      role="tab"
      id={`ticket-context-tab-${props.id}`}
      aria-controls={`ticket-context-panel-${props.id}`}
      aria-selected={props.active}
      tabIndex={props.active ? 0 : -1}
      onKeyDown={handleTabKeyDown}
      onClick={() => props.onSelect(props.id)}
    >
      {props.label}
    </button>
  )
}

function TicketTeamsPanel(props: {
  teams: TicketTeam[]
  onCreate: (name: string, description: string) => Promise<boolean>
  onSetActive: (team: TicketTeam, active: boolean) => Promise<void>
}) {
  const i18n = useI18n()
  const [name, setName] = createSignal('')
  const [description, setDescription] = createSignal('')
  const [saving, setSaving] = createSignal(false)

  const create = async () => {
    if (!name().trim() || saving()) return
    setSaving(true)
    try {
      if (await props.onCreate(name(), description())) {
        setName('')
        setDescription('')
      }
    } finally {
      setSaving(false)
    }
  }

  return (
    <TicketingPanel title={i18n.tr('Ticketing-team', 'Ticketing teams')} icon={UsersRound}>
      <form class="verevon-ticketing-team-form" onSubmit={(event) => { event.preventDefault(); void create() }}>
        <input
          aria-label={i18n.tr('Nytt Ticketing-teamnavn', 'New Ticketing team name')}
          value={name()}
          onInput={(event) => setName(event.currentTarget.value)}
          placeholder={i18n.tr('Teamnavn', 'Team name')}
        />
        <input
          aria-label={i18n.tr('Team-beskrivelse', 'Team description')}
          value={description()}
          onInput={(event) => setDescription(event.currentTarget.value)}
          placeholder={i18n.tr('Valgfri beskrivelse', 'Optional description')}
        />
        <button type="submit" disabled={!name().trim() || saving()}>
          {saving() ? i18n.tr('Oppretter …', 'Creating…') : i18n.tr('Opprett team', 'Create team')}
        </button>
      </form>
      <Show
        when={props.teams.length > 0}
        fallback={<p class="verevon-ticketing-panel-muted">{i18n.tr('Ingen kanoniske team er konfigurert ennå.', 'No canonical Ticketing teams are configured yet.')}</p>}
      >
        <ul class="verevon-ticketing-team-list">
          <For each={props.teams}>
            {(team) => (
              <li>
                <div>
                  <strong>{team.name}</strong>
                  <small>{team.description || (team.active ? i18n.tr('Aktiv', 'Active') : i18n.tr('Inaktiv', 'Inactive'))}</small>
                </div>
                <button type="button" onClick={() => void props.onSetActive(team, !team.active)}>
                  {team.active ? i18n.tr('Deaktiver', 'Deactivate') : i18n.tr('Aktiver', 'Activate')}
                </button>
              </li>
            )}
          </For>
        </ul>
      </Show>
      <p class="verevon-ticketing-panel-muted">{i18n.tr('Disse teamene er Ticketings eneste rutingmyndighet. Innboksgrupper er kun kildekontekst.', 'These teams are Ticketing’s only routing authority. Inbox groups are source context only.')}</p>
    </TicketingPanel>
  )
}

function TicketingPanel(props: { title: string; icon: Component<LucideProps>; children: JSX.Element }) {
  return (
    <section class="verevon-ticketing-panel">
      <div class="verevon-ticketing-panel__header">
        <Dynamic component={props.icon} class="size-4" />
        <h3>{props.title}</h3>
      </div>
      {props.children}
    </section>
  )
}

function TicketMetric(props: { label: string; value: string; tone?: 'danger' }) {
  return (
    <div class={cn('verevon-ticketing-metric', props.tone === 'danger' && 'verevon-ticketing-metric--danger')}>
      <span>{props.label}</span>
      <strong>{props.value}</strong>
    </div>
  )
}

function TicketMacroButton(props: { disabled: boolean; macro: TicketMacro; onClick: () => void }) {
  const i18n = useI18n()
  return (
    <button type="button" class="verevon-ticketing-macro" disabled={props.disabled} onClick={() => props.onClick()}>
      <span>
        <Sparkles class="size-4" />
        {props.macro.name}
      </span>
      <small>{props.macro.description || macroActionSummary(props.macro.actions, i18n.tr)}</small>
    </button>
  )
}

function TicketSideConversationPanel(props: {
  ticket: SupportTicket
  onCreate: (input: { subject: string; body_text: string }) => Promise<void>
  onReply: (input: { sideConversationId: string; body_text: string }) => Promise<void>
  onStatus: (input: { sideConversationId: string; status: 'open' | 'closed' }) => Promise<void>
}) {
  const i18n = useI18n()
  const [subject, setSubject] = createSignal('')
  const [body, setBody] = createSignal('')
  const [replies, setReplies] = createSignal<Record<string, string>>({})
  const threads = () => props.ticket.side_conversations ?? []
  const create = async () => {
    if (!subject().trim() || !body().trim()) return
    await props.onCreate({ subject: subject(), body_text: body() })
    setSubject('')
    setBody('')
  }
  const reply = async (sideConversationId: string) => {
    const bodyText = replies()[sideConversationId]?.trim()
    if (!bodyText) return
    await props.onReply({ sideConversationId, body_text: bodyText })
    setReplies((current) => ({ ...current, [sideConversationId]: '' }))
  }
  return (
    <div class="verevon-ticketing-side-conversations">
      <p class="verevon-ticketing-panel-muted">{i18n.tr('Kun internt. Dette sender aldri en kundeoppdatering og erstatter ikke Verevon Chat.', 'Internal only. This never sends a customer update and does not replace Verevon Chat.')}</p>
      <form class="verevon-ticketing-side-conversation-form" onSubmit={(event) => { event.preventDefault(); void create() }}>
        <input aria-label={i18n.tr('Emne for intern samtale', 'Internal conversation subject')} value={subject()} maxLength={160} onInput={(event) => setSubject(event.currentTarget.value)} placeholder={i18n.tr('Hva trenger du avklaring på?', 'What needs coordination?')} />
        <textarea aria-label={i18n.tr('Første interne melding', 'First internal message')} value={body()} maxLength={4000} onInput={(event) => setBody(event.currentTarget.value)} placeholder={i18n.tr('Skriv kontekst og spørsmål for teamet.', 'Add context and a question for the team.')} />
        <button type="submit" disabled={!subject().trim() || !body().trim()}>{i18n.tr('Start intern samtale', 'Start internal conversation')}</button>
      </form>
      <Show when={threads().length > 0} fallback={<p class="verevon-ticketing-panel-muted">{i18n.tr('Ingen interne samtaler på denne saken ennå.', 'No internal conversations on this ticket yet.')}</p>}>
        <For each={threads()}>{(thread) => (
          <article class="verevon-ticketing-side-thread">
            <header><div><MessageSquare class="size-4" /><strong>{thread.subject}</strong></div><button type="button" onClick={() => void props.onStatus({ sideConversationId: thread.id, status: thread.status === 'open' ? 'closed' : 'open' })}>{thread.status === 'open' ? i18n.tr('Lukk', 'Close') : i18n.tr('Åpne igjen', 'Reopen')}</button></header>
            <For each={thread.messages}>{(message) => <p>{message.body_text}</p>}</For>
            <Show when={thread.status === 'open'}>
              <form class="verevon-ticketing-side-reply" onSubmit={(event) => { event.preventDefault(); void reply(thread.id) }}>
                <input aria-label={i18n.tr(`Svar på ${thread.subject}`, `Reply to ${thread.subject}`)} value={replies()[thread.id] ?? ''} maxLength={4000} onInput={(event) => setReplies((current) => ({ ...current, [thread.id]: event.currentTarget.value }))} placeholder={i18n.tr('Internt svar', 'Internal reply')} />
                <button type="submit" disabled={!replies()[thread.id]?.trim()}>{i18n.tr('Svar', 'Reply')}</button>
              </form>
            </Show>
          </article>
        )}</For>
      </Show>
    </div>
  )
}

function TicketSlaSnapshot(props: { ticket: SupportTicket; policies: SlaPolicy[] }) {
  const i18n = useI18n()
  const policy = () => props.policies.find((item) => item.id === props.ticket.sla_policy_id) ?? props.policies[0]
  return (
    <div class={cn('verevon-ticketing-sla-card', props.ticket.sla_state === 'breached' && 'verevon-ticketing-sla-card--danger')}>
      <TimerReset class="size-4" />
      <div>
        <span>{policy()?.name ?? i18n.tr('SLA-policy', 'SLA policy')}</span>
        <strong>{props.ticket.due_at ? i18n.tr(`Forfaller ${formatRelativeTime(props.ticket.due_at)}`, `Due ${formatRelativeTime(props.ticket.due_at)} from now`) : i18n.tr(`${minutesLabel(policy()?.resolution_minutes ?? 0, i18n.tr)} løsningsmål`, `${minutesLabel(policy()?.resolution_minutes ?? 0, i18n.tr)} resolution target`)}</strong>
      </div>
      <small>{props.ticket.sla_state || 'ok'}</small>
    </div>
  )
}

function TicketTimeline(props: { activities: TicketActivity[] }) {
  const i18n = useI18n()
  return (
    <div class="verevon-ticketing-timeline">
      <div class="verevon-ticketing-timeline__header">
        <FileText class="size-4" />
        <strong>{i18n.tr('Aktivitet', 'Activity')}</strong>
      </div>
      <Show when={props.activities.length > 0} fallback={<p class="verevon-ticketing-timeline__empty">{i18n.tr('Ingen saksaktivitet er registrert ennå.', 'No ticket activity has been recorded yet.')}</p>}>
        <ol>
          <For each={props.activities}>
            {(activity) => (
              <li>
                <TicketActivityIcon action={activity.action} />
                <span>
                  {ticketActivityLabel(activity, i18n.tr)}
                  <time dateTime={activity.created_at}>{i18n.tr(` · ${formatRelativeTime(activity.created_at)} siden`, ` · ${formatRelativeTime(activity.created_at)} ago`)}</time>
                </span>
              </li>
            )}
          </For>
        </ol>
      </Show>
    </div>
  )
}

function TicketActivityIcon(props: { action: TicketActivity['action'] }) {
  if (props.action === 'ticket.created') return <FileText class="size-4" />
  if (props.action === 'ticket.linked') return <Link2 class="size-4" />
  if (props.action === 'ticket.macro_run') return <Zap class="size-4" />
  if (props.action === 'ticket.checklist_created' || props.action === 'ticket.checklist_item_updated') return <ListChecks class="size-4" />
	if (props.action === 'ticket.side_conversation_created' || props.action === 'ticket.side_conversation_message_added' || props.action === 'ticket.side_conversation_updated') return <MessageSquare class="size-4" />
	if (props.action === 'ticket.chat_handoff_requested') return <Bot class="size-4" />
  return <Tag class="size-4" />
}

function ticketActivityLabel(activity: TicketActivity, tr: (no: string, en: string) => string) {
  if (activity.action === 'ticket.created') return tr('Saken ble opprettet', 'Ticket created')
  if (activity.action === 'ticket.linked') {
    const resource = activity.resource_kind ? activity.resource_kind.replace(/_/g, ' ') : tr('ressurs', 'resource')
    return tr(`Lenket ${resource}`, `Linked ${resource}`)
  }
  if (activity.action === 'ticket.macro_run') return tr('Makro ble kjørt', 'Macro run')
  if (activity.action === 'ticket.checklist_created') return tr('Sjekkliste ble lagt til', 'Checklist added')
  if (activity.action === 'ticket.checklist_item_updated') return tr('Sjekklistestatus ble oppdatert', 'Checklist status updated')
	if (activity.action === 'ticket.side_conversation_created') return tr('Intern samtale ble startet', 'Internal conversation started')
	if (activity.action === 'ticket.side_conversation_message_added') return tr('Internt svar ble lagt til', 'Internal reply added')
	if (activity.action === 'ticket.side_conversation_updated') return tr('Intern samtale ble oppdatert', 'Internal conversation updated')
	if (activity.action === 'ticket.chat_handoff_requested') return tr('Verevon Chat-handoff ble forespurt', 'Verevon Chat handoff requested')
  return tr('Saken ble oppdatert', 'Ticket updated')
}

function TicketConversationPanel(props: { ticket: SupportTicket; onOpenInbox: () => void }) {
  const i18n = useI18n()
  const conversation = () => props.ticket.conversation
  return (
    <section class="verevon-ticketing-conversation" aria-label={i18n.tr('Samtale', 'Conversation')}>
      <div class="verevon-ticketing-conversation__header">
        <div>
          <MessageSquare class="size-4" />
          <strong>{i18n.tr('Kundesamtale', 'Customer conversation')}</strong>
        </div>
        <Show when={props.ticket.conversation_id}>
          <button type="button" class="verevon-inbox-button verevon-inbox-button--secondary verevon-inbox-button--sm" onClick={() => props.onOpenInbox()}>
            {i18n.tr('Åpne full samtale', 'Open full conversation')}
          </button>
        </Show>
      </div>
      <Show when={conversation()} fallback={<p class="verevon-ticketing-panel-muted">{i18n.tr('Ingen samtale er knyttet til denne saken.', 'No conversation is linked to this ticket.')}</p>}>
        {(current) => (
          <>
            <div class="verevon-ticketing-conversation__meta">
              <strong>{current().title || i18n.tr('Support-samtale', 'Support conversation')}</strong>
              <span>{current().contact?.name || i18n.tr('Ukjent kunde', 'Unknown customer')} · {current().channel || i18n.tr('Kanal ukjent', 'Unknown channel')}</span>
            </div>
            <div class="verevon-ticketing-conversation__message">
              <span>{current().contact?.name || i18n.tr('Kunde', 'Customer')}</span>
              <p>{current().last_message_preview || i18n.tr('Ingen meldingsforhåndsvisning er tilgjengelig.', 'No message preview is available.')}</p>
            </div>
          </>
        )}
      </Show>
    </section>
  )
}

function TicketLinkedResources(props: { ticket: SupportTicket; onOpenTicket: (ticketId: string) => void }) {
  const i18n = useI18n()
  const links = () => props.ticket.linked_resources ?? []
  return (
    <Show when={links().length > 0}>
      <div class="verevon-ticketing-links">
        <div class="verevon-ticketing-links__header">
          <Link2 class="size-4" />
          <strong>{i18n.tr('Lenkede ressurser', 'Linked resources')}</strong>
        </div>
        <ul>
          <For each={links()}>
            {(link) => {
              const safeUrl = safeTicketResourceUrl(link.resource_url)
              const linkedTicketId = link.resource_kind === 'ticket' ? link.linked_ticket?.id ?? link.resource_id : undefined
              const target = link.linked_ticket
              return (
                <li>
                  <div>
                    <span>{target?.ticket_key || link.label || link.resource_kind.replace(/_/g, ' ')}</span>
                    <small>
                      {ticketRelationshipLabel(link.link_type, i18n.tr)} · {target
                        ? `${ticketStatusLabel(target.status, i18n.tr)} · ${ticketWorkTypeLabel(target.work_type, i18n.tr)}`
                        : link.resource_id || link.resource_url || link.resource_kind}
                    </small>
                  </div>
                  <Show when={safeUrl}>
                    {(url) => (
                      <a href={url()} target="_blank" rel="noreferrer" aria-label={i18n.tr(`Åpne ${link.label || link.resource_kind}`, `Open ${link.label || link.resource_kind}`)}>
                        <ExternalLink class="size-4" />
                      </a>
                    )}
                  </Show>
                  <Show when={linkedTicketId}>
                    {(ticketId) => (
                      <button
                        type="button"
                        onClick={() => props.onOpenTicket(ticketId())}
                        aria-label={i18n.tr(`Åpne ${link.label || 'lenket sak'}`, `Open ${link.label || 'linked ticket'}`)}
                      >
                        <ExternalLink class="size-4" />
                      </button>
                    )}
                  </Show>
                </li>
              )
            }}
          </For>
        </ul>
      </div>
    </Show>
  )
}

function IncidentProblemPanel(props: {
  ticket: SupportTicket
  incidents: SupportIncident[]
  problems: SupportProblem[]
  onDeclareIncident: (ticket: SupportTicket, title: string, problemId?: string) => Promise<boolean>
  onCreateProblem: (title: string) => Promise<boolean>
  onUpdateIncidentStatus: (incident: SupportIncident, status: string) => Promise<void>
  onUpdateProblemStatus: (problem: SupportProblem, status: string) => Promise<void>
}) {
  const i18n = useI18n()
  const [incidentTitle, setIncidentTitle] = createSignal('')
  const [problemTitle, setProblemTitle] = createSignal('')
  const [problemId, setProblemId] = createSignal('')
  const linkedIncidents = createMemo(() => props.incidents.filter((incident) =>
    (incident.ticket_links ?? []).some((link) => link.ticket_id === props.ticket.id),
  ))

  const declare = async () => {
    const created = await props.onDeclareIncident(props.ticket, incidentTitle(), problemId() || undefined)
    if (created) setIncidentTitle('')
  }
  const createProblemRecord = async () => {
    const created = await props.onCreateProblem(problemTitle())
    if (created) setProblemTitle('')
  }

  return (
    <section class="verevon-ticketing-dependency" aria-label={i18n.tr('Hendelser og problemer', 'Incidents and problems')}>
      <div>
        <ShieldAlert class="size-4" />
        <strong>{i18n.tr('Hendelser og problemer', 'Incidents and problems')}</strong>
        <small>{i18n.tr('Egne operative poster. Ingen status endres automatisk på den lenkede saken.', 'Separate operational records. No linked ticket status changes automatically.')}</small>
      </div>

      <Show when={linkedIncidents().length > 0} fallback={
        <div class="verevon-ticketing-dependency__controls">
          <input value={incidentTitle()} onInput={(event) => setIncidentTitle(event.currentTarget.value)} placeholder={i18n.tr('Beskriv hendelsen', 'Describe the incident')} aria-label={i18n.tr('Hendelsestittel', 'Incident title')} />
          <select value={problemId()} onChange={(event) => setProblemId(event.currentTarget.value)} aria-label={i18n.tr('Knyttet problem', 'Linked problem')}>
            <option value="">{i18n.tr('Ingen kjent rotårsak ennå', 'No known root cause yet')}</option>
            <For each={props.problems}>{(problem) => <option value={problem.id}>{problem.problem_key} — {problem.title}</option>}</For>
          </select>
          <button type="button" disabled={!incidentTitle().trim()} onClick={() => void declare()}>{i18n.tr('Erklær hendelse', 'Declare incident')}</button>
        </div>
      }>
        <For each={linkedIncidents()}>
          {(incident) => (
            <div class="verevon-ticketing-automation-rule">
              <RuleRow label={`${incident.incident_key} · ${incident.title}`} detail={`${incident.severity} · ${incident.owner_name || i18n.tr('Ikke tildelt', 'Unassigned')}`} />
              <select value={incident.status} aria-label={i18n.tr(`Status for ${incident.incident_key}`, `Status for ${incident.incident_key}`)} onChange={(event) => void props.onUpdateIncidentStatus(incident, event.currentTarget.value)}>
                <option value="declared">{i18n.tr('Erklært', 'Declared')}</option>
                <option value="investigating">{i18n.tr('Undersøker', 'Investigating')}</option>
                <option value="monitoring">{i18n.tr('Overvåker', 'Monitoring')}</option>
                <option value="resolved">{i18n.tr('Løst', 'Resolved')}</option>
              </select>
            </div>
          )}
        </For>
      </Show>

      <div class="verevon-ticketing-dependency__controls">
        <input value={problemTitle()} onInput={(event) => setProblemTitle(event.currentTarget.value)} placeholder={i18n.tr('Registrer et problem / rotårsak', 'Record a problem / root cause')} aria-label={i18n.tr('Problemtittel', 'Problem title')} />
        <button type="button" disabled={!problemTitle().trim()} onClick={() => void createProblemRecord()}>{i18n.tr('Registrer problem', 'Record problem')}</button>
      </div>
      <For each={props.problems.filter((problem) => problem.status !== 'resolved').slice(0, 3)}>
        {(problem) => (
          <div class="verevon-ticketing-automation-rule">
            <RuleRow label={`${problem.problem_key} · ${problem.title}`} detail={problem.owner_name || i18n.tr('Ikke tildelt', 'Unassigned')} />
            <select value={problem.status} aria-label={i18n.tr(`Status for ${problem.problem_key}`, `Status for ${problem.problem_key}`)} onChange={(event) => void props.onUpdateProblemStatus(problem, event.currentTarget.value)}>
              <option value="investigating">{i18n.tr('Undersøker', 'Investigating')}</option>
              <option value="known_error">{i18n.tr('Kjent feil', 'Known error')}</option>
              <option value="resolved">{i18n.tr('Løst', 'Resolved')}</option>
            </select>
          </div>
        )}
      </For>
    </section>
  )
}

function ticketRelationshipLabel(linkType: string | undefined, tr: TrFn) {
  if (linkType === 'parent') return tr('Overordnet sak', 'Parent ticket')
  if (linkType === 'child') return tr('Underordnet sak', 'Child ticket')
  if (linkType === 'related') return tr('Relatert sak', 'Related ticket')
  return tr('Lenket ressurs', 'Linked resource')
}

function TicketDependencyComposer(props: {
  ticket: SupportTicket
  candidates: SupportTicket[]
  onLink: (target: SupportTicket, linkType: 'parent' | 'child' | 'related') => void
}) {
  const i18n = useI18n()
  const [targetId, setTargetId] = createSignal('')
  const [linkType, setLinkType] = createSignal<'parent' | 'child' | 'related'>('related')
  const candidates = createMemo(() => {
    const existing = new Set(
      (props.ticket.linked_resources ?? [])
        .filter((link) => link.resource_kind === 'ticket')
        .map((link) => link.resource_id)
        .filter((id): id is string => Boolean(id)),
    )
    return props.candidates.filter((candidate) => candidate.id !== props.ticket.id && !existing.has(candidate.id))
  })
  const selected = createMemo(() => candidates().find((candidate) => candidate.id === targetId()) ?? null)

  return (
    <section class="verevon-ticketing-dependency" aria-label={i18n.tr('Koble saker', 'Link tickets')}>
      <div>
        <Link2 class="size-4" />
        <strong>{i18n.tr('Koble saker', 'Link tickets')}</strong>
        <small>{i18n.tr('Opprett et synlig saksforhold med revidert endring.', 'Create a visible ticket relationship with an audited change.')}</small>
      </div>
      <div class="verevon-ticketing-dependency__controls">
        <select aria-label={i18n.tr('Sak som skal lenkes', 'Ticket to link')} value={targetId()} onChange={(event) => setTargetId(event.currentTarget.value)}>
          <option value="">{i18n.tr('Velg sak', 'Select ticket')}</option>
          <For each={candidates()}>
            {(candidate) => <option value={candidate.id}>{candidate.ticket_key} — {candidate.conversation?.title || candidate.intent || candidate.category || i18n.tr('Support-sak', 'Support ticket')}</option>}
          </For>
        </select>
        <select aria-label={i18n.tr('Type saksforhold', 'Ticket relationship type')} value={linkType()} onChange={(event) => setLinkType(event.currentTarget.value as 'parent' | 'child' | 'related')}>
          <option value="related">{i18n.tr('Relatert', 'Related')}</option>
          <option value="parent">{i18n.tr('Overordnet', 'Parent')}</option>
          <option value="child">{i18n.tr('Underordnet', 'Child')}</option>
        </select>
        <button type="button" disabled={!selected()} onClick={() => selected() && props.onLink(selected()!, linkType())}>
          {i18n.tr('Koble sak', 'Link ticket')}
        </button>
      </div>
      <Show when={candidates().length === 0}>
        <p>{i18n.tr('Ingen andre tilgjengelige saker å koble til.', 'No other available tickets to link.')}</p>
      </Show>
    </section>
  )
}

function TicketChecklistPanel(props: {
  ticket: SupportTicket
  onCreateChecklist: () => void
  onToggleItem: (checklist: TicketChecklist, itemId: string, completed: boolean) => void
}) {
  const i18n = useI18n()
  const checklists = () => props.ticket.checklists ?? []
  return (
    <div class="verevon-ticketing-checklists">
      <Show when={checklists().length > 0} fallback={
        <button type="button" class="verevon-ticketing-macro" onClick={props.onCreateChecklist}>
          <span>
            <ListChecks class="size-4" />
            {i18n.tr('Legg til løsningssjekkliste', 'Add resolution checklist')}
          </span>
          <small>{i18n.tr('Eier, konsekvens, kundeoppdatering', 'Owner, impact, customer update')}</small>
        </button>
      }>
        <For each={checklists()}>
          {(checklist) => (
            <div class="verevon-ticketing-checklist">
              <div class="verevon-ticketing-checklist__header">
                <strong>{checklist.name}</strong>
                <small>{checklist.items.filter((item) => item.completed).length}/{checklist.items.length}</small>
              </div>
              <For each={checklist.items}>
                {(item) => (
                  <label class="verevon-ticketing-checklist-item">
                    <input
                      type="checkbox"
                      checked={item.completed}
                      onChange={(event) => props.onToggleItem(checklist, item.id, event.currentTarget.checked)}
                    />
                    <span>{item.label}</span>
                  </label>
                )}
              </For>
            </div>
          )}
        </For>
      </Show>
    </div>
  )
}

function RulesWorkspace(props: {
  activeView?: string
  automationRules: TicketAutomationRule[]
  macros: TicketMacro[]
  slaPolicies: SlaPolicy[]
  views: TicketView[]
  onCreateMacro: (input: { name: string; description: string; status: string }) => Promise<boolean>
  onSelectView: (view: TicketView) => void
	onSetAutomationRuleActive: (rule: TicketAutomationRule, active: boolean) => Promise<void>
	onCreateAutomationRule: (input: { name: string; eventName: string; conditionKey: string; conditionValue: string; actionKey: string; actionValue: string }) => Promise<boolean>
	canManageAutomation: boolean
}) {
  const i18n = useI18n()
  const [macroName, setMacroName] = createSignal('')
  const [macroDescription, setMacroDescription] = createSignal('')
  const [macroStatus, setMacroStatus] = createSignal('waiting_team')
  const [creatingMacro, setCreatingMacro] = createSignal(false)
	const [ruleName, setRuleName] = createSignal('')
	const [ruleEvent, setRuleEvent] = createSignal('ticket.created')
	const [conditionKey, setConditionKey] = createSignal('category')
	const [conditionValue, setConditionValue] = createSignal('')
	const [actionKey, setActionKey] = createSignal('priority')
	const [actionValue, setActionValue] = createSignal('high')
	const [creatingRule, setCreatingRule] = createSignal(false)
  const submitMacro = async () => {
    if (!macroName().trim() || creatingMacro()) return
    setCreatingMacro(true)
    const created = await props.onCreateMacro({ name: macroName(), description: macroDescription(), status: macroStatus() })
    if (created) {
      setMacroName('')
      setMacroDescription('')
      setMacroStatus('waiting_team')
    }
    setCreatingMacro(false)
  }
	const submitRule = async () => {
		if (creatingRule()) return
		setCreatingRule(true)
		const created = await props.onCreateAutomationRule({ name: ruleName(), eventName: ruleEvent(), conditionKey: conditionKey(), conditionValue: conditionValue(), actionKey: actionKey(), actionValue: actionValue() })
		if (created) { setRuleName(''); setConditionValue('') }
		setCreatingRule(false)
	}
  return (
    <div class="verevon-ticketing-rules-workspace">
      <RulesSection title={i18n.tr('Lagrede visninger', 'Saved views')} detail={i18n.tr('Zammad-stil oversikter og Chatwoot-stil egendefinerte filtre', 'Zammad-style overviews and Chatwoot-style custom filters')}>
        <For each={props.views}>
          {(view) => <RuleRow label={view.name} detail={i18n.tr(`${view.scope} · ${view.group_by || 'ugruppert'} · ${Object.keys(view.filter ?? {}).length} filtre`, `${view.scope} · ${view.group_by || 'ungrouped'} · ${Object.keys(view.filter ?? {}).length} filters`)} onClick={() => props.onSelectView(view)} />}
        </For>
      </RulesSection>
      <RulesSection title={i18n.tr('Makroer', 'Macros')} detail={i18n.tr('Gjenbrukbare flertrinns sakhandlinger', 'Reusable multi-step ticket actions')}>
        <div class="verevon-ticketing-macro-create">
          <input value={macroName()} onInput={(event) => setMacroName(event.currentTarget.value)} placeholder={i18n.tr('Makronavn', 'Macro name')} aria-label={i18n.tr('Makronavn', 'Macro name')} required />
          <input value={macroDescription()} onInput={(event) => setMacroDescription(event.currentTarget.value)} placeholder={i18n.tr('Kort beskrivelse', 'Short description')} aria-label={i18n.tr('Makrobeskrivelse', 'Macro description')} />
          <select value={macroStatus()} onChange={(event) => setMacroStatus(event.currentTarget.value)} aria-label={i18n.tr('Makrostatus', 'Macro status')}>
            <option value="waiting_team">{i18n.tr('Venter på team', 'Waiting on team')}</option>
            <option value="waiting_customer">{i18n.tr('Venter på kunde', 'Waiting on customer')}</option>
            <option value="resolved">{i18n.tr('Løst', 'Resolved')}</option>
          </select>
          <button type="button" onClick={() => void submitMacro()} disabled={creatingMacro() || !macroName().trim()}>{creatingMacro() ? i18n.tr('Oppretter …', 'Creating…') : i18n.tr('Opprett makro', 'Create macro')}</button>
        </div>
        <For each={props.macros}>
          {(macro) => <RuleRow label={macro.name} detail={macro.description || macroActionSummary(macro.actions, i18n.tr)} />}
        </For>
      </RulesSection>
      <RulesSection title={i18n.tr('SLA-policyer', 'SLA policies')} detail={i18n.tr('Klokker for første svar, neste svar og løsning', 'First response, next response, and resolution clocks')}>
        <For each={props.slaPolicies}>
          {(policy) => <RuleRow label={policy.name} detail={i18n.tr(`${minutesLabel(policy.first_response_minutes, i18n.tr)} første svar · ${minutesLabel(policy.resolution_minutes, i18n.tr)} løsning`, `${minutesLabel(policy.first_response_minutes, i18n.tr)} first response · ${minutesLabel(policy.resolution_minutes, i18n.tr)} resolution`)} />}
        </For>
      </RulesSection>
      <RulesSection title={i18n.tr('Automatiseringsregler', 'Automation rules')} detail={i18n.tr('Policybegrenset ruting og AI-sikkerhetssperrer', 'Policy-limited routing and AI guardrails')}>
		<Show when={props.canManageAutomation} fallback={<p class="verevon-ticketing-panel-muted">{i18n.tr('Bare eiere og administratorer kan endre automatiseringsregler.', 'Only owners and administrators can change automation rules.')}</p>}>
		<div class="verevon-ticketing-macro-create">
			<input value={ruleName()} onInput={(event) => setRuleName(event.currentTarget.value)} placeholder={i18n.tr('Regelnavn', 'Rule name')} aria-label={i18n.tr('Regelnavn', 'Rule name')} />
			<select value={ruleEvent()} onChange={(event) => setRuleEvent(event.currentTarget.value)} aria-label={i18n.tr('Regelutløser', 'Rule trigger')}><option value="ticket.created">ticket.created</option><option value="ticket.updated">ticket.updated</option></select>
			<select value={conditionKey()} onChange={(event) => setConditionKey(event.currentTarget.value)} aria-label={i18n.tr('Regelbetingelse', 'Rule condition')}><option value="category">category</option><option value="priority">priority</option><option value="severity">severity</option><option value="status">status</option><option value="intent">intent</option><option value="label">label</option><option value="work_type">work type</option></select>
			<Show when={conditionKey() === 'work_type'} fallback={<input value={conditionValue()} onInput={(event) => setConditionValue(event.currentTarget.value)} placeholder={i18n.tr('Betingelsesverdi', 'Condition value')} aria-label={i18n.tr('Betingelsesverdi', 'Condition value')} />}>
				<select value={conditionValue()} onChange={(event) => setConditionValue(event.currentTarget.value)} aria-label={i18n.tr('Arbeidstype', 'Work type')}><option value="">{i18n.tr('Velg arbeidstype', 'Select work type')}</option><option value="customer_case">{i18n.tr('Kundesak', 'Customer case')}</option><option value="internal_work">{i18n.tr('Internt arbeid', 'Internal work')}</option><option value="incident">{i18n.tr('Hendelse', 'Incident')}</option></select>
			</Show>
			<select value={actionKey()} onChange={(event) => setActionKey(event.currentTarget.value)} aria-label={i18n.tr('Regelhandling', 'Rule action')}><option value="priority">priority</option><option value="severity">severity</option><option value="status">status</option><option value="category">category</option><option value="intent">intent</option><option value="labels">labels</option></select>
			<input value={actionValue()} onInput={(event) => setActionValue(event.currentTarget.value)} placeholder={i18n.tr('Handlingsverdi', 'Action value')} aria-label={i18n.tr('Handlingsverdi', 'Action value')} />
			<button type="button" onClick={() => void submitRule()} disabled={creatingRule() || !ruleName().trim() || !conditionValue().trim() || !actionValue().trim()}>{creatingRule() ? i18n.tr('Oppretter …', 'Creating…') : i18n.tr('Opprett regel', 'Create rule')}</button>
		</div>
		</Show>
        <For each={props.automationRules}>
          {(rule) => (
            <div class="verevon-ticketing-automation-rule">
              <RuleRow label={rule.name} detail={`${rule.event_name} · ${rule.active ? i18n.tr('aktiv', 'active') : i18n.tr('pauset', 'paused')}`} />
              <Show when={props.canManageAutomation}>
              <button
                type="button"
                class="verevon-ticketing-automation-rule__toggle"
                onClick={() => void props.onSetAutomationRuleActive(rule, !rule.active)}
                aria-label={rule.active
                  ? i18n.tr(`Deaktiver regel ${rule.name}`, `Disable rule ${rule.name}`)
                  : i18n.tr(`Aktiver regel ${rule.name}`, `Enable rule ${rule.name}`)}
              >
                {rule.active ? i18n.tr('Deaktiver', 'Disable') : i18n.tr('Aktiver', 'Enable')}
              </button>
				</Show>
            </div>
          )}
        </For>
      </RulesSection>
    </div>
  )
}

function RulesSection(props: { title: string; detail: string; children: JSX.Element }) {
  return (
    <section class="verevon-ticketing-rules-section">
      <div>
        <strong>{props.title}</strong>
        <span>{props.detail}</span>
      </div>
      {props.children}
    </section>
  )
}

function RuleRow(props: { label: string; detail: string; onClick?: () => void }) {
  return (
    <button type="button" class="verevon-ticketing-rule-row" onClick={() => props.onClick?.()} disabled={!props.onClick}>
      <span>{props.label}</span>
      <small>{props.detail}</small>
    </button>
  )
}

function TicketLabels(props: { labels: string[] }) {
  return (
    <Show when={props.labels.length > 0}>
      <div class="verevon-ticketing-labels">
        <For each={props.labels.slice(0, 3)}>
          {(label) => <span>{label}</span>}
        </For>
      </div>
    </Show>
  )
}

function TicketStatus(props: { status: string }) {
  return <span class={`verevon-ticketing-status verevon-ticketing-status--${props.status.replace(/_/g, '-')}`}>{props.status.replace(/_/g, ' ')}</span>
}

function ticketStatusLabel(status: string, tr: TrFn) {
  const labels: Record<string, [string, string]> = {
    suggested: ['Foreslått', 'Suggested'],
    open: ['Åpen', 'Open'],
    waiting_customer: ['Venter på kunde', 'Waiting on customer'],
    waiting_team: ['Venter på team', 'Waiting on team'],
    snoozed: ['Utsatt', 'Snoozed'],
    escalated: ['Eskalert', 'Escalated'],
    resolved: ['Løst', 'Resolved'],
    closed: ['Lukket', 'Closed'],
  }
  const label = labels[status]
  return label ? tr(label[0], label[1]) : status.replace(/_/g, ' ')
}

function ticketCustomerLabel(ticket: SupportTicket, tr: TrFn) {
  const contact = ticket.conversation?.contact
  return contact?.name || contact?.email || ticket.category || tr('Generelt', 'General')
}

function TicketField(props: { label: string; value: string }) {
  return (
    <div class="verevon-ticketing-field">
      <span>{props.label}</span>
      <strong>{props.value}</strong>
    </div>
  )
}

async function loadOptionalTicketMetadata<T>(loader: () => Promise<T>, fallback: T): Promise<T> {
  try {
    return await loader()
  } catch {
    return fallback
  }
}

function macroActionSummary(actions: Record<string, unknown>, tr: TrFn) {
  const parts = Object.entries(actions)
    .filter(([, value]) => typeof value === 'string' || Array.isArray(value))
    .map(([key, value]) => `${key.replace(/_/g, ' ')}: ${Array.isArray(value) ? value.join(', ') : value}`)
  return parts.slice(0, 3).join(' · ') || tr('Arbeidsflytmakro for saker', 'Ticket workflow macro')
}

function minutesLabel(minutes: number, tr: TrFn) {
  if (!minutes) return tr('Ingen', 'No')
  if (minutes < 60) return `${minutes}m`
  const hours = Math.round(minutes / 60)
  if (hours < 24) return `${hours}h`
  return `${Math.round(hours / 24)}d`
}

function withTicketParam(pathname: string, search: string, key: string, value: string) {
  const params = new URLSearchParams(search)
  params.set(key, value)
  if (!params.get('queue')) params.set('queue', 'my')
  if (pathname.startsWith('/support')) params.set('surface', 'tickets')
  return `${pathname.startsWith('/support') ? '/support' : '/tickets'}?${params}`
}
