import { A } from '@solidjs/router'
import {
  Bell,
  ChevronLeft,
  ChevronRight,
  CreditCard,
  HelpCircle,
  LogOut,
  Mic,
  Plus,
  Settings,
  User,
  Users,
  type LucideProps,
} from 'lucide-solid'
import { createEffect, createMemo, createSignal, For, Match, Show, Switch, type Component, type JSX } from 'solid-js'
import { Dynamic } from 'solid-js/web'
import type { VelionRoute, WorkspaceIdentity } from '@/features/core/lib/shell-data'
import {
  createNavbarCalendarEvent,
  createNavbarCalendarNote,
  emptyCalendar,
  emptyNotifications,
  markNavbarNotificationRead,
  type CalendarEvent,
  type CalendarNote,
  type CalendarState,
  type NavbarNotification,
  type NavbarPayload,
} from '@/shared/api/navbar-client'
import { cn } from '@/shared/lib/cn'
import { shouldShowWorkspaceAdminNavigation } from '@/shared/session/access'
import { getSession } from '@/shared/session/session-store'

export type CoreNavbarPanelKind = 'assistant' | 'messages' | 'notifications' | 'calendar' | 'profile'

export function CoreNavbarPanel(props: {
  navbarData?: NavbarPayload | null
  onNavigate: (href: VelionRoute) => void
  onRefresh?: () => void
  onSignOut: () => void
  onSupport: () => void
  panel: CoreNavbarPanelKind
  workspace: WorkspaceIdentity
}) {
  const session = getSession()
  const notificationPayload = () => props.navbarData?.notifications ?? emptyNotifications
  const calendarState = () => props.navbarData?.calendar ?? emptyCalendar
  const handleOpenNotification = (id: string) => {
    const refresh = props.onRefresh
    void markNavbarNotificationRead(id)
      .then(() => refresh?.())
      .catch(() => undefined)
  }

  return (
    <Switch>
      <Match when={props.panel === 'assistant'}>
        <AssistantPanel onNavigate={props.onNavigate} />
      </Match>
      <Match when={props.panel === 'messages'}>
        <MessagesDropdown
          configured={notificationPayload().configured}
          messages={notificationPayload().messages}
          onOpen={handleOpenNotification}
        />
      </Match>
      <Match when={props.panel === 'notifications'}>
        <NotificationsDropdown
          configured={notificationPayload().configured}
          notifications={notificationPayload().notifications}
          onOpen={handleOpenNotification}
        />
      </Match>
      <Match when={props.panel === 'calendar'}>
        <CalendarDropdown
          state={calendarState()}
          onRefresh={props.onRefresh}
        />
      </Match>
      <Match when={props.panel === 'profile'}>
        <ProfileDropdown
          canManageWorkspace={shouldShowWorkspaceAdminNavigation(session)}
          planLabel={props.workspace.plan}
          profile={{
            email: props.navbarData?.profile?.email ?? props.workspace.userEmail,
            name: props.navbarData?.profile?.name ?? props.workspace.userName,
          }}
          onNavigate={props.onNavigate}
          onSignOut={props.onSignOut}
          onSupport={props.onSupport}
        />
      </Match>
    </Switch>
  )
}

function MessagesDropdown(props: {
  configured: boolean
  messages: NavbarNotification[]
  onOpen: (id: string) => void
}) {
  return (
    <Panel class="right-24 velion-floating-panel-md">
      <TabHeader tabs={['All', 'Messages', 'Mentions']} />
      <div class="max-h-[380px] overflow-y-auto">
        <Show when={props.configured} fallback={<EmptyPanel text="Connect Novu to show inbox and Velion AI chat messages." />}>
          <Show when={props.messages.length} fallback={<EmptyPanel text="No messages" />}>
            <For each={props.messages}>
              {(message, index) => (
                <MessageRow
                  item={message}
                  bordered={index() < props.messages.length - 1}
                  onOpen={props.onOpen}
                />
              )}
            </For>
          </Show>
        </Show>
      </div>
      <PanelFooter href="/inbox" label="View all messages" />
    </Panel>
  )
}

function NotificationsDropdown(props: {
  configured: boolean
  notifications: NavbarNotification[]
  onOpen: (id: string) => void
}) {
  return (
    <Panel class="right-14 velion-floating-panel-md">
      <TabHeader tabs={['All', 'Systems', 'Unread']} />
      <div class="max-h-[380px] overflow-y-auto">
        <Show when={props.configured} fallback={<EmptyPanel text="Connect Novu to show real notifications." />}>
          <Show when={props.notifications.length} fallback={<EmptyPanel text="No notifications" />}>
            <For each={props.notifications}>
              {(notification, index) => (
                <NotificationRow
                  item={notification}
                  bordered={index() < props.notifications.length - 1}
                  onOpen={props.onOpen}
                />
              )}
            </For>
          </Show>
        </Show>
      </div>
    </Panel>
  )
}

