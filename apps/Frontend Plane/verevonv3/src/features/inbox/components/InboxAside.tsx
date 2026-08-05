import {
  AlertCircle,
  ArrowUpRight,
  Bot,
  CalendarDays,
  CheckCircle2,
  ChevronDown,
  Clock3,
  FileText,
  MessageCircle,
  Play,
  Plus,
  RefreshCw,
  Send,
  Sparkles,
  UserRound,
} from 'lucide-solid'
import { createEffect, createMemo, createResource, createSignal, For, Show } from 'solid-js'
import {
  AccordionSection,
  ActivityItem,
  AsideTabButton,
  CalendarEventRow,
  CalendarNoteRow,
  EmptyAsideState,
  FieldRow,
  HealthRow,
  MiniCalendarGrid,
  SourceRow,
} from '@/features/inbox/components/InboxAsidePrimitives'
import type { InboxModalRequest } from '@/features/inbox/components/InboxWorkModal'
import {
  customerName,
  formatDateKey,
  formatRelativeTime,
  titleCase,
  type InboxRouteFilter,
  type ZammadArticle,
  type ZammadTicket,
} from '@/features/inbox/lib/inbox-model'
import { runAssist, type AssistMessage, type AssistMode, type AssistSource } from '@/features/inbox/lib/inbox-ai'
import { setActiveChatThreadId } from '@/features/chat/lib/chat-thread-history'
import { bindSupportChatThread, readSupportChatThread, type SupportChatThreadScope } from '@/shared/chat/support-chat-thread'
import type { SupportAIMode } from '@/shared/api/organization-client'
import { buildInboxAssistContext } from '@/features/inbox/lib/inbox-ai-context'
import { parseInboxResolutionPlan, parseInboxTriageProposal, type InboxResolutionPlan, type InboxTriageProposal } from '@/features/inbox/lib/inbox-ai-triage'
import { executeTicketMacro } from '@/features/tickets/lib/ticket-actions'
import type { TicketTeam } from '@/shared/api/tickets-client'
import {
  createNavbarCalendarEvent,
  createNavbarCalendarNote,
  getNavbarCalendarState,
} from '@/shared/api/navbar-client'
import { createIncidentProposal, createProblemProposal, createTicketUpdateProposal, getConversationCSATPreference, getConversationFollow, getDraftLease } from '@/shared/api/inbox-client'
import { executeAction } from '@/shared/actions/action-client'
import { ApiError } from '@/shared/api/http'
import { getCSATScorecard, getTicketCSATOutcome, listTicketMacros, type SupportTicket, type TicketMacro } from '@/shared/api/tickets-client'
import { cn } from '@/shared/lib/cn'
import { localeDateTime, useI18n } from '@/shared/i18n'

type AsideTab = 'details' | 'verevon' | 'actions' | 'audit'

export function InboxAside(props: {
  orgId: string
  articles: ZammadArticle[]
  recent: RecentConversationRef[]
  onSelectRecent: (conversationId: string) => void
  onQueueDraftReply: (text: string, zdr: boolean, supportAiMode?: SupportAIMode, proposalGroupId?: string) => Promise<boolean>
  onQueueInternalNote?: (text: string, zdr: boolean, supportAiMode?: SupportAIMode, proposalGroupId?: string) => Promise<boolean>
  onMacroExecuted: (ticket: SupportTicket) => void
  onOpenModal: (modal: InboxModalRequest) => void
  onTriageProposed?: () => void
  filter?: InboxRouteFilter
  selectedTicket: ZammadTicket | null
  ticketTeams?: readonly TicketTeam[]
  userId: string
  visibleTickets?: readonly ZammadTicket[]
}) {
  const i18n = useI18n()
  const [activeTab, setActiveTab] = createSignal<AsideTab>('details')
  const tabs = createMemo(() => [
    { id: 'details' as const, label: i18n.tr('Detaljer', 'Details') },
    { id: 'verevon' as const, label: 'Verevon' },
    { id: 'actions' as const, label: i18n.tr('Handlinger', 'Actions') },
    { id: 'audit' as const, label: i18n.tr('Revisjon', 'Audit') },
  ])

  return (
    <aside aria-label={i18n.tr('Supportkontekst og verktøy', 'Support context and tools')} class="verevon-inbox-aside">
      <div class="verevon-inbox-aside__header">
        <div class="verevon-inbox-aside__tabs" role="tablist" aria-label={i18n.tr('Supportkontekst', 'Support context')}>
          <For each={tabs()}>
            {(tab) => (
              <AsideTabButton
                active={activeTab() === tab.id}
                controls={`inbox-aside-panel-${tab.id}`}
                id={`inbox-aside-tab-${tab.id}`}
                onClick={() => setActiveTab(tab.id)}
              >
                {tab.label}
              </AsideTabButton>
            )}
          </For>
        </div>
      </div>

      <Show when={activeTab() === 'details'}>
        <div id="inbox-aside-panel-details" role="tabpanel" aria-labelledby="inbox-aside-tab-details">
          <DetailsPanel
            onOpenModal={props.onOpenModal}
            onSelectRecent={props.onSelectRecent}
            recent={props.recent}
            selectedTicket={props.selectedTicket}
          />
        </div>
      </Show>
      <Show when={activeTab() === 'verevon'}>
        <div id="inbox-aside-panel-verevon" role="tabpanel" aria-labelledby="inbox-aside-tab-verevon">
          <VerevonPanel
            orgId={props.orgId}
            articles={props.articles}
            onQueueDraftReply={props.onQueueDraftReply}
            onQueueInternalNote={props.onQueueInternalNote}
            onMacroExecuted={props.onMacroExecuted}
            onOpenModal={props.onOpenModal}
            onTriageProposed={props.onTriageProposed}
            filter={props.filter}
            selectedTicket={props.selectedTicket}
            ticketTeams={props.ticketTeams}
            userId={props.userId}
            visibleTickets={props.visibleTickets}
          />
        </div>
      </Show>
      <Show when={activeTab() === 'actions'}>
        <div id="inbox-aside-panel-actions" role="tabpanel" aria-labelledby="inbox-aside-tab-actions">
          <CalendarPanel selectedTicket={props.selectedTicket} />
        </div>
      </Show>
      <Show when={activeTab() === 'audit'}>
        <div id="inbox-aside-panel-audit" role="tabpanel" aria-labelledby="inbox-aside-tab-audit">
          <ActivityPanel orgId={props.orgId} selectedTicket={props.selectedTicket} userId={props.userId} />
        </div>
      </Show>
    </aside>
  )
}

export type RecentConversationRef = {
  conversationId: string
  title: string
  channel: string
  createdAt: string
}

function DetailsPanel(props: {
  onOpenModal: (modal: InboxModalRequest) => void
  onSelectRecent: (conversationId: string) => void
  recent: RecentConversationRef[]
  selectedTicket: ZammadTicket | null
}) {
  const i18n = useI18n()
  return (
    <div class="verevon-inbox-aside-scroll">
      <Show
        when={props.selectedTicket}
        fallback={
          <EmptyAsideState
            icon={<UserRound class="size-6" />}
            title={i18n.tr('Ingen kunde valgt', 'No customer selected')}
            body={i18n.tr(
              'Åpne en sak for å se kundedetaljer, tagger og nylige samtaler.',
              'Open a ticket to see customer details, tags, and recent conversations.',
            )}
          />
        }
      >
        {(ticket) => (
          <>
            <section class="verevon-inbox-customer-card">
              <div class="verevon-inbox-customer-card__identity">
                <div>{(ticket().customer?.firstname?.[0] ?? customerName(ticket())[0] ?? '?').toUpperCase()}</div>
                <div>
                  <div>
                    <h2>{customerName(ticket())}</h2>
                  </div>
                  <Show when={ticket().customer?.email}>
                    <p>{ticket().customer?.email}</p>
                  </Show>
                </div>
              </div>

              <div class="verevon-inbox-field-stack">
                <FieldRow label={i18n.tr('Kanal', 'Channel')} value={titleCase(ticket().channel ?? 'email')} />
                <FieldRow label="Status" value={titleCase(ticket().state?.name ?? 'open')} />
                <FieldRow label={i18n.tr('Prioritet', 'Priority')} value={titleCase(ticket().priority?.name ?? 'normal')} />
                <FieldRow
                  label={i18n.tr('Tildelt', 'Assignee')}
                  value={ticket().owner ? `${ticket().owner?.firstname} ${ticket().owner?.lastname}`.trim() : i18n.tr('Ikke tildelt', 'Unassigned')}
                />
                <FieldRow label={i18n.tr('Team-innboks', 'Team inbox')} value={ticket().group?.name ?? 'Support'} />
                <Show when={ticket().supportTicket?.sla_state}>
                  {(slaState) => <FieldRow label="SLA" value={titleCase(slaState() ?? '')} />}
                </Show>
                <Show when={ticket().supportTicket?.due_at}>
                  {(dueAt) => <FieldRow label={i18n.tr('SLA-frist', 'SLA deadline')} value={formatRelativeTime(dueAt() ?? '')} />}
                </Show>
                <Show when={ticket().supportTicket?.follow_up_at}>
                  {(followUpAt) => <FieldRow label={i18n.tr('Teamoppfølging', 'Team follow-up')} value={i18n.tr(`${formatRelativeTime(followUpAt() ?? '')} siden`, `${formatRelativeTime(followUpAt() ?? '')} from now`)} />}
                </Show>
              </div>
            </section>

            <AccordionSection defaultOpen icon={<FileText class="size-4" />} title={i18n.tr('Samtale', 'Conversation')}>
              <FieldRow label={i18n.tr('Referanse', 'Reference')} value={ticket().number} />
              <FieldRow label={i18n.tr('Emne', 'Subject')} value={ticket().title} />
              <Show when={ticket().customer?.email}>
                <FieldRow label={i18n.tr('E-post', 'Email')} value={ticket().customer!.email} />
              </Show>
              <FieldRow label={i18n.tr('Åpnet', 'Opened')} value={formatRelativeTime(ticket().created_at)} />
            </AccordionSection>

            <AccordionSection defaultOpen icon={<Sparkles class="size-4" />} title={i18n.tr('Tagger', 'Tags')}>
              <Show
                when={(ticket().tags ?? []).length}
                fallback={<p class="verevon-inbox-muted">{i18n.tr('Ingen tagger på denne samtalen ennå.', 'No tags on this conversation yet.')}</p>}
              >
                <div class="verevon-inbox-detail-tags">
                  <For each={ticket().tags ?? []}>{(tag) => <span class="verevon-inbox-detail-tag">{tag}</span>}</For>
                </div>
              </Show>
            </AccordionSection>

            <AccordionSection
              icon={<MessageCircle class="size-4" />}
              title={`${i18n.tr('Nylige samtaler', 'Recent conversations')}${(props.recent?.length ?? 0) ? ` (${props.recent!.length})` : ''}`}
            >
              <Show
                when={(props.recent?.length ?? 0) > 0}
                fallback={<p class="verevon-inbox-muted">{i18n.tr('Ingen andre samtaler fra denne kontakten.', 'No other conversations from this contact.')}</p>}
              >
                <ul class="verevon-inbox-recent-list">
                  <For each={props.recent ?? []}>
                    {(item) => (
                      <li>
                        <button type="button" onClick={() => props.onSelectRecent(item.conversationId)}>
                          <span class="verevon-inbox-recent-list__title">{item.title || i18n.tr('(uten emne)', '(no subject)')}</span>
                          <span class="verevon-inbox-recent-list__meta">
                            {titleCase(item.channel)} · {formatRelativeTime(item.createdAt)}
                          </span>
                        </button>
                      </li>
                    )}
                  </For>
                </ul>
              </Show>
            </AccordionSection>
          </>
        )}
      </Show>
    </div>
  )
}

