import { A, useLocation } from '@solidjs/router'
import {
  ChevronDown,
  ChevronRight,
  CircleUserRound,
  MessageSquarePlus,
  PanelLeftClose,
  PanelLeftOpen,
  Trash2,
} from 'lucide-solid'
import { createEffect, createMemo, createSignal, For, Match, onCleanup, onMount, Show, Switch, type JSX } from 'solid-js'
import { Dynamic } from 'solid-js/web'
import {
  CHAT_ACTIVE_THREAD_CHANGED_EVENT,
  CHAT_ACTIVE_THREAD_KEY,
  CHAT_THREAD_HISTORY_CHANGED_EVENT,
  CHAT_THREAD_HISTORY_KEY,
  clearActiveChatThreadId,
  clearChatThreadHistory,
  readActiveChatThreadId,
  readChatThreadTranscript,
  readChatThreadHistory,
  replaceChatThreadHistory,
  selectChatThread,
  type ChatThreadHistoryItem,
} from '@/features/chat/lib/chat-thread-history'
import { AgentsExpandedSidebarPanel } from '@/features/core/components/sidebar/CoreSidebarAgentsPanel'
import { InboxExpandedSidebarPanel } from '@/features/core/components/sidebar/CoreSidebarInboxPanel'
import { KnowledgeExpandedSidebarPanel } from '@/features/core/components/sidebar/CoreSidebarKnowledgePanel'
import {
  SidebarPanelTitle,
  SidebarSearchField,
} from '@/features/core/components/sidebar/CoreSidebarPrimitives'
import {
  AccountExpandedSidebarPanel,
  SettingsExpandedSidebarPanel,
} from '@/features/core/components/sidebar/CoreSidebarSettingsPanels'
import { TicketingExpandedSidebarPanel } from '@/features/core/components/sidebar/CoreSidebarTicketingPanel'
import type { VelionRoute } from '@/features/core/lib/shell-data'
import {
  getSidebarSectionForPath,
  isSidebarPathActive,
  sidebarSearchAction,
  sidebarSections,
  type SidebarPanelGroup,
  type SidebarPanelItem,
  type SidebarSection,
} from '@/features/core/lib/sidebar-navigation'
import { clearChatThreads, listChatThreads, saveChatThreadSnapshot } from '@/shared/api/chat-client'
import { useI18n } from '@/shared/i18n'
import { cn } from '@/shared/lib/cn'
import { shouldShowWorkspaceAdminNavigation } from '@/shared/session/access'
import { getSession } from '@/shared/session/session-store'

export const SIDEBAR_MINIMIZED_WIDTH = 60
export const SIDEBAR_EXPANDED_WIDTH = 320

