import {
  CheckCheck,
  ChevronDown,
  Clock3,
  Link2,
  Mail,
  Megaphone,
  MessageCircle,
  PenLine,
  Plus,
  Sparkles,
  Star,
  Tag,
  TicketCheck,
  X,
  type LucideProps,
} from 'lucide-solid'
import { createEffect, createSignal, For, Show, type Component, type JSX } from 'solid-js'
import { Dynamic } from 'solid-js/web'
import { AiActionReviewPanel } from '@/features/inbox/components/AiActionReviewPanel'
import { EmailBody } from '@/features/inbox/components/EmailBody'
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

const stateOptions = [
  { id: 1, label: 'New' },
  { id: 2, label: 'Open' },
  { id: 4, label: 'Closed' },
  { id: 6, label: 'Pending reminder' },
] as const

const priorityOptions = [
  { id: 1, label: 'Low' },
  { id: 2, label: 'Normal' },
  { id: 3, label: 'High' },
] as const

export function ConversationPanel(props: {
  agents: Agent[]
  articles: ZammadArticle[]
  articlesLoading: boolean
  groups: Group[]
  notice: string | null
  onAddTag: (tag: string) => void
  onCreateTicket: () => void
  onLinkExistingTicket: () => void
  onOpenModal: (modal: InboxModalRequest) => void
  onPatchTicket: (patch: Record<string, unknown>) => void
  onRemoveTag: (tag: string) => void
  onCreateSocialFollowUp: () => void
  onSendReply: (text: string, internal: boolean) => void
  onSuggestReply: () => void
  onViewTicket: () => void
  replyText: string
  replySending: boolean
  selectedTicket: ZammadTicket | null
  sentiment: TicketSentiment | null
  setReplyText: (value: string) => void
}) {
  const [isInternal, setIsInternal] = createSignal(false)
  let scrollRef: HTMLDivElement | undefined

  createEffect(() => {
    const shouldScroll = Boolean(props.selectedTicket && !props.articlesLoading && props.articles.length >= 0)
    if (!shouldScroll) return
    window.requestAnimationFrame(() => {
      scrollRef?.scrollTo?.({ top: scrollRef.scrollHeight, behavior: 'smooth' })
    })
  })

  return (
    <Show when={props.selectedTicket} fallback={<ConversationEmptyState />}>
      {(ticket) => {
        const contactReason = () => props.sentiment?.sentiment ? titleCase(props.sentiment.sentiment) : ticket().priority?.name ?? 'Support request'

        return (
          <main class="velion-inbox-conversation">
            <ConversationHeader
              agents={props.agents}
              contactReason={contactReason()}
              groups={props.groups}
              onAddTag={props.onAddTag}
              onCreateTicket={props.onCreateTicket}
              onLinkExistingTicket={props.onLinkExistingTicket}
              onOpenModal={props.onOpenModal}
              onPatchTicket={props.onPatchTicket}
              onRemoveTag={props.onRemoveTag}
              onViewTicket={props.onViewTicket}
              selectedTicket={ticket()}
              sentiment={props.sentiment}
            />
            <AiActionReviewPanel conversationId={(ticket() as { conversationId?: string }).conversationId} />
            <ConversationTranscript
              articles={props.articles}
              articlesLoading={props.articlesLoading}
              scrollRef={(node) => { scrollRef = node }}
              selectedTicket={ticket()}
            />
            <ConversationReplyComposer
              isInternal={isInternal()}
              notice={props.notice}
              onOpenModal={props.onOpenModal}
              onPatchTicket={props.onPatchTicket}
              onCreateSocialFollowUp={props.onCreateSocialFollowUp}
              onSendReply={props.onSendReply}
              onSuggestReply={props.onSuggestReply}
              replySending={props.replySending}
              replyText={props.replyText}
              selectedTicket={ticket()}
              setIsInternal={setIsInternal}
              setReplyText={props.setReplyText}
            />
          </main>
        )
      }}
    </Show>
  )
}

