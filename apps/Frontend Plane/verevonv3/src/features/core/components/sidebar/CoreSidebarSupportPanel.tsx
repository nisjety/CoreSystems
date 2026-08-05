import { useLocation, useNavigate } from '@solidjs/router'
import {
  AlertTriangle,
  Bot,
  CheckCheck,
  ChevronDown,
  CircleAlert,
  CircleCheck,
  CircleHelp,
  Clock3,
  Inbox,
  List,
  ListChecks,
  LoaderCircle,
  Send,
  ShieldAlert,
  TicketCheck,
  UserRound,
  UsersRound,
} from 'lucide-solid'
import { createMemo, createResource, createSignal, For, Show, type Component, type JSX } from 'solid-js'
import type { LucideProps } from 'lucide-solid'
import { Dynamic } from 'solid-js/web'
import {
  SidebarPanelTitle,
  SidebarSearchField,
} from '@/features/core/components/sidebar/CoreSidebarPrimitives'
import { useI18n } from '@/shared/i18n'
import { cn } from '@/shared/lib/cn'
import { supportProviderIcon, type SupportProvider } from '@/features/support/components/SupportProviderIcon'
import { deriveConnectedEmailAccounts, deriveConnectedInboxSources, type ConnectedEmailAccount, type EmailAccountSyncHealth } from '@/features/inbox/lib/inbox-sources'
import { listConnections, type IntegrationConnection } from '@/shared/api/integrations-client'
import { getSession } from '@/shared/session/session-store'

type SupportIcon = Component<LucideProps>
type SupportMode = 'conversations' | 'tickets' | 'outbound'

type SupportFilter = {
  id: string
  label: string
  href: string
  icon: SupportIcon
}

type ConnectionCatalog = {
  connections: IntegrationConnection[]
  unavailable: boolean
}

const conversationChannelCatalog: Array<{ channel: SupportProvider; label: string }> = [
  { channel: 'messenger', label: 'Messenger' },
  { channel: 'instagram', label: 'Instagram' },
  { channel: 'whatsapp', label: 'WhatsApp' },
  { channel: 'threads', label: 'Threads' },
  { channel: 'slack', label: 'Slack' },
  { channel: 'teams', label: 'Microsoft Teams' },
  { channel: 'discord', label: 'Discord' },
  { channel: 'linkedin', label: 'LinkedIn' },
  { channel: 'x', label: 'Twitter / X' },
  { channel: 'sms', label: 'SMS' },
]

/**
 * The Support sidebar deliberately contains operational filters, not product
 * administration. Conversation filters follow the personal/shared ownership
 * model used by Intercom, Gorgias, and Outlook; ticket filters focus on the
 * active queues and SLA work that an operator can act on.
 */