export function CoreSidebar(props: {
  activeRoute: VelionRoute
  expanded: boolean
  expandedWidth?: number
  expansionLocked?: boolean
  onExpandedChange: (expanded: boolean) => void
  onOpenSearch: () => void
}) {
  const i18n = useI18n()
  const location = useLocation()
  const session = getSession()
  const visibleSections = () => sidebarSections
    .filter((section) => section.id !== 'settings' || shouldShowWorkspaceAdminNavigation(session))
    .map((section) => localizeSidebarSection(section, i18n))
  const activeSection = () => getSidebarSectionForPath(location.pathname, props.activeRoute, visibleSections())
  const mainSections = () => visibleSections().filter((section) => !section.pinnedBottom)
  const pinnedSections = () => visibleSections().filter((section) => section.pinnedBottom)
  const accountActive = () => location.pathname === '/account' || location.pathname.startsWith('/account/')
  const width = () => `${props.expanded ? (props.expandedWidth ?? SIDEBAR_EXPANDED_WIDTH) : SIDEBAR_MINIMIZED_WIDTH}px`

  const openSection = () => props.onExpandedChange(true)

  return (
    <aside class="velion-sidebar-themed core-sidebar velion-sidebar-type" style={{ width: width() }} aria-label={i18n.tr('Primærnavigasjon', 'Primary navigation')}>
      <div class="core-sidebar__body">
        <div class="core-sidebar__rail">
          <div class="core-sidebar__top-actions">
            <Show when={!props.expansionLocked}>
              <MiniActionButton
                label={props.expanded ? i18n.tr('Slå sammen sidefelt', 'Collapse sidebar') : i18n.tr('Utvid sidefelt', 'Expand sidebar')}
                active={false}
                onClick={() => props.onExpandedChange(!props.expanded)}
              >
                <Show when={props.expanded} fallback={<PanelLeftOpen class="size-[18px]" strokeWidth={1.75} />}>
                  <PanelLeftClose class="size-[18px]" strokeWidth={1.75} />
                </Show>
              </MiniActionButton>
            </Show>
          </div>

          <nav class="core-sidebar__mini-nav" aria-label={i18n.tr('Arbeidsområdeseksjoner', 'Workspace sections')}>
            <div class="core-sidebar__mini-stack">
              <For each={mainSections()}>
                {(section) => (
                  <MiniSectionLink
                    section={section}
                    active={!accountActive() && activeSection().id === section.id}
                    onOpen={openSection}
                  />
                )}
              </For>
            </div>
            <div class="core-sidebar__spacer" />
          </nav>

          <div class="core-sidebar__bottom-actions">
            <div class="core-sidebar__mini-divider" />
            <MiniAccountLink active={accountActive()} i18n={i18n} onOpen={openSection}>
              <CircleUserRound class="size-[18px]" strokeWidth={1.65} />
            </MiniAccountLink>
            <For each={pinnedSections()}>
              {(section) => (
                <MiniSectionLink
                  section={section}
                  active={!accountActive() && activeSection().id === section.id}
                  onOpen={openSection}
                />
              )}
            </For>
            <MiniActionButton
              label={i18n.tr('Søk', 'Search')}
              active={false}
              onClick={() => {
                props.onOpenSearch()
                props.onExpandedChange(true)
              }}
            >
              <sidebarSearchAction.icon class="size-[18px]" strokeWidth={1.65} />
            </MiniActionButton>
          </div>
        </div>

        <Show keyed when={props.expanded ? activeSection() : null}>
          {(section) => (
            <>
              <div aria-hidden="true" class="core-sidebar__splitter" />
              <ExpandedSidebarPanel
                activeSection={section}
                pathname={location.pathname}
                onCollapse={() => props.onExpandedChange(false)}
              />
            </>
          )}
        </Show>
      </div>
    </aside>
  )
}

function ExpandedSidebarPanel(props: {
  activeSection: SidebarSection
  pathname: string
  onCollapse: () => void
}) {
  const [searchQuery, setSearchQuery] = createSignal('')
  const [activeTabId, setActiveTabId] = createSignal<string | null>(null)
  const panelTabs = () => props.activeSection.panelTabs ?? []
  const firstPanelTabId = () => panelTabs()[0]?.id ?? null
  const panelKind = createMemo(() => {
    if (props.activeSection.id === 'messages') return 'messages'
    if (props.activeSection.id === 'inbox') return 'inbox'
    if (props.activeSection.id === 'ticketing') return 'ticketing'
    if (props.activeSection.id === 'agents') return 'agents'
    if (props.activeSection.id === 'knowledge') return 'knowledge'
    if (props.pathname === '/account' || props.pathname.startsWith('/account/')) return 'account'
    if (props.activeSection.id === 'settings') return 'settings'
    return 'generic'
  })

  createEffect(() => {
    setActiveTabId(firstPanelTabId())
    setSearchQuery('')
  })

  return (
    <Switch
      fallback={
        <GenericSidebarPanel
          activeSection={props.activeSection}
          activeTabId={activeTabId()}
          onActiveTabChange={setActiveTabId}
          onCollapse={props.onCollapse}
          pathname={props.pathname}
          panelTabs={panelTabs()}
          searchQuery={searchQuery()}
          onSearchQueryChange={setSearchQuery}
        />
      }
    >
      <Match when={panelKind() === 'messages'}>
        <ChatSidebarPanel onCollapse={() => props.onCollapse()} />
      </Match>
      <Match when={panelKind() === 'inbox'}>
        <InboxExpandedSidebarPanel onCollapse={props.onCollapse} />
      </Match>
      <Match when={panelKind() === 'ticketing'}>
        <TicketingExpandedSidebarPanel onCollapse={props.onCollapse} />
      </Match>
      <Match when={panelKind() === 'agents'}>
        <AgentsExpandedSidebarPanel onCollapse={props.onCollapse} />
      </Match>
      <Match when={panelKind() === 'knowledge'}>
        <KnowledgeExpandedSidebarPanel onCollapse={props.onCollapse} />
      </Match>
      <Match when={panelKind() === 'account'}>
        <AccountExpandedSidebarPanel onCollapse={props.onCollapse} />
      </Match>
      <Match when={panelKind() === 'settings'}>
        <SettingsExpandedSidebarPanel onCollapse={props.onCollapse} />
      </Match>
    </Switch>
  )
}

