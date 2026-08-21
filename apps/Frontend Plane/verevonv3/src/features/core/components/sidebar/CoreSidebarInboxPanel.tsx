import { useLocation } from '@solidjs/router'
import {
  CheckCheck,
  ChevronDown,
  ChevronRight,
  CircleX,
  GitBranch,
  Headphones,
  Inbox,
  LayoutGrid,
  List,
  Mail,
  MessageCircleMore,
  MessageSquare,
  MessagesSquare,
  PenLine,
  Plus,
  SlidersHorizontal,
  ShieldAlert,
  UserRound,
} from '@/shared/icons'
import { createMemo, createSignal, For, Show, type Component } from 'solid-js'
import type { LucideProps } from '@/shared/icons'
import { Dynamic } from '@solidjs/web'
import {
  SidebarEmptyState,
  SidebarPanelTitle,
  SidebarSearchField,
} from '@/features/core/components/sidebar/CoreSidebarPrimitives'
import { useI18n } from '@/shared/i18n'
import { cn } from '@/shared/lib/cn'

type InboxIcon = Component<LucideProps>
type InboxSidebarView =
  | 'mine'
  | 'created-by-you'
  | 'all'
  | 'unassigned'
  | 'spam'
  | 'dashboard'
  | 'ai-all'
  | 'ai-resolved'
  | 'ai-routed'
  | 'ai-abandoned'
  | 'team-admin-support'
  | 'view-messenger'
  | 'view-email'
  | 'view-social'
  | 'view-feedback'
  | 'manage'

type InboxSidebarItem = {
  id: InboxSidebarView
  label: string
  href: string
  icon: InboxIcon
  badge?: number | string
  trailing?: boolean
  subItems?: InboxSidebarSubItem[]
}

type InboxSidebarSubItem = {
  id: string
  label: string
  href: string
}

type InboxSidebarGroup = {
  id: string
  label: string
  defaultExpanded: boolean
  showAddButton?: boolean
  alignBottom?: boolean
  emptyLabel?: string
  items: InboxSidebarItem[]
}

// Channel values MUST match conversation-core's channelForProvider output
// (conversation-core-go/internal/conversation/repository.go): email/microsoft/
// google collapse to "email"; every other provider is its own channel key —
// slack, teams, discord, linkedin, whatsapp, messenger, instagram, and "x" (the provider
// key for Twitter/X — a channel value of "twitter" never exists upstream).
const inboxChannels: InboxSidebarSubItem[] = [
  { id: 'all', label: 'All messages', href: '/inbox?view=mine&channel=all' },
  { id: 'messenger', label: 'Messenger', href: '/inbox?view=mine&channel=messenger' },
  { id: 'instagram', label: 'Instagram', href: '/inbox?view=mine&channel=instagram' },
  { id: 'whatsapp', label: 'WhatsApp', href: '/inbox?view=mine&channel=whatsapp' },
  { id: 'email', label: 'Email', href: '/inbox?view=mine&channel=email' },
  { id: 'slack', label: 'Slack', href: '/inbox?view=mine&channel=slack' },
  { id: 'teams', label: 'Microsoft Teams', href: '/inbox?view=mine&channel=teams' },
  { id: 'discord', label: 'Discord', href: '/inbox?view=mine&channel=discord' },
  { id: 'linkedin', label: 'LinkedIn', href: '/inbox?view=mine&channel=linkedin' },
  { id: 'twitter', label: 'Twitter / X', href: '/inbox?view=mine&channel=x' },
  { id: 'sms', label: 'SMS', href: '/inbox?view=mine&channel=sms' },
]

