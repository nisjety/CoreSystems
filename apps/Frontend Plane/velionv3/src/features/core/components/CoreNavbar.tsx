import { A, useNavigate } from '@solidjs/router'
import {
  Bell,
  Building2,
  CalendarDays,
  Check,
  ChevronLeft,
  ChevronRight,
  Languages,
  MessageSquareMore,
  MoonStar,
  Search,
  Slash,
  Sparkles,
  Sun,
} from 'lucide-solid'
import { createEffect, createSignal, For, onCleanup, onMount, Show, type JSX } from 'solid-js'
import { CoreNavbarPanel, type CoreNavbarPanelKind } from '@/features/core/components/CoreNavbarPanels'
import { getNavbarLabels, type VelionRoute, type WorkspaceIdentity } from '@/features/core/lib/shell-data'
import { signOut } from '@/shared/api/auth-client'
import {
  listOrganizations,
  switchActiveOrganization,
  type OrganizationSummary,
} from '@/shared/api/organization-client'
import {
  saveNavbarTheme,
  searchNavbar,
  type NavbarPayload,
  type NavbarSearchResult,
  type ThemePayload,
} from '@/shared/api/navbar-client'
import { shouldShowWorkspaceAdminNavigation } from '@/shared/session/access'
import { clearSession, getSession, loadSession } from '@/shared/session/session-store'
import { useI18n } from '@/shared/i18n'
import { cn } from '@/shared/lib/cn'

type OpenPanel = 'assistant' | 'messages' | 'notifications' | 'calendar' | 'profile' | 'workspace' | null

