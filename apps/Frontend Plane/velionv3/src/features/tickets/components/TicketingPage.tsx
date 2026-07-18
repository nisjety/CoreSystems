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

const ticketQueues = [
  { id: 'all', label: 'All tickets', icon: List },
  { id: 'suggested', label: 'Suggested by AI', icon: Bot },
  { id: 'my', label: 'My tickets', icon: UserRound },
  { id: 'unassigned', label: 'Unassigned', icon: Inbox },
  { id: 'sla-risk', label: 'SLA risk', icon: AlertTriangle },
  { id: 'escalated', label: 'Escalated', icon: ShieldAlert },
  { id: 'waiting-customer', label: 'Waiting on customer', icon: Clock3 },
  { id: 'waiting-team', label: 'Waiting on team', icon: UsersRound },
  { id: 'resolved', label: 'Resolved', icon: CheckCheck },
  { id: 'rules', label: 'Rules / queues', icon: ListChecks },
] as const

type TicketQueueId = (typeof ticketQueues)[number]['id']

type TicketingContext = {
  email: string
  name: string
  orgId: string
  userId: string
}

function ticketQueueShortLabel(queue: TicketQueueId) {
  return ticketQueues.find((item) => item.id === queue)?.label ?? 'My tickets'
}

function ticketQueueSummary(queue: TicketQueueId, count: number) {
  if (queue === 'rules') return 'Routing workspace'
  if (count === 1) return '1 ticket'
  return `${count} tickets`
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
      setNotice(reason instanceof Error ? reason.message : 'Ticket could not be updated.')
    }
  }

  const runMacroAction = async (ticket: SupportTicket, macro: TicketMacro) => {
    const orgId = ctx()?.orgId
    if (!orgId) return
    setNotice(null)
    try {
      const result = await runTicketMacro(orgId, ticket.id, macro.id)
      replaceTicket(result.ticket)
      setNotice(`Macro "${macro.name}" applied.`)
    } catch (reason) {
      setNotice(reason instanceof Error ? reason.message : 'Macro could not be applied.')
    }
  }

  const createChecklist = async (ticket: SupportTicket) => {
    const orgId = ctx()?.orgId
    if (!orgId) return
    setNotice(null)
    try {
      const checklist = await createTicketChecklist(orgId, ticket.id, {
        name: 'Resolution checklist',
        items: ['Confirm owner', 'Document customer impact', 'Send customer update'],
      })
      replaceTicket({ ...ticket, checklists: [checklist, ...(ticket.checklists ?? [])] })
      setNotice('Checklist added.')
    } catch (reason) {
      setNotice(reason instanceof Error ? reason.message : 'Checklist could not be added.')
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
      setNotice(reason instanceof Error ? reason.message : 'Checklist item could not be updated.')
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
        label: ticket.conversation?.provider ? `${ticket.conversation.provider} thread` : 'Conversation source',
        metadata: { channel: ticket.conversation?.channel, provider: ticket.conversation?.provider },
      })
      replaceTicket({ ...ticket, linked_resources: [link, ...(ticket.linked_resources ?? [])] })
      setNotice('Source linked to ticket.')
    } catch (reason) {
      setNotice(reason instanceof Error ? reason.message : 'Resource could not be linked.')
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
        customerName: ticketCustomerLabel(ticket),
        channel: ticket.conversation?.channel,
        excerpt: ticket.conversation?.last_message_preview,
      })
      setNotice('Social follow-up draft created and linked.')
      navigate('/social/drafts')
    } catch (reason) {
      setNotice(reason instanceof Error ? reason.message : 'Social follow-up could not be created.')
    }
  }

  const snoozeTicket = (ticket: SupportTicket) => {
    const until = new Date(Date.now() + 24 * 60 * 60 * 1000).toISOString()
    void patchTicket(ticket, { status: 'snoozed', snoozed_until: until }, 'Ticket snoozed for 24 hours.')
  }

  return (
    <main class="velion-ticketing-page">
      <section class="velion-ticketing-list" aria-label="Tickets">
        <div class="velion-ticketing-list__header">
          <div>
            <span>Queue</span>
            <h2>{activeQueueMeta().label}</h2>
          </div>
          <small>{ticketQueueSummary(activeQueue(), tickets().length)}</small>
        </div>
        <div class="velion-ticketing-list__tools">
          <label class="velion-ticketing-search">
            <Search class="size-4" />
            <input
              value={searchQuery()}
              onInput={(event) => setSearchQuery(event.currentTarget.value)}
              placeholder="Search tickets"
            />
          </label>
          <div class="velion-ticketing-filter-strip" aria-label="Ticket filters">
            <a href={withTicketParam(location.search, 'sla_state', 'risk')}>SLA risk</a>
            <a href={withTicketParam(location.search, 'priority', 'urgent')}>Urgent</a>
            <a href={withTicketParam(location.search, 'label', 'refund')}>Refund</a>
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
            <div class="velion-ticketing-empty">Loading tickets...</div>
          </Show>
          <Show when={!ticketsRes.loading && ticketsRes.error}>
            <div class="velion-ticketing-empty velion-ticketing-empty--error">{ticketsRes.error instanceof Error ? ticketsRes.error.message : 'Ticketing is unavailable.'}</div>
          </Show>
          <Show when={!ticketsRes.loading && !ticketsRes.error && filteredTickets().length === 0}>
            <div class="velion-ticketing-empty">No tickets in this queue.</div>
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
                      <span>{(ticket.conversation?.title ?? ticket.intent) || ticket.category || 'Support ticket'}</span>
                      <small>{ticketCustomerLabel(ticket)} · {ticket.priority} priority</small>
                      <TicketLabels labels={ticket.labels ?? []} />
                      <div class="velion-ticketing-ticket-row__meta">
                        <span class={cn(ticket.sla_state === 'breached' && 'velion-ticketing-danger-text')}>{ticket.sla_state || 'ok'} SLA</span>
                        <span>{ticket.team_name || ticket.assignee_name || 'Unassigned'}</span>
                      </div>
                    </button>
                  </li>
                )}
              </For>
            </ul>
          </Show>
        </Show>
      </section>

      <section class="velion-ticketing-detail" aria-label="Ticket detail">
        <Show when={selectedTicket()} fallback={<div class="velion-ticketing-empty">Select a ticket.</div>}>
          {(ticket) => (
            <>
              <div class="velion-ticketing-detail__header">
                <div>
                  <span>{ticket().ticket_key}</span>
                  <h2>{(ticket().conversation?.title ?? ticket().intent) || 'Support ticket'}</h2>
                </div>
                <TicketStatus status={ticket().status} />
              </div>

              <Show when={notice()}>
                <p class="velion-ticketing-notice">{notice()}</p>
              </Show>

              <div class="velion-ticketing-detail__quick-actions">
                <button type="button" onClick={() => patchTicket(ticket(), { assignee_user_id: ctx()?.userId, assignee_name: ctx()?.name }, 'Ticket assigned to you.')}>
                  <UserRound class="size-4" />
                  Assign me
                </button>
                <button type="button" onClick={() => patchTicket(ticket(), { status: 'waiting_customer' }, 'Ticket is waiting on customer.')}>
                  <Clock3 class="size-4" />
                  Wait customer
                </button>
                <button type="button" onClick={() => snoozeTicket(ticket())}>
                  <TimerReset class="size-4" />
                  Snooze
                </button>
                <button type="button" onClick={() => patchTicket(ticket(), { status: 'resolved' }, 'Ticket resolved.')}>
                  <CheckCheck class="size-4" />
                  Resolve
                </button>
              </div>

              <div class="velion-ticketing-ai-card">
                <Bot class="size-4" />
                <div>
                  <strong>{ticket().status === 'suggested' ? 'Suggested ticket' : ticket().source === 'ai' ? 'Auto-created ticket' : 'Manual ticket'}</strong>
                  <p>{ticket().ai_reason || ticket().intent || 'Customer follow-up is tracked as a durable ticket.'}</p>
                </div>
                <Show when={ticket().ai_confidence}>
                  {(confidence) => <span>{Math.round(confidence() * 100)}%</span>}
                </Show>
              </div>

              <TicketSlaSnapshot ticket={ticket()} policies={slaPolicies()} />

              <div class="velion-ticketing-field-grid">
                <TicketField label="Priority" value={ticket().priority} />
                <TicketField label="Severity" value={ticket().severity} />
                <TicketField label="Category" value={ticket().category || 'None'} />
                <TicketField label="Team" value={ticket().team_name || 'Unassigned'} />
                <TicketField label="Owner" value={ticket().assignee_name || 'Unassigned'} />
                <TicketField label="Updated" value={`${formatRelativeTime(ticket().updated_at)} ago`} />
              </div>

              <TicketLinkedResources ticket={ticket()} />

              <TicketTimeline ticket={ticket()} />

              <div class="velion-ticketing-actions">
                <Show when={ticket().status === 'suggested'}>
                  <button type="button" class="velion-inbox-button velion-inbox-button--primary velion-inbox-button--sm" onClick={() => patchTicket(ticket(), { status: 'open' }, 'Ticket accepted.')}>
                    Accept
                  </button>
                </Show>
                <button type="button" class="velion-inbox-button velion-inbox-button--secondary velion-inbox-button--sm" onClick={() => createChecklist(ticket())}>
                  Add checklist
                </button>
                <button type="button" class="velion-inbox-button velion-inbox-button--secondary velion-inbox-button--sm" onClick={() => linkConversationSource(ticket())}>
                  Link source
                </button>
                <button type="button" class="velion-inbox-button velion-inbox-button--secondary velion-inbox-button--sm" onClick={() => createSocialFollowUp(ticket())}>
                  Social follow-up
                </button>
                <Show when={ticket().conversation_id}>
                  <button type="button" class="velion-inbox-button velion-inbox-button--secondary velion-inbox-button--sm" onClick={() => navigate(`/inbox?ticketId=${ticket().conversation_id}`)}>
                    Open inbox
                  </button>
                </Show>
              </div>
            </>
          )}
        </Show>
      </section>

      <aside class="velion-ticketing-context" aria-label="Ticket context">
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
  return (
    <div class="velion-ticketing-context__scroll">
      <TicketingPanel title="Queue health" icon={TimerReset}>
        <div class="velion-ticketing-health-grid">
          <TicketMetric label="Queue" value={ticketQueueShortLabel(props.queue)} />
          <TicketMetric label="SLA" value={props.ticket?.sla_state === 'breached' ? 'Breached' : props.ticket?.sla_state === 'risk' ? 'At risk' : 'Tracked'} tone={props.ticket?.sla_state === 'breached' ? 'danger' : undefined} />
          <TicketMetric label="Views" value={String(props.views.length)} />
          <TicketMetric label="Rules" value={String(props.automationRules.filter((rule) => rule.active).length)} />
        </div>
      </TicketingPanel>

      <TicketingPanel title="Customer" icon={UserRound}>
        <Show when={props.ticket} fallback={<p class="velion-ticketing-panel-muted">Select a ticket to load customer context.</p>}>
          {(ticket) => (
            <div class="velion-ticketing-customer">
              <strong>{ticketCustomerLabel(ticket())}</strong>
              <span>{ticket().conversation?.contact?.email || 'No email attached'}</span>
              <small>{ticket().conversation?.channel || 'Conversation'} · {ticket().conversation?.status || ticket().status}</small>
            </div>
          )}
        </Show>
      </TicketingPanel>

      <TicketingPanel title="Macros" icon={Zap}>
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

      <TicketingPanel title="Checklist" icon={ListChecks}>
        <Show when={props.ticket} fallback={<p class="velion-ticketing-panel-muted">Select a ticket to manage checklist work.</p>}>
          {(ticket) => (
            <TicketChecklistPanel
              ticket={ticket()}
              onCreateChecklist={() => props.onCreateChecklist(ticket())}
              onToggleItem={(checklist, itemId, completed) => props.onToggleChecklistItem(ticket(), checklist, itemId, completed)}
            />
          )}
        </Show>
      </TicketingPanel>

      <TicketingPanel title="SLA policies" icon={Clock3}>
        <div class="velion-ticketing-side-conversations">
          <For each={props.slaPolicies.slice(0, 3)}>
            {(policy) => (
              <TicketSideConversation
                label={policy.name}
                status={`${minutesLabel(policy.first_response_minutes)} first response`}
              />
            )}
          </For>
        </div>
      </TicketingPanel>

      <TicketingPanel title="Automation guardrails" icon={ShieldAlert}>
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
  return (
    <button type="button" class="velion-ticketing-macro" disabled={props.disabled} onClick={() => props.onClick()}>
      <span>
        <Sparkles class="size-4" />
        {props.macro.name}
      </span>
      <small>{props.macro.description || macroActionSummary(props.macro.actions)}</small>
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
  const policy = () => props.policies.find((item) => item.id === props.ticket.sla_policy_id) ?? props.policies[0]
  return (
    <div class={cn('velion-ticketing-sla-card', props.ticket.sla_state === 'breached' && 'velion-ticketing-sla-card--danger')}>
      <TimerReset class="size-4" />
      <div>
        <span>{policy()?.name ?? 'SLA policy'}</span>
        <strong>{props.ticket.due_at ? `Due ${formatRelativeTime(props.ticket.due_at)} from now` : `${minutesLabel(policy()?.resolution_minutes ?? 0)} resolution target`}</strong>
      </div>
      <small>{props.ticket.sla_state || 'ok'}</small>
    </div>
  )
}

function TicketTimeline(props: { ticket: SupportTicket }) {
  const links = () => props.ticket.linked_resources ?? []
  return (
    <div class="velion-ticketing-timeline">
      <div class="velion-ticketing-timeline__header">
        <FileText class="size-4" />
        <strong>Activity</strong>
      </div>
      <ol>
        <li>
          <Tag class="size-4" />
          <span>Ticket {props.ticket.ticket_key} entered {props.ticket.status.replace(/_/g, ' ')}</span>
        </li>
        <Show when={props.ticket.ai_reason}>
          <li>
            <Bot class="size-4" />
            <span>AI classified this as {props.ticket.category || props.ticket.intent || 'support work'}</span>
          </li>
        </Show>
        <For each={props.ticket.checklists ?? []}>
          {(checklist) => (
            <li>
              <ListChecks class="size-4" />
              <span>Checklist {checklist.name} has {checklist.items.filter((item) => item.completed).length}/{checklist.items.length} complete</span>
            </li>
          )}
        </For>
        <For each={links()}>
          {(link) => (
            <li>
              <Link2 class="size-4" />
              <span>Linked {link.link_type || 'normal'} {link.label || link.resource_kind.replace(/_/g, ' ')}</span>
            </li>
          )}
        </For>
      </ol>
    </div>
  )
}

function TicketLinkedResources(props: { ticket: SupportTicket }) {
  const links = () => props.ticket.linked_resources ?? []
  return (
    <Show when={links().length > 0}>
      <div class="velion-ticketing-links">
        <div class="velion-ticketing-links__header">
          <Link2 class="size-4" />
          <strong>Linked resources</strong>
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
                    <a href={url()} target="_blank" rel="noreferrer" aria-label={`Open ${link.label || link.resource_kind}`}>
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
  const checklists = () => props.ticket.checklists ?? []
  return (
    <div class="velion-ticketing-checklists">
      <Show when={checklists().length > 0} fallback={
        <button type="button" class="velion-ticketing-macro" onClick={props.onCreateChecklist}>
          <span>
            <ListChecks class="size-4" />
            Add resolution checklist
          </span>
          <small>Owner, impact, customer update</small>
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
  return (
    <div class="velion-ticketing-rules-workspace">
      <RulesSection title="Saved views" detail="Zammad-style overviews and Chatwoot-style custom filters">
        <For each={props.views}>
          {(view) => <RuleRow label={view.name} detail={`${view.scope} · ${view.group_by || 'ungrouped'} · ${Object.keys(view.filter ?? {}).length} filters`} />}
        </For>
      </RulesSection>
      <RulesSection title="Macros" detail="Reusable multi-step ticket actions">
        <For each={props.macros}>
          {(macro) => <RuleRow label={macro.name} detail={macro.description || macroActionSummary(macro.actions)} />}
        </For>
      </RulesSection>
      <RulesSection title="SLA policies" detail="First response, next response, and resolution clocks">
        <For each={props.slaPolicies}>
          {(policy) => <RuleRow label={policy.name} detail={`${minutesLabel(policy.first_response_minutes)} first response · ${minutesLabel(policy.resolution_minutes)} resolution`} />}
        </For>
      </RulesSection>
      <RulesSection title="Automation rules" detail="Policy-limited routing and AI guardrails">
        <For each={props.automationRules}>
          {(rule) => <RuleRow label={rule.name} detail={`${rule.event_name} · ${rule.active ? 'active' : 'paused'}`} />}
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

function ticketCustomerLabel(ticket: SupportTicket) {
  const contact = ticket.conversation?.contact
  return contact?.name || contact?.email || ticket.category || 'General'
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

function macroActionSummary(actions: Record<string, unknown>) {
  const parts = Object.entries(actions)
    .filter(([, value]) => typeof value === 'string' || Array.isArray(value))
    .map(([key, value]) => `${key.replace(/_/g, ' ')}: ${Array.isArray(value) ? value.join(', ') : value}`)
  return parts.slice(0, 3).join(' · ') || 'Ticket workflow macro'
}

function minutesLabel(minutes: number) {
  if (!minutes) return 'No'
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
