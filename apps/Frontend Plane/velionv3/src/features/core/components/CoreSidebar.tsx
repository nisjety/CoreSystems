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
import { createEffect, createMemo, createSignal, For, Match, onMount, Show, Switch, type JSX } from 'solid-js'
import { Dynamic } from 'solid-js/web'
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
import { getThreadMessages } from '@/shared/api/chat-client'
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
  const location = useLocation()
  const session = getSession()
  const visibleSections = () => sidebarSections.filter((section) => (
    section.id !== 'settings' || shouldShowWorkspaceAdminNavigation(session)
  ))
  const activeSection = () => getSidebarSectionForPath(location.pathname, props.activeRoute, visibleSections())
  const mainSections = () => visibleSections().filter((section) => !section.pinnedBottom)
  const pinnedSections = () => visibleSections().filter((section) => section.pinnedBottom)
  const accountActive = () => location.pathname === '/account' || location.pathname.startsWith('/account/')
  const width = () => `${props.expanded ? (props.expandedWidth ?? SIDEBAR_EXPANDED_WIDTH) : SIDEBAR_MINIMIZED_WIDTH}px`

  const openSection = () => props.onExpandedChange(true)

  return (
    <aside class="velion-sidebar-themed core-sidebar velion-sidebar-type" style={{ width: width() }} aria-label="Primary navigation">
      <div class="core-sidebar__body">
        <div class="core-sidebar__rail">
          <div class="core-sidebar__top-actions">
            <Show when={!props.expansionLocked}>
              <MiniActionButton
                label={props.expanded ? 'Collapse sidebar' : 'Expand sidebar'}
                active={false}
                onClick={() => props.onExpandedChange(!props.expanded)}
              >
                <Show when={props.expanded} fallback={<PanelLeftOpen class="size-[18px]" strokeWidth={1.75} />}>
                  <PanelLeftClose class="size-[18px]" strokeWidth={1.75} />
                </Show>
              </MiniActionButton>
            </Show>
          </div>

          <nav class="core-sidebar__mini-nav" aria-label="Workspace sections">
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
            <MiniAccountLink active={accountActive()} onOpen={openSection}>
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
              label={sidebarSearchAction.label}
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
  return (
    <div class="core-sidebar-panel">
      <SidebarPanelTitle onCollapse={props.onCollapse}>{props.activeSection.label}</SidebarPanelTitle>
      <SidebarSearchField value={props.searchQuery} onChange={props.onSearchQueryChange} />

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
  const [error, setError] = createSignal<string | null>(null)
  const [loading, setLoading] = createSignal(false)
  const [session, setSession] = createSignal<{
    preview: string
    threadId: string
    title: string
  } | null>(null)

  const loadStoredThread = async () => {
    const threadId = window.sessionStorage.getItem('velion.chat.threadId')
    if (!threadId) {
      setSession(null)
      return
    }

    setLoading(true)
    setError(null)
    try {
      const messages = await getThreadMessages(threadId)
      const firstUserMessage = messages.find((message) => message.role === 'user')
      const lastMessage = messages.at(-1)
      setSession({
        threadId,
        title: truncateChatText(firstUserMessage?.content ?? 'Current chat thread', 48),
        preview: truncateChatText(lastMessage?.content ?? 'Open live session', 64),
      })
    } catch (reason) {
      setSession(null)
      setError(reason instanceof Error ? reason.message : 'Chat thread unavailable.')
    } finally {
      setLoading(false)
    }
  }

  onMount(() => {
    void loadStoredThread()
  })

  const clearLocalHistory = () => {
    window.sessionStorage.removeItem('velion.chat.threadId')
    setSession(null)
    setError(null)
    props.onCollapse()
  }

  return (
    <div class="core-chat-sidebar">
      <A href="/chat" class="core-chat-sidebar__new">
        <span>
          <MessageSquarePlus class="size-[15px]" strokeWidth={1.75} />
        </span>
        Ny samtale
      </A>

      <nav class="core-chat-sidebar__sessions" aria-label="Chat conversations">
        <Show when={!loading()} fallback={<div class="core-sidebar-empty velion-sidebar-row-normal">Loading conversations...</div>}>
          <Show
            when={session()}
            fallback={<div class="core-sidebar-empty velion-sidebar-row-normal">{error() ?? 'Open chat to load real conversation history.'}</div>}
          >
            {(item) => (
              <A href="/chat" class="core-chat-session core-chat-session--active">
                <span class="velion-sidebar-row-strong">{item().title}</span>
                <small class="velion-sidebar-secondary">{item().preview}</small>
                <em>Current thread</em>
              </A>
            )}
          </Show>
        </Show>
      </nav>

      <button type="button" class="core-chat-sidebar__clear" onClick={clearLocalHistory}>
        <Trash2 class="size-3.5" />
        Clear local history
      </button>
    </div>
  )
}

function truncateChatText(value: string, maxLength: number) {
  const normalized = value.replace(/\s+/g, ' ').trim()
  if (normalized.length <= maxLength) return normalized
  return `${normalized.slice(0, maxLength - 3).trimEnd()}...`
}

function SidebarPanelNavigation(props: {
  section: SidebarSection
  pathname: string
  activeTabId: string | null
  searchQuery: string
}) {
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
      fallback={<div class="core-sidebar-empty velion-sidebar-row-normal">Ingen treff i denne seksjonen.</div>}
    >
      <nav class="core-sidebar-panel-nav" aria-label={`${props.section.label} navigation`}>
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
              aria-label={`${groupExpanded() ? 'Collapse' : 'Expand'} ${props.group.label}`}
              title={`${groupExpanded() ? 'Collapse' : 'Expand'} ${props.group.label}`}
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

function MiniAccountLink(props: { active: boolean; children: JSX.Element; onOpen: () => void }) {
  return (
    <A
      href="/account"
      onClick={props.onOpen}
      aria-current={props.active ? 'page' : undefined}
      aria-label="Account"
      title="Account"
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