export function CoreNavbar(props: {
  activeRoute: VelionRoute
  navbarData?: NavbarPayload | null
  onNavbarRefresh?: () => void
  onSearchOpenChange?: (open: boolean) => void
  searchOpen?: boolean
  workspace: WorkspaceIdentity
}) {
  let headerRef: HTMLElement | undefined
  const i18n = useI18n()
  const navigate = useNavigate()
  const session = getSession()
  const [openPanel, setOpenPanel] = createSignal<OpenPanel>(null)
  const [theme, setTheme] = createSignal<ThemePayload['theme']>('system')
  const [localSearchOpen, setLocalSearchOpen] = createSignal(false)
  const labels = () => getNavbarLabels(props.activeRoute, i18n.locale())
  const profileInitial = () => (props.workspace.userName ?? props.workspace.userEmail ?? props.workspace.name).trim().charAt(0).toUpperCase() || props.workspace.initial
  const messageUnreadCount = () => props.navbarData?.notifications.messages.filter((message) => !message.read).length ?? 0
  const notificationUnreadCount = () =>
    props.navbarData?.notifications.unreadCount ??
    props.navbarData?.notifications.notifications.filter((notification) => !notification.read).length ??
    0
  const searchOpen = () => props.searchOpen ?? localSearchOpen()
  const setSearchOpen = (open: boolean) => {
    if (props.onSearchOpenChange) props.onSearchOpenChange(open)
    else setLocalSearchOpen(open)
  }
  const visibleNavbarPanel = () => {
    const panel = openPanel()
    return panel && panel !== 'workspace' ? panel as CoreNavbarPanelKind : null
  }

  const closeShellOverlays = () => {
    setOpenPanel(null)
    setSearchOpen(false)
  }

  createEffect(() => {
    const remoteTheme = props.navbarData?.theme
    if (remoteTheme?.configured !== false && remoteTheme?.theme) {
      setTheme(remoteTheme.theme)
    }
  })

  onMount(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null
      const isTyping = target?.tagName === 'INPUT' || target?.tagName === 'TEXTAREA' || target?.isContentEditable

      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'k') {
        event.preventDefault()
        setSearchOpen(true)
      }

      if (!isTyping && event.key === '/') {
        event.preventDefault()
        setSearchOpen(true)
      }

      if (event.key === 'Escape') closeShellOverlays()
    }

    window.addEventListener('keydown', handleKeyDown)
    onCleanup(() => window.removeEventListener('keydown', handleKeyDown))
  })

  onMount(() => {
    const closePanelOnOutsidePointer = (event: PointerEvent) => {
      const target = event.target as Node | null
      if (!openPanel() || (target && headerRef?.contains(target))) return
      setOpenPanel(null)
    }

    document.addEventListener('pointerdown', closePanelOnOutsidePointer, true)
    onCleanup(() => document.removeEventListener('pointerdown', closePanelOnOutsidePointer, true))
  })

  const openExclusivePanel = (panel: OpenPanel) => {
    setOpenPanel((current) => (current === panel ? null : panel))
  }

  const handleSignOut = async () => {
    setOpenPanel(null)
    try {
      await signOut()
    } finally {
      clearSession()
      navigate('/login', { replace: true })
    }
  }

  return (
    <>
      <header ref={headerRef} class="dashboard-navbar-bg core-navbar">
        <div class="core-navbar__inner">
          <div class="core-navbar__left">
            <A
              href="/dashboard"
              aria-label={i18n.tr('Gå til hjem', 'Go to home')}
              title={i18n.tr('Gå til hjem', 'Go to home')}
              class="core-navbar__home-mark"
            >
              {props.workspace.initial}
            </A>

            <HistoryNav
              onBack={() => window.history.back()}
              onForward={() => window.history.forward()}
            />

            <Breadcrumb
              moduleLabel={labels().moduleLabel}
              moduleHref={props.activeRoute}
              onWorkspaceClick={() => openExclusivePanel('workspace')}
              tabLabel={labels().tabLabel}
              tabHref={props.activeRoute}
              workspace={props.workspace}
              workspaceActive={openPanel() === 'workspace'}
            />
          </div>

          <div class="core-navbar__search">
            <SearchTrigger onOpen={() => setSearchOpen(true)} />
          </div>

          <div class="core-navbar__actions">
            <NavbarActionButton
              label={i18n.tr('Åpne kunnskapssøk', 'Open knowledge search')}
              onClick={() => setSearchOpen(true)}
              class="core-navbar__mobile-search"
            >
              <Search class="size-4" strokeWidth={1.9} />
            </NavbarActionButton>

            <div class="core-navbar__action-group">
              <NavDivider />

              <NavbarActionButton
                label={i18n.tr('Bytt mørk modus', 'Toggle dark mode')}
                tooltip={i18n.tr('Bytt mørk modus', 'Toggle dark mode')}
                onClick={() => {
                  const nextTheme = nextThemePreference(theme())
                  const refreshNavbar = props.onNavbarRefresh
                  setTheme(nextTheme)
                  applyThemePreference(nextTheme)
                  void saveNavbarTheme(nextTheme, props.workspace.accentColor)
                    .then(() => refreshNavbar?.())
                    .catch(() => undefined)
                }}
              >
                <span class="core-navbar__icon-stack">
                  <Sun class="size-[18px]" strokeWidth={1.85} />
                  <MoonStar class="size-[18px]" strokeWidth={1.85} />
                </span>
              </NavbarActionButton>

              <NavDivider />

              <NavbarActionButton
                label={i18n.tr(`Bytt til ${i18n.nextLocaleName()}`, `Switch to ${i18n.nextLocaleName()}`)}
                tooltip={i18n.tr(`Bytt til ${i18n.nextLocaleName()}`, `Switch to ${i18n.nextLocaleName()}`)}
                onClick={i18n.toggleLocale}
              >
                <span class="core-navbar__language">
                  <Languages class="size-[15px]" strokeWidth={1.85} />
                  <span>{i18n.localeCode()}</span>
                </span>
              </NavbarActionButton>

              <NavDivider />

              <NavbarActionButton
                label={i18n.tr('Åpne AI-assistent', 'Open AI assistant')}
                tooltip={i18n.tr('AI-assistent for gjeldende side', 'AI assistant for current page')}
                active={openPanel() === 'assistant'}
                onClick={() => openExclusivePanel('assistant')}
              >
                <Sparkles class="size-[18px]" strokeWidth={1.85} />
              </NavbarActionButton>

              <BadgeButton count={messageUnreadCount()} label={i18n.tr('Hurtigmeldinger', 'Quick messages')} i18n={i18n}>
                <NavbarActionButton
                  label={i18n.tr(`${messageUnreadCount()} uleste meldinger`, `${messageUnreadCount()} unread messages`)}
                  active={openPanel() === 'messages'}
                  onClick={() => openExclusivePanel('messages')}
                >
                  <MessageSquareMore class="size-[18px]" strokeWidth={1.85} />
                </NavbarActionButton>
              </BadgeButton>

              <BadgeButton count={notificationUnreadCount()} label={i18n.tr('Varsler', 'Notifications')} i18n={i18n}>
                <NavbarActionButton
                  label={i18n.tr(`${notificationUnreadCount()} uleste varsler`, `${notificationUnreadCount()} unread notifications`)}
                  active={openPanel() === 'notifications'}
                  onClick={() => openExclusivePanel('notifications')}
                >
                  <Bell class="size-[18px]" strokeWidth={1.85} />
                </NavbarActionButton>
              </BadgeButton>

              <NavbarActionButton
                label={i18n.tr('Kalender', 'Calendar')}
                active={openPanel() === 'calendar'}
                onClick={() => openExclusivePanel('calendar')}
              >
                <CalendarDays class="size-[18px]" strokeWidth={1.85} />
              </NavbarActionButton>

              <NavDivider />

              <button
                type="button"
                aria-label={i18n.tr('Åpne profilmeny', 'Open profile menu')}
                title={i18n.tr('Åpne profilmeny', 'Open profile menu')}
                onClick={() => openExclusivePanel('profile')}
                class="core-navbar__profile"
              >
                <span aria-hidden="true" class="core-navbar__profile-ring" />
                <span class="core-navbar__profile-avatar">
                  <Show when={props.workspace.userAvatar} fallback={profileInitial()}>
                    {(src) => <img class="core-navbar__profile-avatar-img" src={src()} alt="" />}
                  </Show>
                </span>
              </button>
            </div>

            <Show when={visibleNavbarPanel()}>
              {(panel) => (
                <CoreNavbarPanel
                  navbarData={props.navbarData}
                  panel={panel()}
                  workspace={props.workspace}
                  onNavigate={(href) => {
                    setOpenPanel(null)
                    navigate(href)
                  }}
                  onRefresh={props.onNavbarRefresh}
                  onSignOut={() => void handleSignOut()}
                  onSupport={() => openExclusivePanel('assistant')}
                />
              )}
            </Show>
          </div>
        </div>

        <Show when={openPanel() === 'workspace'}>
          <WorkspaceSwitcher
            canManageWorkspace={shouldShowWorkspaceAdminNavigation(session)}
            workspace={props.workspace}
            onClose={() => setOpenPanel(null)}
            onSwitched={props.onNavbarRefresh}
          />
        </Show>
      </header>

      <Show when={searchOpen()}>
        <GlobalSearchDialog
          onClose={() => setSearchOpen(false)}
          onNavigate={(href) => {
            setSearchOpen(false)
            navigate(href)
          }}
        />
      </Show>
    </>
  )
}