const inboxSidebarGroups: InboxSidebarGroup[] = [
  {
    id: 'inbox-core',
    label: 'Inbox',
    defaultExpanded: true,
    items: [
      { id: 'mine', label: 'Your inbox', icon: Inbox, href: '/inbox?view=mine', subItems: inboxChannels },
      { id: 'created-by-you', label: 'Created by you', icon: PenLine, href: '/inbox?view=created-by-you' },
      { id: 'all', label: 'All', icon: List, href: '/inbox?view=all' },
      { id: 'unassigned', label: 'Unassigned', icon: UserRound, href: '/inbox?view=unassigned' },
      { id: 'spam', label: 'Spam', icon: ShieldAlert, href: '/inbox?view=spam' },
      // Default saved view surfacing every conversation tagged with the
      // pilot-feedback tag -- both filed directly in this org and mirrored in
      // from an external pilot org (see conversation-core-go's
      // Service.mirrorFeedback) -- so the team sees it without knowing to
      // filter manually.
      { id: 'view-feedback', label: 'Feedback', icon: MessageCircleMore, href: '/inbox?view=view-feedback' },
      { id: 'dashboard', label: 'Dashboard', icon: LayoutGrid, href: '/inbox?view=dashboard' },
    ],
  },
  {
    id: 'inbox-ai-agent',
    label: 'Verevon AI Agent',
    defaultExpanded: true,
    showAddButton: true,
    items: [
      { id: 'ai-all', label: 'All conversations', icon: MessagesSquare, href: '/inbox?view=ai-all' },
      { id: 'ai-resolved', label: 'Resolved', icon: CheckCheck, href: '/inbox?view=ai-resolved' },
      { id: 'ai-routed', label: 'Routed', icon: GitBranch, href: '/inbox?view=ai-routed' },
      { id: 'ai-abandoned', label: 'Abandoned', icon: CircleX, href: '/inbox?view=ai-abandoned' },
    ],
  },
  {
    id: 'inbox-team-inboxes',
    label: 'Team inboxes',
    defaultExpanded: true,
    items: [{ id: 'team-admin-support', label: 'Admin Support', icon: Headphones, href: '/inbox?view=team-admin-support' }],
  },
  {
    id: 'inbox-teammates',
    label: 'Teammates',
    defaultExpanded: false,
    showAddButton: true,
    emptyLabel: 'No teammates added yet.',
    items: [],
  },
  {
    id: 'inbox-views',
    label: 'Views',
    defaultExpanded: true,
    items: [
      { id: 'view-messenger', label: 'Messenger', icon: MessageSquare, href: '/inbox?view=view-messenger' },
      { id: 'view-email', label: 'Email', icon: Mail, href: '/inbox?view=view-email' },
      { id: 'view-social', label: 'WhatsApp & Social', icon: MessageSquare, href: '/inbox?view=view-social' },
    ],
  },
  {
    id: 'inbox-manage',
    label: 'Manage',
    defaultExpanded: true,
    alignBottom: true,
    items: [{ id: 'manage', label: 'Manage', icon: SlidersHorizontal, href: '/inbox?view=manage' }],
  },
]

