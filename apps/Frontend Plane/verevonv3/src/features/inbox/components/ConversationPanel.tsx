import {
  CheckCheck,
  ChevronDown,
  Clock3,
  Link2,
  Mail,
  Megaphone,
  MessageCircle,
	MoreHorizontal,
	Paperclip,
  PenLine,
  Plus,
  Sparkles,
  Star,
  Tag,
  TicketCheck,
  X,
  type LucideProps,
} from '@/shared/icons'
import { createEffect, createMemo, createSignal, For, onCleanup, Show, untrack, type Component } from 'solid-js'
import type { JSX } from '@solidjs/web'
import { Dynamic } from '@solidjs/web'
import { AiActionReviewPanel } from '@/features/inbox/components/AiActionReviewPanel'
import { ConversationActivityTimeline } from '@/features/inbox/components/ConversationActivityTimeline'
import { EmailBody } from '@/features/inbox/components/EmailBody'
import { OutboundDeliveryLedger } from '@/features/inbox/components/OutboundDeliveryLedger'
import { SentimentBadge } from '@/features/inbox/components/SentimentBadge'
import type { InboxModalRequest } from '@/features/inbox/components/InboxWorkModal'
import {
  customerInitials,
  customerName,
  formatDate,
  formatRelativeTime,
  formatTimestamp,
  titleCase,
  type Agent,
  type Group,
  type TicketSentiment,
  type ZammadArticle,
  type ZammadTicket,
} from '@/features/inbox/lib/inbox-model'
import { cn } from '@/shared/lib/cn'
import {
  claimDraftLease,
  deleteConversationDraft,
  getConversationDraft,
  getDraftLease,
  releaseDraftLease,
  saveConversationDraft,
} from '@/shared/api/inbox-client'
import { ApiError } from '@/shared/api/http'
import type { TicketTeam } from '@/shared/api/tickets-client'
import { useI18n } from '@/shared/i18n'
import { handleTabKeyDown } from '@/shared/ui/tab-keyboard'

type ConversationCenterTab = 'conversation' | 'ticket' | 'activity'

export function ConversationPanel(props: {
  agents: Agent[]
  articles: ZammadArticle[]
  articlesLoading: boolean
  aiProposalRefreshKey: number
  deliveryRefreshKey: number
  groups: Group[]
  isPinned: boolean
  notice: string | null
  orgId: string
  userId: string
  onAddTag: (tag: string) => void
  onCreateTicket: () => void
  onLinkExistingTicket: () => void
  onOpenModal: (modal: InboxModalRequest) => void
  onPatchTicket: (patch: Record<string, unknown>) => void
  onTicketActionVerified: () => void
  onTogglePinned: () => void
  onRemoveTag: (tag: string) => void
  onCreateSocialFollowUp: () => void
  onResolveTicket: () => void
  onSendReply: (text: string, internal: boolean) => Promise<boolean>
  onSuggestReply: () => void
  onViewTicket: () => void
  isInternal: boolean
  replyText: string
  replySending: boolean
  selectedTicket: ZammadTicket | null
  sentiment: TicketSentiment | null
  setReplyText: (value: string) => void
  setIsInternal: (isInternal: boolean) => void
  ticketTeams: readonly TicketTeam[]
}) {
  const i18n = useI18n()
  const [activeCenterTab, setActiveCenterTab] = createSignal<ConversationCenterTab>('conversation')
  let scrollRef: HTMLDivElement | undefined

  createEffect(
    () => props.selectedTicket?.conversationId,
    () => { setActiveCenterTab('conversation') },
  )

  createEffect(
    () => Boolean(props.selectedTicket && !props.articlesLoading && props.articles.length >= 0),
    (shouldScroll) => {
      if (!shouldScroll) return
      window.requestAnimationFrame(() => {
        scrollRef?.scrollTo?.({ top: scrollRef.scrollHeight, behavior: 'smooth' })
      })
    },
  )

  return (
    <Show when={props.selectedTicket?.conversationId} keyed fallback={<ConversationEmptyState />}>
      {(_conversationId) => {
        void _conversationId
        const ticket = () => props.selectedTicket!
        const contactReason = () => props.sentiment?.sentiment ? titleCase(props.sentiment.sentiment) : ticket().priority?.name ?? i18n.tr('Henvendelse', 'Support request')

        return (
          <main class="verevon-inbox-conversation">
            <ConversationHeader
              agents={props.agents}
              contactReason={contactReason()}
              groups={props.groups}
              isPinned={props.isPinned}
              onAddTag={props.onAddTag}
              onCreateTicket={props.onCreateTicket}
              onLinkExistingTicket={props.onLinkExistingTicket}
              onOpenModal={props.onOpenModal}
              onPatchTicket={props.onPatchTicket}
              onRemoveTag={props.onRemoveTag}
              onResolveTicket={props.onResolveTicket}
              onShowActivity={() => setActiveCenterTab('activity')}
              onTogglePinned={props.onTogglePinned}
              onViewTicket={props.onViewTicket}
              selectedTicket={ticket()}
              sentiment={props.sentiment}
            />
            <div class="verevon-inbox-center-tabs" role="tablist" aria-label={i18n.tr('Samtalearbeidsområde', 'Conversation workspace')}>
              <CenterTab active={activeCenterTab() === 'conversation'} id="conversation" label={i18n.tr('Samtale', 'Conversation')} onSelect={setActiveCenterTab} />
              <CenterTab active={activeCenterTab() === 'ticket'} id="ticket" label={i18n.tr('Sak', 'Ticket')} onSelect={setActiveCenterTab} />
              <CenterTab active={activeCenterTab() === 'activity'} id="activity" label={i18n.tr('Aktivitet', 'Activity')} onSelect={setActiveCenterTab} />
            </div>
            <Show when={activeCenterTab() === 'conversation'}>
              <div class="verevon-inbox-center-panel verevon-inbox-center-panel--conversation" id="conversation-center-panel-conversation" role="tabpanel" aria-labelledby="conversation-center-tab-conversation">
                <ConversationTranscript
                  articles={props.articles}
                  articlesLoading={props.articlesLoading}
                  scrollRef={(node) => { scrollRef = node }}
                  selectedTicket={ticket()}
                />
                <ConversationReplyComposer
                  isInternal={props.isInternal}
                  notice={props.notice}
                  conversationId={ticket().conversationId ?? ''}
                  orgId={props.orgId}
                  userId={props.userId}
                  onOpenModal={props.onOpenModal}
                  onCreateSocialFollowUp={props.onCreateSocialFollowUp}
                  onSendReply={props.onSendReply}
                  onSuggestReply={props.onSuggestReply}
                  replySending={props.replySending}
                  replyText={props.replyText}
                  selectedTicket={ticket()}
                  setIsInternal={props.setIsInternal}
                  setReplyText={props.setReplyText}
                />
              </div>
            </Show>
            <Show when={activeCenterTab() === 'ticket'}>
              <div class="verevon-inbox-center-panel verevon-inbox-center-panel--scroll" id="conversation-center-panel-ticket" role="tabpanel" aria-labelledby="conversation-center-tab-ticket">
                <ConversationTicketPanel
                  onCreateTicket={props.onCreateTicket}
                  onLinkExistingTicket={props.onLinkExistingTicket}
                  onViewTicket={props.onViewTicket}
                  selectedTicket={ticket()}
                />
                <AiActionReviewPanel
                  conversationId={ticket().conversationId}
                  onTicketActionVerified={props.onTicketActionVerified}
                  refreshKey={props.aiProposalRefreshKey}
                  ticketTeams={props.ticketTeams}
                />
              </div>
            </Show>
            <Show when={activeCenterTab() === 'activity'}>
              <div class="verevon-inbox-center-panel verevon-inbox-center-panel--scroll" id="conversation-center-panel-activity" role="tabpanel" aria-labelledby="conversation-center-tab-activity">
                <div class="verevon-inbox-center-section-heading">
                  <Clock3 class="size-4" />
                  <div>
                    <h2>{i18n.tr('Levering og kanalaktivitet', 'Delivery and channel activity')}</h2>
                    <p>{i18n.tr('Autoritative leverandørkvitteringer og ukjente utfall vises her.', 'Authoritative provider receipts and unknown outcomes appear here.')}</p>
                  </div>
                </div>
                <ConversationActivityTimeline
                  conversationId={(ticket() as { conversationId?: string }).conversationId ?? ''}
                  orgId={props.orgId}
                  refreshKey={props.deliveryRefreshKey}
                />
                <OutboundDeliveryLedger
                  conversationId={(ticket() as { conversationId?: string }).conversationId ?? ''}
                  orgId={props.orgId}
                  refreshKey={props.deliveryRefreshKey}
                />
              </div>
            </Show>
          </main>
        )
      }}
    </Show>
  )
}

