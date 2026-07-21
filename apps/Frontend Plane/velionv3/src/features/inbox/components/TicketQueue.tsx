import {
  Archive,
  ArrowDown,
  ArrowDownUp,
  ArrowUp,
  AtSign,
  Camera,
  Check,
  CheckCheck,
  Clock3,
  Gamepad2,
  Hash,
  Inbox,
  Mail,
  MailOpen,
  MessageCircle,
  MessagesSquare,
  MoreHorizontal,
  PanelLeft,
  Pin,
  SlidersHorizontal,
  Users,
  type LucideProps,
} from 'lucide-solid'
import { createEffect, createMemo, createSignal, For, onCleanup, Show, type Component, type JSX } from 'solid-js'
import { Dynamic } from 'solid-js/web'
import type { InboxModalRequest } from '@/features/inbox/components/InboxWorkModal'
import {
  formatRelativeTime,
  type InboxTab,
  type ZammadTicket,
} from '@/features/inbox/lib/inbox-model'
import type { ConnectedInboxSource } from '@/features/inbox/lib/inbox-sources'
import { cn } from '@/shared/lib/cn'
import { useI18n } from '@/shared/i18n'

type SortKey =
  | 'last-message-desc'
  | 'last-message-asc'
  | 'created-desc'
  | 'created-asc'
  | 'priority-desc'
  | 'priority-asc'

type FocusLane = 'focused' | 'other'
type QuickFilter = 'all' | 'unread' | 'mentions' | 'pinned'

export function ticketDisplayTitle(ticket: Pick<ZammadTicket, 'channel' | 'title'>) {
  if (ticket.channel !== 'email') return ticket.title
  return ticket.title.startsWith('Re:') ? ticket.title : `Re: ${ticket.title}`
}

export function ticketCustomerLabel(ticket: Pick<ZammadTicket, 'channel' | 'customer'>) {
  const contactName = ticket.customer
    ? `${ticket.customer.firstname} ${ticket.customer.lastname}`.trim() || ticket.customer.email || 'Unknown'
    : 'Unknown'
  if (ticket.channel !== 'email') return contactName
  return ticket.customer?.email ?? contactName
}