export function SupportExpandedSidebarPanel(props: { onCollapse: () => void }) {
  const i18n = useI18n()
  const location = useLocation()
  const session = getSession()
  const [searchQuery, setSearchQuery] = createSignal('')
  const orgId = () => session.activeOrg?.id ?? ''
  const [connectionCatalog, { refetch: refetchConnections }] = createResource<ConnectionCatalog, string>(orgId, async (id) => {
    if (!id) return { connections: [], unavailable: false }
    try {
      return { connections: await listConnections(id), unavailable: false }
    } catch {
      return { connections: [], unavailable: true }
    }
  })
  const connections = () => connectionCatalog()?.connections ?? []
  const connectionStatusUnavailable = () => connectionCatalog()?.unavailable ?? false
  const emailAccounts = createMemo(() => deriveConnectedEmailAccounts(connections()))
  const mode = (): SupportMode => {
    const surface = new URLSearchParams(location.search).get('surface')
    if (surface === 'outbound') return 'outbound'
    if (surface === 'review') return 'tickets'
    if (location.pathname.startsWith('/tickets') || surface === 'tickets') return 'tickets'
    return 'conversations'
  }

  const conversationQueues = (): SupportFilter[] => [
    { id: 'my-conversations', label: i18n.tr('Mine samtaler', 'My conversations'), href: '/support?view=mine', icon: Inbox },
    { id: 'unassigned-conversations', label: i18n.tr('Uten eier', 'Unassigned'), href: '/support?view=unassigned', icon: UserRound },
    { id: 'all-conversations', label: i18n.tr('Alle samtaler', 'All conversations'), href: '/support?view=all', icon: List },
  ]
  const conversationStatuses = (): SupportFilter[] => [
    { id: 'open-conversations', label: i18n.tr('Alle åpne', 'All open'), href: '/support?view=all&status=open', icon: List },
    { id: 'waiting-conversations', label: i18n.tr('Venter', 'Waiting'), href: '/support?view=all&status=pending', icon: Clock3 },
    { id: 'resolved-conversations', label: i18n.tr('Løst', 'Resolved'), href: '/support?view=all&status=solved', icon: CheckCheck },
  ]
  const conversationChannels = (): SupportFilter[] => {
    const connectedLabels = new Map<string, string>(deriveConnectedInboxSources(connections())
      .filter((source) => source.channel !== 'email')
      .map((source) => [source.channel, source.label]))
    return conversationChannelCatalog.map(({ channel, label }) => ({
      id: channel,
      label: connectedLabels.get(channel) ?? label,
      href: `/support?view=all&channel=${channel}`,
      icon: supportProviderIcon(channel),
    }))
  }
  const ticketQueues = (): SupportFilter[] => [
    { id: 'all-tickets', label: i18n.tr('Alle saker', 'All tickets'), href: '/support?surface=tickets&queue=all', icon: List },
    { id: 'my-tickets', label: i18n.tr('Mine saker', 'My tickets'), href: '/support?surface=tickets&queue=my', icon: TicketCheck },
    { id: 'unassigned-tickets', label: i18n.tr('Ikke tildelt', 'Unassigned'), href: '/support?surface=tickets&queue=unassigned', icon: Inbox },
    { id: 'suggested', label: i18n.tr('Foreslått av AI', 'Suggested by AI'), href: '/support?surface=tickets&queue=suggested', icon: Bot },
  ]
  const ticketWorkflow = (): SupportFilter[] => [
    { id: 'at-risk', label: i18n.tr('SLA-risiko', 'SLA risk'), href: '/support?surface=tickets&queue=sla-risk&sla_state=risk', icon: AlertTriangle },
    { id: 'escalated', label: i18n.tr('Eskalert', 'Escalated'), href: '/support?surface=tickets&queue=escalated', icon: ShieldAlert },
    { id: 'waiting-customer', label: i18n.tr('Venter på kunde', 'Waiting on customer'), href: '/support?surface=tickets&queue=waiting-customer', icon: Clock3 },
    { id: 'waiting-team', label: i18n.tr('Venter på team', 'Waiting on team'), href: '/support?surface=tickets&queue=waiting-team', icon: UsersRound },
    { id: 'resolved', label: i18n.tr('Løst', 'Resolved'), href: '/support?surface=tickets&queue=resolved', icon: CheckCheck },
  ]
  const ticketWorkTypes = (): SupportFilter[] => [
    { id: 'customer-cases', label: i18n.tr('Kundesaker', 'Customer cases'), href: '/support?surface=tickets&work_type=customer_case', icon: TicketCheck },
    { id: 'internal-work', label: i18n.tr('Internt arbeid', 'Internal work'), href: '/support?surface=tickets&work_type=internal_work', icon: ListChecks },
    { id: 'incidents', label: i18n.tr('Hendelser', 'Incidents'), href: '/support?surface=tickets&work_type=incident', icon: ShieldAlert },
  ]
  const ticketMore = (): SupportFilter[] => [
    { id: 'breached', label: i18n.tr('Brutt SLA', 'Breached SLA'), href: '/support?surface=tickets&queue=sla-risk&sla_state=breached', icon: AlertTriangle },
    { id: 'rules', label: i18n.tr('Regler / køer', 'Rules / queues'), href: '/support?surface=tickets&queue=rules', icon: ListChecks },
  ]
  const outboundQueues = (): SupportFilter[] => [
    { id: 'outbound-all', label: i18n.tr('Alle kvitteringer', 'All receipts'), href: '/support?surface=outbound', icon: ListChecks },
    { id: 'outbound-sending', label: i18n.tr('Sender', 'Sending'), href: '/support?surface=outbound&outbound_status=sending', icon: Send },
    { id: 'outbound-retryable', label: i18n.tr('Kan prøves igjen', 'Retryable'), href: '/support?surface=outbound&outbound_status=retryable', icon: Clock3 },
    { id: 'outbound-submitted', label: i18n.tr('Godtatt av leverandør', 'Provider accepted'), href: '/support?surface=outbound&outbound_status=submitted', icon: CheckCheck },
    { id: 'outbound-failed', label: i18n.tr('Mislyktes', 'Failed'), href: '/support?surface=outbound&outbound_status=failed', icon: AlertTriangle },
    { id: 'outbound-unknown', label: i18n.tr('Ukjent utfall', 'Unknown outcome'), href: '/support?surface=outbound&outbound_status=unknown', icon: AlertTriangle },
  ]
  const outboundDelivery = (): SupportFilter[] => [
    { id: 'outbound-unconfirmed', label: i18n.tr('Ikke bekreftet', 'Unconfirmed'), href: '/support?surface=outbound&outbound_delivery_status=unconfirmed', icon: Clock3 },
    { id: 'outbound-delivered', label: i18n.tr('Levert', 'Delivered'), href: '/support?surface=outbound&outbound_delivery_status=delivered', icon: CheckCheck },
    { id: 'outbound-read', label: i18n.tr('Lest', 'Read'), href: '/support?surface=outbound&outbound_delivery_status=read', icon: CheckCheck },
    { id: 'outbound-delivery-failed', label: i18n.tr('Levering mislyktes', 'Delivery failed'), href: '/support?surface=outbound&outbound_delivery_status=failed', icon: AlertTriangle },
  ]
  const filterQuery = () => searchQuery().trim().toLocaleLowerCase()
  const matchesSearch = (items: SupportFilter[]) => {
    const query = filterQuery()
    return query ? items.filter((item) => item.label.toLocaleLowerCase().includes(query)) : items
  }
  const currentHref = createMemo(() => `${location.pathname}${location.search}`)

  const isActive = (href: string) => {
    const target = new URL(href, 'https://verevon.local')
    const current = new URL(currentHref(), 'https://verevon.local')
    const targetSurface = target.searchParams.get('surface')
    const currentTicketMode = current.pathname.startsWith('/tickets') || current.searchParams.get('surface') === 'tickets'
    if ((targetSurface === 'tickets') !== currentTicketMode) return false

    for (const [key, value] of target.searchParams.entries()) {
      if (current.searchParams.get(key) !== value) return false
    }

    if (targetSurface !== 'tickets') {
      const targetView = target.searchParams.get('view') ?? 'mine'
      const currentView = current.searchParams.get('view') ?? 'mine'
      return targetView === currentView
        && (target.searchParams.get('channel') ?? '') === (current.searchParams.get('channel') ?? '')
        && (target.searchParams.get('status') ?? '') === (current.searchParams.get('status') ?? '')
    }

    const ticketFilterKeys = ['queue', 'status', 'work_type', 'team', 'label', 'priority', 'severity', 'sla_state', 'assigned', 'view'] as const
    return ticketFilterKeys.every((key) => (target.searchParams.get(key) ?? '') === (current.searchParams.get(key) ?? ''))
  }

  return (
    <div class="core-sidebar-dedicated-panel">
      <SidebarPanelTitle onCollapse={props.onCollapse}>{i18n.tr('Support', 'Support')}</SidebarPanelTitle>
      <SidebarSearchField
        ariaLabel={i18n.tr('Filtrer supportarbeid', 'Filter support work')}
        class="core-sidebar-search-spacious"
        value={searchQuery()}
        onChange={setSearchQuery}
        placeholder={i18n.tr('Filtrer samtaler, saker eller utgående', 'Filter conversations, tickets, or outbound')}
      />
      <Show when={connectionStatusUnavailable()}>
        <div class="core-sidebar-support-source-status" role="alert">
          <CircleAlert class="size-3.5" strokeWidth={1.8} />
          <span>{i18n.tr('Tilkoblingsstatus kunne ikke lastes. Køene er fortsatt tilgjengelige.', 'Connection status could not be loaded. Queues remain available.')}</span>
          <button type="button" onClick={() => void refetchConnections()}>
            {i18n.tr('Prøv igjen', 'Retry')}
          </button>
        </div>
      </Show>

      <nav class="core-sidebar-dedicated-nav core-sidebar-support-nav" aria-label={i18n.tr('Supportnavigasjon', 'Support navigation')}>
        <div class="core-sidebar-dedicated-nav__stack">
          <Show when={mode() === 'conversations'}>
            <SupportFilterGroup defaultOpen items={matchesSearch(conversationQueues())} label={i18n.tr('Køer', 'Queues')} activeHref={isActive} searching={Boolean(filterQuery())} />
            <SupportFilterGroup defaultOpen items={matchesSearch(conversationStatuses())} label={i18n.tr('Status', 'Status')} activeHref={isActive} searching={Boolean(filterQuery())} />
            <EmailAccountFilter accounts={emailAccounts()} activeHref={isActive} searching={Boolean(filterQuery())} searchQuery={filterQuery()} />
            <SupportFilterGroup defaultOpen items={matchesSearch(conversationChannels())} label={i18n.tr('Kanaler', 'Channels')} activeHref={isActive} searching={Boolean(filterQuery())} />
          </Show>

          <Show when={mode() === 'tickets'}>
            <SupportFilterGroup defaultOpen items={matchesSearch(ticketQueues())} label={i18n.tr('Køer', 'Queues')} activeHref={isActive} searching={Boolean(filterQuery())} />
            <SupportFilterGroup defaultOpen items={matchesSearch(ticketWorkflow())} label={i18n.tr('Arbeidsflyt', 'Workflow')} activeHref={isActive} searching={Boolean(filterQuery())} />
            <SupportFilterGroup items={matchesSearch(ticketWorkTypes())} label={i18n.tr('Arbeidstype', 'Work type')} activeHref={isActive} searching={Boolean(filterQuery())} />
            <SupportFilterGroup defaultOpen items={matchesSearch(ticketMore())} label={i18n.tr('Verktøy', 'Tools')} activeHref={isActive} searching={Boolean(filterQuery())} />
          </Show>

          <Show when={mode() === 'outbound'}>
            <SupportFilterGroup defaultOpen items={matchesSearch(outboundQueues())} label={i18n.tr('Utgående status', 'Outbound state')} activeHref={isActive} searching={Boolean(filterQuery())} />
            <SupportFilterGroup defaultOpen items={matchesSearch(outboundDelivery())} label={i18n.tr('Leveringsbevis', 'Delivery evidence')} activeHref={isActive} searching={Boolean(filterQuery())} />
          </Show>
        </div>
      </nav>
    </div>
  )
}