function VerevonPanel(props: {
  orgId: string
  articles: ZammadArticle[]
  onQueueDraftReply: (text: string, zdr: boolean, supportAiMode?: SupportAIMode, proposalGroupId?: string) => Promise<boolean>
  onQueueInternalNote?: (text: string, zdr: boolean, supportAiMode?: SupportAIMode, proposalGroupId?: string) => Promise<boolean>
  onMacroExecuted: (ticket: SupportTicket) => void
  onOpenModal: (modal: InboxModalRequest) => void
  onTriageProposed?: () => void
  filter?: InboxRouteFilter
  selectedTicket: ZammadTicket | null
  ticketTeams?: readonly TicketTeam[]
  userId: string
  visibleTickets?: readonly ZammadTicket[]
}) {
  const i18n = useI18n()
  const [draft, setDraft] = createSignal<string | null>(null)
  const [draftZdr, setDraftZdr] = createSignal(false)
  const [draftSupportAiMode, setDraftSupportAiMode] = createSignal<SupportAIMode>('review')
  const [draftProposalGroupId, setDraftProposalGroupId] = createSignal<string | undefined>(undefined)
  const [draftKind, setDraftKind] = createSignal<'reply' | 'note'>('reply')
  const [draftLoading, setDraftLoading] = createSignal(false)
  const [summary, setSummary] = createSignal<string | null>(null)
  const [summaryLoading, setSummaryLoading] = createSignal(false)
  const [answer, setAnswer] = createSignal<string | null>(null)
  const [answerLoading, setAnswerLoading] = createSignal(false)
  const [sources, setSources] = createSignal<AssistSource[]>([])
  const [resolutionPlan, setResolutionPlan] = createSignal<InboxResolutionPlan | null>(null)
  const [resolutionPlanZdr, setResolutionPlanZdr] = createSignal(false)
  const [resolutionPlanSupportAiMode, setResolutionPlanSupportAiMode] = createSignal<SupportAIMode>('review')
  const [resolutionPlanGroupId, setResolutionPlanGroupId] = createSignal<string | undefined>(undefined)
  const [error, setError] = createSignal<string | null>(null)
  const [question, setQuestion] = createSignal('')
  const [sharedThreadId, setSharedThreadId] = createSignal<string | null>(null)
  const [runningCard, setRunningCard] = createSignal<string | null>(null)
  let draftRequestID = 0
  let summaryRequestID = 0
  let answerRequestID = 0
  let cardRequestID = 0

  const transcript = createMemo<AssistMessage[]>(() =>
    props.articles.map((a) => ({
      agent: a.sender?.toLowerCase() === 'agent',
      from: a.from,
      body: a.bodyText || stripToText(a.body ?? ''),
    })),
  )
  const customer = () => (props.selectedTicket ? customerName(props.selectedTicket) : undefined)
  const ready = () => Boolean(props.selectedTicket && props.orgId)
  const assistContext = () => buildInboxAssistContext({
    selectedTicket: props.selectedTicket,
    visibleTickets: props.visibleTickets,
    filter: props.filter,
    orgId: props.orgId,
  })
  const supportThreadScope = (): SupportChatThreadScope | null => {
    const conversationId = props.selectedTicket?.conversationId
    if (!props.userId.trim() || !props.orgId.trim() || !conversationId?.trim()) return null
    return { userId: props.userId, orgId: props.orgId, conversationId }
  }
  const applySources = (next: AssistSource[]) => {
    // Evidence belongs to exactly one latest model output. Never leave sources
    // from a prior result visible beside a newer uncited answer or a new case.
    setSources(next)
  }
  const selectionScopeKey = () => JSON.stringify([
    props.userId.trim(),
    props.orgId.trim(),
    props.selectedTicket?.id ?? null,
    props.selectedTicket?.conversationId?.trim() ?? null,
  ])
  const isCurrentSelection = (scopeKey: string) => selectionScopeKey() === scopeKey

  createEffect(() => {
    const selectedScopeKey = selectionScopeKey()
    void selectedScopeKey
    draftRequestID += 1
    summaryRequestID += 1
    answerRequestID += 1
    cardRequestID += 1
    setDraftLoading(false)
    setSummaryLoading(false)
    setAnswerLoading(false)
    setRunningCard(null)
    setDraft(null)
    setSummary(null)
    setAnswer(null)
    setSources([])
    setResolutionPlan(null)
    setResolutionPlanGroupId(undefined)
    setDraftProposalGroupId(undefined)
    setError(null)
    setQuestion('')
    const scope = supportThreadScope()
    setSharedThreadId(scope ? readSupportChatThread(scope) : null)
  })

  const generateDraft = async (instruction?: string, kind: 'reply' | 'note' = 'reply') => {
    if (!ready() || draftLoading()) return
    const selectionKey = selectionScopeKey()
    const requestID = ++draftRequestID
    setDraftLoading(true)
    setDraftKind(kind)
    setError(null)
    applySources([])
    try {
      const res = await runAssist(props.orgId, 'draft', transcript(), { instruction, customer: customer(), contextPack: assistContext() })
      if (!isCurrentSelection(selectionKey) || requestID !== draftRequestID) return
      setDraft(res.text || i18n.tr('Verevon returnerte et tomt svar.', 'Verevon returned an empty reply.'))
      setDraftZdr(res.zdr)
      setDraftSupportAiMode(res.supportAiMode)
      setDraftProposalGroupId(undefined)
      applySources(res.sources)
    } catch {
      if (!isCurrentSelection(selectionKey) || requestID !== draftRequestID) return
      setError(i18n.tr('Verevon kunne ikke generere et svar. Prøv igjen.', 'Verevon could not generate a reply. Try again.'))
    } finally {
      if (isCurrentSelection(selectionKey) && requestID === draftRequestID) setDraftLoading(false)
    }
  }

  const generateSummary = async () => {
    if (!ready() || summaryLoading()) return
    const selectionKey = selectionScopeKey()
    const requestID = ++summaryRequestID
    setSummaryLoading(true)
    setError(null)
    applySources([])
    try {
      const res = await runAssist(props.orgId, 'summarize', transcript(), { customer: customer(), contextPack: assistContext() })
      if (!isCurrentSelection(selectionKey) || requestID !== summaryRequestID) return
      setSummary(res.text || i18n.tr('Ingen oppsummering tilgjengelig.', 'No summary available.'))
      applySources(res.sources)
    } catch {
      if (!isCurrentSelection(selectionKey) || requestID !== summaryRequestID) return
      setError(i18n.tr('Verevon kunne ikke oppsummere. Prøv igjen.', 'Verevon could not summarize. Try again.'))
    } finally {
      if (isCurrentSelection(selectionKey) && requestID === summaryRequestID) setSummaryLoading(false)
    }
  }

  const runCard = async (id: string, mode: AssistMode, instruction?: string) => {
    if (!ready() || runningCard()) return
    const selectionKey = selectionScopeKey()
    const requestID = ++cardRequestID
    setRunningCard(id)
    setError(null)
    applySources([])
    try {
      const res = await runAssist(props.orgId, mode, transcript(), { instruction, customer: customer(), contextPack: assistContext() })
      if (!isCurrentSelection(selectionKey) || requestID !== cardRequestID) return
      applySources(res.sources)
      if (mode === 'draft') {
        setDraft(res.text)
        setDraftZdr(res.zdr)
        setDraftSupportAiMode(res.supportAiMode)
        setDraftProposalGroupId(undefined)
      }
      else setAnswer(res.text)
    } catch {
      if (!isCurrentSelection(selectionKey) || requestID !== cardRequestID) return
      setError(i18n.tr('Verevon-handlingen feilet. Prøv igjen.', 'Verevon action failed. Try again.'))
    } finally {
      if (isCurrentSelection(selectionKey) && requestID === cardRequestID) setRunningCard(null)
    }
  }

  const createResolutionPlan = async () => {
    if (!ready() || runningCard()) return
    const selectionKey = selectionScopeKey()
    const requestID = ++cardRequestID
    setRunningCard('resolution')
    setError(null)
    applySources([])
    try {
      const ticketTeams = (props.ticketTeams ?? []).filter((team) => team.active).map(({ id, name }) => ({ id, name }))
      const result = await runAssist(props.orgId, 'resolution', transcript(), {
        customer: customer(),
        contextPack: assistContext(),
        ticketTeams,
      })
      if (!isCurrentSelection(selectionKey) || requestID !== cardRequestID) return
      applySources(result.sources)
      const plan = parseInboxResolutionPlan(result.text, { ticketTeams })
      if (!plan) {
        setAnswer(i18n.tr(
          'Verevon returnerte ikke en gyldig, begrenset løsningsplan. Ingen utkast eller saksforslag ble opprettet.',
          'Verevon did not return a valid, bounded resolution plan. No draft or ticket proposal was created.',
        ))
        return
      }
      setResolutionPlan(plan)
      setResolutionPlanZdr(result.zdr)
      setResolutionPlanSupportAiMode(result.supportAiMode)
      setResolutionPlanGroupId(newResolutionProposalGroupID())
    } catch {
      if (!isCurrentSelection(selectionKey) || requestID !== cardRequestID) return
      setError(i18n.tr('Verevon kunne ikke lage en løsningsplan. Prøv igjen.', 'Verevon could not prepare a resolution plan. Try again.'))
    } finally {
      if (isCurrentSelection(selectionKey) && requestID === cardRequestID) setRunningCard(null)
    }
  }

  const stageTriageProposal = async (
    proposal: InboxTriageProposal,
    zdr: boolean,
    supportAiMode: SupportAIMode,
    selectionKey: string,
    proposalGroupId?: string,
  ): Promise<boolean> => {
    if (!isCurrentSelection(selectionKey)) return false
    const ticket = props.selectedTicket
    const conversationId = ticket?.conversationId
    if (!ticket || !conversationId || !props.userId) return false
    const evidenceMessageIds = props.articles.map((article) => String(article.id)).slice(0, 25)
    const actor = { type: 'human' as const, orgId: props.orgId, userId: props.userId }
    if (zdr) {
      setAnswer(i18n.tr(
        `ZDR er aktiv: triage-forslaget (${Math.round(proposal.confidence * 100)}% konfidens) vises bare her og blir ikke lagret for gjennomgang. ${proposal.reason}`,
        `ZDR is active: the triage proposal (${Math.round(proposal.confidence * 100)}% confidence) is shown only here and is not retained for review. ${proposal.reason}`,
      ))
      return false
    }
    if (supportAiMode !== 'review') {
      setAnswer(i18n.tr(
        `Assistentmodus er aktiv: triage-forslaget (${Math.round(proposal.confidence * 100)}% konfidens) vises bare her og blir ikke lagret eller brukt. ${proposal.reason}`,
        `Assist mode is active: the triage proposal (${Math.round(proposal.confidence * 100)}% confidence) is shown only here and was not retained or applied. ${proposal.reason}`,
      ))
      return false
    }
    const supportTicket = ticket.supportTicket
    if (supportTicket) {
      await createTicketUpdateProposal({
        conversationId,
        ticketId: supportTicket.id,
        confidence: proposal.confidence,
        reason: proposal.reason,
        evidenceMessageIds,
        suggestedFields: proposal.suggestedFields,
        proposalGroupId,
      })
      if (!isCurrentSelection(selectionKey)) return false
      if (proposal.suggestedFields.work_type === 'incident' && proposal.incident) {
        await createIncidentProposal({
          conversationId,
          ticketId: supportTicket.id,
          title: proposal.incident.title,
          severity: proposal.suggestedFields.severity ?? 'medium',
          customerImpact: proposal.incident.customer_impact,
          confidence: proposal.confidence,
          reason: proposal.reason,
          evidenceMessageIds,
          proposalGroupId,
        })
        if (!isCurrentSelection(selectionKey)) return false
      }
      if (proposal.suggestedFields.work_type === 'incident' && proposal.problem) {
        await createProblemProposal({
          conversationId,
          title: proposal.problem.title,
          summary: proposal.problem.summary,
          rootCause: proposal.problem.root_cause,
          confidence: proposal.confidence,
          reason: proposal.reason,
          evidenceMessageIds,
          proposalGroupId,
        })
        if (!isCurrentSelection(selectionKey)) return false
      }
    } else {
      await executeAction('tickets.classify_conversation', actor, {
        conversationId,
        confidence: proposal.confidence,
        reason: proposal.reason,
        suggestedFields: proposal.suggestedFields,
        evidenceMessageIds,
      })
      if (!isCurrentSelection(selectionKey)) return false
    }
    if (!isCurrentSelection(selectionKey)) return false
    props.onTriageProposed?.()
    setAnswer(i18n.tr(
      supportTicket
        ? proposal.suggestedFields.work_type === 'incident'
          ? `AI-forslag til saksoppdatering, hendelse${proposal.problem ? ' og problem' : ''} opprettet (${Math.round(proposal.confidence * 100)}% konfidens). Gjennomgå hvert forslag og godkjenn eller avvis før noe endres.`
          : `AI-forslag til saksoppdatering opprettet (${Math.round(proposal.confidence * 100)}% konfidens). Gjennomgå feltene og godkjenn eller avvis før saken endres.`
        : `Triage-forslag opprettet (${Math.round(proposal.confidence * 100)}% konfidens). Gjennomgå feltene og godkjenn eller avvis forslaget over før en sak eller ruting endres.`,
      supportTicket
        ? proposal.suggestedFields.work_type === 'incident'
          ? `AI ticket-update, incident${proposal.problem ? ', and Problem' : ''} proposals created (${Math.round(proposal.confidence * 100)}% confidence). Review each proposal and approve or reject it before anything changes.`
          : `AI ticket-update proposal created (${Math.round(proposal.confidence * 100)}% confidence). Review the fields and approve or reject it before the ticket changes.`
        : `Triage proposal created (${Math.round(proposal.confidence * 100)}% confidence). Review the fields and approve or reject the proposal above before any ticket or routing change is made.`,
    ))
    return true
  }

  const proposeTriage = async () => {
    if (!ready() || runningCard()) return
    const ticket = props.selectedTicket
    const ticketID = ticket?.id
    const conversationId = ticket?.conversationId
    if (!ticketID || !conversationId || !props.userId) {
      setError(i18n.tr('Denne samtalen mangler en verifiserbar identitet for AI-triage.', 'This conversation has no verifiable identity for AI triage.'))
      return
    }
    const selectionKey = selectionScopeKey()
    const requestID = ++cardRequestID
    setRunningCard('route')
    setError(null)
    applySources([])
    try {
      const ticketTeams = (props.ticketTeams ?? []).filter((team) => team.active).map(({ id, name }) => ({ id, name }))
      const result = await runAssist(props.orgId, 'triage', transcript(), {
        customer: customer(),
        contextPack: assistContext(),
        ticketTeams,
      })
      if (!isCurrentSelection(selectionKey) || requestID !== cardRequestID) return
      applySources(result.sources)
      const proposal = parseInboxTriageProposal(result.text, { ticketTeams })
      if (!proposal) {
        setAnswer(i18n.tr(
          'Verevon returnerte ikke et gyldig, begrenset triage-forslag. Ingen sak eller ruting ble opprettet.',
          'Verevon did not return a valid, bounded triage proposal. No ticket or routing change was created.',
        ))
        return
      }
      await stageTriageProposal(proposal, result.zdr, result.supportAiMode, selectionKey)
      if (!isCurrentSelection(selectionKey) || requestID !== cardRequestID) return
    } catch {
      if (!isCurrentSelection(selectionKey) || requestID !== cardRequestID) return
      setError(i18n.tr('Verevon kunne ikke opprette et triage-forslag. Ingen sak eller ruting ble endret.', 'Verevon could not create a triage proposal. No ticket or routing change was made.'))
    } finally {
      if (isCurrentSelection(selectionKey) && requestID === cardRequestID) setRunningCard(null)
    }
  }

  const askVerevon = async () => {
    const q = question().trim()
    if (!q || !ready() || answerLoading()) return
    const selectionKey = selectionScopeKey()
    const scope = supportThreadScope()
    const threadId = scope ? readSupportChatThread(scope) ?? undefined : undefined
    const requestID = ++answerRequestID
    setAnswerLoading(true)
    setError(null)
    applySources([])
    try {
      const res = await runAssist(props.orgId, 'ask', transcript(), {
        question: q,
        customer: customer(),
        contextPack: assistContext(),
        threadId,
      })
      if (!isCurrentSelection(selectionKey) || requestID !== answerRequestID) return
      if (res.threadId && scope && supportThreadScope()?.userId === scope.userId && supportThreadScope()?.orgId === scope.orgId && supportThreadScope()?.conversationId === scope.conversationId) {
        bindSupportChatThread(scope, res.threadId)
        setSharedThreadId(res.threadId)
        setActiveChatThreadId(res.threadId)
      }
      setAnswer(res.text || i18n.tr('Verevon hadde ikke noe svar.', 'Verevon had no answer.'))
      applySources(res.sources)
    } catch {
      if (!isCurrentSelection(selectionKey) || requestID !== answerRequestID) return
      setError(i18n.tr('Verevon kunne ikke svare. Prøv igjen.', 'Verevon could not answer. Try again.'))
    } finally {
      if (isCurrentSelection(selectionKey) && requestID === answerRequestID) setAnswerLoading(false)
    }
  }

  return (
    <div class="verevon-inbox-verevon-panel">
      <div class="verevon-inbox-aside-scroll verevon-inbox-aside-scroll--panel">
        <Show
          when={props.selectedTicket}
          fallback={
            <EmptyAsideState
              icon={<Bot class="size-6" />}
              title={i18n.tr('Velg en sak', 'Select a ticket')}
              body={i18n.tr(
                'Verevon kan utkaste svar, oppsummere kontekst og finne relevante kilder når en samtale er åpen.',
                'Verevon can draft replies, summarize context, and surface relevant sources once a conversation is open.',
              )}
            />
          }
        >
          <div class="verevon-inbox-card-stack">
            <section class="verevon-inbox-aside-card verevon-inbox-aside-card--soft" aria-label={i18n.tr('Samtalegrunnlag', 'Conversation context')}>
              <div class="verevon-inbox-card-heading verevon-inbox-card-heading--between">
                <div>
                  <MessageCircle class="size-4" />
                  <h2>{i18n.tr('Samtalegrunnlag', 'Conversation context')}</h2>
                </div>
                <span class="verevon-inbox-muted">{props.articles.length} {i18n.tr('meldinger', 'messages')}</span>
              </div>
              <Show
                when={props.articles.length > 0}
                fallback={<p class="verevon-inbox-muted">{i18n.tr('Ingen meldinger er tilgjengelige i denne lesingen.', 'No messages are available in this authorized read.')}</p>}
              >
                <div class="verevon-inbox-conversation-context">
                  <For each={props.articles.slice(-6)}>
                    {(article) => (
                      <article class="verevon-inbox-conversation-context__message">
                        <div>
                          <strong>{article.internal ? i18n.tr('Privat notat', 'Internal note') : article.from || i18n.tr('Kunde', 'Customer')}</strong>
                          <span>{article.internal ? i18n.tr('Kun for teamet', 'Team-only') : article.sender || i18n.tr('Kunde', 'Customer')}</span>
                        </div>
                        <p>{article.bodyText || stripToText(article.body ?? '')}</p>
                      </article>
                    )}
                  </For>
                </div>
                <small class="verevon-inbox-muted">{i18n.tr('Verevon bruker denne autoriserte samtalen sammen med valgt sak.', 'Verevon uses this permission-scoped conversation together with the selected case.')}</small>
              </Show>
            </section>

            <Show when={error()}>
              <div class="verevon-inbox-aside-card verevon-inbox-aside-card--error" role="alert">
                {error()}
              </div>
            </Show>

            <section class="verevon-inbox-aside-card">
              <div class="verevon-inbox-card-heading">
                <Bot class="size-4" />
                <h2>{i18n.tr('Verevon handlingsplan', 'Verevon action plan')}</h2>
              </div>
              <ActionSuggestion
                title={i18n.tr('Internt handlingsnotat', 'Internal action note')}
                body={i18n.tr('Utkast et kort privat notat for operatører; det sendes aldri til kunden.', 'Draft a concise private operator note; it is never sent to the customer.')}
                actionLabel={draftLoading() && draftKind() === 'note' ? i18n.tr('Lager utkast …', 'Drafting…') : i18n.tr('Utkast', 'Draft')}
                onRun={() => void generateDraft('Write a concise internal note for support operators. Do not address the customer, do not promise delivery, and state only transcript-supported facts.', 'note')}
              />
              <ActionSuggestion
                title={i18n.tr('Bekreft hensikt', 'Confirm intent')}
                body={i18n.tr("Oppdag kundens primære hensikt og beste neste handling.", "Detect the customer's primary intent and the best next action.")}
                actionLabel={runningCard() === 'intent' ? i18n.tr('Kjører …', 'Running…') : i18n.tr('Kjør', 'Run')}
                onRun={() => void runCard('intent', 'intent')}
              />
              <ActionSuggestion
                title={i18n.tr('Kildebasert svar', 'Source-backed reply')}
                body={i18n.tr(
                  'Utkast et svar som kun er basert på fakta som støttes av samtalen.',
                  'Draft a reply grounded only in facts supported by the conversation.',
                )}
                actionLabel={runningCard() === 'source' ? i18n.tr('Kjører …', 'Running…') : i18n.tr('Kjør', 'Run')}
                onRun={() =>
                  void runCard(
                    'source',
                    'draft',
                    'Only assert facts supported by the transcript; do not invent policy or promises.',
                  )
                }
              />
              <ActionSuggestion
                title={i18n.tr('Foreslå triage', 'Propose triage')}
                body={i18n.tr(
                  'Opprett et begrenset, gjennomgåbart forslag til arbeidstype, kategori, hensikt, prioritet og alvorlighetsgrad.',
                  'Create a bounded, reviewable proposal for work type, category, intent, priority, and severity.',
                )}
                actionLabel={runningCard() === 'route' ? i18n.tr('Kjører …', 'Running…') : i18n.tr('Kjør', 'Run')}
                onRun={() => void proposeTriage()}
              />
              <ActionSuggestion
                title={i18n.tr('Lag løsningsplan', 'Prepare resolution plan')}
                body={i18n.tr(
                  'Samle et svarutkast, privat notat og begrenset triage i én plan som fortsatt må iscenesettes og gjennomgås separat.',
                  'Prepare a reply, private note, and bounded triage in one plan; every item still has to be staged and reviewed separately.',
                )}
                actionLabel={runningCard() === 'resolution' ? i18n.tr('Forbereder …', 'Preparing…') : i18n.tr('Forbered', 'Prepare')}
                onRun={() => void createResolutionPlan()}
              />
            </section>

            <Show when={resolutionPlan()}>
              {(plan) => (
                <section class="verevon-inbox-aside-card verevon-inbox-aside-card--soft">
                  <div class="verevon-inbox-card-heading verevon-inbox-card-heading--between">
                    <div>
                      <Sparkles class="size-4" />
                      <h2>{i18n.tr('Foreslått løsningsplan', 'Proposed resolution plan')}</h2>
                    </div>
                    <button type="button" onClick={() => setResolutionPlan(null)} aria-label={i18n.tr('Fjern løsningsplan', 'Dismiss resolution plan')}>
                      {i18n.tr('Fjern', 'Clear')}
                    </button>
                  </div>
                  <p class="verevon-inbox-ai-text">{plan().summary}</p>
                  <Show when={plan().reply}>
                    {(reply) => (
                      <div class="verevon-inbox-field-stack">
                        <strong>{i18n.tr('Svarutkast', 'Reply draft')}</strong>
                        <p class="verevon-inbox-ai-text">{reply()}</p>
                        <button type="button" onClick={() => {
                          setDraft(reply())
                          setDraftKind('reply')
                          setDraftZdr(resolutionPlanZdr())
                          setDraftSupportAiMode(resolutionPlanSupportAiMode())
                          setDraftProposalGroupId(resolutionPlanGroupId())
                          setResolutionPlan((current) => current ? { ...current, reply: undefined } : null)
                        }}>
                          {i18n.tr('Legg i svarhjelp', 'Stage in reply assistance')}
                        </button>
                      </div>
                    )}
                  </Show>
                  <Show when={plan().internalNote}>
                    {(note) => (
                      <div class="verevon-inbox-field-stack">
                        <strong>{i18n.tr('Privat notat', 'Private note')}</strong>
                        <p class="verevon-inbox-ai-text">{note()}</p>
                        <button type="button" onClick={() => {
                          setDraft(note())
                          setDraftKind('note')
                          setDraftZdr(resolutionPlanZdr())
                          setDraftSupportAiMode(resolutionPlanSupportAiMode())
                          setDraftProposalGroupId(resolutionPlanGroupId())
                          setResolutionPlan((current) => current ? { ...current, internalNote: undefined } : null)
                        }}>
                          {i18n.tr('Legg i svarhjelp', 'Stage in reply assistance')}
                        </button>
                      </div>
                    )}
                  </Show>
                  <Show when={plan().triage}>
                    {(triage) => (
                      <div class="verevon-inbox-field-stack">
                        <strong>{i18n.tr('Triage-forslag', 'Triage proposal')}</strong>
                        <p class="verevon-inbox-ai-text">{triage().reason}</p>
                        <p class="verevon-inbox-muted">
                          {i18n.tr('Konfidens', 'Confidence')}: {Math.round(triage().confidence * 100)}% · {Object.entries(triage().suggestedFields).map(([key, value]) => `${key}: ${value}`).join(' · ')}
                        </p>
                        <button type="button" onClick={() => void stageTriageProposal(triage(), resolutionPlanZdr(), resolutionPlanSupportAiMode(), selectionScopeKey(), resolutionPlanGroupId()).then((staged) => {
                          if (staged) setResolutionPlan((current) => current ? { ...current, triage: undefined } : null)
                        })}>
                          {resolutionPlanZdr() || resolutionPlanSupportAiMode() !== 'review'
                            ? i18n.tr('Vis som midlertidig forslag', 'Keep as transient proposal')
                            : i18n.tr('Send triage til gjennomgang', 'Send triage to review')}
                        </button>
                      </div>
                    )}
                  </Show>
                  <p class="verevon-inbox-muted">
                    {i18n.tr(
                      'Planen utfører ingenting. Hvert element må først iscenesettes og deretter gjennomgås i sin egen godkjenningsflyt.',
                      'This plan executes nothing. Each item must first be staged and then reviewed in its own approval flow.',
                    )}
                  </p>
                </section>
              )}
            </Show>

            <Show when={answer()}>
              <section class="verevon-inbox-aside-card verevon-inbox-aside-card--answer">
                <div class="verevon-inbox-card-heading verevon-inbox-card-heading--between">
                  <div>
                    <Bot class="size-4" />
                    <h2>Verevon</h2>
                  </div>
                  <button type="button" onClick={() => setAnswer(null)} aria-label={i18n.tr('Lukk svar', 'Dismiss answer')}>
                    {i18n.tr('Fjern', 'Clear')}
                  </button>
                </div>
                <p class="verevon-inbox-ai-text">{answer()}</p>
              </section>
            </Show>

            <section class="verevon-inbox-aside-card verevon-inbox-aside-card--soft">
              <div class="verevon-inbox-card-heading verevon-inbox-card-heading--between">
                <div>
                  <Sparkles class="size-4" />
                  <h2>{i18n.tr('Svarhjelp', 'Reply assistance')}</h2>
                </div>
                <button type="button" disabled={draftLoading()} onClick={() => void generateDraft()}>
                  <RefreshCw class={cn('size-3.5', draftLoading() && 'verevon-inbox-spin')} />
                  {draftLoading() ? i18n.tr('Lager utkast …', 'Drafting…') : i18n.tr('Generer', 'Generate')}
                </button>
              </div>
              <Show
                when={!draftLoading()}
                fallback={
                  <div class="verevon-inbox-reply-skeleton">
                    <span />
                    <span />
                  </div>
                }
              >
                <Show when={draft()} fallback={<p>{i18n.tr('Generer et forslag til svar basert på denne samtalen.', 'Generate a suggested reply grounded in this conversation.')}</p>}>
                  <p class="verevon-inbox-ai-text">{draft()}</p>
                  <div class="verevon-inbox-draft-actions">
                    <button type="button" class="verevon-inbox-btn-primary" disabled={draftKind() === 'note' && !props.onQueueInternalNote} onClick={() => void (draftKind() === 'note' ? props.onQueueInternalNote?.(draft() ?? '', draftZdr(), draftSupportAiMode(), draftProposalGroupId()) : props.onQueueDraftReply(draft() ?? '', draftZdr(), draftSupportAiMode(), draftProposalGroupId()))}>
                      {draftZdr()
                        ? i18n.tr('Sett inn midlertidig utkast', 'Insert transient draft')
                        : draftSupportAiMode() !== 'review'
                          ? i18n.tr('Sett inn utkast', 'Insert draft')
                        : draftKind() === 'note' ? i18n.tr('Send notat til gjennomgang', 'Send note to review') : i18n.tr('Send til gjennomgang', 'Send to review')}
                    </button>
                    <button type="button" onClick={() => void generateDraft('Rewrite this differently.')}>
                      {i18n.tr('Generer på nytt', 'Regenerate')}
                    </button>
                  </div>
                  <p class="verevon-inbox-muted">
                    {draftZdr()
                      ? i18n.tr('ZDR er aktiv: utkastet kan ikke lagres i gjennomgangskøen.', 'ZDR is active: this draft cannot be retained in the review queue.')
                      : draftSupportAiMode() !== 'review'
                        ? i18n.tr('Assistentmodus er aktiv: utkastet forblir lokalt til en operatør sender eller lagrer det.', 'Assist mode is active: the draft stays local until an operator sends or saves it.')
                      : draftKind() === 'note'
                        ? i18n.tr('Forslaget lagres for gjennomgang av nøyaktig tekst før det lagres som et internt notat.', 'The proposal is retained for exact-text review before it is saved as an internal note.')
                        : i18n.tr('Forslaget lagres for gjennomgang av nøyaktig tekst før det kan sendes.', 'The proposal is retained for exact-text review before it can be sent.')}
                  </p>
                </Show>
              </Show>
            </section>

            <section class="verevon-inbox-aside-card">
              <div class="verevon-inbox-card-heading verevon-inbox-card-heading--between">
                <h2>{i18n.tr('Samtalesammendrag', 'Conversation summary')}</h2>
                <button type="button" disabled={summaryLoading()} onClick={() => void generateSummary()}>
                  {summaryLoading() ? i18n.tr('Oppsummerer …', 'Summarizing…') : i18n.tr('Oppsummer', 'Summarize')}
                </button>
              </div>
              <p class="verevon-inbox-ai-text">
                {summaryLoading()
                  ? i18n.tr('Genererer sammendrag …', 'Generating summary…')
                  : summary() ?? i18n.tr('Oppsummer samtalen og finn kundens hensikt.', 'Summarize the conversation and extract the customer intent.')}
              </p>
            </section>

            <section class="verevon-inbox-aside-card">
              <h2>{i18n.tr('Relevante kilder', 'Relevant sources')}</h2>
              <Show
                when={sources().length}
                fallback={<p class="verevon-inbox-muted">{i18n.tr('Det siste Verevon-svaret hadde ingen eksterne kilder; visningen er kun basert på samtaleutskriften.', 'The latest Verevon output has no external sources; it is based only on the conversation transcript.')}</p>}
              >
                <div class="verevon-inbox-source-stack">
                  <For each={sources()}>{(s) => <SourceRow title={s.title || s.uri || i18n.tr('Kilde', 'Source')} uri={s.uri} excerpt={s.excerpt} />}</For>
                </div>
              </Show>
            </section>

              <MacrosPanel
                orgId={props.orgId}
                onMacroExecuted={props.onMacroExecuted}
                selectedTicket={props.selectedTicket}
                userId={props.userId}
            />
          </div>
        </Show>
      </div>

      <div class="verevon-inbox-ask-verevon">
        <div class="verevon-inbox-ask-verevon__header">
          <span>{i18n.tr('Kontekst fra valgt samtale', 'Context from selected conversation')}</span>
          <Show
            when={sharedThreadId()}
            fallback={<em>{i18n.tr('Spør for å starte delt tråd', 'Ask to start a shared thread')}</em>}
          >
            {(threadId) => (
              <a href="/chat" onClick={() => setActiveChatThreadId(threadId())}>
                {i18n.tr('Åpne i Chat', 'Open in Chat')}
                <ArrowUpRight class="size-3.5" />
              </a>
            )}
          </Show>
        </div>
        <div>
          <input
            value={question()}
            onInput={(event) => setQuestion(event.currentTarget.value)}
            onKeyDown={(event) => {
              if (event.key === 'Enter') void askVerevon()
            }}
            placeholder={i18n.tr('Spør Verevon om denne samtalen', 'Ask Verevon about this conversation')}
            aria-label={i18n.tr('Spør Verevon et spørsmål', 'Ask Verevon a question')}
            disabled={!ready()}
          />
          <button type="button" disabled={answerLoading() || !ready()} onClick={() => void askVerevon()} aria-label={i18n.tr('Send spørsmål til Verevon', 'Send Verevon question')}>
            <Send class={cn('size-3.5', answerLoading() && 'verevon-inbox-spin')} />
          </button>
        </div>
      </div>
    </div>
  )
}