export function TicketQueue(props: {
  activeChannel: string | null
  activeTab: InboxTab
  connectedSources: ConnectedInboxSource[]
  error: string | null
  hasMore: boolean
  label: string
  loading: boolean
  loadingMore: boolean
  onActiveTabChange: (tab: InboxTab) => void
  onLoadMore: () => void
  onOpenModal: (modal: InboxModalRequest) => void
  onSearchChange: (query: string) => void
  onSelectTicket: (ticket: ZammadTicket) => void
  searchQuery: string
  selectedTicketId: number | null
  tickets: ZammadTicket[]
}) {
  let filtersRef: HTMLDivElement | undefined
  let sortRef: HTMLDivElement | undefined
  const [archivedIds, setArchivedIds] = createSignal(new Set<number>())
  const [filtersOpen, setFiltersOpen] = createSignal(false)
  const [focusLane, setFocusLane] = createSignal<FocusLane>('focused')
  const [pinnedIds, setPinnedIds] = createSignal(new Set<number>())
  const [quickFilter, setQuickFilter] = createSignal<QuickFilter>('all')
  const [readIds, setReadIds] = createSignal(new Set<number>())
  const [selectedIds, setSelectedIds] = createSignal(new Set<number>())
  const [snoozedIds, setSnoozedIds] = createSignal(new Set<number>())
  const [sortKey, setSortKey] = createSignal<SortKey>('last-message-desc')
  const [sortOpen, setSortOpen] = createSignal(false)
  const i18n = useI18n()

  const quickFilters = createMemo<Array<{ id: QuickFilter; label: string; icon?: Component<LucideProps> }>>(() => [
    { id: 'all', label: i18n.tr('Alle', 'All') },
    { id: 'unread', label: i18n.tr('Ulest', 'Unread'), icon: MailOpen },
    { id: 'mentions', label: i18n.tr('Omtaler', 'Mentions'), icon: AtSign },
    { id: 'pinned', label: i18n.tr('Festet', 'Pinned'), icon: Pin },
  ])

  const laneCounts = createMemo(() => ({
    focused: props.tickets.filter((ticket) => !archivedIds().has(ticket.id) && !snoozedIds().has(ticket.id) && isFocusedTicket(ticket)).length,
    other: props.tickets.filter((ticket) => !archivedIds().has(ticket.id) && !snoozedIds().has(ticket.id) && !isFocusedTicket(ticket)).length,
  }))

  const visibleTickets = createMemo(() => props.tickets.filter((ticket) => {
    if (archivedIds().has(ticket.id) || snoozedIds().has(ticket.id)) return false
    if (focusLane() === 'focused' && !isFocusedTicket(ticket)) return false
    if (focusLane() === 'other' && isFocusedTicket(ticket)) return false
    if (quickFilter() === 'unread' && readIds().has(ticket.id)) return false
    if (quickFilter() === 'mentions' && !isMentionedTicket(ticket)) return false
    if (quickFilter() === 'pinned' && !pinnedIds().has(ticket.id)) return false
    return true
  }))
  const sortedTickets = createMemo(() => sortTickets(visibleTickets(), sortKey(), pinnedIds()))
  const allVisibleSelected = createMemo(() => sortedTickets().length > 0 && sortedTickets().every((ticket) => selectedIds().has(ticket.id)))
  const selectedCount = createMemo(() => sortedTickets().filter((ticket) => selectedIds().has(ticket.id)).length)
  const activeSources = createMemo(() => props.connectedSources.filter((source) => source.channel === props.activeChannel))
  const connectedEmptyState = createMemo(() => {
    if (props.tickets.length > 0 || activeSources().length === 0) return null
    const labels = activeSources().map((source) => source.label)
    const sourceList = formatSourceList(labels, i18n)
    return {
      title: i18n.tr(`${sourceList} er tilkoblet`, `${sourceList} ${labels.length === 1 ? 'is' : 'are'} connected`),
      body: i18n.tr(
        'Ingen samtaler har kommet inn i denne kilden ennå. Nye meldinger vises her så snart innhentingen leverer dem.',
        'No conversations have arrived in this source yet. New messages will appear here as soon as ingestion delivers them.',
      ),
    }
  })

  createEffect(() => {
    const visibleIds = new Set(sortedTickets().map((ticket) => ticket.id))
    setSelectedIds((current) => retainSetValues(current, visibleIds))
  })

  createEffect(() => {
    if (!filtersOpen() && !sortOpen()) return

    const closeOnOutside = (event: MouseEvent) => {
      const target = event.target as Node
      if (filtersRef?.contains(target) || sortRef?.contains(target)) return
      setFiltersOpen(false)
      setSortOpen(false)
    }

    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        setFiltersOpen(false)
        setSortOpen(false)
      }
    }

    document.addEventListener('mousedown', closeOnOutside)
    document.addEventListener('keydown', closeOnEscape)
    onCleanup(() => {
      document.removeEventListener('mousedown', closeOnOutside)
      document.removeEventListener('keydown', closeOnEscape)
    })
  })

  const toggleAllVisible = () => {
    const ticketIds = sortedTickets().map((ticket) => ticket.id)
    setSelectedIds((current) => allVisibleSelected() ? removeManyFromSet(current, ticketIds) : addManyToSet(current, ticketIds))
  }

  return (
    <section class="velion-inbox-queue" aria-label={props.label}>
      <div class="velion-inbox-queue__topbar">
        <div class="velion-inbox-queue__title">
          <PanelLeft class="size-4" strokeWidth={2} />
          <Inbox class="size-4" strokeWidth={2} />
          <h1>{i18n.tr('Innboks', 'Inbox')}</h1>
        </div>

        <div class="velion-inbox-queue__tools">
          <div ref={filtersRef} class="velion-inbox-menu-anchor">
            <button
              type="button"
              onClick={() => {
                setFiltersOpen((open) => !open)
                setSortOpen(false)
              }}
              class={cn('velion-inbox-icon-action', filtersOpen() && 'velion-inbox-icon-action--active')}
              aria-expanded={filtersOpen()}
              aria-label={i18n.tr('Åpne innboksfiltre', 'Open inbox filters')}
              title={i18n.tr('Åpne innboksfiltre', 'Open inbox filters')}
            >
              <SlidersHorizontal class="size-4" strokeWidth={1.9} />
            </button>
            <Show when={filtersOpen()}>
              <FiltersMenu
                activeTab={props.activeTab}
                onActiveTabChange={props.onActiveTabChange}
                onClose={() => setFiltersOpen(false)}
                onOpenModal={props.onOpenModal}
                onSearchChange={props.onSearchChange}
                searchQuery={props.searchQuery}
              />
            </Show>
          </div>

          <div ref={sortRef} class="velion-inbox-menu-anchor">
            <button
              type="button"
              onClick={() => {
                setSortOpen((open) => !open)
                setFiltersOpen(false)
              }}
              class={cn('velion-inbox-icon-action', sortOpen() && 'velion-inbox-icon-action--active')}
              aria-expanded={sortOpen()}
              aria-label={i18n.tr('Sorter samtaler', 'Sort conversations')}
              title={i18n.tr('Sorter samtaler', 'Sort conversations')}
            >
              <ArrowDownUp class="size-4" strokeWidth={1.9} />
            </button>
            <Show when={sortOpen()}>
              <SortMenu
                activeSort={sortKey()}
                onSelect={(nextSort) => {
                  setSortKey(nextSort)
                  setSortOpen(false)
                }}
              />
            </Show>
          </div>
        </div>
      </div>

      <div class="velion-inbox-queue__lanes">
        <div class="velion-inbox-focus-switch">
          <FocusLaneButton active={focusLane() === 'focused'} count={laneCounts().focused} label={i18n.tr('Fokusert', 'Focused')} onClick={() => setFocusLane('focused')} />
          <FocusLaneButton active={focusLane() === 'other'} count={laneCounts().other} label={i18n.tr('Annet', 'Other')} onClick={() => setFocusLane('other')} />
        </div>
        <div class="velion-inbox-quick-filters">
          <For each={quickFilters()}>
            {(filter) => {
              const Icon = filter.icon
              const textFilter = filter.id === 'all'
              return (
                <button
                  type="button"
                  onClick={() => setQuickFilter(filter.id)}
                  class={cn(
                    'velion-inbox-quick-filter',
                    textFilter ? 'velion-inbox-quick-filter--text' : 'velion-inbox-quick-filter--icon',
                    quickFilter() === filter.id && 'velion-inbox-quick-filter--active',
                  )}
                  aria-pressed={quickFilter() === filter.id}
                  aria-label={i18n.tr(`Vis ${filter.label.toLowerCase()} samtaler`, `Show ${filter.label.toLowerCase()} conversations`)}
                  title={i18n.tr(`Vis ${filter.label.toLowerCase()} samtaler`, `Show ${filter.label.toLowerCase()} conversations`)}
                >
                  {Icon ? <Icon class="size-3.5" strokeWidth={1.9} /> : null}
                  <span classList={{ 'sr-only': !textFilter }}>{filter.label}</span>
                </button>
              )
            }}
          </For>
        </div>
      </div>

      <div class="velion-inbox-queue__bulk">
        <label>
          <input
            type="checkbox"
            checked={allVisibleSelected()}
            onChange={toggleAllVisible}
          />
          {selectedCount() ? i18n.tr(`${selectedCount()} valgt`, `${selectedCount()} selected`) : i18n.tr('Velg alle', 'Select all')}
        </label>
        <div class={cn('velion-inbox-bulk-actions', selectedCount() ? 'velion-inbox-bulk-actions--active' : '')}>
          <BulkActionButton disabled={!selectedCount()} label={i18n.tr('Merk valgte som lest', 'Mark selected as read')} onClick={() => setReadIds((current) => addManyToSet(current, selectedIds()))}>
            <CheckCheck class="size-3.5" />
          </BulkActionButton>
          <BulkActionButton
            disabled={!selectedCount()}
            label={i18n.tr('Utsett valgte', 'Snooze selected')}
            onClick={() => {
              setSnoozedIds((current) => addManyToSet(current, selectedIds()))
              setSelectedIds(new Set<number>())
            }}
          >
            <Clock3 class="size-3.5" />
          </BulkActionButton>
          <BulkActionButton
            disabled={!selectedCount()}
            label={i18n.tr('Arkiver valgte', 'Archive selected')}
            onClick={() => {
              setArchivedIds((current) => addManyToSet(current, selectedIds()))
              setSelectedIds(new Set<number>())
            }}
          >
            <Archive class="size-3.5" />
          </BulkActionButton>
        </div>
      </div>

      <div class="velion-inbox-queue__list">
        <Show when={props.loading}>
          <LoadingRows />
        </Show>
        <Show when={!props.loading && props.error}>
          <QueueEmptyState tone="error" title={i18n.tr('Support er ikke tilkoblet', 'Support is not connected')} body={props.error ?? ''} />
        </Show>
        <Show when={!props.loading && !props.error && sortedTickets().length === 0}>
          <QueueEmptyState
            title={connectedEmptyState()?.title ?? i18n.tr('Ingen elementer', 'No items')}
            body={connectedEmptyState()?.body ?? i18n.tr('Denne visningen er tom for nå. Endre filtre eller bytt felt for å se flere samtaler.', 'This view is clear for now. Change filters or switch lanes to see more conversations.')}
          />
        </Show>
        <Show when={!props.loading && !props.error && sortedTickets().length > 0}>
          <ul aria-label={i18n.tr('Saker', 'Tickets')} class="velion-inbox-ticket-list">
            <For each={sortedTickets()}>
              {(ticket) => (
                <li>
                  <TicketRow
                    active={props.selectedTicketId === ticket.id}
                    checked={selectedIds().has(ticket.id)}
                    pinned={pinnedIds().has(ticket.id)}
                    unread={!readIds().has(ticket.id)}
                    ticket={ticket}
                    onArchive={() => {
                      setArchivedIds((current) => addToSet(current, ticket.id))
                      setSelectedIds((current) => removeFromSet(current, ticket.id))
                    }}
                    onClick={() => {
                      setReadIds((current) => addToSet(current, ticket.id))
                      props.onSelectTicket(ticket)
                    }}
                    onSnooze={() => {
                      setSnoozedIds((current) => addToSet(current, ticket.id))
                      setSelectedIds((current) => removeFromSet(current, ticket.id))
                    }}
                    onTogglePinned={() => setPinnedIds((current) => toggleSetValue(current, ticket.id))}
                    onToggleSelected={() => setSelectedIds((current) => toggleSetValue(current, ticket.id))}
                  />
                </li>
              )}
            </For>
          </ul>
        </Show>
        <Show when={!props.loading && !props.error && props.hasMore}>
          <button
            type="button"
            class="velion-inbox-load-more"
            disabled={props.loadingMore}
            onClick={props.onLoadMore}
          >
            {props.loadingMore ? i18n.tr('Laster eldre samtaler …', 'Loading older conversations…') : i18n.tr('Last inn eldre samtaler', 'Load older conversations')}
          </button>
        </Show>
      </div>
    </section>
  )
}