function CenterTab(props: {
  active: boolean
  id: ConversationCenterTab
  label: string
  onSelect: (tab: ConversationCenterTab) => void
}) {
  return (
    <button
      type="button"
      role="tab"
      id={`conversation-center-tab-${props.id}`}
      aria-controls={`conversation-center-panel-${props.id}`}
      aria-selected={props.active ? 'true' : 'false'}
      tabindex={props.active ? 0 : -1}
      onKeyDown={handleTabKeyDown}
      onClick={() => props.onSelect(props.id)}
    >
      {props.label}
    </button>
  )
}

function ConversationTicketPanel(props: {
  onCreateTicket: () => void
  onLinkExistingTicket: () => void
  onViewTicket: () => void
  selectedTicket: ZammadTicket
}) {
  const i18n = useI18n()
  const supportTicket = () => props.selectedTicket.supportTicket

  return (
    <section class="verevon-inbox-center-ticket-card">
      <div class="verevon-inbox-center-section-heading verevon-inbox-center-section-heading--between">
        <div>
          <TicketCheck class="size-4" />
          <div>
            <h2>{supportTicket()?.ticket_key ?? i18n.tr('Ingen koblet sak', 'No linked ticket')}</h2>
            <p>{i18n.tr('Hold varig arbeidsstatus adskilt fra selve samtalen.', 'Keep durable work state separate from the conversation itself.')}</p>
          </div>
        </div>
        <Show when={supportTicket()}>
          <button type="button" onClick={() => props.onViewTicket()}>{i18n.tr('Åpne sak', 'Open ticket')}</button>
        </Show>
      </div>
      <Show
        when={supportTicket()}
        fallback={(
          <div class="verevon-inbox-center-ticket-empty">
            <p>{i18n.tr('Opprett en ny sak eller koble samtalen til eksisterende arbeid.', 'Create a new ticket or connect this conversation to existing work.')}</p>
            <div>
              <button type="button" onClick={props.onCreateTicket}>{i18n.tr('Opprett sak', 'Create ticket')}</button>
              <button type="button" onClick={props.onLinkExistingTicket}>{i18n.tr('Koble eksisterende', 'Link existing')}</button>
            </div>
          </div>
        )}
      >
        {(linked) => (
          <dl class="verevon-inbox-center-ticket-fields">
            <div><dt>{i18n.tr('Status', 'Status')}</dt><dd>{titleCase(linked().status)}</dd></div>
            <div><dt>{i18n.tr('Prioritet', 'Priority')}</dt><dd>{titleCase(props.selectedTicket.priority?.name ?? i18n.tr('Normal', 'Normal'))}</dd></div>
            <div><dt>{i18n.tr('Frist', 'Due')}</dt><dd>{linked().due_at ? formatDate(linked().due_at!) : i18n.tr('Ikke satt', 'Not set')}</dd></div>
            <div><dt>SLA</dt><dd>{titleCase(linked().sla_state ?? i18n.tr('Ikke satt', 'Not set'))}</dd></div>
          </dl>
        )}
      </Show>
    </section>
  )
}