function GlobalSearchDialog(props: {
  onClose: () => void
  onNavigate: (href: string) => void
}) {
  const i18n = useI18n()
  let inputRef: HTMLInputElement | undefined
  let searchTimer: number | undefined
  let searchController: AbortController | undefined
  const [error, setError] = createSignal<string | null>(null)
  const [loading, setLoading] = createSignal(false)
  const [query, setQuery] = createSignal('')
  const [results, setResults] = createSignal<NavbarSearchResult[]>([])

  createEffect(() => {
    const trimmed = query().trim()
    window.clearTimeout(searchTimer)
    searchController?.abort()

    if (trimmed.length < 2) {
      setError(null)
      setLoading(false)
      setResults([])
      return
    }

    const controller = new AbortController()
    searchController = controller
    searchTimer = window.setTimeout(() => {
      setLoading(true)
      setError(null)
      searchNavbar(trimmed, controller.signal)
        .then((payload) => {
          setResults(payload.results)
        })
        .catch((searchError: unknown) => {
          if (controller.signal.aborted) return
          setError(searchError instanceof Error ? searchError.message : i18n.tr('Søk feilet.', 'Search failed.'))
          setResults([])
        })
        .finally(() => {
          if (!controller.signal.aborted) setLoading(false)
        })
    }, 220)
  })

  onMount(() => {
    inputRef?.focus()
  })

  onCleanup(() => {
    window.clearTimeout(searchTimer)
    searchController?.abort()
  })

  return (
    <div class="core-search-overlay" role="dialog" aria-modal="true" aria-label={i18n.tr('Kunnskapssøk', 'Knowledge search')}>
      <button type="button" class="core-search-overlay__scrim" aria-label={i18n.tr('Lukk globalt søk', 'Close global search')} onClick={() => props.onClose()} />
      <div class="core-search-overlay__panel velion-panel-in">
        <div class="core-search-overlay__header">
          <Search class="size-4 core-search-overlay__icon" strokeWidth={1.8} />
          <input
            ref={inputRef}
            value={query()}
            onInput={(event) => setQuery(event.currentTarget.value)}
            placeholder={i18n.tr('Søk i kunnskapsbasen', 'Search the knowledge base')}
            aria-label={i18n.tr('Søk i kunnskapsbasen', 'Search the knowledge base')}
          />
          <button type="button" class="core-search-overlay__close" onClick={() => props.onClose()} aria-label={i18n.tr('Lukk globalt søk', 'Close global search')}>
            Esc
          </button>
        </div>
        <div class="core-search-overlay__results">
          <Show
            when={query().trim().length >= 2}
            fallback={<EmptySearchPanel text={i18n.tr('Begynn å skrive for å søke i arbeidsområdets kunnskap.', 'Start typing to search workspace knowledge.')} />}
          >
            <Show
              when={!loading()}
              fallback={<EmptySearchPanel text={i18n.tr('Søker ...', 'Searching ...')} />}
            >
              <Show when={!error()} fallback={<EmptySearchPanel text={error() ?? i18n.tr('Søk feilet.', 'Search failed.')} />}>
                <Show
                  when={results().length > 0}
                  fallback={<EmptySearchPanel text={i18n.tr('Ingen matchende kunnskapsoppføringer.', 'No matching knowledge records.')} />}
                >
                  <For each={results()}>
                    {(result) => (
                      <button
                        type="button"
                        class="core-search-result"
                        onClick={() => props.onNavigate(result.href)}
                      >
                        <span>
                          <strong>{result.label}</strong>
                          <small>{result.excerpt}</small>
                        </span>
                        <em>{result.source}</em>
                      </button>
                    )}
                  </For>
                </Show>
              </Show>
            </Show>
          </Show>
        </div>
      </div>
    </div>
  )
}