function formatSourceList(labels: string[], i18n: ReturnType<typeof useI18n>): string {
  if (labels.length < 2) return labels[0] ?? i18n.tr('Denne kilden', 'This source')
  if (labels.length === 2) return labels.join(i18n.tr(' og ', ' and '))
  return `${labels.slice(0, -1).join(', ')}${i18n.tr(', og ', ', and ')}${labels.at(-1)}`
}

function FiltersMenu(props: {
  activeTab: InboxTab
  onActiveTabChange: (tab: InboxTab) => void
  onClose: () => void
  onOpenModal: (modal: InboxModalRequest) => void
  onSearchChange: (query: string) => void
  searchQuery: string
}) {
  const i18n = useI18n()
  const inboxTabs = createMemo<Array<{ id: InboxTab; label: string }>>(() => [
    { id: 'all', label: i18n.tr('Alle', 'All') },
    { id: 'open', label: i18n.tr('Åpen', 'Open') },
    { id: 'pending', label: i18n.tr('Venter', 'Pending') },
    { id: 'solved', label: i18n.tr('Løst', 'Solved') },
  ])
  const filterMenuItems = createMemo(() => [
    { label: i18n.tr('Din innboks', 'Your inbox'), href: '/inbox?view=mine', description: i18n.tr('Samtaler tildelt deg, holdt inne i det aktive innboks-arbeidsområdet.', 'Conversations assigned to you, kept inside the active inbox workspace.') },
    { label: i18n.tr('Alle samtaler', 'All conversations'), href: '/inbox?view=all', description: i18n.tr('En full team-kø-visning for å overvåke alle aktive samtaler.', 'A full team queue view for monitoring every active conversation.') },
    { label: i18n.tr('Ikke tildelt', 'Unassigned'), href: '/inbox?view=unassigned', description: i18n.tr('Saker som Velion eller en menneskelig operatør bør rute til en eier.', 'Tickets that Velion or a human operator should route to an owner.') },
    { label: i18n.tr('Omtaler', 'Mentions'), href: '/inbox?view=mentions', description: i18n.tr('Samtaletråder der en operatør eller AI-arbeidsflyt ble nevnt.', 'Conversation threads where an operator or AI workflow was mentioned.') },
    { label: 'Messenger', href: '/inbox?view=view-messenger', description: i18n.tr('Messenger-kanal-samtaler uten å forlate innboksflaten.', 'Messenger-channel conversations without leaving the inbox surface.') },
    { label: i18n.tr('E-post', 'Email'), href: '/inbox?view=view-email', description: i18n.tr('E-post-kanal-samtaler uten å åpne en egen side.', 'Email-channel conversations without opening a separate page.') },
  ])
  return (
    <div class="velion-popover velion-inbox-menu velion-inbox-menu--filters">
      <div class="velion-inbox-menu__label">Status</div>
      <div class="velion-inbox-menu__group">
        <For each={inboxTabs()}>
          {(tab) => (
            <button
              type="button"
              onClick={() => {
                props.onActiveTabChange(tab.id)
                props.onClose()
              }}
              class={cn('velion-inbox-menu__row', props.activeTab === tab.id && 'velion-inbox-menu__row--active')}
            >
              {tab.label}
              <Show when={props.activeTab === tab.id}>
                <Check class="size-3.5" />
              </Show>
            </button>
          )}
        </For>
      </div>
      <div class="velion-inbox-menu__divider" />
      <div class="velion-inbox-menu__label">{i18n.tr('Visninger', 'Views')}</div>
      <div class="velion-inbox-menu__group">
        <For each={filterMenuItems()}>
          {(item) => (
            <button
              type="button"
              onClick={() => {
                props.onOpenModal({
                  type: 'view',
                  title: item.label,
                  description: item.description,
                  sourceHref: item.href,
                })
                props.onClose()
              }}
              class="velion-inbox-menu__row"
            >
              {item.label}
            </button>
          )}
        </For>
      </div>
      <Show when={props.searchQuery}>
        <div class="velion-inbox-menu__divider" />
        <button
          type="button"
          onClick={() => {
            props.onSearchChange('')
            props.onClose()
          }}
          class="velion-inbox-menu__row velion-inbox-menu__row--full"
        >
          {i18n.tr('Fjern søk', 'Clear search')}
        </button>
      </Show>
    </div>
  )
}