function ConversationEmptyState() {
  const i18n = useI18n()
  return (
    <main class="verevon-inbox-conversation-empty">
      <div>
        <div class="verevon-inbox-conversation-empty__icon">
          <MessageCircle class="size-8" strokeWidth={1.45} />
        </div>
        <p>{i18n.tr('Velg en sak for å se samtalen', 'Select a ticket to view the conversation')}</p>
        <small>{i18n.tr('Kundedetaljer, samtalehistorikk og svarverktøyet vises her.', 'Customer details, conversation history, and the reply composer will appear here.')}</small>
      </div>
    </main>
  )
}

function ConversationHeader(props: {
  agents: Agent[]
  contactReason: string
  groups: Group[]
  isPinned: boolean
  onAddTag: (tag: string) => void
  onCreateTicket: () => void
  onLinkExistingTicket: () => void
  onOpenModal: (modal: InboxModalRequest) => void
  onPatchTicket: (patch: Record<string, unknown>) => void
  onRemoveTag: (tag: string) => void
  onResolveTicket: () => void
  onShowActivity: () => void
  onTogglePinned: () => void
  onViewTicket: () => void
  selectedTicket: ZammadTicket
  sentiment: TicketSentiment | null
}) {
  const i18n = useI18n()
  const supportTicket = () => props.selectedTicket.supportTicket ?? null
  const [moreOpen, setMoreOpen] = createSignal(false)
  return (
    <div class="verevon-inbox-conversation-header">
      <div class="verevon-inbox-conversation-header__top">
        <div class="verevon-inbox-conversation-header__title">
          <div>
            <h2>{props.selectedTicket.title}</h2>
            <span>#{props.selectedTicket.number}</span>
          </div>
          <p>
            <Clock3 class="size-3.5" />
            {i18n.tr(`Oppdatert for ${formatRelativeTime(props.selectedTicket.updated_at)} siden`, `Updated ${formatRelativeTime(props.selectedTicket.updated_at)} ago`)}
            <Show when={props.sentiment}>
              {(sentiment) => <SentimentBadge sentiment={sentiment().sentiment} />}
            </Show>
          </p>
        </div>

        <div class="verevon-inbox-conversation-header__actions">
          <HeaderIconButton
            label={props.isPinned
              ? i18n.tr('Løsne samtale fra personlig visning', 'Unpin conversation from personal view')
              : i18n.tr('Fest samtale i personlig visning', 'Pin conversation to personal view')}
            onClick={props.onTogglePinned}
          >
            <Star class={cn('size-4', props.isPinned && 'fill-current')} />
          </HeaderIconButton>
            <HeaderIconButton
              label={i18n.tr('Koble til eksisterende sak', 'Link to existing ticket')}
              onClick={props.onLinkExistingTicket}
            >
              <Link2 class="size-4" />
            </HeaderIconButton>
          <div class="verevon-inbox-conversation-header__more-menu">
            <HeaderIconButton
              label={i18n.tr('Flere samtalehandlinger', 'More conversation actions')}
              onClick={() => setMoreOpen((open) => !open)}
            >
              <MoreHorizontal class="size-4" />
            </HeaderIconButton>
            <Show when={moreOpen()}>
              <div class="verevon-inbox-conversation-header__more-popover" role="menu">
                <button type="button" role="menuitem" onClick={() => { setMoreOpen(false); props.onShowActivity() }}>
                  {i18n.tr('Vis leveringsaktivitet', 'Show delivery activity')}
                </button>
              </div>
            </Show>
          </div>
          <Show
            when={supportTicket()}
            fallback={(
              <button type="button" onClick={props.onCreateTicket} class="verevon-inbox-button verevon-inbox-button--secondary verevon-inbox-button--xs">
                <Plus class="size-3.5" />
                {i18n.tr('Opprett sak', 'Create ticket')}
              </button>
            )}
          >
            {(ticket) => (
              <button type="button" onClick={props.onViewTicket} class="verevon-inbox-button verevon-inbox-button--secondary verevon-inbox-button--xs">
                <TicketCheck class="size-3.5" />
                {ticket().ticket_key}
              </button>
            )}
          </Show>
          <button
            type="button"
            onClick={() => supportTicket() ? props.onResolveTicket() : props.onPatchTicket({ state_id: 4 })}
            class="verevon-inbox-button verevon-inbox-button--primary verevon-inbox-button--xs"
          >
            <CheckCheck class="size-3.5" />
            {supportTicket() ? i18n.tr('Løs sak', 'Resolve ticket') : i18n.tr('Lukk samtale', 'Close conversation')}
          </button>
        </div>
      </div>

      <ConversationToolbar
        agents={props.agents}
        groups={props.groups}
        onAddTag={props.onAddTag}
        onPatchTicket={props.onPatchTicket}
        onRemoveTag={props.onRemoveTag}
        onResolveTicket={props.onResolveTicket}
        selectedTicket={props.selectedTicket}
      />

      <div class="verevon-inbox-conversation-header__reason">
        <div>
          <span>{i18n.tr('Kontaktårsak:', 'Contact reason:')}</span>
          <strong>{props.contactReason}</strong>
          <TicketDecisionChip ticket={props.selectedTicket} />
        </div>
        <button
          type="button"
          onClick={() => props.onOpenModal({
            type: 'work',
            title: i18n.tr('Samtaleinnsikt', 'Conversation intelligence'),
            description: i18n.tr(
              'Se hensikt, sentiment, SLA, eierskap og foreslåtte neste steg for den valgte saken i denne modalen.',
              'Review intent, sentiment, SLA, ownership, and suggested next actions for the selected ticket in this modal.',
            ),
            primaryAction: i18n.tr('Oppdater kontekst', 'Update context'),
          })}
        >
          {i18n.tr('Vis mer', 'Show more')}
        </button>
      </div>
    </div>
  )
}