function GenericSidebarPanel(props: {
  activeSection: SidebarSection
  activeTabId: string | null
  onActiveTabChange: (tabId: string) => void
  onCollapse: () => void
  onSearchQueryChange: (value: string) => void
  panelTabs: NonNullable<SidebarSection['panelTabs']>
  pathname: string
  searchQuery: string
}) {
  const i18n = useI18n()
  return (
    <div class="core-sidebar-panel">
      <SidebarPanelTitle onCollapse={props.onCollapse}>{props.activeSection.label}</SidebarPanelTitle>
      <SidebarSearchField
        ariaLabel={i18n.tr('Filtrer sidefeltseksjon', 'Filter sidebar section')}
        placeholder={i18n.tr('Filtrer denne seksjonen', 'Filter this section')}
        value={props.searchQuery}
        onChange={props.onSearchQueryChange}
      />

      <Show when={props.panelTabs.length}>
        <div class="core-sidebar-tabs velion-sidebar-row-strong">
          <For each={props.panelTabs}>
            {(tab) => (
              <button
                type="button"
                onClick={() => props.onActiveTabChange(tab.id)}
                title={tab.label}
                classList={{ 'velion-sidebar-tab-active core-sidebar-tabs__tab--active': props.activeTabId === tab.id }}
              >
                {tab.label}
              </button>
            )}
          </For>
        </div>
      </Show>

      <SidebarPanelNavigation
        section={props.activeSection}
        pathname={props.pathname}
        activeTabId={props.activeTabId}
        searchQuery={props.searchQuery}
      />
    </div>
  )
}