function EmptySearchPanel(props: { text: string }) {
  return <div class="core-search-empty">{props.text}</div>
}

function nextThemePreference(current: ThemePayload['theme']): ThemePayload['theme'] {
  if (current === 'dark') return 'light'
  if (current === 'light') return 'system'
  return 'dark'
}

function applyThemePreference(theme: ThemePayload['theme']) {
  const prefersDark = window.matchMedia?.('(prefers-color-scheme: dark)').matches ?? false
  document.documentElement.classList.toggle('dark', theme === 'dark' || (theme === 'system' && prefersDark))
}

function SearchTrigger(props: { onOpen: () => void }) {
  const i18n = useI18n()
  return (
    <button
      type="button"
      onClick={() => props.onOpen()}
      title={i18n.tr('Åpne kunnskapssøk', 'Open knowledge search')}
      class="velion-navbar-search-trigger"
      aria-label={i18n.tr('Åpne kunnskapssøk', 'Open knowledge search')}
    >
      <Search class="velion-navbar-search-icon" strokeWidth={1.8} />
      <span class="velion-navbar-search-label">{i18n.tr('Søk i kunnskapsbasen', 'Search knowledge base')}</span>
      <span class="velion-navbar-shortcut-key velion-navbar-shortcut-key-min">/</span>
      <span class="velion-navbar-shortcut-key velion-navbar-shortcut-key-wide">CMD+K</span>
    </button>
  )
}

function Breadcrumb(props: {
  moduleLabel: string
  moduleHref: VelionRoute
  onWorkspaceClick?: () => void
  tabLabel: string
  tabHref: VelionRoute
  workspace: WorkspaceIdentity
  workspaceActive?: boolean
}) {
  return (
    <div class="core-breadcrumb">
      <BreadcrumbSeparator />
      <button
        type="button"
        onClick={() => props.onWorkspaceClick?.()}
        aria-expanded={props.workspaceActive}
        class="core-breadcrumb__workspace"
      >
        <span>{props.workspace.name}</span>
        <strong
          classList={{
            'core-breadcrumb__plan--paid': props.workspace.plan.trim().toLowerCase() !== 'trial',
          }}
        >
          {props.workspace.plan}
        </strong>
      </button>
      <BreadcrumbSeparator />
      <A href={props.moduleHref}>{props.moduleLabel}</A>
      <BreadcrumbSeparator />
      <A href={props.tabHref} class="core-breadcrumb__muted">{props.tabLabel}</A>
    </div>
  )
}