function TicketDecisionChip(props: { ticket: ZammadTicket }) {
  const i18n = useI18n()
  const supportTicket = () => props.ticket.supportTicket ?? null
  const label = () => {
    const ticket = supportTicket()
    if (!ticket) return i18n.tr('Ingen sak nødvendig', 'No ticket needed')
    if (ticket.status === 'suggested') {
      const confidence = ticket.ai_confidence ? ` · ${Math.round(ticket.ai_confidence * 100)}%` : ''
      return i18n.tr(`Foreslått sak${confidence}`, `Suggested ticket${confidence}`)
    }
    if (ticket.source === 'ai') return i18n.tr('Automatisk opprettet sak', 'Auto-created ticket')
    return i18n.tr('Sak opprettet', 'Ticket created')
  }
  const tone = () => {
    const ticket = supportTicket()
    if (!ticket) return 'none'
    if (ticket.status === 'suggested') return 'suggested'
    if (ticket.source === 'ai') return 'auto'
    return 'manual'
  }
  return <span class={`verevon-inbox-ticket-decision verevon-inbox-ticket-decision--${tone()}`}>{label()}</span>
}

function ConversationToolbar(props: {
  agents: Agent[]
  groups: Group[]
  onAddTag: (tag: string) => void
  onPatchTicket: (patch: Record<string, unknown>) => void
  onRemoveTag: (tag: string) => void
  onResolveTicket: () => void
  selectedTicket: ZammadTicket
}) {
  const i18n = useI18n()
  const stateOptions = createMemo(() => [
    { id: 1, label: i18n.tr('Ny', 'New') },
    { id: 2, label: i18n.tr('Åpen', 'Open') },
    { id: 4, label: i18n.tr('Lukket', 'Closed') },
    { id: 6, label: i18n.tr('Venter påminnelse', 'Pending reminder') },
  ] as const)
  const priorityOptions = createMemo(() => [
    { id: 1, label: i18n.tr('Lav', 'Low') },
    { id: 2, label: i18n.tr('Normal', 'Normal') },
    { id: 3, label: i18n.tr('Høy', 'High') },
  ] as const)
  return (
    <div class="verevon-inbox-conversation-toolbar">
      <button
        type="button"
        onClick={() => props.selectedTicket.supportTicket ? props.onResolveTicket() : props.onPatchTicket({ state_id: 4 })}
        class="verevon-inbox-button verevon-inbox-button--secondary verevon-inbox-button--xs"
      >
        <CheckCheck class="size-3.5" />
        {props.selectedTicket.supportTicket ? i18n.tr('Løs sak', 'Resolve ticket') : i18n.tr('Lukk samtale', 'Close conversation')}
      </button>
      <TagEditor tags={props.selectedTicket.tags ?? []} onAdd={props.onAddTag} onRemove={props.onRemoveTag} />
      <SelectShell>
        <select
          aria-label={i18n.tr('Samtalestatus', 'Conversation status')}
          value={props.selectedTicket.state?.id ?? 2}
          onChange={(event) => props.onPatchTicket({ state_id: Number(event.currentTarget.value) })}
        >
          <For each={stateOptions()}>
            {(option) => <option value={option.id}>{option.label}</option>}
          </For>
        </select>
      </SelectShell>
      <SelectShell>
        <select
          aria-label={i18n.tr('Samtaleprioritet', 'Conversation priority')}
          value={props.selectedTicket.priority?.id ?? 2}
          onChange={(event) => props.onPatchTicket({ priority_id: Number(event.currentTarget.value) })}
        >
          <For each={priorityOptions()}>
            {(option) => <option value={option.id}>{option.label}</option>}
          </For>
        </select>
      </SelectShell>
      <Show when={props.agents.length}>
        <SelectShell>
          <select
            aria-label={i18n.tr('Tildelt', 'Assignee')}
            value={props.selectedTicket.owner?.id ?? 0}
            onChange={(event) => props.onPatchTicket({ owner_id: Number(event.currentTarget.value) })}
          >
            <option value={0}>{i18n.tr('Ikke tildelt', 'Unassigned')}</option>
            <For each={props.agents}>
              {(agent) => <option value={agent.id}>{agent.firstname} {agent.lastname}</option>}
            </For>
          </select>
        </SelectShell>
      </Show>
      <Show when={props.groups.length}>
        <SelectShell>
          <select
            aria-label={i18n.tr('Team-innboks', 'Group')}
            value={props.selectedTicket.group?.id ?? 0}
            onChange={(event) => props.onPatchTicket({ group_id: Number(event.currentTarget.value) })}
          >
            <For each={props.groups}>
              {(group) => <option value={group.id}>{group.name}</option>}
            </For>
          </select>
        </SelectShell>
      </Show>
    </div>
  )
}

