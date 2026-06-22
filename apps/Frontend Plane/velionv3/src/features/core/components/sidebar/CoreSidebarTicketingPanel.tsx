import { A, useLocation } from '@solidjs/router'
import {
  AlertTriangle,
  Bot,
  CheckCheck,
  ChevronDown,
  ChevronRight,
  Clock3,
  GitBranch,
  Inbox,
  Link2,
  ListChecks,
  Plus,
  ShieldAlert,
  SlidersHorizontal,
  Tag,
  TicketCheck,
  UserRound,
  UsersRound,
  Zap,
} from 'lucide-solid'
import type { LucideProps } from 'lucide-solid'
import { createMemo, createSignal, For, Show, type Component } from 'solid-js'
import { Dynamic } from 'solid-js/web'
import {
  SidebarEmptyState,
  SidebarPanelTitle,
  SidebarSearchField,
} from '@/features/core/components/sidebar/CoreSidebarPrimitives'
import { useI18n } from '@/shared/i18n'
import { cn } from '@/shared/lib/cn'

type TicketingIcon = Component<LucideProps>
type TicketingSidebarView =
  | 'suggested'
  | 'my'
  | 'unassigned'
  | 'sla-risk'
  | 'escalated'
  | 'waiting-customer'
  | 'waiting-team'
  | 'resolved'
  | 'rules'
  | 'classifications'
  | 'macros'
  | 'sla-policies'
  | 'breached-sla'
  | 'refund-handoffs'
  | 'support-team'
  | 'billing-team'
  | 'linked-social'
  | 'external-links'
  | 'label-refund'
  | 'label-security'
  | 'label-delivery'

type TicketingSidebarItem = {
  id: TicketingSidebarView
  label: string
  href: string
  icon: TicketingIcon
  trailing?: boolean
  subItems?: TicketingSidebarSubItem[]
}

type TicketingSidebarSubItem = {
  id: string
  label: string
  href: string
}

type TicketingSidebarGroup = {
  id: string
  label: string
  defaultExpanded: boolean
  showAddButton?: boolean
  alignBottom?: boolean
  emptyLabel?: string
  items: TicketingSidebarItem[]
}

const myTicketStatusItems: TicketingSidebarSubItem[] = [
  { id: 'my-all', label: 'All tickets', href: '/tickets?queue=my' },
  { id: 'my-open', label: 'Open', href: '/tickets?queue=my&status=open' },
  { id: 'my-waiting-customer', label: 'Waiting on customer', href: '/tickets?queue=my&status=waiting_customer' },
  { id: 'my-waiting-team', label: 'Waiting on team', href: '/tickets?queue=my&status=waiting_team' },
  { id: 'my-resolved', label: 'Resolved', href: '/tickets?queue=resolved' },
]

const ticketingSidebarGroups: TicketingSidebarGroup[] = [
  {
    id: 'ticketing-tickets',
    label: 'Tickets',
    defaultExpanded: true,
    items: [
      { id: 'suggested', label: 'Suggested by AI', icon: Bot, href: '/tickets?queue=suggested' },
      { id: 'my', label: 'My tickets', icon: TicketCheck, href: '/tickets?queue=my', subItems: myTicketStatusItems },
      { id: 'unassigned', label: 'Unassigned', icon: Inbox, href: '/tickets?queue=unassigned' },
      { id: 'sla-risk', label: 'SLA risk', icon: AlertTriangle, href: '/tickets?queue=sla-risk' },
      { id: 'escalated', label: 'Escalated', icon: ShieldAlert, href: '/tickets?queue=escalated' },
      { id: 'waiting-customer', label: 'Waiting on customer', icon: Clock3, href: '/tickets?queue=waiting-customer' },
      { id: 'waiting-team', label: 'Waiting on team', icon: UsersRound, href: '/tickets?queue=waiting-team' },
      { id: 'resolved', label: 'Resolved', icon: CheckCheck, href: '/tickets?queue=resolved' },
    ],
  },
  {
    id: 'ticketing-saved-views',
    label: 'Saved views',
    defaultExpanded: true,
    showAddButton: true,
    items: [
      { id: 'breached-sla', label: 'Breached SLA', icon: AlertTriangle, href: '/tickets?queue=sla-risk&sla_state=breached' },
      { id: 'refund-handoffs', label: 'Refund handoffs', icon: Tag, href: '/tickets?queue=my&label=refund' },
      { id: 'linked-social', label: 'Linked social posts', icon: Link2, href: '/tickets?queue=rules&view=linked-social' },
      { id: 'external-links', label: 'External resources', icon: Link2, href: '/tickets?queue=rules&view=external-links' },
    ],
  },
  {
    id: 'ticketing-automation',
    label: 'Automation',
    defaultExpanded: true,
    showAddButton: true,
    items: [
      { id: 'rules', label: 'Rules / queues', icon: ListChecks, href: '/tickets?queue=rules' },
      { id: 'macros', label: 'Macros', icon: Zap, href: '/tickets?queue=rules&view=macros' },
      { id: 'classifications', label: 'AI classifications', icon: GitBranch, href: '/tickets?queue=rules&view=classifications' },
    ],
  },
  {
    id: 'ticketing-teams',
    label: 'Teams',
    defaultExpanded: true,
    showAddButton: true,
    items: [
      { id: 'support-team', label: 'Support team', icon: UserRound, href: '/tickets?queue=unassigned&team=support' },
      { id: 'billing-team', label: 'Billing team', icon: UsersRound, href: '/tickets?queue=my&team=billing' },
    ],
  },
  {
    id: 'ticketing-sla',
    label: 'SLA',
    defaultExpanded: true,
    items: [
      { id: 'sla-policies', label: 'SLA policies', icon: SlidersHorizontal, href: '/tickets?queue=rules&view=sla-policies' },
      { id: 'sla-risk', label: 'At risk', icon: AlertTriangle, href: '/tickets?queue=sla-risk&sla_state=risk' },
      { id: 'escalated', label: 'Escalated', icon: ShieldAlert, href: '/tickets?queue=escalated' },
    ],
  },
  {
    id: 'ticketing-labels',
    label: 'Labels',
    defaultExpanded: true,
    alignBottom: true,
    items: [
      { id: 'label-refund', label: 'Refund', icon: Tag, href: '/tickets?queue=my&label=refund' },
      { id: 'label-security', label: 'Security', icon: Tag, href: '/tickets?queue=escalated&label=security' },
      { id: 'label-delivery', label: 'Delivery', icon: Tag, href: '/tickets?queue=my&label=delivery' },
    ],
  },
]