function newResolutionProposalGroupID(): string {
  const random = typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function'
    ? crypto.randomUUID().replaceAll('-', '')
    : `${Date.now()}${Math.random().toString(36).slice(2)}`
  return `resolution_${random}`
}

// Lightweight HTML→text for building the AI prompt from an article whose only
// body is HTML (the visible transcript uses the sandboxed EmailBody renderer).
function stripToText(value: string): string {
  return value
    .replace(/<style[\s\S]*?<\/style>/gi, ' ')
    .replace(/<[^>]*>/g, ' ')
    .replace(/&nbsp;/g, ' ')
    .replace(/\s+/g, ' ')
    .trim()
}

function CalendarPanel(props: { selectedTicket: ZammadTicket | null }) {
  const i18n = useI18n()
  const [calendar, { refetch: refetchCalendar }] = createResource(() => getNavbarCalendarState())
  const [selectedDate, setSelectedDate] = createSignal(new Date())
  const [eventTitle, setEventTitle] = createSignal('')
  const [noteText, setNoteText] = createSignal('')
  const [saving, setSaving] = createSignal<'event' | 'note' | null>(null)
  const [error, setError] = createSignal<string | null>(null)
  const selectedKey = () => formatDateKey(selectedDate())
  const events = () => calendar()?.events ?? []
  const notes = () => calendar()?.notes ?? []
  const selectedEvents = createMemo(() => events().filter((event) => formatDateKey(new Date(event.start)) === selectedKey()))
  const selectedNotes = createMemo(() => notes().filter((note) => note.date === selectedKey()))
  const upcomingEvents = createMemo(() => [...events()].sort((a, b) => new Date(a.start).getTime() - new Date(b.start).getTime()).slice(0, 4))

  const saveFollowUp = async () => {
    const fallbackTitle = props.selectedTicket
      ? i18n.tr(`Oppfølging av sak #${props.selectedTicket.number}`, `Follow up on ticket #${props.selectedTicket.number}`)
      : i18n.tr('Innboks-oppfølging', 'Inbox follow-up')
    const title = (eventTitle().trim() || fallbackTitle).slice(0, 160)
    const start = new Date(selectedDate())
    start.setHours(9, 0, 0, 0)
    const end = new Date(start)
    end.setMinutes(end.getMinutes() + 30)
    setSaving('event')
    try {
      await createNavbarCalendarEvent({ end: end.toISOString(), start: start.toISOString(), title, type: 'inbox-follow-up' })
      await refetchCalendar()
      setEventTitle('')
      setError(null)
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : i18n.tr('Oppfølgingen kunne ikke lagres.', 'The follow-up could not be saved.'))
    } finally {
      setSaving(null)
    }
  }

  const saveNote = async () => {
    const text = noteText().trim()
    if (!text) return
    setSaving('note')
    try {
      await createNavbarCalendarNote({ date: selectedKey(), kind: 'note', text })
      await refetchCalendar()
      setNoteText('')
      setError(null)
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : i18n.tr('Notatet kunne ikke lagres.', 'The note could not be saved.'))
    } finally {
      setSaving(null)
    }
  }

  return (
    <div class="verevon-inbox-aside-scroll verevon-inbox-calendar-panel">
      <div class="verevon-inbox-panel-title">
        <div>
          <h2>{i18n.tr('Innboks-kalender', 'Inbox calendar')}</h2>
          <p>{i18n.tr('Dine personlige oppfølginger og notater. Dette endrer ikke sakens status eller ansvar.', 'Your personal follow-ups and notes. This does not change ticket status or ownership.')}</p>
        </div>
        <CalendarDays class="size-5" />
      </div>

      <MiniCalendarGrid events={events()} selectedDate={selectedDate()} onSelect={setSelectedDate} />

      <section class="verevon-inbox-aside-card">
        <div class="verevon-inbox-card-heading verevon-inbox-card-heading--between">
          <h3>{selectedDate().toLocaleDateString(localeDateTime(i18n.locale()), { weekday: 'short', month: 'short', day: 'numeric' })}</h3>
        </div>
        <div class="verevon-inbox-calendar-day-list">
          <Show when={selectedEvents().length || selectedNotes().length} fallback={<p>{i18n.tr('Ingen hendelser denne dagen.', 'No events for this day.')}</p>}>
            <For each={selectedEvents()}>{(event) => <CalendarEventRow event={event} />}</For>
            <For each={selectedNotes()}>{(note) => <CalendarNoteRow note={note} />}</For>
          </Show>
        </div>
      </section>

      <section class="verevon-inbox-aside-card verevon-inbox-aside-card--muted">
        <h3>{i18n.tr('Planlegg oppfølging', 'Schedule follow-up')}</h3>
        <p>{i18n.tr('Opprett en personlig påminnelse. Den endrer ikke teamets sak.', 'Create a personal reminder. It does not change the team ticket.')}</p>
        <div class="verevon-inbox-inline-entry">
          <Plus class="size-4" />
          <input
            value={eventTitle()}
            onInput={(event) => setEventTitle(event.currentTarget.value)}
            onKeyDown={(event) => { if (event.key === 'Enter') void saveFollowUp() }}
            placeholder={props.selectedTicket ? i18n.tr(`Oppfølging av sak #${props.selectedTicket.number}`, `Follow up on ticket #${props.selectedTicket.number}`) : i18n.tr('Tittel på oppfølging', 'Follow-up title')}
            aria-label={i18n.tr('Tittel på oppfølging', 'Follow-up title')}
          />
          <button type="button" disabled={saving() === 'event'} onClick={() => void saveFollowUp()}>{saving() === 'event' ? i18n.tr('Lagrer …', 'Saving…') : i18n.tr('Legg til', 'Add')}</button>
        </div>
      </section>

      <section class="verevon-inbox-aside-card">
        <h3>{i18n.tr('Kalendernotat', 'Calendar note')}</h3>
        <textarea
          rows={3}
          value={noteText()}
          onInput={(event) => setNoteText(event.currentTarget.value)}
          placeholder={i18n.tr('Legg til et privat oppfølgingsnotat …', 'Add a private follow-up note...')}
          aria-label={i18n.tr('Privat oppfølgingsnotat', 'Private follow-up note')}
        />
        <button type="button" disabled={!noteText().trim() || saving() === 'note'} onClick={() => void saveNote()} class="verevon-inbox-button verevon-inbox-button--primary verevon-inbox-button--xs">
          {saving() === 'note' ? i18n.tr('Lagrer …', 'Saving…') : i18n.tr('Lagre notat', 'Save note')}
        </button>
      </section>

      <Show when={calendar.error || error()}>
        <p role="alert" class="verevon-inbox-error">{error() ?? i18n.tr('Kalenderen kunne ikke lastes. Ingen endring ble lagret.', 'The calendar could not be loaded. No change was saved.')}</p>
      </Show>

      <section class="verevon-inbox-aside-card">
        <h3>{i18n.tr('Kommende', 'Upcoming')}</h3>
        <Show when={upcomingEvents().length} fallback={<p>{i18n.tr('Ingen kommende kalenderhendelser.', 'No upcoming calendar events.')}</p>}>
          <For each={upcomingEvents()}>{(event) => <CalendarEventRow event={event} />}</For>
        </Show>
      </section>
    </div>
  )
}