function ConversationTranscript(props: {
  articles: ZammadArticle[]
  articlesLoading: boolean
  scrollRef: (node: HTMLDivElement) => void
  selectedTicket: ZammadTicket
}) {
  const i18n = useI18n()
  return (
    <div ref={props.scrollRef} class="verevon-inbox-transcript">
      <div class="verevon-inbox-transcript__date">
        <span>{formatDate(props.selectedTicket.created_at)}</span>
      </div>

      <Show when={props.articlesLoading}>
        <div class="verevon-inbox-transcript__loading">
          <span />
          <span />
        </div>
      </Show>
      <Show when={!props.articlesLoading && props.articles.length > 0}>
        <div class="verevon-inbox-transcript__articles">
          <For each={props.articles}>
            {(article) => <ArticleBubble article={article} ticket={props.selectedTicket} />}
          </For>
        </div>
      </Show>
      <Show when={!props.articlesLoading && props.articles.length === 0}>
        <p class="verevon-inbox-transcript__empty">{i18n.tr('Ingen meldinger i denne samtalen.', 'No articles in this conversation.')}</p>
      </Show>
    </div>
  )
}

function ConversationReplyComposer(props: {
  conversationId: string
  isInternal: boolean
  notice: string | null
  orgId: string
  userId: string
  onOpenModal: (modal: InboxModalRequest) => void
  onCreateSocialFollowUp: () => void
  onSendReply: (text: string, internal: boolean) => Promise<boolean>
  onSuggestReply: () => void
  replySending: boolean
  replyText: string
  selectedTicket: ZammadTicket
  setIsInternal: (isInternal: boolean) => void
  setReplyText: (value: string) => void
}) {
  const i18n = useI18n()
  const [leaseState, setLeaseState] = createSignal<'idle' | 'claiming' | 'owned' | 'blocked' | 'unavailable'>('idle')
  const [foreignDraftLease, setForeignDraftLease] = createSignal(false)
  const [draftState, setDraftState] = createSignal<'idle' | 'saved' | 'recovered' | 'not-retained' | 'failed'>('idle')
  let renewTimer: number | undefined
  let presenceTimer: number | undefined
  let saveTimer: number | undefined
  let claimInFlight: Promise<boolean> | null = null
  let leaseGeneration = 0
  let disposed = false
  let composerFocused = false
  let sendInFlight = false
  let hasLocalEdit = false
  // This value is passed while the Show child is live. Cleanup runs after the
  // child accessor may be invalid, so it must never re-read selectedTicket.
  const activeConversationId = untrack(() => props.conversationId)
  const activeOrgId = untrack(() => props.orgId)
  const currentUserId = () => props.userId
  const stopRenewal = () => {
    if (renewTimer !== undefined) window.clearInterval(renewTimer)
    renewTimer = undefined
  }
  const stopDraftSave = () => {
    if (saveTimer !== undefined) window.clearTimeout(saveTimer)
    saveTimer = undefined
  }
  const refreshDraftPresence = async () => {
    try {
      const lease = await getDraftLease(activeOrgId, activeConversationId)
      if (disposed) return
      const expiresAt = Date.parse(lease.expires_at)
      setForeignDraftLease(
        Boolean(lease.user_id && currentUserId() && lease.user_id !== currentUserId() && Number.isFinite(expiresAt) && expiresAt > Date.now()),
      )
    } catch (reason) {
      if (disposed) return
      if (reason instanceof ApiError && reason.status === 404) setForeignDraftLease(false)
    }
  }
  const markRetentionResult = (reason: unknown) => {
    if (reason instanceof ApiError && (reason.status === 412 || reason.code === 'zdr_draft_persistence_forbidden')) {
      setDraftState('not-retained')
      return
    }
    setDraftState('failed')
  }
  const removeDraft = async () => {
    try {
      await deleteConversationDraft(activeOrgId, activeConversationId)
      setDraftState('idle')
    } catch (reason) {
      markRetentionResult(reason)
    }
  }
  const release = () => {
    leaseGeneration += 1
    stopRenewal()
    const id = activeConversationId
    if (id && leaseState() === 'owned') void releaseDraftLease(activeOrgId, id).catch(() => undefined)
    setLeaseState('idle')
  }
  const loseLease = () => {
    stopRenewal()
    setLeaseState('unavailable')
  }
  const claim = (): Promise<boolean> => {
    const id = activeConversationId
    if (!id || leaseState() === 'blocked') return Promise.resolve(false)
    if (leaseState() === 'owned') return Promise.resolve(true)
    if (claimInFlight) return claimInFlight
    const generation = leaseGeneration
    setLeaseState('claiming')
    claimInFlight = claimDraftLease(activeOrgId, id).then(async () => {
      if (disposed || generation !== leaseGeneration) {
        await releaseDraftLease(activeOrgId, id).catch(() => undefined)
        return false
      }
      setLeaseState('owned')
      setForeignDraftLease(false)
      stopRenewal()
      const renew = () => { void claimDraftLease(activeOrgId, id).catch(loseLease) }
      renewTimer = window.setInterval(renew, 30_000)
      return true
    // eslint-disable-next-line solid/reactivity -- this rejection is part of the explicit lease-claim request lifecycle.
    }).catch((reason) => {
      if (disposed || generation !== leaseGeneration) return false
      setLeaseState(reason instanceof ApiError && (reason.status === 409 || reason.code === 'conflict') ? 'blocked' : 'unavailable')
      if (reason instanceof ApiError && (reason.status === 409 || reason.code === 'conflict')) void refreshDraftPresence()
      return false
    }).finally(() => {
      claimInFlight = null
    })
    return claimInFlight
  }
  const persistDraft = async () => {
    stopDraftSave()
    const body = props.replyText.trim()
    if (!body) {
      if (activeConversationId) await removeDraft()
      return
    }
    if (!await claim()) return
    try {
      await saveConversationDraft(activeOrgId, activeConversationId, body, props.isInternal)
      setDraftState('saved')
    } catch (reason) {
      markRetentionResult(reason)
    }
  }
  const scheduleDraftSave = () => {
    stopDraftSave()
    if (draftState() === 'not-retained') return
    if (!props.replyText.trim()) {
      void removeDraft()
      return
    }
    // eslint-disable-next-line solid/reactivity -- this debounce is deliberately driven by the input event.
    saveTimer = window.setTimeout(() => { void persistDraft() }, 750)
  }
  const send = async () => {
    if (sendInFlight || !props.replyText.trim() || props.replySending || leaseState() === 'blocked') return
    sendInFlight = true
    try {
      if (!await claim()) return
      const accepted = await props.onSendReply(props.replyText, props.isInternal)
      if (accepted) {
        stopDraftSave()
        void deleteConversationDraft(activeOrgId, activeConversationId).catch(() => undefined)
        release()
      }
    } finally {
      sendInFlight = false
    }
  }
  createEffect(
    () => undefined,
    () => {
      // The keyed composer is recreated for a new conversation. Reset the mode
      // before an owned recovery record can restore its saved internal/reply mode.
      props.setIsInternal(false)
      // eslint-disable-next-line solid/reactivity -- recovery is intentionally started once per keyed composer mount.
      void getConversationDraft(activeOrgId, activeConversationId).then((draft) => {
        if (hasLocalEdit || !draft?.body_text) return
        props.setReplyText(draft.body_text)
        props.setIsInternal(draft.internal)
        setDraftState('recovered')
      }).catch((reason) => {
        if (reason instanceof ApiError && reason.status === 404) return
        markRetentionResult(reason)
      })
      void refreshDraftPresence()
      // eslint-disable-next-line solid/reactivity -- the current keyed composer owns this bounded presence poll.
      presenceTimer = window.setInterval(() => { void refreshDraftPresence() }, 15_000)
    },
  )
  onCleanup(() => {
    disposed = true
    stopDraftSave()
    if (presenceTimer !== undefined) window.clearInterval(presenceTimer)
    release()
  })
  return (
    <div class="verevon-inbox-composer-wrap">
      <Show when={props.notice}>
        <p class="verevon-inbox-notice">{props.notice}</p>
      </Show>
      <Show when={leaseState() === 'blocked'}>
        <p role="alert" class="verevon-inbox-notice">{i18n.tr('En annen operatør skriver et utkast. Svarfeltet er låst til leien utløper eller frigis.', 'Another operator is drafting. The composer is locked until their lease expires or is released.')}</p>
      </Show>
      <Show when={foreignDraftLease() && leaseState() !== 'blocked'}>
        <div class="verevon-inbox-composer__draft-status verevon-inbox-composer__draft-status--presence" role="status">
          <span>{i18n.tr('En annen kollega skriver et svar.', 'Another teammate is composing a reply.')}</span>
          <button type="button" onClick={() => void refreshDraftPresence()}>
            {i18n.tr('Kontroller utkaststatus igjen', 'Check drafting status again')}
          </button>
        </div>
      </Show>
      <Show when={leaseState() === 'unavailable'}>
        <p role="alert" class="verevon-inbox-notice">{i18n.tr('Utkastbeskyttelsen kunne ikke bekreftes. Meldingen er ikke sendt; prøv Send på nytt når forbindelsen er tilbake.', 'Draft protection could not be confirmed. The message was not sent; retry Send when the connection is back.')}</p>
      </Show>
      <Show when={draftState() === 'recovered'}>
        <p class="verevon-inbox-composer__draft-status">{i18n.tr('Ditt lagrede utkast er gjenopprettet.', 'Your saved draft has been restored.')}</p>
      </Show>
      <Show when={draftState() === 'saved'}>
        <p class="verevon-inbox-composer__draft-status">{i18n.tr('Personlig utkast lagret.', 'Personal draft saved.')}</p>
      </Show>
      <Show when={draftState() === 'not-retained'}>
        <p class="verevon-inbox-composer__draft-status">{i18n.tr('ZDR er aktiv: personlige utkast beholdes bare i dette åpne feltet.', 'ZDR is active: personal drafts remain only in this open reply field.')}</p>
      </Show>
      <Show when={draftState() === 'failed'}>
        <p role="alert" class="verevon-inbox-notice">{i18n.tr('Personlig utkast kunne ikke lagres. Teksten er fortsatt i dette feltet.', 'Your personal draft could not be saved. The text remains in this field.')}</p>
      </Show>
      <div class="verevon-inbox-composer">
        <ConversationReplyComposerHeader
          isInternal={props.isInternal}
          selectedTicket={props.selectedTicket}
          setIsInternal={(internal) => {
            props.setIsInternal(internal)
            if (props.replyText.trim()) scheduleDraftSave()
          }}
        />
        <textarea
          rows={4}
          value={props.replyText}
          maxlength={8000}
          disabled={leaseState() === 'blocked' || foreignDraftLease()}
          onFocus={() => {
            composerFocused = true
            void claim()
          }}
          onBlur={() => {
            composerFocused = false
            // eslint-disable-next-line solid/reactivity -- this continuation belongs to the blur event and releases only after its save settles.
            void persistDraft().finally(() => {
              if (!composerFocused) release()
            })
          }}
          onInput={(event) => {
            hasLocalEdit = true
            props.setReplyText(event.currentTarget.value)
            // eslint-disable-next-line solid/reactivity -- continuation intentionally follows this input's lease claim.
            void claim().then((owned) => { if (owned) scheduleDraftSave() })
          }}
          onKeyDown={(event) => {
            if ((event.metaKey || event.ctrlKey) && event.key === 'Enter') {
              event.preventDefault()
              void send()
            }
          }}
          placeholder={props.isInternal ? i18n.tr('Legg til et internt notat …', 'Add an internal note...') : i18n.tr(`Svar til ${customerName(props.selectedTicket)} …`, `Reply to ${customerName(props.selectedTicket)}...`)}
          aria-label={props.isInternal ? i18n.tr('Legg til et internt notat', 'Add an internal note') : i18n.tr(`Svar til ${customerName(props.selectedTicket)}`, `Reply to ${customerName(props.selectedTicket)}`)}
        />
          <ConversationReplyComposerFooter
            isInternal={props.isInternal}
            onOpenModal={props.onOpenModal}
            onCreateSocialFollowUp={props.onCreateSocialFollowUp}
            onSendReply={send}
            onSuggestReply={props.onSuggestReply}
            replySending={props.replySending}
            replyText={props.replyText}
            leaseBlocked={leaseState() === 'blocked' || foreignDraftLease()}
          />
      </div>
    </div>
  )
}

