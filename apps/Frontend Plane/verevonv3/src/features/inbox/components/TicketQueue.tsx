import { type Component, For, Show, createEffect, createMemo, createSignal } from 'solid-js'
import {
  ArrowDown,
  ArrowDownUp,
  ArrowUp,
  Check,
  Clock3,
  X,
  Inbox,
  MailOpen,
  MessageCircle,
  MoreHorizontal,
  Pin,
  Plus,
  RefreshCw,
  Search,
  SlidersHorizontal,
  type LucideProps,
} from '@/shared/icons'
import type { JSX } from '@solidjs/web'
import { Dynamic } from '@solidjs/web'
import {
  formatRelativeTime,
  type InboxTab,
  type ZammadTicket,
} from '@/features/inbox/lib/inbox-model'
import type { ConnectedEmailAccount, ConnectedInboxSource } from '@/features/inbox/lib/inbox-sources'
import { supportProviderIcon } from '@/features/support/components/SupportProviderIcon'
import { cn } from '@/shared/lib/cn'
import { useI18n } from '@/shared/i18n'

type SortKey =
  | 'last-message-desc'
  | 'last-message-asc'
  | 'created-desc'
  | 'created-asc'
  | 'priority-desc'
  | 'priority-asc'

type QuickFilter = 'all' | 'unread' | 'pinned'
type InboxRefreshTarget = {
  channel: 'email' | 'teams' | 'slack'
  connectionIds: string[]
  label: string
}

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
  activeEmailAccount?: ConnectedEmailAccount | null
  activeTab: InboxTab
  connectingInboxProvider?: string | null
  connectionStatusMessage?: string | null
  connectionStatusUnavailable?: boolean
  connectedSources: ConnectedInboxSource[]
  error: string | null
  hasMore: boolean
  label: string
  loading: boolean
  loadingMore: boolean
  metaSetupRequired?: 'instagram' | 'messenger' | 'whatsapp' | null
  discordSetupRequired?: boolean
  onActiveTabChange: (tab: InboxTab) => void
  onConnectInbox?: (provider: 'google' | 'microsoft') => void
  onAddSharedMailbox?: () => void
  onClearActiveEmailAccount?: () => void
  onLoadMore: () => void
  onMarkRead: (ticket: ZammadTicket) => void
	/** Requests an actual provider fetch through the inbox worker. The handler
	 * owns the durable job receipt; this component only supplies the visible
	 * channel and its already-authorized connection ids. */
	onRefreshInbox?: (channel: 'email' | 'teams' | 'slack', connectionIds: string[]) => void
  onSearchChange: (query: string) => void
  onRetryConnectionStatus?: () => void
  onSelectTicket: (ticket: ZammadTicket) => void
  onSnoozeTicket: (ticket: ZammadTicket) => void
  onTogglePinned: (ticket: ZammadTicket) => void
  pinnedConversationIds: string[]
	providerRefreshNotice?: string | null
	refreshingInbox?: boolean
  readConversationIds: string[]
  searchQuery: string
  selectedTicketId: number | null
  tickets: ZammadTicket[]
}) {
  let filtersRef: HTMLDivElement | undefined
  let sortRef: HTMLDivElement | undefined
  const [filtersOpen, setFiltersOpen] = createSignal(false)
  const [quickFilter, setQuickFilter] = createSignal<QuickFilter>('all')
  const [sortKey, setSortKey] = createSignal<SortKey>('last-message-desc')
  const [sortOpen, setSortOpen] = createSignal(false)
  const i18n = useI18n()
  const pinnedConversationIds = createMemo(() => new Set(props.pinnedConversationIds))
  const readConversationIds = createMemo(() => new Set(props.readConversationIds))

  const quickFilters = createMemo<Array<{ id: QuickFilter; label: string; icon?: Component<LucideProps> }>>(() => [
    { id: 'all', label: i18n.tr('Alle', 'All') },
    { id: 'unread', label: i18n.tr('Ulest', 'Unread'), icon: MailOpen },
    { id: 'pinned', label: i18n.tr('Festet', 'Pinned'), icon: Pin },
  ])

  const visibleTickets = createMemo(() => props.tickets.filter((ticket) => {
    if (quickFilter() === 'unread' && readConversationIds().has(ticketPreferenceId(ticket))) return false
    if (quickFilter() === 'pinned' && !pinnedConversationIds().has(ticketPreferenceId(ticket))) return false
    return true
  }))
  const sortedTickets = createMemo(() => sortTickets(visibleTickets(), sortKey(), pinnedConversationIds()))
  const activeSources = createMemo(() => props.connectedSources.filter((source) => source.channel === props.activeChannel))
  const inboxRefreshTarget = createMemo<InboxRefreshTarget | null>(() => {
		const channel = props.activeChannel === 'email'
			? 'email'
			: props.activeChannel === 'teams'
				? 'teams'
				: props.activeChannel === 'slack'
					? 'slack'
					: null
		if (!props.onRefreshInbox || !channel) return null
		if (channel === 'email' && props.activeEmailAccount) {
			return { channel, connectionIds: [props.activeEmailAccount.id], label: props.activeEmailAccount.label }
		}
		const connectionIds = [...new Set(activeSources().flatMap((source) => source.connectionIds))]
		if (connectionIds.length === 0) return null
		return { channel, connectionIds, label: activeSources().map((source) => source.label).join(' + ') || channel }
	})
  const connectedEmptyState = createMemo(() => {
    if (props.tickets.length > 0 || activeSources().length === 0) return null
    const labels = activeSources().map((source) => source.label)
    const sourceList = formatSourceList(labels, i18n)
    if (props.activeChannel === 'instagram') {
      return {
        title: i18n.tr('Instagram-kontoen er tilkoblet', 'Instagram account is connected'),
        body: i18n.tr(
          'Ingen samtaler er mottatt ennå. Kontroller webhook-levering og Metas test- eller publiseringsstatus før du forventer nye meldinger.',
          'No conversations have been received yet. Verify webhook delivery and Meta app testing or publish status before expecting new messages.',
        ),
      }
    }
    if (props.activeChannel === 'discord') {
      return {
        title: i18n.tr('Discord-tilkoblingen er autorisert', 'Discord connection is authorized'),
        body: i18n.tr(
          'Ingen meldinger er mottatt ennå. Discord-innboksen krever at Verevon-boten er installert på serveren, Message Content Intent er aktivert, DISCORD_BOT_TOKEN er konfigurert i den lokale Docker-stacken, og at en administrert server er bundet til tilkoblingen.',
          'No messages have arrived yet. The Discord inbox requires the Verevon bot to be installed in the server, Message Content Intent to be enabled, DISCORD_BOT_TOKEN configured in the local Docker stack, and a managed server bound to the connection.',
        ),
      }
    }
    if (props.activeChannel === 'slack') {
      return {
        title: i18n.tr('Slack er tilkoblet', 'Slack is connected'),
        body: i18n.tr(
          'Ingen meldinger er mottatt ennå. Slack-innboksen leser kanaler, direktemeldinger og gruppesamtaler Verevon-appen har tilgang til. Inviter appen til kanalen og send en ny melding før du forventer innhold her.',
          'No messages have arrived yet. The Slack inbox reads channels, direct messages, and group conversations that the Verevon app can access. Invite the app to the channel and send a new message before expecting content here.',
        ),
      }
    }
    if (props.activeChannel === 'x') {
      return {
        title: i18n.tr('Twitter / X er tilkoblet', 'Twitter / X is connected'),
        body: i18n.tr(
          'Ingen direktemeldinger er mottatt ennå. X DM-innhenting krever X API Pro eller høyere, og starter først når kontoen har tilgang til dette nivået.',
          'No direct messages have arrived yet. X DM ingestion requires X API Pro or above and starts only when the account has access to that tier.',
        ),
      }
    }
    return {
      title: i18n.tr(`${sourceList} er tilkoblet`, `${sourceList} ${labels.length === 1 ? 'is' : 'are'} connected`),
      body: i18n.tr(
        'Ingen samtaler har kommet inn i denne kilden ennå. Nye meldinger vises her så snart innhentingen leverer dem.',
        'No conversations have arrived in this source yet. New messages will appear here as soon as ingestion delivers them.',
      ),
    }
  })
  const linkedinSetupEmptyState = createMemo(() => {
    if (props.tickets.length > 0 || props.activeChannel !== 'linkedin' || activeSources().length > 0) return null
    return {
      title: i18n.tr('LinkedIn er ikke klar for innboks', 'LinkedIn inbox is not enabled'),
      body: i18n.tr(
        'LinkedIn er tilgjengelig som kanalfilter, men den lokale innboks-innhentingen for LinkedIn er ikke aktivert ennå. Den nåværende tilkoblingen brukes til identitet og sosiale publiseringsflyter.',
        'LinkedIn is available as a channel filter, but local LinkedIn inbox ingestion is not enabled yet. The current connection is used for identity and social publishing workflows.',
      ),
    }
  })
  const metaSetupEmptyState = createMemo(() => {
    if (props.tickets.length > 0 || !props.metaSetupRequired) return null
    if (props.metaSetupRequired === 'messenger') {
      return {
        title: i18n.tr('Messenger trenger en Facebook-side', 'Messenger needs a Facebook Page'),
        body: i18n.tr(
          'Meta-innloggingen er fullført, men ingen administrerbar Facebook-side ble funnet. Gi den godkjente Facebook-brukeren full kontroll over siden, og synkroniser deretter Meta i Integrasjoner.',
          'Meta consent is complete, but no manageable Facebook Page was found. Give the approved Facebook user full control of the Page, then sync Meta in Integrations.',
        ),
      }
    }
    if (props.metaSetupRequired === 'instagram') {
      return {
        title: i18n.tr('Instagram trenger en koblet profesjonell konto', 'Instagram needs a linked professional account'),
        body: i18n.tr(
          'Koble en Instagram Business- eller Creator-konto til Facebook-siden, og synkroniser deretter Meta i Integrasjoner. Ikke behandle Instagram som klar før en konto er oppdaget.',
          'Link an Instagram Business or Creator account to the Facebook Page, then sync Meta in Integrations. Instagram is not ready until an account is discovered.',
        ),
      }
    }
    return null
  })
  const discordSetupEmptyState = createMemo(() => {
    if (props.tickets.length > 0 || !props.discordSetupRequired) return null
    return {
      title: i18n.tr('Discord trenger oppsett for innboks', 'Discord inbox needs setup'),
      body: i18n.tr(
        'Koble til Discord i Integrasjoner, installer Verevon-boten på en server, aktiver Message Content Intent, og konfigurer DISCORD_BOT_TOKEN i den lokale Docker-stacken. Meldinger vises når serveren er bundet og synkroniseringen er frisk.',
        'Connect Discord in Integrations, install the Verevon bot in a server, enable Message Content Intent, and configure DISCORD_BOT_TOKEN in the local Docker stack. Messages appear after the server is bound and sync is healthy.',
      ),
    }
  })

  createEffect(
    () => ({ filtersOpen: filtersOpen(), sortOpen: sortOpen() }),
    ({ filtersOpen, sortOpen }) => {
      if (!filtersOpen && !sortOpen) return

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
      return () => {
        document.removeEventListener('mousedown', closeOnOutside)
        document.removeEventListener('keydown', closeOnEscape)
      }
    },
  )

  return (
    <section class="verevon-inbox-queue" aria-label={props.label}>
      <div class="verevon-inbox-queue__topbar">
        <div class="verevon-inbox-queue__title">
          <button
            type="button"
            class="verevon-inbox-title-action"
            onClick={props.onAddSharedMailbox}
            aria-label={i18n.tr('Legg til delt postboks', 'Add shared mailbox')}
            title={i18n.tr('Legg til delt postboks', 'Add shared mailbox')}
          >
            <Plus class="size-4" strokeWidth={2} />
          </button>
          <Inbox class="size-4" strokeWidth={2} />
          <h1 title={props.activeEmailAccount?.label}>
            {props.activeEmailAccount?.label ?? i18n.tr('Innboks', 'Inbox')}
          </h1>
          <Show when={props.activeEmailAccount}>
            <button
                type="button"
                class="verevon-inbox-title-clear"
                onClick={props.onClearActiveEmailAccount}
                aria-label={i18n.tr('Vis alle e-postkontoer', 'Show all email accounts')}
                title={i18n.tr('Vis alle e-postkontoer', 'Show all email accounts')}
              >
                <X class="size-3" strokeWidth={2} />
              </button>
          </Show>
        </div>

        <div class="verevon-inbox-queue__search">
          <Search class="size-3.5" aria-hidden="true" />
          <input
            id="verevon-inbox-search"
            type="search"
            value={props.searchQuery}
            onInput={(event) => props.onSearchChange(event.currentTarget.value)}
            placeholder={i18n.tr('Søk', 'Search')}
            aria-label={i18n.tr('Søk i samtaler', 'Search conversations')}
            aria-keyshortcuts="/"
          />
        </div>

        <div class="verevon-inbox-queue__tools">
			<Show when={inboxRefreshTarget()}>
				{(target) => (
					<button
						type="button"
						class="verevon-inbox-icon-action"
						disabled={props.refreshingInbox}
						onClick={() => props.onRefreshInbox?.(target().channel, target().connectionIds)}
						aria-label={i18n.tr(`Hent nye ${target().label}-meldinger`, `Fetch new ${target().label} messages`)}
						title={i18n.tr('Hent nye meldinger fra leverandøren', 'Fetch new messages from provider')}
					>
						<RefreshCw class={cn('size-4', props.refreshingInbox && 'animate-spin')} strokeWidth={1.9} />
					</button>
				)}
			</Show>
          <div ref={filtersRef} class="verevon-inbox-menu-anchor">
            <button
              type="button"
              onClick={() => {
                setFiltersOpen((open) => !open)
                setSortOpen(false)
              }}
              class={cn('verevon-inbox-icon-action', filtersOpen() && 'verevon-inbox-icon-action--active')}
              aria-expanded={filtersOpen() ? 'true' : 'false'}
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
                onSearchChange={props.onSearchChange}
                searchQuery={props.searchQuery}
              />
            </Show>
          </div>

          <div ref={sortRef} class="verevon-inbox-menu-anchor">
            <button
              type="button"
              onClick={() => {
                setSortOpen((open) => !open)
                setFiltersOpen(false)
              }}
              class={cn('verevon-inbox-icon-action', sortOpen() && 'verevon-inbox-icon-action--active')}
              aria-expanded={sortOpen() ? 'true' : 'false'}
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

      <div class="verevon-inbox-queue__lanes">
        <div class="verevon-inbox-focus-switch">
          <StatusLaneButton active={props.activeTab === 'all'} label={i18n.tr('Alle', 'All')} onClick={() => props.onActiveTabChange('all')} />
          <StatusLaneButton active={props.activeTab === 'open'} label={i18n.tr('Åpne', 'Open')} onClick={() => props.onActiveTabChange('open')} />
          <StatusLaneButton active={props.activeTab === 'pending'} label={i18n.tr('Venter', 'Waiting')} onClick={() => props.onActiveTabChange('pending')} />
          <StatusLaneButton active={props.activeTab === 'solved'} label={i18n.tr('Løste', 'Resolved')} onClick={() => props.onActiveTabChange('solved')} />
        </div>
        <div class="verevon-inbox-quick-filters">
          <For each={quickFilters()}>
            {(filter) => {
              const Icon = filter.icon
              const textFilter = filter.id === 'all'
              return (
                <button
                  type="button"
                  onClick={() => setQuickFilter(filter.id)}
                  class={cn(
                    'verevon-inbox-quick-filter',
                    textFilter ? 'verevon-inbox-quick-filter--text' : 'verevon-inbox-quick-filter--icon',
                    quickFilter() === filter.id && 'verevon-inbox-quick-filter--active',
                  )}
                  aria-pressed={quickFilter() === filter.id ? 'true' : 'false'}
                  aria-label={i18n.tr(`Vis ${filter.label.toLowerCase()} samtaler`, `Show ${filter.label.toLowerCase()} conversations`)}
                  title={i18n.tr(`Vis ${filter.label.toLowerCase()} samtaler`, `Show ${filter.label.toLowerCase()} conversations`)}
                >
                  {Icon ? <Icon class="size-3.5" strokeWidth={1.9} /> : null}
                  <span class={{ 'sr-only': !textFilter }}>{filter.label}</span>
                </button>
              )
            }}
          </For>
        </div>
      </div>

      <Show when={props.connectionStatusUnavailable}>
        <div class="verevon-inbox-connection-status" role="status">
          <p>
            {i18n.tr('Tilkoblingsstatusen kunne ikke lastes. Samtalene er fortsatt tilgjengelige.', 'Connection status could not be loaded. Conversations remain available.')}
            <Show when={props.connectionStatusMessage}> {props.connectionStatusMessage}</Show>
          </p>
          <button type="button" onClick={() => props.onRetryConnectionStatus?.()}>{i18n.tr('Prøv tilkoblingsstatus på nytt', 'Retry connection status')}</button>
        </div>
      </Show>
		<Show when={props.providerRefreshNotice}>
			<div class="verevon-inbox-connection-status" role="status">
				<p>{props.providerRefreshNotice}</p>
			</div>
		</Show>

      <div class="verevon-inbox-queue__list">
        <Show when={props.loading}>
          <LoadingRows />
        </Show>
        <Show when={!props.loading && props.error}>
          <QueueEmptyState tone="error" title={i18n.tr('Support er ikke tilkoblet', 'Support is not connected')} body={props.error ?? ''} />
        </Show>
        <Show when={!props.loading && !props.error && sortedTickets().length === 0}>
          <QueueEmptyState
            title={metaSetupEmptyState()?.title ?? discordSetupEmptyState()?.title ?? linkedinSetupEmptyState()?.title ?? connectedEmptyState()?.title ?? i18n.tr('Ingen elementer', 'No items')}
            body={metaSetupEmptyState()?.body ?? discordSetupEmptyState()?.body ?? linkedinSetupEmptyState()?.body ?? connectedEmptyState()?.body ?? i18n.tr('Denne visningen er tom for nå. Endre filtre eller bytt felt for å se flere samtaler.', 'This view is clear for now. Change filters or switch lanes to see more conversations.')}
          >
            <Show when={metaSetupEmptyState() || discordSetupEmptyState()}>
              <div class="verevon-inbox-queue-empty__actions">
                <a href="/settings/integrations" link class="verevon-inbox-button verevon-inbox-button--primary verevon-inbox-button--sm">
                  {i18n.tr('Åpne Integrasjoner', 'Open Integrations')}
                </a>
              </div>
            </Show>
            <Show when={props.connectedSources.length === 0 && !props.activeChannel && props.onConnectInbox}>
              <div class="verevon-inbox-queue-empty__actions">
                <button type="button" class="verevon-inbox-button verevon-inbox-button--primary verevon-inbox-button--sm" disabled={Boolean(props.connectingInboxProvider)} onClick={() => props.onConnectInbox?.('google')}>
                  {props.connectingInboxProvider === 'google' ? i18n.tr('Åpner Gmail …', 'Opening Gmail...') : i18n.tr('Koble til Gmail', 'Connect Gmail')}
                </button>
                <button type="button" class="verevon-inbox-button verevon-inbox-button--secondary verevon-inbox-button--sm" disabled={Boolean(props.connectingInboxProvider)} onClick={() => props.onConnectInbox?.('microsoft')}>
                  {props.connectingInboxProvider === 'microsoft' ? i18n.tr('Åpner Outlook …', 'Opening Outlook...') : i18n.tr('Koble til Outlook', 'Connect Outlook')}
                </button>
              </div>
            </Show>
          </QueueEmptyState>
        </Show>
        <Show when={!props.loading && !props.error && sortedTickets().length > 0}>
          <ul aria-label={i18n.tr('Saker', 'Tickets')} aria-keyshortcuts="j k" class="verevon-inbox-ticket-list">
            <For each={sortedTickets()}>
              {(ticket) => (
                <li>
                  <TicketRow
                    active={props.selectedTicketId === ticket.id}
                    pinned={pinnedConversationIds().has(ticketPreferenceId(ticket))}
                    unread={!readConversationIds().has(ticketPreferenceId(ticket))}
                    ticket={ticket}
                    onClick={() => {
                      props.onMarkRead(ticket)
                      props.onSelectTicket(ticket)
                    }}
                    onSnooze={() => props.onSnoozeTicket(ticket)}
                    onTogglePinned={() => props.onTogglePinned(ticket)}
                  />
                </li>
              )}
            </For>
          </ul>
        </Show>
        <Show when={!props.loading && !props.error && props.hasMore}>
          <button
            type="button"
            class="verevon-inbox-load-more"
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
  return (
    <div class="verevon-popover verevon-inbox-menu verevon-inbox-menu--filters">
      <div class="verevon-inbox-menu__label">Status</div>
      <div class="verevon-inbox-menu__group">
        <For each={inboxTabs()}>
          {(tab) => (
            <button
              type="button"
              onClick={() => {
                props.onActiveTabChange(tab.id)
                props.onClose()
              }}
              class={cn('verevon-inbox-menu__row', props.activeTab === tab.id && 'verevon-inbox-menu__row--active')}
            >
              {tab.label}
              <Show when={props.activeTab === tab.id}>
                <Check class="size-3.5" />
              </Show>
            </button>
          )}
        </For>
      </div>
      <Show when={props.searchQuery}>
        <div class="verevon-inbox-menu__divider" />
        <button
          type="button"
          onClick={() => {
            props.onSearchChange('')
            props.onClose()
          }}
          class="verevon-inbox-menu__row verevon-inbox-menu__row--full"
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
    <div class="verevon-popover verevon-inbox-menu verevon-inbox-menu--sort">
      <For each={sortOptions()}>
        {(option) => {
          const Icon = option.icon
          return (
            <button type="button" onClick={() => props.onSelect(option.id)} class="verevon-inbox-sort-row">
              <Icon class="size-4" />
              <span>{option.label}</span>
              <Show when={props.activeSort === option.id}>
                <Check class="size-4 verevon-inbox-sort-row__check" />
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
    <div class="verevon-inbox-loading-rows">
      <For each={[1, 2, 3]}>
        {() => (
          <div class="verevon-inbox-loading-row">
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

function QueueEmptyState(props: { body: string; children?: JSX.Element; title: string; tone?: 'neutral' | 'error' }) {
  return (
    <div class="verevon-inbox-queue-empty">
      <div>
        <div class="verevon-inbox-queue-empty__icon">
          <MessageCircle class="size-5" strokeWidth={1.45} />
        </div>
        <h2>{props.title}</h2>
        <p class={{ 'verevon-inbox-queue-empty__error': props.tone === 'error' }}>{props.body}</p>
        {props.children}
      </div>
    </div>
  )
}

function TicketRow(props: {
  active: boolean
  pinned: boolean
  onClick: () => void
  onSnooze: () => void
  onTogglePinned: () => void
  ticket: ZammadTicket
  unread: boolean
}) {
  const i18n = useI18n()
  return (
    <div class={cn('verevon-inbox-ticket-row', props.active && 'verevon-inbox-ticket-row--active')}>
      <button
        type="button"
        aria-label={props.ticket.title}
        aria-pressed={props.active ? 'true' : 'false'}
        onClick={() => props.onClick()}
        class="verevon-inbox-ticket-row__main"
      >
        <div>
          <div class="verevon-inbox-ticket-row__meta">
            <Show when={props.unread}>
              <span class="verevon-inbox-ticket-row__unread" aria-label={i18n.tr('Ulest samtale', 'Unread conversation')} />
            </Show>
            <span class={cn('verevon-inbox-ticket-row__customer', props.unread && 'verevon-inbox-ticket-row__customer--unread')}>
              {ticketCustomerLabel(props.ticket)}
            </span>
            <Show when={props.pinned}>
              <Pin class="size-3 verevon-inbox-ticket-row__pin" strokeWidth={1.8} />
            </Show>
            <ChannelMark channel={props.ticket.channel} provider={props.ticket.provider} />
            <span class="verevon-inbox-ticket-row__more">
              <MoreHorizontal class="size-3" strokeWidth={1.8} />
            </span>
            <span>{i18n.tr(`for ${formatRelativeTime(props.ticket.updated_at)} siden`, `${formatRelativeTime(props.ticket.updated_at)} ago`)}</span>
          </div>
          <div class={cn('verevon-inbox-ticket-row__subject', props.unread && 'verevon-inbox-ticket-row__subject--unread')}>
            {ticketDisplayTitle(props.ticket)}
          </div>
          <div class="verevon-inbox-ticket-row__preview">
            {ticketPreview(props.ticket, i18n)}
          </div>
        </div>
      </button>
      <div class="verevon-inbox-ticket-row__actions">
        <RowActionButton label={props.pinned ? i18n.tr('Løsne samtale', 'Unpin conversation') : i18n.tr('Fest samtale', 'Pin conversation')} onClick={props.onTogglePinned}>
          <Pin class={cn('size-3.5', props.pinned && 'verevon-inbox-ticket-row__pin')} />
        </RowActionButton>
        <RowActionButton label={i18n.tr('Utsett samtale', 'Snooze conversation')} onClick={props.onSnooze}>
          <Clock3 class="size-3.5" />
        </RowActionButton>
      </div>
    </div>
  )
}


function ChannelMark(props: { channel?: string; provider?: string }) {
  const i18n = useI18n()
  const config = createMemo(() => channelMarkConfig((props.provider || props.channel || 'email').toLowerCase(), i18n))

  return (
    <span
      class="verevon-inbox-ticket-row__channel"
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
      return { icon: supportProviderIcon('email'), label: 'Gmail' }
    case 'microsoft':
    case 'outlook':
      return { icon: supportProviderIcon('email'), label: 'Outlook' }
    case 'slack':
      return { icon: supportProviderIcon('slack'), label: 'Slack' }
    case 'teams':
      return { icon: supportProviderIcon('teams'), label: 'Teams' }
    case 'discord':
      return { icon: supportProviderIcon('discord'), label: 'Discord' }
    case 'linkedin':
      return { icon: supportProviderIcon('linkedin'), label: 'LinkedIn' }
    case 'whatsapp':
      return { icon: supportProviderIcon('whatsapp'), label: 'WhatsApp' }
    case 'messenger':
      return { icon: supportProviderIcon('messenger'), label: 'Messenger' }
    case 'instagram':
      return { icon: supportProviderIcon('instagram'), label: 'Instagram' }
    case 'x':
    case 'twitter':
      return { icon: supportProviderIcon('x'), label: 'X' }
    default:
      return { icon: supportProviderIcon('email'), label: key === 'email' ? i18n.tr('E-post', 'Email') : key }
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
      class="verevon-inbox-row-action"
    >
      {props.children}
    </button>
  )
}

function StatusLaneButton(props: { active: boolean; label: string; onClick: () => void }) {
  return (
    <button
      type="button"
      onClick={() => props.onClick()}
      class={cn('verevon-inbox-focus-button', props.active && 'verevon-inbox-focus-button--active')}
      aria-pressed={props.active ? 'true' : 'false'}
    >
      <span>{props.label}</span>
    </button>
  )
}

function ticketPreferenceId(ticket: ZammadTicket) {
  return ticket.conversationId ?? String(ticket.id)
}

function sortTickets(tickets: ZammadTicket[], sortKey: SortKey, pinnedIds: Set<string>) {
  const sorted = [...tickets]

  return sorted.sort((a, b) => {
    const pinnedDelta = Number(pinnedIds.has(ticketPreferenceId(b))) - Number(pinnedIds.has(ticketPreferenceId(a)))
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


function ticketPreview(ticket: ZammadTicket, i18n: ReturnType<typeof useI18n>) {
  return ticket.lastMessagePreview?.trim() || i18n.tr('Ingen forhåndsvisning tilgjengelig', 'No message preview available')
}