type DraftLeaseActivity =
  | { state: 'active'; ownerUserId: string; expiresAt: string }
  | { state: 'none' }
  | { state: 'unavailable' }

type ConversationFollowActivity =
  | { state: 'following' }
  | { state: 'not-following' }
  | { state: 'unavailable' }

type CSATPreferenceActivity =
  | { state: 'opted-in' }
  | { state: 'not-opted-in' }
  | { state: 'unavailable' }

type TicketCSATOutcomeActivity =
  | { state: 'recorded'; score: number; recordedAt?: string }
  | { state: 'not-recorded' }
  | { state: 'unavailable' }

function ActivityPanel(props: { orgId: string; selectedTicket: ZammadTicket | null; userId: string }) {
  const i18n = useI18n()
  const conversationId = createMemo(() => (props.selectedTicket as (ZammadTicket & { conversationId?: string }) | null)?.conversationId ?? '')
  const supportTicketId = createMemo(() => props.selectedTicket?.supportTicket?.id ?? '')
  const [draftLease, { refetch: refetchDraftLease }] = createResource(
    () => ({ orgId: props.orgId, conversationId: conversationId() }),
    async ({ orgId, conversationId }): Promise<DraftLeaseActivity> => {
      if (!orgId || !conversationId) return { state: 'unavailable' }
      try {
        const lease = await getDraftLease(orgId, conversationId)
        return { state: 'active', ownerUserId: lease.user_id, expiresAt: lease.expires_at }
      } catch (reason) {
        if (reason instanceof ApiError && reason.status === 404) return { state: 'none' }
        return { state: 'unavailable' }
      }
    },
  )
  const activeDraftLease = createMemo(() => {
    const lease = draftLease()
    return lease?.state === 'active' ? lease : null
  })
  const [conversationFollow, { refetch: refetchConversationFollow }] = createResource(
    () => ({ orgId: props.orgId, conversationId: conversationId() }),
    async ({ orgId, conversationId }): Promise<ConversationFollowActivity> => {
      if (!orgId || !conversationId || !props.userId) return { state: 'unavailable' }
      try {
        await getConversationFollow(orgId, conversationId)
        return { state: 'following' }
      } catch (reason) {
        if (reason instanceof ApiError && reason.status === 404) return { state: 'not-following' }
        return { state: 'unavailable' }
      }
    },
  )
  const [savingFollow, setSavingFollow] = createSignal(false)
  const [followError, setFollowError] = createSignal<string | null>(null)
  const toggleConversationFollow = async () => {
    const orgId = props.orgId
    const selectedConversationId = conversationId()
    const current = conversationFollow()
    if (!orgId || !selectedConversationId || !props.userId || !current || current.state === 'unavailable') return
    setSavingFollow(true)
    setFollowError(null)
    try {
      await executeAction('inbox.follow_conversation', {
        type: 'human', orgId, userId: props.userId,
      }, {
        conversationId: selectedConversationId,
        following: current.state !== 'following',
      })
      await refetchConversationFollow()
    } catch {
      setFollowError(i18n.tr('Følgepreferansen kunne ikke oppdateres. Ingen lokal endring ble antatt.', 'The follow preference could not be updated. No local change was assumed.'))
    } finally {
      setSavingFollow(false)
    }
  }
  const [csatPreference, { refetch: refetchCSATPreference }] = createResource(
    () => ({ orgId: props.orgId, conversationId: conversationId() }),
    async ({ orgId, conversationId }): Promise<CSATPreferenceActivity> => {
      if (!orgId || !conversationId) return { state: 'unavailable' }
      try { return (await getConversationCSATPreference(orgId, conversationId)).opted_in ? { state: 'opted-in' } : { state: 'not-opted-in' } }
      catch { return { state: 'unavailable' } }
    },
  )
  const [savingCSATPreference, setSavingCSATPreference] = createSignal(false)
  const [csatPreferenceError, setCSATPreferenceError] = createSignal<string | null>(null)
  const toggleCSATPreference = async () => {
    const current = csatPreference(); const orgId = props.orgId; const selectedConversationId = conversationId()
    if (!current || current.state === 'unavailable' || !orgId || !selectedConversationId || !props.userId) return
    setSavingCSATPreference(true); setCSATPreferenceError(null)
    try {
      await executeAction('inbox.set_csat_preference', { type: 'human', orgId, userId: props.userId }, { conversationId: selectedConversationId, optedIn: current.state !== 'opted-in' })
      await refetchCSATPreference()
    } catch { setCSATPreferenceError(i18n.tr('Tilbakemeldingspreferansen kunne ikke oppdateres. Ingen lokal endring ble antatt.', 'The feedback preference could not be updated. No local change was assumed.')) }
    finally { setSavingCSATPreference(false) }
  }
  const [csatOutcome, { refetch: refetchCSATOutcome }] = createResource(
    () => ({ orgId: props.orgId, ticketId: supportTicketId() }),
    async ({ orgId, ticketId }): Promise<TicketCSATOutcomeActivity> => {
      if (!orgId || !ticketId) return { state: 'unavailable' }
      try {
        const outcome = await getTicketCSATOutcome(orgId, ticketId)
        return { state: 'recorded', score: outcome.score, recordedAt: outcome.recorded_at }
      } catch (reason) {
        if (reason instanceof ApiError && reason.status === 404) return { state: 'not-recorded' }
        return { state: 'unavailable' }
      }
    },
  )
  const [csatScorecard] = createResource(
    () => props.orgId,
    async (orgId) => orgId ? getCSATScorecard(orgId) : null,
  )
  const [selectedCSATScore, setSelectedCSATScore] = createSignal(0)
  const [savingCSATOutcome, setSavingCSATOutcome] = createSignal(false)
  const [csatOutcomeError, setCSATOutcomeError] = createSignal<string | null>(null)
  const recordedCSATOutcome = createMemo(() => {
    const outcome = csatOutcome()
    return outcome?.state === 'recorded' ? outcome : null
  })
  const recordedCSATScore = createMemo(() => recordedCSATOutcome()?.score ?? null)
  const isTerminalSupportTicket = createMemo(() => {
    const status = props.selectedTicket?.supportTicket?.status
    return status === 'resolved' || status === 'closed'
  })
  const recordCSATOutcome = async () => {
    const orgId = props.orgId
    const ticketId = supportTicketId()
    const score = selectedCSATScore()
    if (!orgId || !ticketId || !props.userId || !isTerminalSupportTicket() || csatPreference()?.state !== 'opted-in' || score < 1 || score > 5) return
    setSavingCSATOutcome(true)
    setCSATOutcomeError(null)
    try {
      await executeAction('tickets.record_csat_outcome', { type: 'human', orgId, userId: props.userId }, { ticketId, score })
      await refetchCSATOutcome()
    } catch {
      setCSATOutcomeError(i18n.tr('Kundetilbakemeldingen kunne ikke lagres. Ingen lokal endring ble antatt.', 'The customer outcome could not be recorded. No local change was assumed.'))
    } finally {
      setSavingCSATOutcome(false)
    }
  }
  return (
    <Show
      when={props.selectedTicket}
      fallback={
        <EmptyAsideState
          icon={<Clock3 class="size-6" />}
          title={i18n.tr('Ingen aktivitet valgt', 'No activity selected')}
          body={i18n.tr(
            'Åpne en sak for å se arbeidsflythelse, samarbeidsstatus og oppfølgingsautomatisering.',
            'Open a ticket to see workflow health, collaboration state, and follow-up automation.',
          )}
        />
      }
    >
      {(ticket) => (
        <div class="verevon-inbox-aside-scroll verevon-inbox-activity-panel">
          <section class="verevon-inbox-aside-card">
            <div class="verevon-inbox-card-heading">
              <CheckCircle2 class="size-4 verevon-inbox-success" />
              <h2>{i18n.tr('Arbeidsflythelse', 'Workflow health')}</h2>
            </div>
            <div class="verevon-inbox-field-stack">
              <HealthRow
                label={i18n.tr('Eierskap', 'Ownership')}
                value={ticket().owner ? `${ticket().owner?.firstname} ${ticket().owner?.lastname}` : i18n.tr('Trenger eier', 'Needs owner')}
                tone={ticket().owner ? 'neutral' : 'warning'}
              />
              <HealthRow label={i18n.tr('Kø', 'Queue')} value={ticket().group?.name ?? 'Support'} tone="neutral" />
              <Show when={ticket().supportTicket?.sla_state}>
                {(slaState) => (
                  <HealthRow
                    label={i18n.tr('SLA-status', 'SLA status')}
                    value={titleCase(slaState() ?? '')}
                    tone={slaState() === 'ok' ? 'success' : 'warning'}
                  />
                )}
              </Show>
              <Show when={ticket().supportTicket?.due_at}>
                {(dueAt) => (
                  <HealthRow
                    label={i18n.tr('SLA-frist', 'SLA deadline')}
                    value={formatRelativeTime(dueAt() ?? '')}
                    tone={ticket().supportTicket?.sla_state === 'ok' ? 'success' : 'warning'}
                  />
                )}
              </Show>
            </div>
          </section>

          <section class="verevon-inbox-aside-card">
            <div class="verevon-inbox-card-heading verevon-inbox-card-heading--between">
              <div class="verevon-inbox-card-heading">
              <UserRound class="size-4" />
              <h2>{i18n.tr('Teamsamarbeid', 'Team collaboration')}</h2>
              </div>
              <button type="button" disabled={draftLease.loading} onClick={() => void refetchDraftLease()}>
                {draftLease.loading ? i18n.tr('Sjekker …', 'Checking…') : i18n.tr('Oppdater', 'Refresh')}
              </button>
            </div>
            <div class="verevon-inbox-activity-stack">
              <Show
                when={!draftLease.loading}
                fallback={<ActivityItem title={i18n.tr('Sjekker utkastbeskyttelse', 'Checking draft protection')} body={i18n.tr('Leser den kanoniske utkastleien for denne samtalen.', 'Reading the canonical draft lease for this conversation.')} />}
              >
                <Show
                  when={activeDraftLease()}
                  fallback={
                    <Show
                      when={draftLease()?.state === 'none'}
                      fallback={<ActivityItem title={i18n.tr('Utkaststatus ikke tilgjengelig', 'Draft state unavailable')} body={i18n.tr('Verevon kan ikke bekrefte samarbeidstilstanden akkurat nå. Ingen slutning om hvem som skriver blir vist.', 'Verevon cannot verify collaboration state right now. It does not infer who is drafting.')} />}
                    >
                      <ActivityItem title={i18n.tr('Ingen aktiv utkastleie', 'No active draft lease')} body={i18n.tr('Bekreftet fra samtaletjenesten. Dette kan endres når en operatør begynner å skrive.', 'Confirmed by conversation-core. This can change when an operator starts drafting.')} />
                    </Show>
                  }
                >
                  {(lease) => (
                    <ActivityItem
                      title={lease().ownerUserId === props.userId ? i18n.tr('Du skriver et utkast', 'You are drafting') : i18n.tr('En operatør skriver et utkast', 'An operator is drafting')}
                      body={i18n.tr(`Utkastbeskyttelsen er aktiv til ${formatRelativeTime(lease().expiresAt)}. Operatøridentitet deles ikke her.`, `Draft protection is active until ${formatRelativeTime(lease().expiresAt)}. Operator identity is not shown here.`)}
                    />
                  )}
                </Show>
              </Show>
              <ActivityItem
                title={i18n.tr('Interne kommentarer', 'Internal comments')}
                body={i18n.tr('Bruk internt notat i svarfeltet. Sidekommentarer er ikke tilgjengelige ennå.', 'Use an internal note in the composer. Side comments are not available yet.')}
              />
              <div class="verevon-inbox-activity-item verevon-inbox-activity-item--action">
                <span />
                <div>
                  <p>{i18n.tr('Følg samtale', 'Follow conversation')}</p>
                  <small>
                    {conversationFollow.loading || savingFollow()
                      ? i18n.tr('Oppdaterer preferansen …', 'Updating your preference…')
                      : conversationFollow()?.state === 'following'
                        ? i18n.tr('Du følger denne samtalen. Varslingslevering blir aktiv når varslingsintegrasjonen er koblet til.', 'You follow this conversation. Delivery activates when the notification integration is connected.')
                        : conversationFollow()?.state === 'not-following'
                          ? i18n.tr('Lagre en personlig følgepreferanse uten å dele kundedata.', 'Save a personal follow preference without sharing customer data.')
                          : i18n.tr('Følgepreferansen kan ikke bekreftes akkurat nå.', 'The follow preference cannot be confirmed right now.')}
                  </small>
                  <button
                    type="button"
                    class="verevon-inbox-button verevon-inbox-button--secondary verevon-inbox-button--xs"
                    disabled={!conversationFollow() || conversationFollow.loading || savingFollow() || conversationFollow()?.state === 'unavailable'}
                    onClick={() => void toggleConversationFollow()}
                  >
                    {savingFollow()
                      ? i18n.tr('Oppdaterer …', 'Updating…')
                      : conversationFollow()?.state === 'following'
                        ? i18n.tr('Slutt å følge', 'Unfollow')
                        : i18n.tr('Følg samtale', 'Follow conversation')}
                  </button>
                  <Show when={followError()}>
                    {(message) => <small role="alert" class="verevon-inbox-error">{message()}</small>}
                  </Show>
                </div>
              </div>
              <div class="verevon-inbox-activity-item verevon-inbox-activity-item--action">
                <span />
                <div>
                  <p>{i18n.tr('Be om tilbakemelding', 'Request feedback')}</p>
                  <small>{csatPreference.loading || savingCSATPreference() ? i18n.tr('Oppdaterer preferansen …', 'Updating preference…') : csatPreference()?.state === 'opted-in' ? i18n.tr('En operatør har registrert kundens samtykke til en fremtidig tilfredshetsundersøkelse etter løsning.', 'An operator recorded the customer’s consent to a future satisfaction survey after resolution.') : csatPreference()?.state === 'not-opted-in' ? i18n.tr('Ingen spørreundersøkelse blir sendt uten kundens uttrykkelige samtykke.', 'No survey will be sent without the customer’s explicit preference.') : i18n.tr('Tilbakemeldingspreferansen kan ikke bekreftes akkurat nå.', 'The feedback preference cannot be confirmed right now.')}</small>
                  <button type="button" class="verevon-inbox-button verevon-inbox-button--secondary verevon-inbox-button--xs" disabled={!csatPreference() || csatPreference.loading || savingCSATPreference() || csatPreference()?.state === 'unavailable'} onClick={() => void toggleCSATPreference()}>
                    {savingCSATPreference() ? i18n.tr('Oppdaterer …', 'Updating…') : csatPreference()?.state === 'opted-in' ? i18n.tr('Trekk tilbake samtykke', 'Withdraw consent') : i18n.tr('Registrer samtykke', 'Record consent')}
                  </button>
                  <Show when={csatPreferenceError()}>{(message) => <small role="alert" class="verevon-inbox-error">{message()}</small>}</Show>
                </div>
              </div>
              <div class="verevon-inbox-activity-item verevon-inbox-activity-item--action">
                <span />
                <div>
                  <p>{i18n.tr('Kundetilfredshet', 'Customer satisfaction')}</p>
                  <small>
                    {csatOutcome.loading || savingCSATOutcome()
                      ? i18n.tr('Leser eller lagrer kundeutfallet …', 'Reading or recording the customer outcome…')
                      : recordedCSATScore() !== null
                        ? i18n.tr(`Kunden ga ${recordedCSATScore()}/5. Dette er et registrert utfall, ikke en påstått utsendt undersøkelse.`, `Customer rating recorded: ${recordedCSATScore()}/5. This is a recorded outcome, not a claimed survey send.`)
                        : csatOutcome()?.state === 'not-recorded'
                          ? i18n.tr('Ingen kundevurdering er registrert ennå.', 'No customer rating has been recorded yet.')
                          : i18n.tr('Kundeutfallet kan ikke bekreftes akkurat nå.', 'The customer outcome cannot be verified right now.')}
                  </small>
                  <Show when={csatScorecard() && !csatScorecard.loading}>
                    <small>{i18n.tr(`${csatScorecard()?.rated_tickets ?? 0} registrerte vurderinger i arbeidsområdet`, `${csatScorecard()?.rated_tickets ?? 0} recorded ratings in this workspace`)}</small>
                  </Show>
                  <Show when={isTerminalSupportTicket() && csatPreference()?.state === 'opted-in' && csatOutcome()?.state !== 'unavailable'}>
                    <div class="verevon-inbox-csat-score-picker" role="group" aria-label={i18n.tr('Registrer kundevurdering', 'Record customer rating')}>
                      <For each={[1, 2, 3, 4, 5]}>
                        {(score) => <button type="button" class={cn('verevon-inbox-button verevon-inbox-button--secondary verevon-inbox-button--xs', selectedCSATScore() === score && 'is-active')} aria-pressed={selectedCSATScore() === score} onClick={() => setSelectedCSATScore(score)}>{score}</button>}
                      </For>
                    </div>
                    <button type="button" class="verevon-inbox-button verevon-inbox-button--secondary verevon-inbox-button--xs" disabled={selectedCSATScore() === 0 || savingCSATOutcome()} onClick={() => void recordCSATOutcome()}>
                      {savingCSATOutcome() ? i18n.tr('Lagrer …', 'Recording…') : i18n.tr('Registrer vurdering', 'Record rating')}
                    </button>
                  </Show>
                  <Show when={csatOutcomeError()}>{(message) => <small role="alert" class="verevon-inbox-error">{message()}</small>}</Show>
                </div>
              </div>
            </div>
          </section>

          <section class="verevon-inbox-aside-card verevon-inbox-aside-card--soft">
            <div class="verevon-inbox-card-heading">
              <AlertCircle class="size-4" />
              <h2>{i18n.tr('Automatiseringsregler', 'Automation hooks')}</h2>
            </div>
            <p class="verevon-inbox-aside-muted">
              {i18n.tr(
                'Ingen automatiseringsregel eller SLA-signal er verifisert for denne samtalen ennå.',
                'No automation rule or SLA signal is verified for this conversation yet.',
              )}
            </p>
          </section>
        </div>
      )}
    </Show>
  )
}