function ConversationReplyComposerHeader(props: {
  isInternal: boolean
  selectedTicket: ZammadTicket
  setIsInternal: (isInternal: boolean) => void
}) {
  const i18n = useI18n()
  return (
    <div class="verevon-inbox-composer__header">
      <ModeButton active={!props.isInternal} icon={MessageCircle} label={i18n.tr('Svar', 'Reply')} onClick={() => props.setIsInternal(false)} showChevron />
      <ModeButton active={props.isInternal} icon={PenLine} label={i18n.tr('Internt notat', 'Internal note')} onClick={() => props.setIsInternal(true)} warning />
      <div class="verevon-inbox-composer__to">
        <span>{i18n.tr('Til:', 'To:')}</span>
        <strong>{props.selectedTicket.customer?.email ?? customerName(props.selectedTicket)}</strong>
        <ChevronDown class="size-3.5" />
      </div>
    </div>
  )
}

function ConversationReplyComposerFooter(props: {
  isInternal: boolean
  onOpenModal: (modal: InboxModalRequest) => void
  onCreateSocialFollowUp: () => void
  onSendReply: () => void
  onSuggestReply: () => void
  leaseBlocked: boolean
  replySending: boolean
  replyText: string
}) {
  const i18n = useI18n()
  return (
    <div class="verevon-inbox-composer__footer">
      <div class="verevon-inbox-composer__footer-row">
        <div class="verevon-inbox-composer__tool-row">
          <IconButton label={i18n.tr('Lag utkast med Verevon', 'Draft with Verevon')} onClick={props.onSuggestReply}>
            <Sparkles class="size-4" />
          </IconButton>
          <IconButton label={i18n.tr('Opprett sosial oppfølging', 'Create social follow-up')} onClick={props.onCreateSocialFollowUp}>
            <Megaphone class="size-4" />
          </IconButton>
        </div>
        <div class="verevon-inbox-composer__send-row">
          <button
            type="button"
            disabled={!props.replyText.trim() || props.replySending || props.leaseBlocked}
            onMouseDown={(event) => event.preventDefault()}
            onClick={() => props.onSendReply()}
            class="verevon-inbox-button verevon-inbox-button--secondary verevon-inbox-button--sm"
          >
            {props.replySending ? i18n.tr('Sender …', 'Sending...') : i18n.tr('Send', 'Send')}
          </button>
        </div>
      </div>
    </div>
  )
}