function SortMenu(props: { activeSort: SortKey; onSelect: (sort: SortKey) => void }) {
  const i18n = useI18n()
  const sortOptions = createMemo<Array<{ id: SortKey; label: string; icon: Component<LucideProps> }>>(() => [
    { id: 'last-message-desc', label: i18n.tr('Siste melding', 'Last message'), icon: ArrowDown },
    { id: 'last-message-asc', label: i18n.tr('Siste melding', 'Last message'), icon: ArrowUp },
    { id: 'created-desc', label: i18n.tr('Opprettet', 'Created'), icon: ArrowDown },
    { id: 'created-asc', label: i18n.tr('Opprettet', 'Created'), icon: ArrowUp },
    { id: 'priority-desc', label: i18n.tr('Prioritet', 'Priority'), icon: ArrowDown },
    { id: 'priority-asc', label: i18n.tr('Prioritet', 'Priority'), icon: ArrowUp },
  ])
  return (
    <div class="velion-popover velion-inbox-menu velion-inbox-menu--sort">
      <For each={sortOptions()}>
        {(option) => {
          const Icon = option.icon
          return (
            <button type="button" onClick={() => props.onSelect(option.id)} class="velion-inbox-sort-row">
              <Icon class="size-4" />
              <span>{option.label}</span>
              <Show when={props.activeSort === option.id}>
                <Check class="size-4 velion-inbox-sort-row__check" />
              </Show>
            </button>
          )
        }}
      </For>
    </div>
  )
}

