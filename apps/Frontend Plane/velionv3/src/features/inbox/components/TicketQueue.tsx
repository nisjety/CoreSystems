import {
  Archive,
  ArrowDown,
  ArrowDownUp,
  ArrowUp,
  AtSign,
  Check,
  CheckCheck,
  Clock3,
  Inbox,
  Mail,
  MailOpen,
  MessageCircle,
  MoreHorizontal,
  PanelLeft,
  Pin,
  SlidersHorizontal,
  type LucideProps,
} from 'lucide-solid'
import { createEffect, createMemo, createSignal, For, onCleanup, Show, type Component, type JSX } from 'solid-js'
import type { InboxModalRequest } from '@/features/inbox/components/InboxWorkModal'
import {
  customerName,
  formatRelativeTime,
  type InboxTab,
  type ZammadTicket,
} from '@/features/inbox/lib/inbox-model'
import { cn } from '@/shared/lib/cn'

const inboxTabs: Array<{ id: InboxTab; label: string }> = [
  { id: 'all', label: 'All' },
  { id: 'open', label: 'Open' },
  { id: 'pending', label: 'Pending' },
  { id: 'solved', label: 'Solved' },
]

const filterMenuItems = [
  { label: 'Your inbox', href: '/inbox?view=mine', description: 'Conversations assigned to you, kept inside the active inbox workspace.' },
  { label: 'All conversations', href: '/inbox?view=all', description: 'A full team queue view for monitoring every active conversation.' },
  { label: 'Unassigned', href: '/inbox?view=unassigned', description: 'Tickets that Velion or a human operator should route to an owner.' },
  { label: 'Mentions', href: '/inbox?view=mentions', description: 'Conversation threads where an operator or AI workflow was mentioned.' },
  { label: 'Messenger', href: '/inbox?view=view-messenger', description: 'Messenger-channel conversations without leaving the inbox surface.' },
  { label: 'Email', href: '/inbox?view=view-email', description: 'Email-channel conversations without opening a separate page.' },
] as const

type SortKey =
  | 'last-message-desc'
  | 'last-message-asc'
  | 'created-desc'
  | 'created-asc'
  | 'priority-desc'
  | 'priority-asc'

const sortOptions: Array<{ id: SortKey; label: string; icon: Component<LucideProps> }> = [
  { id: 'last-message-desc', label: 'Last message', icon: ArrowDown },
  { id: 'last-message-asc', label: 'Last message', icon: ArrowUp },
  { id: 'created-desc', label: 'Created', icon: ArrowDown },
  { id: 'created-asc', label: 'Created', icon: ArrowUp },
  { id: 'priority-desc', label: 'Priority', icon: ArrowDown },
  { id: 'priority-asc', label: 'Priority', icon: ArrowUp },
]

type FocusLane = 'focused' | 'other'
type QuickFilter = 'all' | 'unread' | 'mentions' | 'pinned'

const quickFilters: Array<{ id: QuickFilter; label: string; icon?: Component<LucideProps> }> = [
  { id: 'all', label: 'All' },
  { id: 'unread', label: 'Unread', icon: MailOpen },
  { id: 'mentions', label: 'Mentions', icon: AtSign },
  { id: 'pinned', label: 'Pinned', icon: Pin },
]

export function TicketQueue(props: {
  activeTab: InboxTab
  error: string | null
  label: string
  loading: boolean
  onActiveTabChange: (tab: InboxTab) => void
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
          <h1>Inbox</h1>
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
              aria-label="Open inbox filters"
              title="Open inbox filters"
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
              aria-label="Sort conversations"
              title="Sort conversations"
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
          <FocusLaneButton active={focusLane() === 'focused'} count={laneCounts().focused} label="Focused" onClick={() => setFocusLane('focused')} />
          <FocusLaneButton active={focusLane() === 'other'} count={laneCounts().other} label="Other" onClick={() => setFocusLane('other')} />
        </div>
        <div class="velion-inbox-quick-filters">
          <For each={quickFilters}>
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
                  aria-label={`Show ${filter.label.toLowerCase()} conversations`}
                  title={`Show ${filter.label.toLowerCase()} conversations`}
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
          {selectedCount() ? `${selectedCount()} selected` : 'Select all'}
        </label>
        <div class={cn('velion-inbox-bulk-actions', selectedCount() ? 'velion-inbox-bulk-actions--active' : '')}>
          <BulkActionButton disabled={!selectedCount()} label="Mark selected as read" onClick={() => setReadIds((current) => addManyToSet(current, selectedIds()))}>
            <CheckCheck class="size-3.5" />
          </BulkActionButton>
          <BulkActionButton
            disabled={!selectedCount()}
            label="Snooze selected"
            onClick={() => {
              setSnoozedIds((current) => addManyToSet(current, selectedIds()))
              setSelectedIds(new Set<number>())
            }}
          >
            <Clock3 class="size-3.5" />
          </BulkActionButton>
          <BulkActionButton
            disabled={!selectedCount()}
            label="Archive selected"
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
          <QueueEmptyState tone="error" title="Support is not connected" body={props.error ?? ''} />
        </Show>
        <Show when={!props.loading && !props.error && sortedTickets().length === 0}>
          <QueueEmptyState title="No items" body="This view is clear for now. Change filters or switch lanes to see more conversations." />
        </Show>
        <Show when={!props.loading && !props.error && sortedTickets().length > 0}>
          <ul aria-label="Tickets" class="velion-inbox-ticket-list">
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
      </div>
    </section>
  )
}