export function InboxExpandedSidebarPanel(props: { onCollapse: () => void }) {
  const i18n = useI18n()
  const location = useLocation()
  const [searchQuery, setSearchQuery] = createSignal('')
  const [expandedGroups, setExpandedGroups] = createSignal<Record<string, boolean>>(
    Object.fromEntries(inboxSidebarGroups.map((group) => [group.id, group.defaultExpanded])),
  )
  const [expandedItems, setExpandedItems] = createSignal<Record<string, boolean>>(getDefaultInboxExpandedItems())
  const normalizedSearch = () => searchQuery().trim().toLowerCase()
  const activeParams = () => getInboxActiveParams(location.pathname, location.search)
  const localizedGroups = createMemo(() => localizeInboxGroups(i18n))
  const visibleGroups = createMemo(() => getVisibleInboxGroups(localizedGroups(), normalizedSearch()))
  const topVisibleGroups = () => visibleGroups().filter((group) => !group.alignBottom)
  const bottomVisibleGroups = () => visibleGroups().filter((group) => group.alignBottom)

  const toggleGroup = (groupId: string) => {
    setExpandedGroups((current) => ({ ...current, [groupId]: !current[groupId] }))
  }

  const toggleItem = (itemId: string) => {
    setExpandedItems((current) => ({ ...current, [itemId]: !current[itemId] }))
  }

  return (
    <div class="core-sidebar-dedicated-panel">
      <SidebarPanelTitle onCollapse={props.onCollapse}>{i18n.tr('Innboks', 'Inbox')}</SidebarPanelTitle>

      <SidebarSearchField
        ariaLabel={i18n.tr('Filtrer innboksseksjon', 'Filter inbox section')}
        class="core-sidebar-search-spacious"
        value={searchQuery()}
        onChange={setSearchQuery}
      />

      <nav class="core-sidebar-dedicated-nav" aria-label={i18n.tr('Innboksnavigasjon', 'Inbox navigation')}>
        <div class="core-sidebar-dedicated-nav__stack">
          <For each={topVisibleGroups()}>
            {(group) => (
              <InboxSidebarGroup
                activeChannel={activeParams().channel}
                activeView={activeParams().view}
                group={group}
                expanded={Boolean(normalizedSearch()) || (expandedGroups()[group.id] ?? group.defaultExpanded)}
                expandedItems={expandedItems()}
                forceExpandItems={Boolean(normalizedSearch())}
                onToggle={() => toggleGroup(group.id)}
                onToggleItem={toggleItem}
              />
            )}
          </For>
          <div class="core-sidebar-dedicated-nav__spacer" />
          <For each={bottomVisibleGroups()}>
            {(group) => (
              <InboxSidebarGroup
                activeChannel={activeParams().channel}
                activeView={activeParams().view}
                group={group}
                expanded={Boolean(normalizedSearch()) || (expandedGroups()[group.id] ?? group.defaultExpanded)}
                expandedItems={expandedItems()}
                forceExpandItems={Boolean(normalizedSearch())}
                onToggle={() => toggleGroup(group.id)}
                onToggleItem={toggleItem}
              />
            )}
          </For>
        </div>
      </nav>
    </div>
  )
}

function InboxSidebarGroup(props: {
  activeChannel: string | null
  activeView: string
  expanded: boolean
  expandedItems: Record<string, boolean>
  forceExpandItems: boolean
  group: InboxSidebarGroup
  onToggle: () => void
  onToggleItem: (itemId: string) => void
}) {
  const i18n = useI18n()
  return (
    <section>
      <div class="core-sidebar-dedicated-group-header">
        <button type="button" onClick={() => props.onToggle()} aria-expanded={props.expanded ? 'true' : 'false'}>
          <h2 class="verevon-sidebar-group-title">{props.group.label}</h2>
          <ChevronDown class={cn('size-4 core-sidebar-chevron', !props.expanded && '-rotate-90')} strokeWidth={2.1} />
        </button>
        <Show when={props.group.showAddButton}>
          <button
            type="button"
            class="core-sidebar-round-add"
            aria-label={i18n.tr(`Legg til ${props.group.label.toLowerCase()}`, `Add ${props.group.label.toLowerCase()}`)}
            title={i18n.tr(`Legg til ${props.group.label.toLowerCase()}`, `Add ${props.group.label.toLowerCase()}`)}
          >
            <Plus class="size-4" strokeWidth={1.9} />
          </button>
        </Show>
      </div>

      <Show when={props.expanded}>
        <Show
          when={props.group.items.length}
          fallback={<SidebarEmptyState label={props.group.emptyLabel ?? i18n.tr('Ingen elementer.', 'No items.')} />}
        >
          <div class="core-sidebar-link-list">
            <For each={props.group.items}>
              {(item) => (
                <InboxSidebarItem
                  active={isInboxItemActive(item, props.activeView, props.activeChannel)}
                  activeChannel={props.activeChannel}
                  activeView={props.activeView}
                  expanded={props.forceExpandItems || Boolean(props.expandedItems[item.id])}
                  item={item}
                  onToggle={() => props.onToggleItem(item.id)}
                />
              )}
            </For>
          </div>
        </Show>
      </Show>
    </section>
  )
}