function LoadingRows() {
  return (
    <div class="velion-inbox-loading-rows">
      <For each={[1, 2, 3]}>
        {() => (
          <div class="velion-inbox-loading-row">
            <span />
            <div>
              <span />
              <span />
              <span />
            </div>
          </div>
        )}
      </For>
    </div>
  )
}

function QueueEmptyState(props: { body: string; title: string; tone?: 'neutral' | 'error' }) {
  return (
    <div class="velion-inbox-queue-empty">
      <div>
        <div class="velion-inbox-queue-empty__icon">
          <MessageCircle class="size-5" strokeWidth={1.45} />
        </div>
        <h2>{props.title}</h2>
        <p classList={{ 'velion-inbox-queue-empty__error': props.tone === 'error' }}>{props.body}</p>
      </div>
    </div>
  )
}

function TicketRow(props: {
  active: boolean
  checked: boolean
  pinned: boolean
  onClick: () => void
  onArchive: () => void
  onSnooze: () => void
  onTogglePinned: () => void
  onToggleSelected: () => void
  ticket: ZammadTicket
  unread: boolean
}) {
  const i18n = useI18n()
  return (
    <div class={cn('velion-inbox-ticket-row', props.active && 'velion-inbox-ticket-row--active')}>
      <input
        type="checkbox"
        checked={props.checked}
        onChange={() => props.onToggleSelected()}
        aria-label={i18n.tr(`Velg ${props.ticket.title}`, `Select ${props.ticket.title}`)}
      />
      <button
        type="button"
        aria-label={props.ticket.title}
        aria-pressed={props.active}
        onClick={() => props.onClick()}
        class="velion-inbox-ticket-row__main"
      >
        <div>
          <div class="velion-inbox-ticket-row__meta">
            <Show when={props.unread}>
              <span class="velion-inbox-ticket-row__unread" aria-label={i18n.tr('Ulest samtale', 'Unread conversation')} />
            </Show>
            <span class={cn('velion-inbox-ticket-row__customer', props.unread && 'velion-inbox-ticket-row__customer--unread')}>
              {ticketCustomerLabel(props.ticket)}
            </span>
            <Show when={props.pinned}>
              <Pin class="size-3 velion-inbox-ticket-row__pin" strokeWidth={1.8} />
            </Show>
            <ChannelMark channel={props.ticket.channel} provider={props.ticket.provider} />
            <span class="velion-inbox-ticket-row__more">
              <MoreHorizontal class="size-3" strokeWidth={1.8} />
            </span>
            <span>{i18n.tr(`for ${formatRelativeTime(props.ticket.updated_at)} siden`, `${formatRelativeTime(props.ticket.updated_at)} ago`)}</span>
          </div>
          <div class={cn('velion-inbox-ticket-row__subject', props.unread && 'velion-inbox-ticket-row__subject--unread')}>
            {ticketDisplayTitle(props.ticket)}
          </div>
          <div class="velion-inbox-ticket-row__preview">
            {ticketPreview(props.ticket, i18n)}
          </div>
        </div>
      </button>
      <div class="velion-inbox-ticket-row__actions">
        <RowActionButton label={props.pinned ? i18n.tr('Løsne samtale', 'Unpin conversation') : i18n.tr('Fest samtale', 'Pin conversation')} onClick={props.onTogglePinned}>
          <Pin class={cn('size-3.5', props.pinned && 'velion-inbox-ticket-row__pin')} />
        </RowActionButton>
        <RowActionButton label={i18n.tr('Utsett samtale', 'Snooze conversation')} onClick={props.onSnooze}>
          <Clock3 class="size-3.5" />
        </RowActionButton>
        <RowActionButton label={i18n.tr('Arkiver samtale', 'Archive conversation')} onClick={props.onArchive}>
          <Archive class="size-3.5" />
        </RowActionButton>
      </div>
    </div>
  )
}