function ChatSidebarPanel(props: { onCollapse: () => void }) {
  const i18n = useI18n()
  const [error, setError] = createSignal<string | null>(null)
  const [loading, setLoading] = createSignal(false)
  const [sessions, setSessions] = createSignal<ChatThreadHistoryItem[]>([])
  const [activeThreadId, setActiveThreadId] = createSignal<string | null>(null)

  const refreshLocalSessions = () => {
    setActiveThreadId(readActiveChatThreadId())
    setSessions(readChatThreadHistory())
  }

  const migrateLocalSessions = async (
    localSessions: ChatThreadHistoryItem[],
    serverSessions: ChatThreadHistoryItem[],
  ) => {
    const serverThreadIds = new Set(serverSessions.map((item) => item.threadId))
    const unsynced = localSessions.filter((item) => !serverThreadIds.has(item.threadId))
    if (unsynced.length === 0) return false

    const results = await Promise.allSettled(unsynced.map((item) => {
      const transcript = readChatThreadTranscript(item.threadId)
      return saveChatThreadSnapshot(item.threadId, {
        title: item.title,
        preview: item.preview,
        updatedAt: transcript?.updatedAt ?? item.updatedAt,
        turns: transcript?.turns,
        taskSteps: transcript?.taskSteps,
      })
    }))

    return results.some((result) => result.status === 'fulfilled')
  }

  const refreshServerSessions = async () => {
    const localSessions = readChatThreadHistory()
    setLoading(localSessions.length === 0)
    setError(null)
    try {
      const serverSessions = await listChatThreads()
      setActiveThreadId(readActiveChatThreadId())
      setSessions(replaceChatThreadHistory([...serverSessions, ...localSessions]))

      if (await migrateLocalSessions(localSessions, serverSessions)) {
        const refreshedSessions = await listChatThreads().catch(() => serverSessions)
        setSessions(replaceChatThreadHistory([...refreshedSessions, ...localSessions]))
      }
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : i18n.tr('Kunne ikke laste lagrede samtaler.', 'Could not load saved conversations.'))
      refreshLocalSessions()
    } finally {
      setLoading(false)
    }
  }

  onMount(() => {
    refreshLocalSessions()
    void refreshServerSessions()

    const handleHistoryChange = () => {
      refreshLocalSessions()
    }
    const handleStorageChange = (event: StorageEvent) => {
      if (
        event.key === CHAT_ACTIVE_THREAD_KEY ||
        event.key === CHAT_THREAD_HISTORY_KEY ||
        event.key === null
      ) {
        refreshLocalSessions()
      }
    }

    window.addEventListener(CHAT_ACTIVE_THREAD_CHANGED_EVENT, handleHistoryChange)
    window.addEventListener(CHAT_THREAD_HISTORY_CHANGED_EVENT, handleHistoryChange)
    window.addEventListener('storage', handleStorageChange)
    onCleanup(() => {
      window.removeEventListener(CHAT_ACTIVE_THREAD_CHANGED_EVENT, handleHistoryChange)
      window.removeEventListener(CHAT_THREAD_HISTORY_CHANGED_EVENT, handleHistoryChange)
      window.removeEventListener('storage', handleStorageChange)
    })
  })

  const clearHistory = () => {
    clearChatThreadHistory()
    clearActiveChatThreadId()
    setSessions([])
    setActiveThreadId(null)
    setError(null)
    void clearChatThreads().catch(() => undefined)
    props.onCollapse()
  }

  const openThread = (threadId: string) => {
    selectChatThread(threadId)
    setActiveThreadId(threadId)
  }

  const startNewChat = () => {
    clearActiveChatThreadId()
    setActiveThreadId(null)
    setError(null)
  }

  const sessionCount = () => sessions().length

  return (
    <div class="core-chat-sidebar">
      <A href="/chat" class="core-chat-sidebar__new" onClick={startNewChat}>
        <span>
          <MessageSquarePlus class="size-[15px]" strokeWidth={1.75} />
        </span>
        Ny samtale
      </A>

      <nav class="core-chat-sidebar__sessions" aria-label={i18n.tr('Chat-samtaler', 'Chat conversations')}>
        <Show when={!loading()} fallback={<div class="core-sidebar-empty velion-sidebar-row-normal">{i18n.tr('Laster samtaler ...', 'Loading conversations ...')}</div>}>
          <Show
            when={sessionCount() > 0}
            fallback={<div class="core-sidebar-empty velion-sidebar-row-normal">{error() ?? i18n.tr('Åpne chat for å laste ekte samtalehistorikk.', 'Open chat to load real conversation history.')}</div>}
          >
            <For each={sessions()}>
              {(item) => (
                <button
                  type="button"
                  class="core-chat-session"
                  classList={{ 'core-chat-session--active': item.threadId === activeThreadId() }}
                  onClick={() => openThread(item.threadId)}
                  aria-current={item.threadId === activeThreadId() ? 'page' : undefined}
                >
                  <span class="velion-sidebar-row-strong" title={item.title}>{item.title}</span>
                  <em>{formatChatUpdatedAt(item.updatedAt, i18n)}</em>
                </button>
              )}
            </For>
          </Show>
        </Show>
      </nav>

      <button type="button" class="core-chat-sidebar__clear" onClick={clearHistory}>
        <Trash2 class="size-3.5" />
        {i18n.tr('Tøm chathistorikk', 'Clear chat history')}
      </button>
    </div>
  )
}

function formatChatUpdatedAt(value: string, i18n: ReturnType<typeof useI18n>) {
  const updatedAt = Date.parse(value)
  if (Number.isNaN(updatedAt)) return i18n.tr('Lagret tråd', 'Saved thread')
  const ageMs = Date.now() - updatedAt
  if (i18n.locale() === 'no') {
    if (ageMs < 60_000) return 'Akkurat nå'
    if (ageMs < 3_600_000) return `${Math.max(1, Math.round(ageMs / 60_000))}m siden`
    if (ageMs < 86_400_000) return `${Math.max(1, Math.round(ageMs / 3_600_000))}t siden`
    return new Intl.DateTimeFormat('nb-NO', { month: 'short', day: 'numeric' }).format(new Date(updatedAt))
  }
  if (ageMs < 60_000) return 'Just now'
  if (ageMs < 3_600_000) return `${Math.max(1, Math.round(ageMs / 60_000))}m ago`
  if (ageMs < 86_400_000) return `${Math.max(1, Math.round(ageMs / 3_600_000))}h ago`
  return new Intl.DateTimeFormat('en-US', { month: 'short', day: 'numeric' }).format(new Date(updatedAt))
}