function FiltersMenu(props: {
  activeTab: InboxTab
  onActiveTabChange: (tab: InboxTab) => void
  onClose: () => void
  onOpenModal: (modal: InboxModalRequest) => void
  onSearchChange: (query: string) => void
  searchQuery: string
}) {
  return (
    <div class="velion-popover velion-inbox-menu velion-inbox-menu--filters">
      <div class="velion-inbox-menu__label">Status</div>
      <div class="velion-inbox-menu__group">
        <For each={inboxTabs}>
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
      <div class="velion-inbox-menu__label">Views</div>
      <div class="velion-inbox-menu__group">
        <For each={filterMenuItems}>
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
          Clear search
        </button>
      </Show>
    </div>
  )
}

function SortMenu(props: { activeSort: SortKey; onSelect: (sort: SortKey) => void }) {
  return (
    <div class="velion-popover velion-inbox-menu velion-inbox-menu--sort">
      <For each={sortOptions}>
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
  return (
    <div class={cn('velion-inbox-ticket-row', props.active && 'velion-inbox-ticket-row--active')}>
      <input
        type="checkbox"
        checked={props.checked}
        onChange={() => props.onToggleSelected()}
        aria-label={`Select ${props.ticket.title}`}
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
              <span class="velion-inbox-ticket-row__unread" aria-label="Unread conversation" />
            </Show>
            <span class={cn('velion-inbox-ticket-row__customer', props.unread && 'velion-inbox-ticket-row__customer--unread')}>
              {props.ticket.customer?.email ?? customerName(props.ticket)}
            </span>
            <Show when={props.pinned}>
              <Pin class="size-3 velion-inbox-ticket-row__pin" strokeWidth={1.8} />
            </Show>
            <Mail class="size-3 velion-inbox-ticket-row__mail" strokeWidth={1.7} />
            <span class="velion-inbox-ticket-row__more">
              <MoreHorizontal class="size-3" strokeWidth={1.8} />
            </span>
            <span>{formatRelativeTime(props.ticket.updated_at)} ago</span>
          </div>
          <div class={cn('velion-inbox-ticket-row__subject', props.unread && 'velion-inbox-ticket-row__subject--unread')}>
            {props.ticket.title.startsWith('Re:') ? props.ticket.title : `Re: ${props.ticket.title}`}
          </div>
          <div class="velion-inbox-ticket-row__preview">
            {ticketPreview(props.ticket)}
          </div>
        </div>
      </button>
      <div class="velion-inbox-ticket-row__actions">
        <RowActionButton label={props.pinned ? 'Unpin conversation' : 'Pin conversation'} onClick={props.onTogglePinned}>
          <Pin class={cn('size-3.5', props.pinned && 'velion-inbox-ticket-row__pin')} />
        </RowActionButton>
        <RowActionButton label="Snooze conversation" onClick={props.onSnooze}>
          <Clock3 class="size-3.5" />
        </RowActionButton>
        <RowActionButton label="Archive conversation" onClick={props.onArchive}>
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

function ticketPreview(ticket: ZammadTicket) {
  if (ticket.tags?.length) {
    return `Regarding your ${ticket.tags.join(', ')} request, we are checking the details and will follow up shortly.`
  }

  if (ticket.group?.name) {
    return `Could you please confirm the status with ${ticket.group.name} and let me know when I can expect an update?`
  }

  return 'Could you please confirm the status and let me know when I can expect an update?'
}
