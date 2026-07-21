import { useLocation, useNavigate } from '@solidjs/router'
import {
  AlertTriangle,
  Bot,
  CheckCheck,
  ChevronRight,
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
import { getAuthSession, getSessionContext } from '@/shared/api/auth-client'
import { createSocialDraftFromInbox } from '@/shared/api/social-client'
import {
  createTicketChecklist,
  linkTicketResource,
  listSlaPolicies,
  listTicketAutomationRules,
  listTicketMacros,
  listTickets,
  listTicketViews,
  runTicketMacro,
  updateTicket,
  updateTicketChecklistItem,
  type SlaPolicy,
  type SupportTicket,
  type TicketAutomationRule,
  type TicketChecklist,
  type TicketMacro,
  type TicketView,
  type UpdateTicketInput,
} from '@/shared/api/tickets-client'
import { cn } from '@/shared/lib/cn'
import { translateApiError, useI18n } from '@/shared/i18n'

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

type TicketingContext = {
  email: string
  name: string
  orgId: string
  userId: string
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

async function loadTicketingContext(): Promise<TicketingContext> {
  const [session, ctx] = await Promise.all([getAuthSession(), getSessionContext()])
  return {
    email: session?.user.email ?? '',
    name: session?.user.name ?? '',
    orgId: ctx.orgs[0]?.id ?? '',
    userId: session?.user.id ?? '',
  }
}

export default function TicketingPage() {
  const i18n = useI18n()
  const location = useLocation()
  const navigate = useNavigate()
  const [ctx] = createResource(loadTicketingContext)
  const [selectedId, setSelectedId] = createSignal<string | null>(null)
  const [notice, setNotice] = createSignal<string | null>(null)
  const [searchQuery, setSearchQuery] = createSignal('')

  const activeQueue = createMemo<TicketQueueId>(() => {
    const value = new URLSearchParams(location.search).get('queue') as TicketQueueId | null
    // Default to the "All tickets" queue so the Ticketing page opens populated
    // rather than on an empty "My tickets" (assigned-to-me) view.
    return ticketQueues.some((queue) => queue.id === value) ? value! : 'all'
  })
  const activeParams = createMemo(() => new URLSearchParams(location.search))
  const activeStatus = createMemo(() => activeParams().get('status') ?? undefined)
  const activeTeam = createMemo(() => activeParams().get('team') ?? undefined)
  const activeLabel = createMemo(() => activeParams().get('label') ?? undefined)
  const activePriority = createMemo(() => activeParams().get('priority') ?? undefined)
  const activeSeverity = createMemo(() => activeParams().get('severity') ?? undefined)
  const activeSlaState = createMemo(() => activeParams().get('sla_state') ?? undefined)
  const activeView = createMemo(() => activeParams().get('view') ?? undefined)
  const requestedTicketId = createMemo(() => activeParams().get('ticketId'))

  const [viewsRes] = createResource(() => ctx()?.orgId ?? '', (orgId) => (orgId ? optionalTicketResource(() => listTicketViews(orgId), [] as TicketView[]) : Promise.resolve([] as TicketView[])))
  const [macrosRes] = createResource(() => ctx()?.orgId ?? '', (orgId) => (orgId ? optionalTicketResource(() => listTicketMacros(orgId), [] as TicketMacro[]) : Promise.resolve([] as TicketMacro[])))
  const [slaRes] = createResource(() => ctx()?.orgId ?? '', (orgId) => (orgId ? optionalTicketResource(() => listSlaPolicies(orgId), [] as SlaPolicy[]) : Promise.resolve([] as SlaPolicy[])))
  const [rulesRes] = createResource(() => ctx()?.orgId ?? '', (orgId) => (orgId ? optionalTicketResource(() => listTicketAutomationRules(orgId), [] as TicketAutomationRule[]) : Promise.resolve([] as TicketAutomationRule[])))

  const savedViews = () => viewsRes() ?? []
  const macros = () => (macrosRes() ?? []).filter((macro) => macro.active)
  const slaPolicies = () => slaRes() ?? []
  const automationRules = () => rulesRes() ?? []

  const [ticketsRes, { mutate }] = createResource(
    () => ({
      orgId: ctx()?.orgId ?? '',
      queue: activeQueue(),
      userId: ctx()?.userId ?? '',
      status: activeStatus(),
      team: activeTeam(),
      label: activeLabel(),
      priority: activePriority(),
      severity: activeSeverity(),
      slaState: activeSlaState(),
      q: searchQuery().trim(),
    }),
    (source) => {
      if (!source.orgId || source.queue === 'rules') return Promise.resolve([] as SupportTicket[])
      return optionalTicketResource(() => listTickets(source.orgId, {
        // 'all' sends no queue filter (backend $4='' returns every ticket).
        queue: source.queue === 'all' ? undefined : source.queue,
        assigned: source.queue === 'my' ? source.userId : undefined,
        status: source.status,
        team: source.team,
        label: source.label,
        priority: source.priority,
        severity: source.severity,
        sla_state: source.slaState,
        q: source.q,
        limit: 100,
      }), [] as SupportTicket[])
    },
  )

  const tickets = () => ticketsRes() ?? []
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
  const activeQueueMeta = createMemo(() => ticketQueues.find((queue) => queue.id === activeQueue()) ?? ticketQueues[1])
  const selectedTicket = createMemo(() => filteredTickets().find((ticket) => ticket.id === selectedId()) ?? filteredTickets()[0] ?? null)

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
    mutate((items) => (items ?? []).map((item) => (item.id === updated.id ? updated : item)))
  }

  const patchTicket = async (ticket: SupportTicket, patch: UpdateTicketInput, message: string) => {
    const orgId = ctx()?.orgId
    if (!orgId) return
    setNotice(null)
    try {
      const updated = await updateTicket(orgId, ticket.id, patch)
      replaceTicket(updated)
      setNotice(message)
    } catch (reason) {
      setNotice(translateApiError(reason, i18n.tr, { no: 'Saken kunne ikke oppdateres.', en: 'Ticket could not be updated.' }))
    }
  }

  const runMacroAction = async (ticket: SupportTicket, macro: TicketMacro) => {
    const orgId = ctx()?.orgId
    if (!orgId) return
    setNotice(null)
    try {
      const result = await runTicketMacro(orgId, ticket.id, macro.id)
      replaceTicket(result.ticket)
      setNotice(i18n.tr(`Makroen "${macro.name}" ble kjørt.`, `Macro "${macro.name}" applied.`))
    } catch (reason) {
      setNotice(translateApiError(reason, i18n.tr, { no: 'Makroen kunne ikke kjøres.', en: 'Macro could not be applied.' }))
    }
  }

  const createChecklist = async (ticket: SupportTicket) => {
    const orgId = ctx()?.orgId
    if (!orgId) return
    setNotice(null)
    try {
      const checklist = await createTicketChecklist(orgId, ticket.id, {
        name: i18n.tr('Løsningssjekkliste', 'Resolution checklist'),
        items: [
          i18n.tr('Bekreft eier', 'Confirm owner'),
          i18n.tr('Dokumenter kundepåvirkning', 'Document customer impact'),
          i18n.tr('Send kundeoppdatering', 'Send customer update'),
        ],
      })
      replaceTicket({ ...ticket, checklists: [checklist, ...(ticket.checklists ?? [])] })
      setNotice(i18n.tr('Sjekkliste lagt til.', 'Checklist added.'))
    } catch (reason) {
      setNotice(translateApiError(reason, i18n.tr, { no: 'Sjekklisten kunne ikke legges til.', en: 'Checklist could not be added.' }))
    }
  }

  const toggleChecklistItem = async (ticket: SupportTicket, checklist: TicketChecklist, itemId: string, completed: boolean) => {
    const orgId = ctx()?.orgId
    if (!orgId) return
    setNotice(null)
    try {
      const updatedChecklist = await updateTicketChecklistItem(orgId, ticket.id, checklist.id, itemId, { completed })
      replaceTicket({
        ...ticket,
        checklists: (ticket.checklists ?? []).map((item) => (item.id === updatedChecklist.id ? updatedChecklist : item)),
      })
    } catch (reason) {
      setNotice(translateApiError(reason, i18n.tr, { no: 'Sjekklistepunktet kunne ikke oppdateres.', en: 'Checklist item could not be updated.' }))
    }
  }

  const linkConversationSource = async (ticket: SupportTicket) => {
    const orgId = ctx()?.orgId
    if (!orgId) return
    setNotice(null)
    try {
      const link = await linkTicketResource(orgId, ticket.id, {
        link_type: 'related',
        resource_kind: 'conversation_source',
        resource_id: ticket.conversation_id,
        label: ticket.conversation?.provider ? i18n.tr(`${ticket.conversation.provider}-tråd`, `${ticket.conversation.provider} thread`) : i18n.tr('Samtalekilde', 'Conversation source'),
        metadata: { channel: ticket.conversation?.channel, provider: ticket.conversation?.provider },
      })
      replaceTicket({ ...ticket, linked_resources: [link, ...(ticket.linked_resources ?? [])] })
      setNotice(i18n.tr('Kilde lenket til saken.', 'Source linked to ticket.'))
    } catch (reason) {
      setNotice(translateApiError(reason, i18n.tr, { no: 'Ressursen kunne ikke lenkes.', en: 'Resource could not be linked.' }))
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

  return (
    <main class="velion-ticketing-page">
      <section class="velion-ticketing-list" aria-label={i18n.tr('Saker', 'Tickets')}>
        <div class="velion-ticketing-list__header">
          <div>
            <span>{i18n.tr('Kø', 'Queue')}</span>
            <h2>{i18n.tr(activeQueueMeta().labelNo, activeQueueMeta().label)}</h2>
          </div>
          <small>{ticketQueueSummary(activeQueue(), tickets().length, i18n.tr)}</small>
        </div>
        <div class="velion-ticketing-list__tools">
          <label class="velion-ticketing-search">
            <Search class="size-4" />
            <input
              value={searchQuery()}
              onInput={(event) => setSearchQuery(event.currentTarget.value)}
              placeholder={i18n.tr('Søk i saker', 'Search tickets')}
            />
          </label>
          <div class="velion-ticketing-filter-strip" aria-label={i18n.tr('Sakfiltre', 'Ticket filters')}>
            <a href={withTicketParam(location.search, 'sla_state', 'risk')}>{i18n.tr('SLA-risiko', 'SLA risk')}</a>
            <a href={withTicketParam(location.search, 'priority', 'urgent')}>{i18n.tr('Haster', 'Urgent')}</a>
            <a href={withTicketParam(location.search, 'label', 'refund')}>{i18n.tr('Refusjon', 'Refund')}</a>
          </div>
        </div>
        <Show when={activeQueue() !== 'rules'} fallback={
          <RulesWorkspace
            activeView={activeView()}
            automationRules={automationRules()}
            macros={macros()}
            slaPolicies={slaPolicies()}
            views={savedViews()}
          />
        }>
          <Show when={ticketsRes.loading}>
            <div class="velion-ticketing-empty">{i18n.tr('Laster saker …', 'Loading tickets...')}</div>
          </Show>
          <Show when={!ticketsRes.loading && ticketsRes.error}>
            <div class="velion-ticketing-empty velion-ticketing-empty--error">{translateApiError(ticketsRes.error, i18n.tr, { no: 'Saksbehandlingen er utilgjengelig akkurat nå.', en: 'Ticketing is unavailable.' })}</div>
          </Show>
          <Show when={!ticketsRes.loading && !ticketsRes.error && filteredTickets().length === 0}>
            <div class="velion-ticketing-empty">{i18n.tr('Ingen saker i denne køen.', 'No tickets in this queue.')}</div>
          </Show>
          <Show when={!ticketsRes.loading && !ticketsRes.error && filteredTickets().length > 0}>
            <ul class="velion-ticketing-ticket-list">
              <For each={filteredTickets()}>
                {(ticket) => (
                  <li>
                    <button
                      type="button"
                      onClick={() => setSelectedId(ticket.id)}
                      class={cn('velion-ticketing-ticket-row', selectedTicket()?.id === ticket.id && 'velion-ticketing-ticket-row--active')}
                    >
                      <div class="velion-ticketing-ticket-row__top">
                        <strong>{ticket.ticket_key}</strong>
                        <TicketStatus status={ticket.status} />
                      </div>
                      <span>{(ticket.conversation?.title ?? ticket.intent) || ticket.category || i18n.tr('Support-sak', 'Support ticket')}</span>
                      <small>{ticketCustomerLabel(ticket, i18n.tr)} · {ticket.priority} {i18n.tr('prioritet', 'priority')}</small>
                      <TicketLabels labels={ticket.labels ?? []} />
                      <div class="velion-ticketing-ticket-row__meta">
                        <span class={cn(ticket.sla_state === 'breached' && 'velion-ticketing-danger-text')}>{ticket.sla_state || 'ok'} SLA</span>
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

      <section class="velion-ticketing-detail" aria-label={i18n.tr('Sakdetaljer', 'Ticket detail')}>
        <Show when={selectedTicket()} fallback={<div class="velion-ticketing-empty">{i18n.tr('Velg en sak.', 'Select a ticket.')}</div>}>
          {(ticket) => (
            <>
              <div class="velion-ticketing-detail__header">
                <div>
                  <span>{ticket().ticket_key}</span>
                  <h2>{(ticket().conversation?.title ?? ticket().intent) || i18n.tr('Support-sak', 'Support ticket')}</h2>
                </div>
                <TicketStatus status={ticket().status} />
              </div>

              <Show when={notice()}>
                <p class="velion-ticketing-notice">{notice()}</p>
              </Show>

              <div class="velion-ticketing-detail__quick-actions">
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
                <button type="button" onClick={() => patchTicket(ticket(), { status: 'resolved' }, i18n.tr('Saken er løst.', 'Ticket resolved.'))}>
                  <CheckCheck class="size-4" />
                  {i18n.tr('Løs', 'Resolve')}
                </button>
              </div>

              <div class="velion-ticketing-ai-card">
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

              <div class="velion-ticketing-field-grid">
                <TicketField label={i18n.tr('Prioritet', 'Priority')} value={ticket().priority} />
                <TicketField label={i18n.tr('Alvorlighetsgrad', 'Severity')} value={ticket().severity} />
                <TicketField label={i18n.tr('Kategori', 'Category')} value={ticket().category || i18n.tr('Ingen', 'None')} />
                <TicketField label={i18n.tr('Team', 'Team')} value={ticket().team_name || i18n.tr('Ikke tildelt', 'Unassigned')} />
                <TicketField label={i18n.tr('Eier', 'Owner')} value={ticket().assignee_name || i18n.tr('Ikke tildelt', 'Unassigned')} />
                <TicketField label={i18n.tr('Oppdatert', 'Updated')} value={i18n.tr(`${formatRelativeTime(ticket().updated_at)} siden`, `${formatRelativeTime(ticket().updated_at)} ago`)} />
              </div>

              <TicketLinkedResources ticket={ticket()} />

              <TicketTimeline ticket={ticket()} />

              <div class="velion-ticketing-actions">
                <Show when={ticket().status === 'suggested'}>
                  <button type="button" class="velion-inbox-button velion-inbox-button--primary velion-inbox-button--sm" onClick={() => patchTicket(ticket(), { status: 'open' }, i18n.tr('Saken er akseptert.', 'Ticket accepted.'))}>
                    {i18n.tr('Godta', 'Accept')}
                  </button>
                </Show>
                <button type="button" class="velion-inbox-button velion-inbox-button--secondary velion-inbox-button--sm" onClick={() => createChecklist(ticket())}>
                  {i18n.tr('Legg til sjekkliste', 'Add checklist')}
                </button>
                <button type="button" class="velion-inbox-button velion-inbox-button--secondary velion-inbox-button--sm" onClick={() => linkConversationSource(ticket())}>
                  {i18n.tr('Lenk kilde', 'Link source')}
                </button>
                <button type="button" class="velion-inbox-button velion-inbox-button--secondary velion-inbox-button--sm" onClick={() => createSocialFollowUp(ticket())}>
                  {i18n.tr('Sosial oppfølging', 'Social follow-up')}
                </button>
                <Show when={ticket().conversation_id}>
                  <button type="button" class="velion-inbox-button velion-inbox-button--secondary velion-inbox-button--sm" onClick={() => navigate(`/inbox?ticketId=${ticket().conversation_id}`)}>
                    {i18n.tr('Åpne innboks', 'Open inbox')}
                  </button>
                </Show>
              </div>
            </>
          )}
        </Show>
      </section>

      <aside class="velion-ticketing-context" aria-label={i18n.tr('Sakkontekst', 'Ticket context')}>
        <TicketingContextPanel
          automationRules={automationRules()}
          macros={macros()}
          onCreateChecklist={createChecklist}
          onMacroRun={runMacroAction}
          onToggleChecklistItem={toggleChecklistItem}
          queue={activeQueue()}
          slaPolicies={slaPolicies()}
          ticket={selectedTicket()}
          views={savedViews()}
        />
      </aside>
    </main>
  )
}

function TicketingContextPanel(props: {
  automationRules: TicketAutomationRule[]
  macros: TicketMacro[]
  onCreateChecklist: (ticket: SupportTicket) => void
  onMacroRun: (ticket: SupportTicket, macro: TicketMacro) => void
  onToggleChecklistItem: (ticket: SupportTicket, checklist: TicketChecklist, itemId: string, completed: boolean) => void
  queue: TicketQueueId
  slaPolicies: SlaPolicy[]
  ticket: SupportTicket | null
  views: TicketView[]
}) {
  const i18n = useI18n()
  return (
    <div class="velion-ticketing-context__scroll">
      <TicketingPanel title={i18n.tr('Køhelse', 'Queue health')} icon={TimerReset}>
        <div class="velion-ticketing-health-grid">
          <TicketMetric label={i18n.tr('Kø', 'Queue')} value={ticketQueueShortLabel(props.queue, i18n.tr)} />
          <TicketMetric label="SLA" value={props.ticket?.sla_state === 'breached' ? i18n.tr('Brutt', 'Breached') : props.ticket?.sla_state === 'risk' ? i18n.tr('I faresonen', 'At risk') : i18n.tr('Spores', 'Tracked')} tone={props.ticket?.sla_state === 'breached' ? 'danger' : undefined} />
          <TicketMetric label={i18n.tr('Visninger', 'Views')} value={String(props.views.length)} />
          <TicketMetric label={i18n.tr('Regler', 'Rules')} value={String(props.automationRules.filter((rule) => rule.active).length)} />
        </div>
      </TicketingPanel>

      <TicketingPanel title={i18n.tr('Kunde', 'Customer')} icon={UserRound}>
        <Show when={props.ticket} fallback={<p class="velion-ticketing-panel-muted">{i18n.tr('Velg en sak for å laste kundekontekst.', 'Select a ticket to load customer context.')}</p>}>
          {(ticket) => (
            <div class="velion-ticketing-customer">
              <strong>{ticketCustomerLabel(ticket(), i18n.tr)}</strong>
              <span>{ticket().conversation?.contact?.email || i18n.tr('Ingen e-post registrert', 'No email attached')}</span>
              <small>{ticket().conversation?.channel || i18n.tr('Samtale', 'Conversation')} · {ticket().conversation?.status || ticket().status}</small>
            </div>
          )}
        </Show>
      </TicketingPanel>

      <TicketingPanel title={i18n.tr('Makroer', 'Macros')} icon={Zap}>
        <div class="velion-ticketing-macro-list">
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
        <Show when={props.ticket} fallback={<p class="velion-ticketing-panel-muted">{i18n.tr('Velg en sak for å administrere sjekklistearbeid.', 'Select a ticket to manage checklist work.')}</p>}>
          {(ticket) => (
            <TicketChecklistPanel
              ticket={ticket()}
              onCreateChecklist={() => props.onCreateChecklist(ticket())}
              onToggleItem={(checklist, itemId, completed) => props.onToggleChecklistItem(ticket(), checklist, itemId, completed)}
            />
          )}
        </Show>
      </TicketingPanel>

      <TicketingPanel title={i18n.tr('SLA-policyer', 'SLA policies')} icon={Clock3}>
        <div class="velion-ticketing-side-conversations">
          <For each={props.slaPolicies.slice(0, 3)}>
            {(policy) => (
              <TicketSideConversation
                label={policy.name}
                status={i18n.tr(`${minutesLabel(policy.first_response_minutes, i18n.tr)} første svar`, `${minutesLabel(policy.first_response_minutes, i18n.tr)} first response`)}
              />
            )}
          </For>
        </div>
      </TicketingPanel>

      <TicketingPanel title={i18n.tr('Automatiseringssperrer', 'Automation guardrails')} icon={ShieldAlert}>
        <ul class="velion-ticketing-policy-list">
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
  )
}

function TicketingPanel(props: { title: string; icon: Component<LucideProps>; children: JSX.Element }) {
  return (
    <section class="velion-ticketing-panel">
      <div class="velion-ticketing-panel__header">
        <Dynamic component={props.icon} class="size-4" />
        <h3>{props.title}</h3>
      </div>
      {props.children}
    </section>
  )
}

function TicketMetric(props: { label: string; value: string; tone?: 'danger' }) {
  return (
    <div class={cn('velion-ticketing-metric', props.tone === 'danger' && 'velion-ticketing-metric--danger')}>
      <span>{props.label}</span>
      <strong>{props.value}</strong>
    </div>
  )
}

function TicketMacroButton(props: { disabled: boolean; macro: TicketMacro; onClick: () => void }) {
  const i18n = useI18n()
  return (
    <button type="button" class="velion-ticketing-macro" disabled={props.disabled} onClick={() => props.onClick()}>
      <span>
        <Sparkles class="size-4" />
        {props.macro.name}
      </span>
      <small>{props.macro.description || macroActionSummary(props.macro.actions, i18n.tr)}</small>
    </button>
  )
}

function TicketSideConversation(props: { label: string; status: string }) {
  return (
    <button type="button" class="velion-ticketing-side-conversation">
      <MessageSquare class="size-4" />
      <span>{props.label}</span>
      <small>{props.status}</small>
      <ChevronRight class="size-4" />
    </button>
  )
}

function TicketSlaSnapshot(props: { ticket: SupportTicket; policies: SlaPolicy[] }) {
  const i18n = useI18n()
  const policy = () => props.policies.find((item) => item.id === props.ticket.sla_policy_id) ?? props.policies[0]
  return (
    <div class={cn('velion-ticketing-sla-card', props.ticket.sla_state === 'breached' && 'velion-ticketing-sla-card--danger')}>
      <TimerReset class="size-4" />
      <div>
        <span>{policy()?.name ?? i18n.tr('SLA-policy', 'SLA policy')}</span>
        <strong>{props.ticket.due_at ? i18n.tr(`Forfaller ${formatRelativeTime(props.ticket.due_at)}`, `Due ${formatRelativeTime(props.ticket.due_at)} from now`) : i18n.tr(`${minutesLabel(policy()?.resolution_minutes ?? 0, i18n.tr)} løsningsmål`, `${minutesLabel(policy()?.resolution_minutes ?? 0, i18n.tr)} resolution target`)}</strong>
      </div>
      <small>{props.ticket.sla_state || 'ok'}</small>
    </div>
  )
}

function TicketTimeline(props: { ticket: SupportTicket }) {
  const i18n = useI18n()
  const links = () => props.ticket.linked_resources ?? []
  return (
    <div class="velion-ticketing-timeline">
      <div class="velion-ticketing-timeline__header">
        <FileText class="size-4" />
        <strong>{i18n.tr('Aktivitet', 'Activity')}</strong>
      </div>
      <ol>
        <li>
          <Tag class="size-4" />
          <span>{i18n.tr(`Sak ${props.ticket.ticket_key} gikk inn i ${props.ticket.status.replace(/_/g, ' ')}`, `Ticket ${props.ticket.ticket_key} entered ${props.ticket.status.replace(/_/g, ' ')}`)}</span>
        </li>
        <Show when={props.ticket.ai_reason}>
          <li>
            <Bot class="size-4" />
            <span>{i18n.tr(`AI klassifiserte dette som ${props.ticket.category || props.ticket.intent || 'supportarbeid'}`, `AI classified this as ${props.ticket.category || props.ticket.intent || 'support work'}`)}</span>
          </li>
        </Show>
        <For each={props.ticket.checklists ?? []}>
          {(checklist) => (
            <li>
              <ListChecks class="size-4" />
              <span>{i18n.tr(`Sjekklisten ${checklist.name} har ${checklist.items.filter((item) => item.completed).length}/${checklist.items.length} fullført`, `Checklist ${checklist.name} has ${checklist.items.filter((item) => item.completed).length}/${checklist.items.length} complete`)}</span>
            </li>
          )}
        </For>
        <For each={links()}>
          {(link) => (
            <li>
              <Link2 class="size-4" />
              <span>{i18n.tr(`Lenket ${link.link_type || 'normal'} ${link.label || link.resource_kind.replace(/_/g, ' ')}`, `Linked ${link.link_type || 'normal'} ${link.label || link.resource_kind.replace(/_/g, ' ')}`)}</span>
            </li>
          )}
        </For>
      </ol>
    </div>
  )
}

function TicketLinkedResources(props: { ticket: SupportTicket }) {
  const i18n = useI18n()
  const links = () => props.ticket.linked_resources ?? []
  return (
    <Show when={links().length > 0}>
      <div class="velion-ticketing-links">
        <div class="velion-ticketing-links__header">
          <Link2 class="size-4" />
          <strong>{i18n.tr('Lenkede ressurser', 'Linked resources')}</strong>
        </div>
        <ul>
          <For each={links()}>
            {(link) => (
              <li>
                <div>
                  <span>{link.label || link.resource_kind.replace(/_/g, ' ')}</span>
                  <small>{link.link_type || 'normal'} · {link.resource_id || link.resource_url || link.resource_kind}</small>
                </div>
                <Show when={link.resource_url}>
                  {(url) => (
                    <a href={url()} target="_blank" rel="noreferrer" aria-label={i18n.tr(`Åpne ${link.label || link.resource_kind}`, `Open ${link.label || link.resource_kind}`)}>
                      <ExternalLink class="size-4" />
                    </a>
                  )}
                </Show>
              </li>
            )}
          </For>
        </ul>
      </div>
    </Show>
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
    <div class="velion-ticketing-checklists">
      <Show when={checklists().length > 0} fallback={
        <button type="button" class="velion-ticketing-macro" onClick={props.onCreateChecklist}>
          <span>
            <ListChecks class="size-4" />
            {i18n.tr('Legg til løsningssjekkliste', 'Add resolution checklist')}
          </span>
          <small>{i18n.tr('Eier, konsekvens, kundeoppdatering', 'Owner, impact, customer update')}</small>
        </button>
      }>
        <For each={checklists()}>
          {(checklist) => (
            <div class="velion-ticketing-checklist">
              <div class="velion-ticketing-checklist__header">
                <strong>{checklist.name}</strong>
                <small>{checklist.items.filter((item) => item.completed).length}/{checklist.items.length}</small>
              </div>
              <For each={checklist.items}>
                {(item) => (
                  <label class="velion-ticketing-checklist-item">
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
}) {
  const i18n = useI18n()
  return (
    <div class="velion-ticketing-rules-workspace">
      <RulesSection title={i18n.tr('Lagrede visninger', 'Saved views')} detail={i18n.tr('Zammad-stil oversikter og Chatwoot-stil egendefinerte filtre', 'Zammad-style overviews and Chatwoot-style custom filters')}>
        <For each={props.views}>
          {(view) => <RuleRow label={view.name} detail={i18n.tr(`${view.scope} · ${view.group_by || 'ugruppert'} · ${Object.keys(view.filter ?? {}).length} filtre`, `${view.scope} · ${view.group_by || 'ungrouped'} · ${Object.keys(view.filter ?? {}).length} filters`)} />}
        </For>
      </RulesSection>
      <RulesSection title={i18n.tr('Makroer', 'Macros')} detail={i18n.tr('Gjenbrukbare flertrinns sakhandlinger', 'Reusable multi-step ticket actions')}>
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
        <For each={props.automationRules}>
          {(rule) => <RuleRow label={rule.name} detail={`${rule.event_name} · ${rule.active ? i18n.tr('aktiv', 'active') : i18n.tr('pauset', 'paused')}`} />}
        </For>
      </RulesSection>
    </div>
  )
}

function RulesSection(props: { title: string; detail: string; children: JSX.Element }) {
  return (
    <section class="velion-ticketing-rules-section">
      <div>
        <strong>{props.title}</strong>
        <span>{props.detail}</span>
      </div>
      {props.children}
    </section>
  )
}

function RuleRow(props: { label: string; detail: string }) {
  return (
    <div class="velion-ticketing-rule-row">
      <span>{props.label}</span>
      <small>{props.detail}</small>
    </div>
  )
}

function TicketLabels(props: { labels: string[] }) {
  return (
    <Show when={props.labels.length > 0}>
      <div class="velion-ticketing-labels">
        <For each={props.labels.slice(0, 3)}>
          {(label) => <span>{label}</span>}
        </For>
      </div>
    </Show>
  )
}

function TicketStatus(props: { status: string }) {
  return <span class={`velion-ticketing-status velion-ticketing-status--${props.status.replace(/_/g, '-')}`}>{props.status.replace(/_/g, ' ')}</span>
}

function ticketCustomerLabel(ticket: SupportTicket, tr: TrFn) {
  const contact = ticket.conversation?.contact
  return contact?.name || contact?.email || ticket.category || tr('Generelt', 'General')
}

function TicketField(props: { label: string; value: string }) {
  return (
    <div class="velion-ticketing-field">
      <span>{props.label}</span>
      <strong>{props.value}</strong>
    </div>
  )
}

async function optionalTicketResource<T>(loader: () => Promise<T>, fallback: T): Promise<T> {
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

function withTicketParam(search: string, key: string, value: string) {
  const params = new URLSearchParams(search)
  params.set(key, value)
  if (!params.get('queue')) params.set('queue', 'my')
  return `/tickets?${params}`
}