function EmailAccountFilter(props: {
  accounts: ReturnType<typeof deriveConnectedEmailAccounts>
  activeHref: (href: string) => boolean
  searching: boolean
  searchQuery: string
}) {
  const i18n = useI18n()
  const [open, setOpen] = createSignal(true)
  const matchingAccounts = () => props.searchQuery
    ? props.accounts.filter((account) => `${account.label} ${account.providerKey} ${account.sharedMailboxes.join(' ')}`.toLocaleLowerCase().includes(props.searchQuery))
    : props.accounts
  const expanded = () => props.searching || open()
  return (
    <Show when={matchingAccounts().length > 0}>
      <section class="core-sidebar-group core-sidebar-email-group">
        <div class="core-sidebar-group__header">
          <button type="button" aria-expanded={expanded()} onClick={() => setOpen((value) => !value)}>
            <span class="verevon-sidebar-group-title">{i18n.tr('E-post', 'Email')}</span>
            <ChevronDown class={cn('size-3.5', !expanded() && '-rotate-90')} strokeWidth={1.8} />
          </button>
        </div>
        <Show when={expanded()}>
          <div class="core-sidebar-email-accounts">
            <For each={matchingAccounts()}>
              {(account) => {
                const href = `/support?view=all&channel=email&connection_id=${encodeURIComponent(account.id)}`
                return <div class="core-sidebar-email-account">
                  <SupportSidebarLink href={href} active={props.activeHref(href)} class={cn('core-sidebar-panel-link core-sidebar-email-account__link', props.activeHref(href) && 'verevon-sidebar-panel-active core-sidebar-panel-link--active')}>
                    <Dynamic component={supportProviderIcon(account.providerKey)} class="core-sidebar-panel-link__icon" strokeWidth={1.7} />
                    <span class="core-sidebar-panel-link__label" title={account.label}>{account.label}</span>
                  </SupportSidebarLink>
                  <EmailAccountHealthBadge account={account} />
                  <Show when={account.sharedMailboxes.length > 0}>
                    <div class="core-sidebar-email-account__shared" aria-label={i18n.tr('Delte postbokser', 'Shared mailboxes')}>
                      <For each={account.sharedMailboxes}>
                        {(mailbox) => <SupportSidebarLink href={href} class="core-sidebar-email-account__shared-link" title={i18n.tr('Viser denne tilkoblingens postbokser', 'Shows this connection’s mailboxes')}>
                          <span aria-hidden="true">+</span>{mailbox}
                        </SupportSidebarLink>}
                      </For>
                    </div>
                  </Show>
                </div>
              }}
            </For>
          </div>
        </Show>
      </section>
    </Show>
  )
}