function CalendarDropdown(props: {
  onRefresh?: () => void
  state: CalendarState
}) {
  const [activeTab, setActiveTab] = createSignal<'calendar' | 'notes'>('calendar')
  const [calendar, setCalendar] = createSignal<CalendarState>(emptyCalendar)
  const [error, setError] = createSignal<string | null>(null)
  const [eventTitle, setEventTitle] = createSignal('')
  const [noteText, setNoteText] = createSignal('')
  const [selectedDate, setSelectedDate] = createSignal(new Date())
  const selectedKey = () => formatDateKey(selectedDate())
  const selectedEvents = createMemo(() => calendar().events.filter((event) => formatDateKey(new Date(event.start)) === selectedKey()))
  const selectedNotes = createMemo(() => calendar().notes.filter((note) => note.date === selectedKey()))

  createEffect(() => {
    setCalendar(props.state)
  })

  const saveEvent = async () => {
    const title = eventTitle().trim()
    if (!title) return

    try {
      const start = new Date(selectedDate())
      start.setHours(9, 0, 0, 0)
      const end = new Date(start)
      end.setHours(9, 30, 0, 0)
      const saved = await createNavbarCalendarEvent({
        end: end.toISOString(),
        start: start.toISOString(),
        title,
        type: 'event',
      })
      setCalendar((current) => ({ ...current, events: [saved.event, ...current.events] }))
      setEventTitle('')
      setError(null)
      props.onRefresh?.()
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : 'Calendar event could not be saved.')
    }
  }

  const saveNote = async () => {
    const text = noteText().trim()
    if (!text) return

    try {
      const saved = await createNavbarCalendarNote({
        date: selectedKey(),
        kind: 'note',
        text,
      })
      setCalendar((current) => ({ ...current, notes: [saved.note, ...current.notes] }))
      setNoteText('')
      setError(null)
      props.onRefresh?.()
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : 'Calendar note could not be saved.')
    }
  }

  return (
    <Panel class="right-6 velion-floating-panel-md">
      <div class="core-calendar-panel__tabs">
        <For each={['calendar', 'notes'] as const}>
          {(tab) => (
            <button
              type="button"
              onClick={() => setActiveTab(tab)}
              classList={{ 'core-calendar-panel__tab--active': activeTab() === tab }}
            >
              {tab}
            </button>
          )}
        </For>
      </div>

      <Show
        when={activeTab() === 'calendar'}
        fallback={
          <CalendarNotes
            noteText={noteText()}
            notes={selectedNotes()}
            selectedDate={selectedDate()}
            onNoteTextChange={setNoteText}
            onSaveNote={() => void saveNote()}
          />
        }
      >
        <div class="core-calendar-panel">
          <div class="core-calendar-panel__header">
            <h3>{selectedDate().toLocaleDateString('en-US', { month: 'short', year: 'numeric' })}</h3>
            <div>
              <MiniIcon label="Previous" onClick={() => setSelectedDate((current) => shiftDate(current, -7))}>
                <ChevronLeft class="size-4" />
              </MiniIcon>
              <MiniIcon label="Next" onClick={() => setSelectedDate((current) => shiftDate(current, 7))}>
                <ChevronRight class="size-4" />
              </MiniIcon>
            </div>
          </div>

          <CalendarGrid
            events={calendar().events}
            selectedDate={selectedDate()}
            onSelect={setSelectedDate}
          />

          <div class="core-calendar-panel__events">
            <p>{selectedDate().toLocaleDateString('en-US', { weekday: 'short', month: 'short', day: 'numeric' })}</p>
            <Show when={selectedEvents().length} fallback={<span>No events for this day</span>}>
              <For each={selectedEvents()}>
                {(event) => <CalendarEventRow event={event} />}
              </For>
            </Show>
            <div class="core-calendar-panel__add">
              <button type="button" aria-label="Add calendar event" onClick={() => void saveEvent()}>
                <Plus class="size-3.5" />
              </button>
              <input
                placeholder="Add event..."
                aria-label="Calendar event title"
                value={eventTitle()}
                onInput={(event) => setEventTitle(event.currentTarget.value)}
                onKeyDown={(event) => {
                  if (event.key === 'Enter') void saveEvent()
                }}
              />
            </div>
          </div>
        </div>
      </Show>

      <Show when={error()}>
        {(message) => <p class="core-calendar-panel__error">{message()}</p>}
      </Show>
    </Panel>
  )
}