function SidebarPanelNavigation(props: {
  section: SidebarSection
  pathname: string
  activeTabId: string | null
  searchQuery: string
}) {
  const i18n = useI18n()
  const normalizedSearchQuery = () => normalizeSearchText(props.searchQuery.trim())
  const isSearchActive = () => normalizedSearchQuery().length > 0
  const [expandedGroups, setExpandedGroups] = createSignal<Set<string>>(new Set())
  const [expandedItems, setExpandedItems] = createSignal<Set<string>>(new Set())
  const activeItemIds = () => getActiveItemIds(props.section, props.pathname)
  const visibleGroups = createMemo(() => {
    const query = normalizedSearchQuery()
    const searchActive = query.length > 0
    const activeTabId = props.activeTabId

    return props.section.panelGroups.reduce<SidebarPanelGroup[]>((groups, group) => {
      const items = group.items.filter((item) => {
        if (!searchActive && activeTabId && item.tabId && item.tabId !== activeTabId) return false
        if (!searchActive) return true
        return [item.label, item.description, item.href].some((value) => normalizeSearchText(value).includes(query))
      })

      if (items.length === 0) return groups
      return [...groups, { ...group, items }]
    }, [])
  })
  const topGroups = () => visibleGroups().filter((group) => !group.alignBottom)
  const bottomGroups = () => visibleGroups().filter((group) => group.alignBottom)

  createEffect(() => {
    setExpandedGroups(new Set(getDefaultExpandedGroups(props.section)))
    setExpandedItems(new Set(getActiveItemIds(props.section, props.pathname)))
  })

  const toggleGroup = (groupId: string) => {
    setExpandedGroups((current) => {
      const next = new Set(current)
      if (next.has(groupId)) next.delete(groupId)
      else next.add(groupId)
      return next
    })
  }

  const toggleItem = (itemId: string) => {
    setExpandedItems((current) => {
      const next = new Set(current)
      if (next.has(itemId)) next.delete(itemId)
      else next.add(itemId)
      return next
    })
  }

  return (
    <Show
      when={visibleGroups().length > 0}
      fallback={<div class="core-sidebar-empty velion-sidebar-row-normal">{i18n.tr('Ingen treff i denne seksjonen.', 'No matches in this section.')}</div>}
    >
      <nav class="core-sidebar-panel-nav" aria-label={i18n.tr(`${props.section.label} navigasjon`, `${props.section.label} navigation`)}>
        <div>
          <For each={topGroups()}>
            {(group) => (
              <SidebarGroup
                group={group}
                pathname={props.pathname}
                activeItemIds={activeItemIds()}
                expandedGroups={expandedGroups()}
                expandedItems={expandedItems()}
                isSearchActive={isSearchActive()}
                onToggleGroup={toggleGroup}
                onToggleItem={toggleItem}
              />
            )}
          </For>
        </div>
        <Show when={bottomGroups().length}>
          <div class="core-sidebar-panel-nav__bottom">
            <For each={bottomGroups()}>
              {(group) => (
                <SidebarGroup
                  group={group}
                  pathname={props.pathname}
                  activeItemIds={activeItemIds()}
                  expandedGroups={expandedGroups()}
                  expandedItems={expandedItems()}
                  isSearchActive={isSearchActive()}
                  onToggleGroup={toggleGroup}
                  onToggleItem={toggleItem}
                />
              )}
            </For>
          </div>
        </Show>
      </nav>
    </Show>
  )
}