function InboxSidebarItem(props: {
  active: boolean
  activeChannel: string | null
  activeView: string
  expanded: boolean
  item: InboxSidebarItem
  onToggle: () => void
}) {
  const i18n = useI18n()
  const hasSubItems = () => Boolean(props.item.subItems?.length)
  const subNavigationId = () => `inbox-sidebar-${props.item.id}-subitems`

  return (
    <Show
      when={!hasSubItems()}
      fallback={
        <div>
          <div class={cn('core-sidebar-inbox-dropdown', props.active && 'core-sidebar-inbox-dropdown--active')}>
            <a
              href={props.item.href}
              link
              aria-current={props.active ? 'page' : undefined}
              class="core-sidebar-inbox-dropdown__link"
            >
              <Dynamic component={props.item.icon} class="core-sidebar-dedicated-icon" strokeWidth={1.75} />
              <span>{props.item.label}</span>
              <InboxBadge value={props.item.badge} />
            </a>
            <button
              type="button"
              onClick={props.onToggle}
              aria-controls={subNavigationId()}
              aria-expanded={props.expanded ? 'true' : 'false'}
              aria-label={props.expanded ? i18n.tr(`Skjul ${props.item.label}`, `Hide ${props.item.label}`) : i18n.tr(`Vis ${props.item.label}`, `Show ${props.item.label}`)}
              title={props.expanded ? i18n.tr(`Skjul ${props.item.label}`, `Hide ${props.item.label}`) : i18n.tr(`Vis ${props.item.label}`, `Show ${props.item.label}`)}
            >
              <ChevronRight class={cn('size-4 transition-transform', props.expanded && 'rotate-90')} strokeWidth={1.9} />
            </button>
          </div>
          <Show when={props.expanded}>
            <nav id={subNavigationId()} class="core-sidebar-subnav" aria-label={`${props.item.label} subnavigation`}>
              <div class="core-sidebar-subnav__line" />
              <For each={props.item.subItems}>
                {(subItem) => (
                  <InboxSidebarSubItem
                    active={isInboxSubItemActive(subItem, props.activeView, props.activeChannel)}
                    item={subItem}
                  />
                )}
              </For>
            </nav>
          </Show>
        </div>
      }
    >
      <a
        href={props.item.href}
        link
        aria-current={props.active ? 'page' : undefined}
        class={cn('core-sidebar-section-link', props.active && 'core-sidebar-section-link--active')}
      >
        <Dynamic component={props.item.icon} class="core-sidebar-dedicated-icon" strokeWidth={1.75} />
        <span>{props.item.label}</span>
        <InboxBadge value={props.item.badge} />
        <Show when={props.item.trailing}>
          <ChevronRight class="size-4 core-sidebar-muted-chevron" strokeWidth={1.9} />
        </Show>
      </a>
    </Show>
  )
}

function InboxSidebarSubItem(props: { active: boolean; item: InboxSidebarSubItem }) {
  return (
    <a
      href={props.item.href}
      link
      class={cn('core-sidebar-subnav-link', props.active && 'core-sidebar-subnav-link--active')}
    >
      {props.item.label}
    </a>
  )
}

function InboxBadge(props: { value?: number | string }) {
  return (
    <Show when={props.value}>
      <span class="core-sidebar-badge">{props.value}</span>
    </Show>
  )
}

function getInboxActiveParams(pathname: string, search: string) {
  if (search) {
    const params = new URLSearchParams(search)
    return {
      view: params.get('view') ?? 'mine',
      channel: params.get('channel'),
    }
  }

  if (!pathname.startsWith('/inbox/')) {
    return { view: 'mine', channel: null }
  }

  const slug = pathname.split('/').slice(2).filter(Boolean)
  if (slug[0] === 'channels') return { view: 'mine', channel: slug[1] ?? null }
  return { view: slug[0] ?? 'mine', channel: null }
}

function isInboxItemActive(item: InboxSidebarItem, activeView: string, activeChannel: string | null) {
  if (item.id === 'mine') return activeView === 'mine'
  return !activeChannel && activeView === item.id
}