function CalendarNotes(props: {
  noteText: string
  notes: CalendarNote[]
  onNoteTextChange: (value: string) => void
  onSaveNote: () => void
  selectedDate: Date
}) {
  return (
    <div class="core-calendar-panel">
      <p class="core-calendar-panel__date-label">{props.selectedDate.toLocaleDateString('en-US', { month: 'short', day: 'numeric' })} ·</p>
      <h3 class="core-calendar-panel__today">Today</h3>
      <div class="core-calendar-panel__notes">
        <Show when={props.notes.length} fallback={<EmptyPanel text="No notes for this day" />}>
          <For each={props.notes}>
            {(note) => (
              <div>
                <span>{formatEventTime(new Date(note.createdAt))}</span>
                <p>{note.text}</p>
              </div>
            )}
          </For>
        </Show>
      </div>
      <div class="core-calendar-panel__add">
        <button type="button" aria-label="Add calendar note" onClick={() => props.onSaveNote()}>
          <Plus class="size-3.5" />
        </button>
        <input
          placeholder="Start typing..."
          aria-label="Calendar note text"
          value={props.noteText}
          onInput={(event) => props.onNoteTextChange(event.currentTarget.value)}
          onKeyDown={(event) => {
            if (event.key === 'Enter') props.onSaveNote()
          }}
        />
        <Mic class="size-4" />
      </div>
    </div>
  )
}

function ProfileDropdown(props: {
  canManageWorkspace: boolean
  onNavigate: (href: VelionRoute) => void
  onSignOut: () => void
  onSupport: () => void
  planLabel?: string
  profile: { email?: string | null; name?: string | null }
}) {
  return (
    <Panel class="right-0 velion-floating-panel-sm p-2">
      <div class="core-profile-card">
        <div>{props.profile.name ?? 'Account'}</div>
        <Show when={props.profile.email}>
          {(email) => <div>{email()}</div>}
        </Show>
      </div>
      <ProfileItem href="/account" icon={User} label="Profile" onNavigate={props.onNavigate} />
      <Show when={props.canManageWorkspace}>
        <ProfileItem href="/settings/members" icon={Users} label="Community" onNavigate={props.onNavigate} />
        <ProfileItem href="/settings/billing" icon={CreditCard} label="Subscription" badge={props.planLabel} onNavigate={props.onNavigate} />
        <ProfileItem href="/settings/workspace" icon={Settings} label="Settings" onNavigate={props.onNavigate} />
      </Show>
      <div class="core-menu-divider" />
      <button type="button" onClick={() => props.onSupport()} class="velion-menu-item">
        <HelpCircle class="size-[17px]" strokeWidth={1.7} />
        <span>Help center</span>
      </button>
      <button type="button" class="velion-menu-item" onClick={() => props.onSignOut()}>
        <LogOut class="size-[17px]" strokeWidth={1.7} />
        <span>Sign out</span>
      </button>
    </Panel>
  )
}

function AssistantPanel(props: { onNavigate: (href: VelionRoute) => void }) {
  return (
    <Panel class="right-32 velion-floating-panel-sm p-3">
      <div class="core-assistant-panel">
        <strong>AI assistant</strong>
        <p>Page-aware help for current workspace actions.</p>
        <button type="button" onClick={() => props.onNavigate('/chat')}>
          Open chat
        </button>
      </div>
    </Panel>
  )
}

function NotificationRow(props: {
  bordered: boolean
  item: NavbarNotification
  onOpen: (id: string) => void
}) {
  return (
    <A
      href={props.item.href ?? '/inbox'}
      onClick={() => props.onOpen(props.item.id)}
      class={cn('core-notification-row', props.bordered ? 'core-notification-row--bordered' : '')}
    >
      <div>
        <span class="core-notification-row__icon">
          N
          <span><Bell class="size-2" strokeWidth={2.5} /></span>
        </span>
        <div>
          <div>
            <strong>{props.item.title}</strong>
            <Show when={!props.item.read}>
              <span class="core-unread-dot" />
            </Show>
          </div>
          <p>{props.item.body}</p>
          <Show when={props.item.createdAt}>
            {(createdAt) => <small>{formatRelativeTime(createdAt())}</small>}
          </Show>
        </div>
      </div>
    </A>
  )
}

function MessageRow(props: {
  bordered: boolean
  item: NavbarNotification
  onOpen: (id: string) => void
}) {
  return (
    <A
      href={props.item.href ?? '/inbox'}
      onClick={() => props.onOpen(props.item.id)}
      class={cn('core-message-row', props.bordered ? 'core-notification-row--bordered' : '')}
    >
      <div>
        <span>{props.item.title.charAt(0).toUpperCase()}</span>
        <div>
          <div>
            <strong>{props.item.title}</strong>
            <Show when={props.item.createdAt}>
              {(createdAt) => <small>{formatRelativeTime(createdAt())}</small>}
            </Show>
          </div>
          <p>{props.item.body}</p>
        </div>
        <Show when={!props.item.read}>
          <span class="core-unread-dot" />
        </Show>
      </div>
    </A>
  )
}