function SidebarGroup(props: {
  group: SidebarPanelGroup
  pathname: string
  activeItemIds: Set<string>
  expandedGroups: Set<string>
  expandedItems: Set<string>
  isSearchActive: boolean
  onToggleGroup: (groupId: string) => void
  onToggleItem: (itemId: string) => void
}) {
  const i18n = useI18n()
  const groupExpanded = () => !props.group.collapsible || props.isSearchActive || props.expandedGroups.has(props.group.id)

  return (
    <section class="core-sidebar-group">
      <Show when={props.group.showHeader !== false}>
        <div class="core-sidebar-group__header">
          <span class="velion-sidebar-group-title">{props.group.label}</span>
          <Show when={props.group.collapsible}>
            <button
              type="button"
              onClick={() => props.onToggleGroup(props.group.id)}
              aria-expanded={groupExpanded()}
              aria-label={groupExpanded() ? i18n.tr(`Slå sammen ${props.group.label}`, `Collapse ${props.group.label}`) : i18n.tr(`Utvid ${props.group.label}`, `Expand ${props.group.label}`)}
              title={groupExpanded() ? i18n.tr(`Slå sammen ${props.group.label}`, `Collapse ${props.group.label}`) : i18n.tr(`Utvid ${props.group.label}`, `Expand ${props.group.label}`)}
            >
              <Show when={groupExpanded()} fallback={<ChevronRight class="size-4" strokeWidth={2.1} />}>
                <ChevronDown class="size-4" strokeWidth={2.1} />
              </Show>
            </button>
          </Show>
        </div>
      </Show>

      <Show when={groupExpanded()}>
        <div class="core-sidebar-group__items">
          <For each={props.group.items}>
            {(item) => (
              <SidebarPanelLink
                item={item}
                pathname={props.pathname}
                expanded={props.expandedItems.has(item.id) || props.activeItemIds.has(item.id)}
                onToggle={() => props.onToggleItem(item.id)}
              />
            )}
          </For>
        </div>
      </Show>
    </section>
  )
}

function SidebarPanelLink(props: {
  item: SidebarPanelItem
  pathname: string
  expanded: boolean
  onToggle: () => void
}) {
  const active = () => isSidebarPathActive(props.pathname, props.item.href, props.item.aliases)
  const hasSubItems = () => Boolean(props.item.subItems?.length)

  return (
    <Show
      when={!hasSubItems()}
      fallback={
        <div>
          <button
            type="button"
            onClick={props.onToggle}
            class={cn('core-sidebar-panel-link', active() ? 'velion-sidebar-panel-active core-sidebar-panel-link--active' : '')}
            aria-expanded={props.expanded}
            title={props.item.label}
          >
            <PanelItemIcon icon={props.item.icon} active={active()} />
            <span class={cn('core-sidebar-panel-link__label', active() ? 'velion-sidebar-row' : 'velion-sidebar-row-normal')}>{props.item.label}</span>
            <Show when={props.expanded} fallback={<ChevronRight class="size-3.5" strokeWidth={2.2} />}>
              <ChevronDown class="size-3.5" strokeWidth={2.2} />
            </Show>
          </button>
        </div>
      }
    >
      <A href={props.item.href} aria-current={active() ? 'page' : undefined} class={cn('core-sidebar-panel-link', active() ? 'velion-sidebar-panel-active core-sidebar-panel-link--active' : '')}>
        <PanelItemIcon icon={props.item.icon} active={active()} />
        <span class={cn('core-sidebar-panel-link__label', active() ? 'velion-sidebar-row' : 'velion-sidebar-row-normal')}>{props.item.label}</span>
      </A>
    </Show>
  )
}

function PanelItemIcon(props: { icon: SidebarPanelItem['icon']; active: boolean }) {
  return <Dynamic component={props.icon} class={cn('core-sidebar-panel-link__icon', props.active ? 'core-sidebar-panel-link__icon--active' : '')} strokeWidth={1.7} />
}

function MiniSectionLink(props: { section: SidebarSection; active: boolean; onOpen: () => void }) {
  return (
    <A
      href={props.section.href}
      onClick={() => props.onOpen()}
      aria-current={props.active ? 'page' : undefined}
      aria-label={props.section.label}
      title={props.section.label}
      class={cn('core-mini-nav-button', props.active ? 'velion-sidebar-mini-active' : '')}
    >
      <Dynamic component={props.section.icon} class="size-[18px]" strokeWidth={props.active ? 2 : 1.6} />
    </A>
  )
}