function ConversationEmptyState() {
  return (
    <main class="velion-inbox-conversation-empty">
      <div>
        <div class="velion-inbox-conversation-empty__icon">
          <MessageCircle class="size-8" strokeWidth={1.45} />
        </div>
        <p>Select a ticket to view the conversation</p>
        <small>Customer details, conversation history, and the reply composer will appear here.</small>
      </div>
    </main>
  )
}

function ConversationHeader(props: {
  agents: Agent[]
  contactReason: string
  groups: Group[]
  onAddTag: (tag: string) => void
  onCreateTicket: () => void
  onLinkExistingTicket: () => void
  onOpenModal: (modal: InboxModalRequest) => void
  onPatchTicket: (patch: Record<string, unknown>) => void
  onRemoveTag: (tag: string) => void
  onViewTicket: () => void
  selectedTicket: ZammadTicket
  sentiment: TicketSentiment | null
}) {
  const supportTicket = () => props.selectedTicket.supportTicket ?? null
  return (
    <div class="velion-inbox-conversation-header">
      <div class="velion-inbox-conversation-header__top">
        <div class="velion-inbox-conversation-header__title">
          <div>
            <h2>{props.selectedTicket.title}</h2>
            <span>#{props.selectedTicket.number}</span>
          </div>
          <p>
            <Clock3 class="size-3.5" />
            Updated {formatRelativeTime(props.selectedTicket.updated_at)} ago
            <Show when={props.sentiment}>
              {(sentiment) => <SentimentBadge sentiment={sentiment().sentiment} />}
            </Show>
          </p>
        </div>

        <div class="velion-inbox-conversation-header__actions">
          <HeaderIconButton
            label="Star conversation"
            onClick={() => props.onOpenModal({
              type: 'work',
              title: 'Watch conversation',
              description: 'Keep this conversation in a monitored queue and let Velion surface changes, SLA risk, and customer replies here.',
              primaryAction: 'Start watch',
            })}
          >
            <Star class="size-4" />
          </HeaderIconButton>
            <HeaderIconButton
              label="Link to existing ticket"
              onClick={props.onLinkExistingTicket}
            >
              <Link2 class="size-4" />
            </HeaderIconButton>
          <Show
            when={supportTicket()}
            fallback={(
              <button type="button" onClick={props.onCreateTicket} class="velion-inbox-button velion-inbox-button--secondary velion-inbox-button--xs">
                <Plus class="size-3.5" />
                Create ticket
              </button>
            )}
          >
            {(ticket) => (
              <button type="button" onClick={props.onViewTicket} class="velion-inbox-button velion-inbox-button--secondary velion-inbox-button--xs">
                <TicketCheck class="size-3.5" />
                {ticket().ticket_key}
              </button>
            )}
          </Show>
          <button type="button" onClick={() => props.onPatchTicket({ state_id: 4 })} class="velion-inbox-button velion-inbox-button--primary velion-inbox-button--xs">
            <CheckCheck class="size-3.5" />
            Close
          </button>
        </div>
      </div>

      <ConversationToolbar
        agents={props.agents}
        groups={props.groups}
        onAddTag={props.onAddTag}
        onPatchTicket={props.onPatchTicket}
        onRemoveTag={props.onRemoveTag}
        selectedTicket={props.selectedTicket}
      />

      <div class="velion-inbox-conversation-header__reason">
        <div>
          <span>Contact reason:</span>
          <strong>{props.contactReason}</strong>
          <TicketDecisionChip ticket={props.selectedTicket} />
        </div>
        <button
          type="button"
          onClick={() => props.onOpenModal({
            type: 'work',
            title: 'Conversation intelligence',
            description: 'Review intent, sentiment, SLA, ownership, and suggested next actions for the selected ticket in this modal.',
            primaryAction: 'Update context',
          })}
        >
          Show more
        </button>
      </div>
    </div>
  )
}