export function EmailAccountHealthBadge(props: { account: ConnectedEmailAccount }) {
  const i18n = useI18n()
  const health = (): EmailAccountSyncHealth => props.account.syncHealth
  const state = (): { icon: SupportIcon; label: string; title: string } => {
    const lastSync = props.account.lastSyncAt
      ? new Date(props.account.lastSyncAt).toLocaleString()
      : null
    switch (health()) {
      case 'synced':
        return {
          icon: CircleCheck,
          label: i18n.tr('Synkronisert', 'Synced'),
          title: i18n.tr(
            `Siste innbokssynk ble fullført${lastSync ? ` ${lastSync}` : ''}. Dette er ikke bevis på levering eller lesing.`,
            `The last inbox sync completed${lastSync ? ` ${lastSync}` : ''}. This is not proof of delivery or read.`,
          ),
        }
      case 'syncing':
        return {
          icon: LoaderCircle,
          label: i18n.tr('Synkroniserer', 'Syncing'),
          title: i18n.tr('Innbokssynk kjører. Eksisterende samtaler er fortsatt tilgjengelige.', 'An inbox sync is running. Existing conversations remain available.'),
        }
      case 'needs_reconnect':
        return {
          icon: CircleAlert,
          label: i18n.tr('Koble til på nytt', 'Reconnect'),
          title: i18n.tr('Tilkoblingen trenger ny autorisasjon før nye e-poster kan hentes.', 'This connection needs authorization again before new mail can be fetched.'),
        }
      case 'attention':
        return {
          icon: CircleAlert,
          label: i18n.tr('Trenger oppmerksomhet', 'Needs attention'),
          title: i18n.tr('Siste innbokssynk mislyktes. Eksisterende samtaler er uendret.', 'The latest inbox sync failed. Existing conversations are unchanged.'),
        }
      default:
        return {
          icon: CircleHelp,
          label: i18n.tr('Status ukjent', 'Status unavailable'),
          title: i18n.tr('Verevon har ikke en bekreftet status for siste innbokssynk.', 'Verevon has no confirmed state for the latest inbox sync.'),
        }
    }
  }

  return <span
    class={cn('core-sidebar-email-account__health', `core-sidebar-email-account__health--${health()}`)}
    title={state().title}
  >
    <Dynamic component={state().icon} class={cn('size-3', health() === 'syncing' && 'animate-spin')} strokeWidth={1.8} />
    <span>{state().label}</span>
  </span>
}