function MiniAccountLink(props: { active: boolean; children: JSX.Element; i18n: ReturnType<typeof useI18n>; onOpen: () => void }) {
  return (
    <A
      href="/account"
      onClick={props.onOpen}
      aria-current={props.active ? 'page' : undefined}
      aria-label={props.i18n.tr('Konto', 'Account')}
      title={props.i18n.tr('Konto', 'Account')}
      class={cn('core-mini-nav-button', props.active ? 'velion-sidebar-mini-active' : '')}
    >
      {props.children}
    </A>
  )
}

function MiniActionButton(props: {
  active: boolean
  children: JSX.Element
  label: string
  onClick: () => void
}) {
  return (
    <button
      type="button"
      onClick={() => props.onClick()}
      aria-label={props.label}
      title={props.label}
      class={cn('core-mini-nav-button', props.active ? 'velion-sidebar-mini-active' : '')}
    >
      {props.children}
    </button>
  )
}

function getDefaultExpandedGroups(section: SidebarSection) {
  const expandedGroups = new Set<string>()
  for (const group of section.panelGroups) {
    if (group.defaultExpanded) expandedGroups.add(group.id)
  }
  return expandedGroups
}

function getActiveItemIds(section: SidebarSection, pathname: string) {
  const activeItemIds = new Set<string>()
  for (const group of section.panelGroups) {
    for (const item of group.items) {
      if (isSidebarPathActive(pathname, item.href, item.aliases)) activeItemIds.add(item.id)
    }
  }
  return activeItemIds
}

function normalizeSearchText(value: string) {
  return value
    .normalize('NFD')
    .replace(/[\u0300-\u036f]/g, '')
    .toLowerCase()
}