function isInboxSubItemActive(item: InboxSidebarSubItem, activeView: string, activeChannel: string | null) {
  const href = new URL(item.href, 'https://verevon.local')
  const itemView = href.searchParams.get('view') ?? 'mine'
  const itemChannel = href.searchParams.get('channel')
  return activeView === itemView && (activeChannel ?? null) === (itemChannel ?? null)
}

function getDefaultInboxExpandedItems() {
  const expandedItems: Record<string, boolean> = {}
  for (const group of inboxSidebarGroups) {
    for (const item of group.items) {
      if (item.subItems?.length) expandedItems[item.id] = item.id === 'mine'
    }
  }
  return expandedItems
}

function getVisibleInboxGroups(sourceGroups: InboxSidebarGroup[], normalizedSearch: string) {
  return sourceGroups.reduce<InboxSidebarGroup[]>((groups, group) => {
    const items = normalizedSearch
      ? group.items.filter((item) => (
        item.label.toLowerCase().includes(normalizedSearch) ||
        item.subItems?.some((subItem) => subItem.label.toLowerCase().includes(normalizedSearch))
      ))
      : group.items

    if (items.length > 0 || group.emptyLabel) return [...groups, { ...group, items }]
    return groups
  }, [])
}

const inboxLabels: Record<string, readonly [string, string]> = {
  'group:inbox-core': ['Innboks', 'Inbox'],
  'group:inbox-ai-agent': ['Verevon AI-agent', 'Verevon AI Agent'],
  'group:inbox-team-inboxes': ['Teaminnbokser', 'Team inboxes'],
  'group:inbox-teammates': ['Teamkolleger', 'Teammates'],
  'group:inbox-views': ['Visninger', 'Views'],
  'group:inbox-manage': ['Administrer', 'Manage'],
  'item:mine': ['Din innboks', 'Your inbox'],
  'item:created-by-you': ['Opprettet av deg', 'Created by you'],
  'item:all': ['Alle', 'All'],
  'item:unassigned': ['Uten eier', 'Unassigned'],
  'item:spam': ['Spam', 'Spam'],
  'item:view-feedback': ['Tilbakemelding', 'Feedback'],
  'item:dashboard': ['Dashboard', 'Dashboard'],
  'item:ai-all': ['Alle samtaler', 'All conversations'],
  'item:ai-resolved': ['Løst', 'Resolved'],
  'item:ai-routed': ['Rutet', 'Routed'],
  'item:ai-abandoned': ['Forlatt', 'Abandoned'],
  'item:team-admin-support': ['Admin-support', 'Admin Support'],
  'item:view-messenger': ['Messenger', 'Messenger'],
  'item:view-email': ['E-post', 'Email'],
  'item:view-social': ['WhatsApp og sosialt', 'WhatsApp & Social'],
  'item:manage': ['Administrer', 'Manage'],
  'sub:all': ['Alle meldinger', 'All messages'],
}

function localizeInboxGroups(i18n: ReturnType<typeof useI18n>): InboxSidebarGroup[] {
  return inboxSidebarGroups.map((group) => ({
    ...group,
    emptyLabel: group.emptyLabel ? i18n.tr('Ingen teamkolleger er lagt til ennå.', 'No teammates added yet.') : undefined,
    label: localizeInboxLabel(`group:${group.id}`, group.label, i18n),
    items: group.items.map((item) => ({
      ...item,
      label: localizeInboxLabel(`item:${item.id}`, item.label, i18n),
      subItems: item.subItems?.map((subItem) => ({
        ...subItem,
        label: localizeInboxLabel(`sub:${subItem.id}`, subItem.label, i18n),
      })),
    })),
  }))
}

function localizeInboxLabel(key: string, fallback: string, i18n: ReturnType<typeof useI18n>): string {
  const label = inboxLabels[key]
  return label ? i18n.tr(label[0], label[1]) : fallback
}