function HistoryNav(props: { onBack: () => void; onForward: () => void }) {
  const i18n = useI18n()
  return (
    <div class="core-history-nav">
      <NavbarActionButton label={i18n.tr('Gå tilbake', 'Go back')} tooltip={i18n.tr('Gå tilbake', 'Go back')} onClick={props.onBack}>
        <ChevronLeft class="size-4" strokeWidth={2.1} />
      </NavbarActionButton>
      <NavbarActionButton label={i18n.tr('Gå fremover', 'Go forward')} tooltip={i18n.tr('Gå fremover', 'Go forward')} onClick={props.onForward}>
        <ChevronRight class="size-4" strokeWidth={2.1} />
      </NavbarActionButton>
    </div>
  )
}

function NavbarActionButton(props: {
  active?: boolean
  children: JSX.Element
  class?: string
  disabled?: boolean
  label: string
  onClick?: () => void
  tooltip?: string
}) {
  return (
    <button
      type="button"
      onClick={() => props.onClick?.()}
      disabled={props.disabled}
      aria-label={props.label}
      title={props.tooltip ?? props.label}
      data-active={props.active ? 'true' : undefined}
      class={cn('velion-navbar-action-button', props.class)}
    >
      {props.children}
    </button>
  )
}

function BadgeButton(props: { children: JSX.Element; count: number; i18n: ReturnType<typeof useI18n>; label: string }) {
  return (
    <div class="core-badge-button">
      {props.children}
      <Show when={props.count > 0}>
        <span aria-label={props.i18n.tr(`${props.count} uleste ${props.label.toLowerCase()}`, `${props.count} unread ${props.label.toLowerCase()}`)}>
          {Math.min(props.count, 9)}
          {props.count > 9 ? '+' : null}
        </span>
      </Show>
    </div>
  )
}

function NavDivider() {
  return <div class="core-nav-divider" aria-hidden="true" />
}

function BreadcrumbSeparator() {
  return <Slash class="size-3.5" strokeWidth={2} />
}

function WorkspaceSwitcher(props: {
  canManageWorkspace: boolean
  onClose: () => void
  onSwitched?: () => void
  workspace: WorkspaceIdentity
}) {
  const i18n = useI18n()
  const session = getSession()
  const [organizations, setOrganizations] = createSignal<OrganizationSummary[]>([])
  const [loading, setLoading] = createSignal(true)
  const [error, setError] = createSignal<string | null>(null)
  const [switchingId, setSwitchingId] = createSignal<string | null>(null)

  onMount(() => {
    listOrganizations()
      .then(setOrganizations)
      .catch(() => setError(i18n.tr('Kunne ikke laste arbeidsområder.', 'Could not load workspaces.')))
      .finally(() => setLoading(false))
  })

  const switchOrganization = async (organizationId: string) => {
    if (switchingId()) return
    if (organizationId === session.activeOrg?.id) {
      props.onClose()
      return
    }
    setSwitchingId(organizationId)
    setError(null)
    try {
      await switchActiveOrganization(organizationId)
      await loadSession()
      props.onSwitched?.()
      props.onClose()
    } catch {
      setError(i18n.tr(
        'Kunne ikke bytte arbeidsområde. Tilgangen kan ha blitt endret.',
        'Could not switch workspace. Your access may have changed.',
      ))
    } finally {
      setSwitchingId(null)
    }
  }

  return (
    <div class="core-workspace-switcher">
      <Show when={!loading()} fallback={<p role="status">{i18n.tr('Laster arbeidsområder …', 'Loading workspaces…')}</p>}>
        <For each={organizations()}>
          {(organization) => {
            const active = () => organization.id === session.activeOrg?.id
            return (
              <button
                type="button"
                aria-pressed={active()}
                disabled={Boolean(switchingId())}
                onClick={() => void switchOrganization(organization.id)}
                classList={{
                  'core-workspace-switcher__row': true,
                  'core-workspace-switcher__row--active': active(),
                }}
              >
                <span class="core-workspace-mark">{organization.name.charAt(0).toUpperCase()}</span>
                <span>
                  <strong>{organization.name}</strong>
                  <small>
                    {i18n.tr('Organisasjonsarbeidsområde', 'Organization workspace')}
                  </small>
                </span>
                <Show when={active()}>
                  <Check class="size-3.5" aria-hidden="true" />
                </Show>
              </button>
            )
          }}
        </For>
      </Show>

      <Show when={error()}>
        <p role="alert">{error()}</p>
      </Show>

      <Show when={props.canManageWorkspace}>
        <A href="/settings/workspace" onClick={props.onClose} class="core-workspace-switcher__manage">
          <Building2 class="size-3.5" />
          {i18n.tr('Administrer arbeidsområder og medlemmer', 'Manage workspaces and members')}
        </A>
      </Show>
    </div>
  )
}