function ArticleBubble(props: { article: ZammadArticle; ticket: ZammadTicket }) {
  const i18n = useI18n()
  const agentMessage = () => props.article.sender?.toLowerCase() === 'agent'
  const senderName = () => props.article.from || (agentMessage() ? 'Verevon Support' : customerName(props.ticket))
  const senderEmail = () => props.article.fromEmail
  const submissionReceipt = () => agentMessage() && !props.article.internal && props.article.provider

  return (
    <article class="verevon-inbox-article">
      <div class={cn('verevon-inbox-article__avatar', agentMessage() ? 'verevon-inbox-article__avatar--agent' : 'verevon-inbox-article__avatar--customer')}>
        {agentMessage() ? 'A' : customerInitials(props.ticket)}
      </div>
      <div class="verevon-inbox-article__body">
        <div class="verevon-inbox-article__meta">
          <span class={{ 'verevon-inbox-article__agent-name': agentMessage() }}>{senderName()}</span>
          <Show when={senderEmail() && senderEmail() !== senderName()}>
            <span class="verevon-inbox-article__email">&lt;{senderEmail()}&gt;</span>
          </Show>
          <Mail class="size-3.5" />
          <time>{formatTimestamp(props.article.created_at)}</time>
        </div>
        <div class={cn('verevon-inbox-article__bubble', props.article.internal && 'verevon-inbox-article__bubble--internal', agentMessage() && !props.article.internal && 'verevon-inbox-article__bubble--agent')}>
          <EmailBody html={props.article.bodyHtml ?? props.article.body} text={props.article.bodyText} />
        </div>
		<Show when={(props.article.attachments?.length ?? 0) > 0}>
			<ul
				class="verevon-inbox-article__attachments"
				aria-label={i18n.tr('Vedlegg', 'Attachments')}
				title={i18n.tr('Vedleggsmetadata er tilgjengelig. Sikker nedlasting er ikke aktivert ennå.', 'Attachment metadata is available. Secure download is not enabled yet.')}
			>
				<For each={props.article.attachments}>
					{(attachment) => (
						<li class="verevon-inbox-article__attachment">
							<Paperclip class="size-3.5" />
							<span>{attachment.filename}</span>
							<span class="verevon-inbox-article__attachment-size">{formatAttachmentSize(attachment.sizeBytes)}</span>
						</li>
					)}
				</For>
			</ul>
		</Show>
        <Show when={props.article.internal}>
          <span class="verevon-inbox-article__internal">{i18n.tr('Internt notat', 'Internal note')}</span>
        </Show>
        <Show when={submissionReceipt()}>
          <span class="verevon-inbox-article__submitted">
            {i18n.tr(
              `Sendt til ${titleCase(props.article.provider!)}; leverandøren har akseptert forespørselen. Levering er ikke bekreftet.`,
              `Submitted to ${titleCase(props.article.provider!)}; the provider accepted the request. Delivery is not confirmed.`,
            )}
          </span>
        </Show>
      </div>
    </article>
  )
}