export function TicketingExpandedSidebarPanel(props: { onCollapse: () => void }) {
  const i18n = useI18n()
  const location = useLocation()
  const [searchQuery, setSearchQuery] = createSignal('')
  const [expandedGroups, setExpandedGroups] = createSignal<Record<string, boolean>>(
    Object.fromEntries(ticketingSidebarGroups.map((group) => [group.id, group.defaultExpanded])),
  )
  const [expandedItems, setExpandedItems] = createSignal<Record<string, boolean>>(getDefaultTicketingExpandedItems())
  const normalizedSearch = () => searchQuery().trim().toLowerCase()
  const activeParams = () => getTicketingActiveParams(location.search)
  const localizedGroups = createMemo(() => localizeTicketingGroups(i18n))
  const visibleGroups = createMemo(() => getVisibleTicketingGroups(localizedGroups(), normalizedSearch()))
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
      <SidebarPanelTitle onCollapse={props.onCollapse}>{i18n.tr('Saker', 'Ticketing')}</SidebarPanelTitle>

      <SidebarSearchField
        ariaLabel={i18n.tr('Filtrer saksseksjon', 'Filter ticketing section')}
        class="core-sidebar-search-spacious"
        value={searchQuery()}
        onChange={setSearchQuery}
      />

      <nav class="core-sidebar-dedicated-nav" aria-label={i18n.tr('Saksnavigasjon', 'Ticketing navigation')}>
        <div class="core-sidebar-dedicated-nav__stack">
          <For each={topVisibleGroups()}>
            {(group) => (
              <TicketingSidebarGroup
                activeQueue={activeParams().queue}
                activeLabel={activeParams().label}
                activeSlaState={activeParams().slaState}
                activeStatus={activeParams().status}
                activeTeam={activeParams().team}
                activeView={activeParams().view}
                expanded={Boolean(normalizedSearch()) || (expandedGroups()[group.id] ?? group.defaultExpanded)}
                expandedItems={expandedItems()}
                forceExpandItems={Boolean(normalizedSearch())}
                group={group}
                onToggle={() => toggleGroup(group.id)}
                onToggleItem={toggleItem}
              />
            )}
          </For>
          <div class="core-sidebar-dedicated-nav__spacer" />
          <For each={bottomVisibleGroups()}>
            {(group) => (
              <TicketingSidebarGroup
                activeQueue={activeParams().queue}
                activeLabel={activeParams().label}
                activeSlaState={activeParams().slaState}
                activeStatus={activeParams().status}
                activeTeam={activeParams().team}
                activeView={activeParams().view}
                expanded={Boolean(normalizedSearch()) || (expandedGroups()[group.id] ?? group.defaultExpanded)}
                expandedItems={expandedItems()}
                forceExpandItems={Boolean(normalizedSearch())}
                group={group}
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

function TicketingSidebarGroup(props: {
  activeQueue: string
  activeLabel: string | null
  activeSlaState: string | null
  activeStatus: string | null
  activeTeam: string | null
  activeView: string | null
  expanded: boolean
  expandedItems: Record<string, boolean>
  forceExpandItems: boolean
  group: TicketingSidebarGroup
  onToggle: () => void
  onToggleItem: (itemId: string) => void
}) {
  const i18n = useI18n()
  return (
    <section>
      <div class="core-sidebar-dedicated-group-header">
        <button type="button" onClick={() => props.onToggle()} aria-expanded={props.expanded}>
          <h2 class="velion-sidebar-group-title">{props.group.label}</h2>
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
                <TicketingSidebarItem
                  active={isTicketingItemActive(item, {
                    queue: props.activeQueue,
                    status: props.activeStatus,
                    view: props.activeView,
                    team: props.activeTeam,
                    label: props.activeLabel,
                    slaState: props.activeSlaState,
                  })}
                  activeQueue={props.activeQueue}
                  activeLabel={props.activeLabel}
                  activeSlaState={props.activeSlaState}
                  activeStatus={props.activeStatus}
                  activeTeam={props.activeTeam}
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

function TicketingSidebarItem(props: {
  active: boolean
  activeQueue: string
  activeLabel: string | null
  activeSlaState: string | null
  activeStatus: string | null
  activeTeam: string | null
  activeView: string | null
  expanded: boolean
  item: TicketingSidebarItem
  onToggle: () => void
}) {
  const i18n = useI18n()
  const hasSubItems = () => Boolean(props.item.subItems?.length)
  const subNavigationId = () => `ticketing-sidebar-${props.item.id}-subitems`

  return (
    <Show
      when={!hasSubItems()}
      fallback={
        <div>
          <div class={cn('core-sidebar-inbox-dropdown', props.active && 'core-sidebar-inbox-dropdown--active')}>
            <A
              href={props.item.href}
              aria-current={props.active ? 'page' : undefined}
              class="core-sidebar-inbox-dropdown__link"
            >
              <Dynamic component={props.item.icon} class="core-sidebar-dedicated-icon" strokeWidth={1.75} />
              <span>{props.item.label}</span>
            </A>
            <button
              type="button"
              onClick={props.onToggle}
              aria-controls={subNavigationId()}
              aria-expanded={props.expanded}
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
                  <TicketingSidebarSubItem
                    active={isTicketingSubItemActive(subItem, {
                      queue: props.activeQueue,
                      status: props.activeStatus,
                      view: props.activeView,
                      team: props.activeTeam,
                      label: props.activeLabel,
                      slaState: props.activeSlaState,
                    })}
                    item={subItem}
                  />
                )}
              </For>
            </nav>
          </Show>
        </div>
      }
    >
      <A
        href={props.item.href}
        aria-current={props.active ? 'page' : undefined}
        class={cn('core-sidebar-section-link', props.active && 'core-sidebar-section-link--active')}
      >
        <Dynamic component={props.item.icon} class="core-sidebar-dedicated-icon" strokeWidth={1.75} />
        <span>{props.item.label}</span>
        <Show when={props.item.trailing}>
          <ChevronRight class="size-4 core-sidebar-muted-chevron" strokeWidth={1.9} />
        </Show>
      </A>
    </Show>
  )
}

function TicketingSidebarSubItem(props: { active: boolean; item: TicketingSidebarSubItem }) {
  return (
    <A
      href={props.item.href}
      class={cn('core-sidebar-subnav-link', props.active && 'core-sidebar-subnav-link--active')}
    >
      {props.item.label}
    </A>
  )
}

function getTicketingActiveParams(search: string) {
  const params = new URLSearchParams(search)
  return {
    queue: params.get('queue') ?? 'my',
    status: params.get('status'),
    view: params.get('view'),
    team: params.get('team'),
    label: params.get('label'),
    slaState: params.get('sla_state'),
  }
}

type TicketingActiveParams = ReturnType<typeof getTicketingActiveParams>

function isTicketingItemActive(item: TicketingSidebarItem, active: TicketingActiveParams) {
  const href = new URL(item.href, 'https://velion.local')
  const itemQueue = href.searchParams.get('queue') ?? 'my'
  const itemParams = {
    status: href.searchParams.get('status'),
    view: href.searchParams.get('view'),
    team: href.searchParams.get('team'),
    label: href.searchParams.get('label'),
    slaState: href.searchParams.get('sla_state'),
  }
  const hasSpecificParam = Object.values(itemParams).some(Boolean)
  if (hasSpecificParam) {
    return itemQueue === active.queue &&
      itemParams.status === active.status &&
      itemParams.view === active.view &&
      itemParams.team === active.team &&
      itemParams.label === active.label &&
      itemParams.slaState === active.slaState
  }
  if (item.id === 'my') {
    return active.queue === 'my' && !active.status && !active.view && !active.team && !active.label && !active.slaState
  }
  return !active.status && !active.view && !active.team && !active.label && !active.slaState && itemQueue === active.queue
}

function isTicketingSubItemActive(item: TicketingSidebarSubItem, active: TicketingActiveParams) {
  const href = new URL(item.href, 'https://velion.local')
  const itemQueue = href.searchParams.get('queue') ?? 'my'
  const itemStatus = href.searchParams.get('status')
  const itemView = href.searchParams.get('view')
  const itemTeam = href.searchParams.get('team')
  const itemLabel = href.searchParams.get('label')
  const itemSlaState = href.searchParams.get('sla_state')
  return active.queue === itemQueue &&
    active.status === itemStatus &&
    active.view === itemView &&
    active.team === itemTeam &&
    active.label === itemLabel &&
    active.slaState === itemSlaState
}

function getDefaultTicketingExpandedItems() {
  const expandedItems: Record<string, boolean> = {}
  for (const group of ticketingSidebarGroups) {
    for (const item of group.items) {
      if (item.subItems?.length) expandedItems[item.id] = item.id === 'my'
    }
  }
  return expandedItems
}

function getVisibleTicketingGroups(sourceGroups: TicketingSidebarGroup[], normalizedSearch: string) {
  return sourceGroups.reduce<TicketingSidebarGroup[]>((groups, group) => {
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

const ticketingLabels: Record<string, readonly [string, string]> = {
  'group:ticketing-tickets': ['Saker', 'Tickets'],
  'group:ticketing-saved-views': ['Lagrede visninger', 'Saved views'],
  'group:ticketing-automation': ['Automatisering', 'Automation'],
  'group:ticketing-teams': ['Team', 'Teams'],
  'group:ticketing-sla': ['SLA', 'SLA'],
  'group:ticketing-labels': ['Etiketter', 'Labels'],
  'item:suggested': ['Foreslått av AI', 'Suggested by AI'],
  'item:my': ['Mine saker', 'My tickets'],
  'item:unassigned': ['Uten eier', 'Unassigned'],
  'item:sla-risk': ['SLA-risiko', 'SLA risk'],
  'item:escalated': ['Eskalert', 'Escalated'],
  'item:waiting-customer': ['Venter på kunde', 'Waiting on customer'],
  'item:waiting-team': ['Venter på team', 'Waiting on team'],
  'item:resolved': ['Løst', 'Resolved'],
  'item:breached-sla': ['Brutt SLA', 'Breached SLA'],
  'item:refund-handoffs': ['Refusjonsoverleveringer', 'Refund handoffs'],
  'item:linked-social': ['Tilkoblede sosiale poster', 'Linked social posts'],
  'item:external-links': ['Eksterne ressurser', 'External resources'],
  'item:rules': ['Regler/køer', 'Rules / queues'],
  'item:macros': ['Makroer', 'Macros'],
  'item:classifications': ['AI-klassifiseringer', 'AI classifications'],
  'item:support-team': ['Supportteam', 'Support team'],
  'item:billing-team': ['Faktureringsteam', 'Billing team'],
  'item:sla-policies': ['SLA-policyer', 'SLA policies'],
  'item:label-refund': ['Refusjon', 'Refund'],
  'item:label-security': ['Sikkerhet', 'Security'],
  'item:label-delivery': ['Levering', 'Delivery'],
  'sub:my-all': ['Alle saker', 'All tickets'],
  'sub:my-open': ['Åpne', 'Open'],
  'sub:my-waiting-customer': ['Venter på kunde', 'Waiting on customer'],
  'sub:my-waiting-team': ['Venter på team', 'Waiting on team'],
  'sub:my-resolved': ['Løst', 'Resolved'],
}

function localizeTicketingGroups(i18n: ReturnType<typeof useI18n>): TicketingSidebarGroup[] {
  return ticketingSidebarGroups.map((group) => ({
    ...group,
    label: localizeTicketingLabel(`group:${group.id}`, group.label, i18n),
    items: group.items.map((item) => ({
      ...item,
      label: localizeTicketingLabel(`item:${item.id}`, item.label, i18n),
      subItems: item.subItems?.map((subItem) => ({
        ...subItem,
        label: localizeTicketingLabel(`sub:${subItem.id}`, subItem.label, i18n),
      })),
    })),
  }))
}

function localizeTicketingLabel(key: string, fallback: string, i18n: ReturnType<typeof useI18n>): string {
  const label = ticketingLabels[key]
  return label ? i18n.tr(label[0], label[1]) : fallback
}