function SupportFilterGroup(props: { activeHref: (href: string) => boolean; defaultOpen?: boolean; items: SupportFilter[]; label: string; searching: boolean }) {
  const [open, setOpen] = createSignal(Boolean(props.defaultOpen))
  const expanded = () => props.searching || open()
  return (
    <Show when={props.items.length > 0}>
      <section class="core-sidebar-group">
        <div class="core-sidebar-group__header">
          <button type="button" aria-expanded={expanded()} onClick={() => setOpen((value) => !value)}>
            <span class="verevon-sidebar-group-title">{props.label}</span>
            <ChevronDown class={cn('size-3.5', !expanded() && '-rotate-90')} strokeWidth={1.8} />
          </button>
        </div>
        <Show when={expanded()}>
          <div class="core-sidebar-group__items">
            <For each={props.items}>
              {(item) => <SupportFilterLink active={props.activeHref(item.href)} item={item} />}
            </For>
          </div>
        </Show>
      </section>
    </Show>
  )
}

function SupportFilterLink(props: { active: boolean; item: SupportFilter }) {
  return (
    <SupportSidebarLink href={props.item.href} active={props.active} class={cn('core-sidebar-panel-link', props.active && 'verevon-sidebar-panel-active core-sidebar-panel-link--active')}>
      <Dynamic component={props.item.icon} class="core-sidebar-panel-link__icon" strokeWidth={1.7} />
      <span class="core-sidebar-panel-link__label">{props.item.label}</span>
    </SupportSidebarLink>
  )
}

function SupportSidebarLink(props: { active?: boolean; children: JSX.Element; class: string; href: string; title?: string }) {
  const navigate = useNavigate()
  return (
    <a
      href={props.href}
      class={props.class}
      title={props.title}
      aria-current={props.active ? 'page' : undefined}
      onClick={(event) => {
        if (event.defaultPrevented || event.button !== 0 || event.metaKey || event.altKey || event.ctrlKey || event.shiftKey) return
        event.preventDefault()
        void navigate(props.href)
      }}
    >
      {props.children}
    </a>
  )
}