const sidebarLabels: Record<string, readonly [string, string]> = {
  'section:overview': ['Oversikt', 'Overview'],
  'section:messages': ['Chat', 'Chat'],
  'section:studio': ['Studio', 'Studio'],
  'section:inbox': ['Innboks', 'Inbox'],
  'section:ticketing': ['Saker', 'Ticketing'],
  'section:social': ['Sosialt', 'Social'],
  'section:agents': ['Agenter', 'Agents'],
  'section:ingestions': ['Innhenting', 'Ingestions'],
  'section:knowledge': ['Kunnskap', 'Knowledge'],
  'section:insights': ['Innsikt', 'Insights'],
  'section:settings': ['Innstillinger', 'Settings'],
  'tab:my-account': ['Min konto', 'My account'],
  'tab:shared': ['Delt med meg', 'Shared with me'],
  'group:overview-core': ['Oversikt', 'Overview'],
  'group:messages-core': ['Chat', 'Chat'],
  'group:studio-create': ['Opprett', 'Create'],
  'group:studio-linked': ['Tilkoblede systemer', 'Linked systems'],
  'group:inbox-core': ['Innboks', 'Inbox'],
  'group:ticketing-core': ['Køer', 'Queues'],
  'group:social-channels': ['Kanaler', 'Channels'],
  'group:social-plan': ['Plan', 'Plan'],
  'group:social-intelligence': ['Intelligens', 'Intelligence'],
  'group:social-reuse': ['Gjenbruk', 'Reuse'],
  'group:agents-core': ['Agenter', 'Agents'],
  'group:ingestions-core': ['Innhenting', 'Ingestions'],
  'group:knowledge-core': ['Kunnskap', 'Knowledge'],
  'group:insights-core': ['Måling', 'Measure'],
  'group:settings-core': ['Innstillinger', 'Settings'],
  'item:overview-home': ['Hjem', 'Home'],
  'item:overview-chat': ['Velion Chat', 'Velion Chat'],
  'item:overview-studio': ['Studio', 'Studio'],
  'item:overview-inbox': ['Innboks', 'Inbox'],
  'item:overview-ticketing': ['Saker', 'Ticketing'],
  'item:overview-social': ['Sosialt', 'Social'],
  'item:overview-knowledge': ['Kunnskap', 'Knowledge'],
  'item:overview-insights': ['Innsikt', 'Insights'],
  'item:overview-leads': ['Leads', 'Leads'],
  'item:overview-agents': ['Agenter', 'Agents'],
  'item:messages-start': ['Ny samtale', 'New conversation'],
  'item:messages-inbox': ['Samtaler', 'Conversations'],
  'item:studio-canvas': ['Canvas', 'Canvas'],
  'item:studio-campaigns': ['Kampanjeplanlegger', 'Campaign planner'],
  'item:studio-templates': ['Maler', 'Templates'],
  'item:studio-social-drafts': ['Sosiale utkast', 'Social drafts'],
  'item:studio-knowledge-assets': ['Kunnskapsressurser', 'Knowledge assets'],
  'item:inbox-home': ['Din innboks', 'Your inbox'],
  'item:inbox-ai': ['AI-samtaler', 'AI conversations'],
  'item:ticketing-suggested': ['Foreslått av AI', 'Suggested by AI'],
  'item:ticketing-my': ['Mine saker', 'My tickets'],
  'item:ticketing-unassigned': ['Uten eier', 'Unassigned'],
  'item:ticketing-sla-risk': ['SLA-risiko', 'SLA risk'],
  'item:social-accounts': ['Kontoer', 'Accounts'],
  'item:social-calendar': ['Kalender', 'Calendar'],
  'item:social-drafts': ['Utkast', 'Drafts'],
  'item:social-approvals': ['Godkjenninger', 'Approvals'],
  'item:social-campaigns': ['Kampanjer', 'Campaigns'],
  'item:social-competitors': ['Konkurrentovervåking', 'Competitor watch'],
  'item:social-trends': ['Trender', 'Trends'],
  'item:social-evergreen': ['Evergreen-kø', 'Evergreen queue'],
  'item:agents-all': ['Alle agenter', 'All agents'],
  'item:agents-chat': ['Arbeidsflate', 'Workspace'],
  'item:ingestions-home': ['Arbeidsflate', 'Workspace'],
  'item:ingestions-knowledge': ['Kunnskap', 'Knowledge'],
  'item:ingestions-chat': ['Spør Velion', 'Ask Velion'],
  'item:knowledge-overview': ['Datakilder', 'Data sources'],
  'item:knowledge-chat': ['Spør kunnskapen', 'Ask knowledge'],
  'item:insights-overview': ['Oversikt', 'Overview'],
  'item:insights-social': ['Sosialt', 'Social'],
  'item:insights-inbox': ['Innboks', 'Inbox'],
  'item:insights-agents': ['Agenter', 'Agents'],
  'item:insights-campaigns': ['Kampanjer', 'Campaigns'],
  'item:insights-experiments': ['Eksperimenter', 'Experiments'],
  'item:settings-workspace': ['Arbeidsområde', 'Workspace'],
  'item:settings-members': ['Medlemmer', 'Members'],
  'item:settings-billing': ['Fakturering', 'Billing'],
  'item:settings-security': ['Sikkerhet', 'Security'],
  'item:settings-trust': ['Tillitssenter', 'Trust Center'],
  'item:settings-router-policy': ['Router-policy', 'Router policy'],
  'item:settings-finetune': ['Finjusteringsjobber', 'Fine-tune jobs'],
}

function localizeSidebarSection(section: SidebarSection, i18n: ReturnType<typeof useI18n>): SidebarSection {
  return {
    ...section,
    label: translateSidebarLabel(`section:${section.id}`, section.label, i18n),
    panelTabs: section.panelTabs?.map((tab) => ({
      ...tab,
      label: translateSidebarLabel(`tab:${tab.id}`, tab.label, i18n),
    })),
    panelGroups: section.panelGroups.map((group) => ({
      ...group,
      label: translateSidebarLabel(`group:${group.id}`, group.label, i18n),
      items: group.items.map((item) => ({
        ...item,
        label: translateSidebarLabel(`item:${item.id}`, item.label, i18n),
        subItems: item.subItems?.map((subItem) => ({
          ...subItem,
          label: translateSidebarLabel(`subitem:${subItem.id}`, subItem.label, i18n),
        })),
      })),
    })),
  }
}

function translateSidebarLabel(key: string, fallback: string, i18n: ReturnType<typeof useI18n>): string {
  const label = sidebarLabels[key]
  return label ? i18n.tr(label[0], label[1]) : fallback
}