function MacrosPanel(props: {
  orgId: string
  onMacroExecuted: (ticket: SupportTicket) => void
  selectedTicket: ZammadTicket | null
  userId: string
}) {
  const i18n = useI18n()
  const [expanded, setExpanded] = createSignal(false)
  const [runningMacroId, setRunningMacroId] = createSignal<string | null>(null)
  const [error, setError] = createSignal<string | null>(null)
  const [previewMacro, setPreviewMacro] = createSignal<TicketMacro | null>(null)
  const [macrosRes] = createResource(
    () => (expanded() && props.orgId ? props.orgId : ''),
    (id) => (id ? listTicketMacros(id).catch(() => [] as TicketMacro[]) : Promise.resolve([] as TicketMacro[])),
  )
  const macros = () => (macrosRes() ?? []).filter((macro) => macro.active)

  const applyMacro = async (macro: TicketMacro) => {
    const ticket = props.selectedTicket?.supportTicket
    if (!ticket || !props.orgId || !props.userId || runningMacroId()) return
    setRunningMacroId(macro.id)
    setError(null)
    try {
      const updated = await executeTicketMacro(
        { type: 'human', orgId: props.orgId, userId: props.userId },
        ticket,
        macro.id,
        macro.updated_at,
      )
      props.onMacroExecuted(updated)
    } catch {
      setError(i18n.tr('Makroen kunne ikke brukes. Ingen sak ble endret.', 'The macro could not be applied. No ticket was changed.'))
    } finally {
      setRunningMacroId(null)
    }
  }

  return (
    <section class="verevon-inbox-macros">
      <button type="button" onClick={() => setExpanded((value) => !value)} aria-expanded={expanded()}>
        <span>{i18n.tr('Makroer', 'Macros')}</span>
        <ChevronDown class={cn('size-4', expanded() && 'rotate-180')} />
      </button>
      <Show when={expanded()}>
        <Show when={error()}>{(message) => <p class="verevon-inbox-macros__empty" role="alert">{message()}</p>}</Show>
        <Show when={!macrosRes.loading} fallback={<p class="verevon-inbox-macros__empty">{i18n.tr('Laster makroer …', 'Loading macros…')}</p>}>
          <ul>
            <For each={macros()} fallback={<li class="verevon-inbox-macros__empty">{i18n.tr('Ingen makroer konfigurert. Opprett dem under Innstillinger → Makroer.', 'No macros configured. Create them in Settings → Macros.')}</li>}>
              {(macro) => (
                <li>
                  <span title={macro.description}>{macro.name}</span>
                  <button type="button" disabled={!props.selectedTicket?.supportTicket || Boolean(runningMacroId())} onClick={() => setPreviewMacro(macro)}>
                    <Play class="size-3" />
                    {runningMacroId() === macro.id ? i18n.tr('Bruker …', 'Applying…') : i18n.tr('Bruk', 'Apply')}
                  </button>
                </li>
              )}
            </For>
          </ul>
        </Show>
      </Show>
      <Show when={previewMacro()}>
        {(macro) => (
          <div class="verevon-ticketing-macro-preview" role="dialog" aria-modal="true" aria-label={i18n.tr('Bekreft makro', 'Confirm macro')}>
            <div class="verevon-ticketing-macro-preview__card">
              <h2>{i18n.tr('Gjennomgå makro før kjøring', 'Review macro before running')}</h2>
              <p>{i18n.tr(`Makroen "${macro().name}" endrer den lenkede support-saken.`, `The "${macro().name}" macro changes the linked support ticket.`)}</p>
              <h3>{i18n.tr('Handlinger', 'Actions')}</h3>
              <pre>{JSON.stringify(macro().actions, null, 2)}</pre>
              <Show when={Object.keys(macro().conditions).length > 0}>
                <h3>{i18n.tr('Betingelser', 'Conditions')}</h3>
                <pre>{JSON.stringify(macro().conditions, null, 2)}</pre>
              </Show>
              <small>{i18n.tr('Kjøring avvises dersom makroen endres etter denne gjennomgangen.', 'Execution is rejected if this macro changes after this review.')}</small>
              <div><button type="button" onClick={() => setPreviewMacro(null)}>{i18n.tr('Avbryt', 'Cancel')}</button><button type="button" onClick={() => { const selected = macro(); setPreviewMacro(null); void applyMacro(selected) }}>{i18n.tr('Kjør makro', 'Run macro')}</button></div>
            </div>
          </div>
        )}
      </Show>
    </section>
  )
}

function ActionSuggestion(props: { actionLabel?: string; body: string; onRun?: () => void; title: string }) {
  return (
    <div class="verevon-inbox-action-suggestion">
      <div>
        <strong>{props.title}</strong>
        <p>{props.body}</p>
      </div>
      <Show when={props.actionLabel}>
        <button type="button" onClick={() => props.onRun?.()}>{props.actionLabel}</button>
      </Show>
    </div>
  )
}