function BulkActionButton(props: { children: JSX.Element; disabled?: boolean; label: string; onClick: () => void }) {
  return (
    <button
      type="button"
      disabled={props.disabled}
      onClick={() => props.onClick()}
      aria-label={props.label}
      title={props.label}
      class="velion-inbox-bulk-button"
    >
      {props.children}
    </button>
  )
}

function ChannelMark(props: { channel?: string; provider?: string }) {
  const i18n = useI18n()
  const config = createMemo(() => channelMarkConfig((props.provider || props.channel || 'email').toLowerCase(), i18n))

  return (
    <span
      class="velion-inbox-ticket-row__channel"
      aria-label={i18n.tr(`${config().label} samtale`, `${config().label} conversation`)}
      title={config().label}
    >
      <Dynamic component={config().icon} class="size-3" strokeWidth={1.7} />
      <span>{config().label}</span>
    </span>
  )
}

function channelMarkConfig(key: string, i18n: ReturnType<typeof useI18n>): { icon: Component<LucideProps>; label: string } {
  switch (key) {
    case 'google':
      return { icon: Mail, label: 'Gmail' }
    case 'microsoft':
    case 'outlook':
      return { icon: Mail, label: 'Outlook' }
    case 'slack':
      return { icon: Hash, label: 'Slack' }
    case 'teams':
      return { icon: Users, label: 'Teams' }
    case 'discord':
      return { icon: Gamepad2, label: 'Discord' }
    case 'whatsapp':
      return { icon: MessageCircle, label: 'WhatsApp' }
    case 'messenger':
      return { icon: MessagesSquare, label: 'Messenger' }
    case 'instagram':
      return { icon: Camera, label: 'Instagram' }
    case 'x':
    case 'twitter':
      return { icon: AtSign, label: 'X' }
    default:
      return { icon: Mail, label: key === 'email' ? i18n.tr('E-post', 'Email') : key }
  }
}