function CalendarGrid(props: {
  events: CalendarEvent[]
  onSelect: (date: Date) => void
  selectedDate: Date
}) {
  const days = () => buildCalendarDays(props.selectedDate)
  const eventDates = () => new Set(props.events.map((event) => formatDateKey(new Date(event.start))))

  return (
    <div class="core-calendar-grid">
      <div>
        <For each={['S', 'M', 'T', 'W', 'T', 'F', 'S']}>
          {(day) => <span>{day}</span>}
        </For>
      </div>
      <div>
        <For each={days()}>
          {(day) => {
            const dayKey = () => formatDateKey(day)
            return (
              <button
                type="button"
                onClick={() => props.onSelect(day)}
                classList={{ 'core-calendar-grid__day--selected': sameDay(day, props.selectedDate) }}
              >
                {day.getDate()}
                <span classList={{ 'core-calendar-grid__event-dot': eventDates().has(dayKey()) }} />
              </button>
            )
          }}
        </For>
      </div>
    </div>
  )
}

function CalendarEventRow(props: { event: CalendarEvent }) {
  return (
    <div class="core-calendar-event-row">
      <span />
      <div>
        <p>{props.event.title}</p>
        <small>{formatEventRange(props.event)}</small>
      </div>
    </div>
  )
}

function ProfileItem(props: {
  badge?: string
  href: VelionRoute
  icon: Component<LucideProps>
  label: string
  onNavigate: (href: VelionRoute) => void
}) {
  return (
    <button type="button" onClick={() => props.onNavigate(props.href)} class="velion-menu-item">
      <Dynamic component={props.icon} class="size-[17px]" strokeWidth={1.7} />
      <span>{props.label}</span>
      <Show when={props.badge}>
        {(badge) => <em>{badge()}</em>}
      </Show>
    </button>
  )
}

function Panel(props: { children: JSX.Element; class?: string }) {
  return <div class={cn('velion-popover velion-floating-panel core-navbar-panel', props.class)}>{props.children}</div>
}

function TabHeader(props: { tabs: string[] }) {
  return (
    <div class="core-navbar-panel-tabs">
      <For each={props.tabs}>
        {(tab, index) => (
          <button type="button" classList={{ 'core-navbar-panel-tabs__tab--active': index() === 0 }}>
            {tab}
            <Show when={index() === 0}>
              <span />
            </Show>
          </button>
        )}
      </For>
    </div>
  )
}

function EmptyPanel(props: { text: string }) {
  return (
    <div class="core-empty-panel">
      <p>{props.text}</p>
    </div>
  )
}

function PanelFooter(props: { href: VelionRoute; label: string }) {
  return (
    <div class="core-panel-footer">
      <A href={props.href}>{props.label}</A>
    </div>
  )
}

function MiniIcon(props: { children: JSX.Element; label: string; onClick: () => void }) {
  return (
    <button type="button" aria-label={props.label} onClick={() => props.onClick()} class="core-calendar-panel__mini-icon">
      {props.children}
    </button>
  )
}

function shiftDate(date: Date, days: number) {
  const next = new Date(date)
  next.setDate(next.getDate() + days)
  return next
}

function buildCalendarDays(anchor: Date) {
  const weekStart = shiftDate(anchor, -anchor.getDay())
  return Array.from({ length: 7 }, (_unused, index) => shiftDate(weekStart, index))
}

function sameDay(first: Date, second: Date) {
  return first.getFullYear() === second.getFullYear() &&
    first.getMonth() === second.getMonth() &&
    first.getDate() === second.getDate()
}

function formatDateKey(date: Date) {
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}-${String(date.getDate()).padStart(2, '0')}`
}

function formatEventRange(event: CalendarEvent) {
  const start = new Date(event.start)
  const end = new Date(event.end)
  return `${formatEventTime(start)} - ${formatEventTime(end)}`
}

function formatEventTime(date: Date) {
  return date.toLocaleTimeString('en-US', { hour: 'numeric', minute: '2-digit' })
}

function formatRelativeTime(value: string) {
  const deltaSeconds = Math.max(0, Math.floor((Date.now() - new Date(value).getTime()) / 1000))
  if (deltaSeconds < 60) return 'now'
  if (deltaSeconds < 3600) return `${Math.floor(deltaSeconds / 60)}m ago`
  if (deltaSeconds < 86_400) return `${Math.floor(deltaSeconds / 3600)}h ago`
  return `${Math.floor(deltaSeconds / 86_400)}d ago`
}