function TicketDecisionChip(props: { ticket: ZammadTicket }) {
  const supportTicket = () => props.ticket.supportTicket ?? null
  const label = () => {
    const ticket = supportTicket()
    if (!ticket) return 'No ticket needed'
    if (ticket.status === 'suggested') {
      const confidence = ticket.ai_confidence ? ` · ${Math.round(ticket.ai_confidence * 100)}%` : ''
      return `Suggested ticket${confidence}`
    }
    if (ticket.source === 'ai') return 'Auto-created ticket'
    return 'Ticket created'
  }
  const tone = () => {
    const ticket = supportTicket()
    if (!ticket) return 'none'
    if (ticket.status === 'suggested') return 'suggested'
    if (ticket.source === 'ai') return 'auto'
    return 'manual'
  }
  return <span class={`velion-inbox-ticket-decision velion-inbox-ticket-decision--${tone()}`}>{label()}</span>
}

function ConversationToolbar(props: {
  agents: Agent[]
  groups: Group[]
  onAddTag: (tag: string) => void
  onPatchTicket: (patch: Record<string, unknown>) => void
  onRemoveTag: (tag: string) => void
  selectedTicket: ZammadTicket
}) {
  return (
    <div class="velion-inbox-conversation-toolbar">
      <button type="button" onClick={() => props.onPatchTicket({ state_id: 4 })} class="velion-inbox-button velion-inbox-button--secondary velion-inbox-button--xs">
        <CheckCheck class="size-3.5" />
        Close
      </button>
      <TagEditor tags={props.selectedTicket.tags ?? []} onAdd={props.onAddTag} onRemove={props.onRemoveTag} />
      <SelectShell>
        <select
          aria-label="Conversation status"
          value={props.selectedTicket.state?.id ?? 2}
          onChange={(event) => props.onPatchTicket({ state_id: Number(event.currentTarget.value) })}
        >
          <For each={stateOptions}>
            {(option) => <option value={option.id}>{option.label}</option>}
          </For>
        </select>
      </SelectShell>
      <SelectShell>
        <select
          aria-label="Conversation priority"
          value={props.selectedTicket.priority?.id ?? 2}
          onChange={(event) => props.onPatchTicket({ priority_id: Number(event.currentTarget.value) })}
        >
          <For each={priorityOptions}>
            {(option) => <option value={option.id}>{option.label}</option>}
          </For>
        </select>
      </SelectShell>
      <Show when={props.agents.length}>
        <SelectShell>
          <select
            aria-label="Assignee"
            value={props.selectedTicket.owner?.id ?? 0}
            onChange={(event) => props.onPatchTicket({ owner_id: Number(event.currentTarget.value) })}
          >
            <option value={0}>Unassigned</option>
            <For each={props.agents}>
              {(agent) => <option value={agent.id}>{agent.firstname} {agent.lastname}</option>}
            </For>
          </select>
        </SelectShell>
      </Show>
      <Show when={props.groups.length}>
        <SelectShell>
          <select
            aria-label="Group"
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
  return (
    <div ref={props.scrollRef} class="velion-inbox-transcript">
      <div class="velion-inbox-transcript__date">
        <span>{formatDate(props.selectedTicket.created_at)}</span>
      </div>

      <Show when={props.articlesLoading}>
        <div class="velion-inbox-transcript__loading">
          <span />
          <span />
        </div>
      </Show>
      <Show when={!props.articlesLoading && props.articles.length > 0}>
        <div class="velion-inbox-transcript__articles">
          <For each={props.articles}>
            {(article) => <ArticleBubble article={article} ticket={props.selectedTicket} />}
          </For>
        </div>
      </Show>
      <Show when={!props.articlesLoading && props.articles.length === 0}>
        <p class="velion-inbox-transcript__empty">No articles in this conversation.</p>
      </Show>
    </div>
  )
}

function ConversationReplyComposer(props: {
  isInternal: boolean
  notice: string | null
  onOpenModal: (modal: InboxModalRequest) => void
  onPatchTicket: (patch: Record<string, unknown>) => void
  onCreateSocialFollowUp: () => void
  onSendReply: (text: string, internal: boolean) => void
  onSuggestReply: () => void
  replySending: boolean
  replyText: string
  selectedTicket: ZammadTicket
  setIsInternal: (isInternal: boolean) => void
  setReplyText: (value: string) => void
}) {
  return (
    <div class="velion-inbox-composer-wrap">
      <Show when={props.notice}>
        <p class="velion-inbox-notice">{props.notice}</p>
      </Show>
      <div class="velion-inbox-composer">
        <ConversationReplyComposerHeader
          isInternal={props.isInternal}
          selectedTicket={props.selectedTicket}
          setIsInternal={props.setIsInternal}
        />
        <textarea
          rows={4}
          value={props.replyText}
          onInput={(event) => props.setReplyText(event.currentTarget.value)}
          onKeyDown={(event) => {
            if ((event.metaKey || event.ctrlKey) && event.key === 'Enter') {
              event.preventDefault()
              props.onSendReply(props.replyText, props.isInternal)
            }
          }}
          placeholder={props.isInternal ? 'Add an internal note...' : `Reply to ${customerName(props.selectedTicket)}...`}
          aria-label={props.isInternal ? 'Add an internal note' : `Reply to ${customerName(props.selectedTicket)}`}
        />
          <ConversationReplyComposerFooter
            isInternal={props.isInternal}
            onOpenModal={props.onOpenModal}
            onPatchTicket={props.onPatchTicket}
            onCreateSocialFollowUp={props.onCreateSocialFollowUp}
            onSendReply={props.onSendReply}
          onSuggestReply={props.onSuggestReply}
          replySending={props.replySending}
          replyText={props.replyText}
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
  return (
    <div class="velion-inbox-composer__header">
      <ModeButton active={!props.isInternal} icon={MessageCircle} label="Reply" onClick={() => props.setIsInternal(false)} />
      <ModeButton active={props.isInternal} icon={PenLine} label="Internal note" onClick={() => props.setIsInternal(true)} warning />
      <div class="velion-inbox-composer__to">
        <span>To:</span>
        <strong>{props.selectedTicket.customer?.email ?? customerName(props.selectedTicket)}</strong>
        <ChevronDown class="size-3.5" />
      </div>
    </div>
  )
}

function ConversationReplyComposerFooter(props: {
  isInternal: boolean
  onOpenModal: (modal: InboxModalRequest) => void
  onPatchTicket: (patch: Record<string, unknown>) => void
  onCreateSocialFollowUp: () => void
  onSendReply: (text: string, internal: boolean) => void
  onSuggestReply: () => void
  replySending: boolean
  replyText: string
}) {
  return (
    <div class="velion-inbox-composer__footer">
      <div class="velion-inbox-composer__footer-row">
        <div class="velion-inbox-composer__tool-row">
          <IconButton label="Draft with Velion" onClick={props.onSuggestReply}>
            <Sparkles class="size-4" />
          </IconButton>
          <IconButton label="Create social follow-up" onClick={props.onCreateSocialFollowUp}>
            <Megaphone class="size-4" />
          </IconButton>
        </div>
        <div class="velion-inbox-composer__send-row">
          <button
            type="button"
            disabled={!props.replyText.trim() || props.replySending}
            onClick={() => props.onSendReply(props.replyText, props.isInternal)}
            class="velion-inbox-button velion-inbox-button--secondary velion-inbox-button--sm"
          >
            {props.replySending ? 'Sending...' : 'Send'}
          </button>
          <button
            type="button"
            disabled={!props.replyText.trim() || props.replySending}
            onClick={() => {
              props.onSendReply(props.replyText, props.isInternal)
              props.onPatchTicket({ state_id: 4 })
            }}
            class="velion-inbox-button velion-inbox-button--primary velion-inbox-button--sm"
          >
            Send & Close
          </button>
        </div>
      </div>
    </div>
  )
}


function ArticleBubble(props: { article: ZammadArticle; ticket: ZammadTicket }) {
  const agentMessage = () => props.article.sender?.toLowerCase() === 'agent'
  const senderName = () => props.article.from || (agentMessage() ? 'Velion Support' : customerName(props.ticket))
  const senderEmail = () => props.article.fromEmail

  return (
    <article class="velion-inbox-article">
      <div class={cn('velion-inbox-article__avatar', agentMessage() ? 'velion-inbox-article__avatar--agent' : 'velion-inbox-article__avatar--customer')}>
        {agentMessage() ? 'A' : customerInitials(props.ticket)}
      </div>
      <div class="velion-inbox-article__body">
        <div class="velion-inbox-article__meta">
          <span classList={{ 'velion-inbox-article__agent-name': agentMessage() }}>{senderName()}</span>
          <Show when={senderEmail() && senderEmail() !== senderName()}>
            <span class="velion-inbox-article__email">&lt;{senderEmail()}&gt;</span>
          </Show>
          <Mail class="size-3.5" />
          <time>{formatTimestamp(props.article.created_at)}</time>
        </div>
        <div class={cn('velion-inbox-article__bubble', props.article.internal && 'velion-inbox-article__bubble--internal', agentMessage() && !props.article.internal && 'velion-inbox-article__bubble--agent')}>
          <EmailBody html={props.article.bodyHtml ?? props.article.body} text={props.article.bodyText} />
        </div>
        <Show when={props.article.internal}>
          <span class="velion-inbox-article__internal">Internal note</span>
        </Show>
      </div>
    </article>
  )
}

function TagEditor(props: { tags: string[]; onAdd: (tag: string) => void; onRemove: (tag: string) => void }) {
  const [adding, setAdding] = createSignal(false)
  const [draft, setDraft] = createSignal('')
  let inputRef: HTMLInputElement | undefined

  createEffect(() => {
    if (adding()) window.requestAnimationFrame(() => inputRef?.focus())
  })

  const submit = () => {
    props.onAdd(draft())
    setDraft('')
    setAdding(false)
  }

  return (
    <div class="velion-inbox-tag-editor">
      <Tag class="size-3.5" />
      <For each={props.tags}>
        {(tag) => (
          <span class="velion-inbox-tag">
            {tag}
            <button type="button" onClick={() => props.onRemove(tag)} aria-label={`Remove tag ${tag}`}>
              <X class="size-3" />
            </button>
          </span>
        )}
      </For>
      <Show
        when={adding()}
        fallback={
          <button type="button" onClick={() => setAdding(true)} class="velion-inbox-add-tag">
            <Plus class="size-3.5" />
            Add Tags
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
          placeholder="tag..."
          aria-label="New tag"
        />
      </Show>
    </div>
  )
}

function ModeButton(props: { active: boolean; icon: Component<LucideProps>; label: string; onClick: () => void; warning?: boolean }) {
  return (
    <button
      type="button"
      onClick={() => props.onClick()}
      class={cn(
        'velion-inbox-mode-button',
        props.active && 'velion-inbox-mode-button--active',
        props.active && props.warning && 'velion-inbox-mode-button--warning',
      )}
    >
      <Dynamic component={props.icon} class="size-3.5" />
      {props.label}
      <Show when={props.label === 'Reply'}>
        <ChevronDown class="size-3.5" />
      </Show>
    </button>
  )
}

function IconButton(props: { children: JSX.Element; label: string; onClick?: () => void }) {
  return (
    <button type="button" onClick={() => props.onClick?.()} aria-label={props.label} title={props.label} class="velion-inbox-icon-button velion-inbox-icon-button--xs">
      {props.children}
    </button>
  )
}

function HeaderIconButton(props: { children: JSX.Element; label: string; onClick?: () => void }) {
  return (
    <button type="button" onClick={() => props.onClick?.()} aria-label={props.label} title={props.label} class="velion-inbox-icon-button">
      {props.children}
    </button>
  )
}

function SelectShell(props: { children: JSX.Element }) {
  return (
    <div class="velion-inbox-select">
      {props.children}
      <ChevronDown class="size-3" />
    </div>
  )
}