function RowActionButton(props: { children: JSX.Element; label: string; onClick: () => void }) {
  return (
    <button
      type="button"
      onClick={(event) => {
        event.stopPropagation()
        props.onClick()
      }}
      aria-label={props.label}
      title={props.label}
      class="velion-inbox-row-action"
    >
      {props.children}
    </button>
  )
}

function FocusLaneButton(props: { active: boolean; count: number; label: string; onClick: () => void }) {
  return (
    <button
      type="button"
      onClick={() => props.onClick()}
      class={cn('velion-inbox-focus-button', props.active && 'velion-inbox-focus-button--active')}
      aria-pressed={props.active}
    >
      <span>{props.label}</span>
      <small>{props.count}</small>
    </button>
  )
}

function addToSet(values: Set<number>, value: number) {
  if (values.has(value)) return values
  return new Set([...values, value])
}

function addManyToSet(values: Set<number>, nextValues: Iterable<number>) {
  const next = new Set(values)
  let changed = false

  for (const value of nextValues) {
    if (!next.has(value)) {
      next.add(value)
      changed = true
    }
  }

  return changed ? next : values
}

function removeFromSet(values: Set<number>, value: number) {
  if (!values.has(value)) return values
  const next = new Set(values)
  next.delete(value)
  return next
}