function formatAttachmentSize(bytes: number): string {
	if (!Number.isFinite(bytes) || bytes <= 0) return '0 B'
	if (bytes < 1024) return `${Math.round(bytes)} B`
	if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`
	return `${(bytes / (1024 * 1024)).toFixed(bytes >= 10 * 1024 * 1024 ? 0 : 1)} MB`
}

function TagEditor(props: { tags: string[]; onAdd: (tag: string) => void; onRemove: (tag: string) => void }) {
  const i18n = useI18n()
  const [adding, setAdding] = createSignal(false)
  const [draft, setDraft] = createSignal('')
  let inputRef: HTMLInputElement | undefined

  createEffect(
    () => adding(),
    (isAdding) => {
      if (isAdding) window.requestAnimationFrame(() => inputRef?.focus())
    },
  )

  const submit = () => {
    props.onAdd(draft())
    setDraft('')
    setAdding(false)
  }

  return (
    <div class="verevon-inbox-tag-editor">
      <Tag class="size-3.5" />
      <For each={props.tags}>
        {(tag) => (
          <span class="verevon-inbox-tag">
            {tag}
            <button type="button" onClick={() => props.onRemove(tag)} aria-label={i18n.tr(`Fjern tag ${tag}`, `Remove tag ${tag}`)}>
              <X class="size-3" />
            </button>
          </span>
        )}
      </For>
      <Show
        when={adding()}
        fallback={
          <button type="button" onClick={() => setAdding(true)} class="verevon-inbox-add-tag">
            <Plus class="size-3.5" />
            {i18n.tr('Legg til tagger', 'Add Tags')}
          </button>
        }
      >
        <input
          ref={inputRef}
          value={draft()}
          onInput={(event) => setDraft(event.currentTarget.value)}
          onBlur={submit}
          onKeyDown={(event) => {
            if (event.key === 'Enter') submit()
            if (event.key === 'Escape') setAdding(false)
          }}
          placeholder={i18n.tr('tag …', 'tag...')}
          aria-label={i18n.tr('Ny tag', 'New tag')}
        />
      </Show>
    </div>
  )
}

function ModeButton(props: { active: boolean; icon: Component<LucideProps>; label: string; onClick: () => void; showChevron?: boolean; warning?: boolean }) {
  return (
    <button
      type="button"
      onClick={() => props.onClick()}
      class={cn(
        'verevon-inbox-mode-button',
        props.active && 'verevon-inbox-mode-button--active',
        props.active && props.warning && 'verevon-inbox-mode-button--warning',
      )}
    >
      <Dynamic component={props.icon} class="size-3.5" />
      {props.label}
      <Show when={props.showChevron}>
        <ChevronDown class="size-3.5" />
      </Show>
    </button>
  )
}

function IconButton(props: { children: JSX.Element; label: string; onClick?: () => void }) {
  return (
    <button type="button" onClick={() => props.onClick?.()} aria-label={props.label} title={props.label} class="verevon-inbox-icon-button verevon-inbox-icon-button--xs">
      {props.children}
    </button>
  )
}

function HeaderIconButton(props: { children: JSX.Element; label: string; onClick?: () => void }) {
  return (
    <button type="button" onClick={() => props.onClick?.()} aria-label={props.label} title={props.label} class="verevon-inbox-icon-button">
      {props.children}
    </button>
  )
}

function SelectShell(props: { children: JSX.Element }) {
  return (
    <div class="verevon-inbox-select">
      {props.children}
      <ChevronDown class="size-3" />
    </div>
  )
}