function removeManyFromSet(values: Set<number>, nextValues: Iterable<number>) {
  const next = new Set(values)
  let changed = false

  for (const value of nextValues) {
    if (next.delete(value)) changed = true
  }

  return changed ? next : values
}

function retainSetValues(values: Set<number>, allowedValues: Set<number>) {
  const next = new Set<number>()
  for (const value of values) {
    if (allowedValues.has(value)) next.add(value)
  }
  return next.size === values.size ? values : next
}

function toggleSetValue(values: Set<number>, value: number) {
  const next = new Set(values)
  if (next.has(value)) next.delete(value)
  else next.add(value)
  return next
}

function sortTickets(tickets: ZammadTicket[], sortKey: SortKey, pinnedIds: Set<number>) {
  const sorted = [...tickets]

  return sorted.sort((a, b) => {
    const pinnedDelta = Number(pinnedIds.has(b.id)) - Number(pinnedIds.has(a.id))
    if (pinnedDelta !== 0) return pinnedDelta
    if (sortKey === 'created-desc') return compareDates(b.created_at, a.created_at)
    if (sortKey === 'created-asc') return compareDates(a.created_at, b.created_at)
    if (sortKey === 'priority-desc') return priorityScore(b) - priorityScore(a)
    if (sortKey === 'priority-asc') return priorityScore(a) - priorityScore(b)
    if (sortKey === 'last-message-asc') return compareDates(a.updated_at, b.updated_at)
    return compareDates(b.updated_at, a.updated_at)
  })
}

function compareDates(a: string, b: string) {
  return new Date(a).getTime() - new Date(b).getTime()
}

function priorityScore(ticket: ZammadTicket) {
  const value = ticket.priority?.name?.toLowerCase() ?? ''
  if (value.includes('high') || value === '3') return 3
  if (value.includes('normal') || value === '2') return 2
  return 1
}

function isFocusedTicket(ticket: ZammadTicket) {
  const state = ticket.state?.name?.toLowerCase() ?? ''
  const priority = ticket.priority?.name?.toLowerCase() ?? ''
  if (state.includes('spam') || state.includes('closed') || state.includes('solved')) return false
  return priority.includes('high') || priority.includes('normal') || priority === '2' || priority === '3' || state.includes('open')
}

function isMentionedTicket(ticket: ZammadTicket) {
  const text = `${ticket.title} ${ticket.tags?.join(' ') ?? ''}`.toLowerCase()
  return text.includes('@') || text.includes('mention') || text.includes('urgent') || text.includes('vip')
}

function ticketPreview(ticket: ZammadTicket, i18n: ReturnType<typeof useI18n>) {
  return ticket.lastMessagePreview?.trim() || i18n.tr('Ingen forhåndsvisning tilgjengelig', 'No message preview available')
}
