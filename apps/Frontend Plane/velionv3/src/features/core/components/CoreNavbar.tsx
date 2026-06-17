import { A, useNavigate } from '@solidjs/router'
import {
  Bell,
  Building2,
  CalendarDays,
  Check,
  ChevronLeft,
  ChevronRight,
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
  saveNavbarTheme,
  searchNavbar,
  type NavbarPayload,
  type NavbarSearchResult,
  type ThemePayload,
} from '@/shared/api/navbar-client'
import { shouldShowWorkspaceAdminNavigation } from '@/shared/session/access'
import { clearSession, getSession } from '@/shared/session/session-store'
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
  const navigate = useNavigate()
  const session = getSession()
  const [openPanel, setOpenPanel] = createSignal<OpenPanel>(null)
  const [theme, setTheme] = createSignal<ThemePayload['theme']>('system')
  const [localSearchOpen, setLocalSearchOpen] = createSignal(false)
  const labels = () => getNavbarLabels(props.activeRoute)
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
              aria-label="Go to home"
              title="Go to home"
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
              label="Open knowledge search"
              onClick={() => setSearchOpen(true)}
              class="core-navbar__mobile-search"
            >
              <Search class="size-4" strokeWidth={1.9} />
            </NavbarActionButton>

            <div class="core-navbar__action-group">
              <NavDivider />

              <NavbarActionButton
                label="Toggle dark mode"
                tooltip="Toggle dark mode"
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
                label="Open AI assistant"
                tooltip="AI assistant for current page"
                active={openPanel() === 'assistant'}
                onClick={() => openExclusivePanel('assistant')}
              >
                <Sparkles class="size-[18px]" strokeWidth={1.85} />
              </NavbarActionButton>

              <BadgeButton count={messageUnreadCount()} label="Quick messages">
                <NavbarActionButton
                  label={`${messageUnreadCount()} unread messages`}
                  active={openPanel() === 'messages'}
                  onClick={() => openExclusivePanel('messages')}
                >
                  <MessageSquareMore class="size-[18px]" strokeWidth={1.85} />
                </NavbarActionButton>
              </BadgeButton>

              <BadgeButton count={notificationUnreadCount()} label="Notifications">
                <NavbarActionButton
                  label={`${notificationUnreadCount()} unread notifications`}
                  active={openPanel() === 'notifications'}
                  onClick={() => openExclusivePanel('notifications')}
                >
                  <Bell class="size-[18px]" strokeWidth={1.85} />
                </NavbarActionButton>
              </BadgeButton>

              <NavbarActionButton
                label="Calendar"
                active={openPanel() === 'calendar'}
                onClick={() => openExclusivePanel('calendar')}
              >
                <CalendarDays class="size-[18px]" strokeWidth={1.85} />
              </NavbarActionButton>

              <NavDivider />

              <button
                type="button"
                aria-label="Open profile menu"
                title="Open profile menu"
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
          setError(searchError instanceof Error ? searchError.message : 'Search failed.')
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
    <div class="core-search-overlay" role="dialog" aria-modal="true" aria-label="Knowledge search">
      <button type="button" class="core-search-overlay__scrim" aria-label="Close global search" onClick={() => props.onClose()} />
      <div class="core-search-overlay__panel velion-panel-in">
        <div class="core-search-overlay__header">
          <Search class="size-4 core-search-overlay__icon" strokeWidth={1.8} />
          <input
            ref={inputRef}
            value={query()}
            onInput={(event) => setQuery(event.currentTarget.value)}
            placeholder="Search the knowledge base"
            aria-label="Search the knowledge base"
          />
          <button type="button" class="core-search-overlay__close" onClick={() => props.onClose()} aria-label="Close global search">
            Esc
          </button>
        </div>
        <div class="core-search-overlay__results">
          <Show
            when={query().trim().length >= 2}
            fallback={<EmptySearchPanel text="Start typing to search workspace knowledge." />}
          >
            <Show
              when={!loading()}
              fallback={<EmptySearchPanel text="Searching…" />}
            >
              <Show when={!error()} fallback={<EmptySearchPanel text={error() ?? 'Search failed.'} />}>
                <Show
                  when={results().length > 0}
                  fallback={<EmptySearchPanel text="No matching knowledge records." />}
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
  return (
    <button
      type="button"
      onClick={() => props.onOpen()}
      title="Open knowledge search"
      class="velion-navbar-search-trigger"
      aria-label="Open knowledge search"
    >
      <Search class="velion-navbar-search-icon" strokeWidth={1.8} />
      <span class="velion-navbar-search-label">Search knowledge base</span>
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
        <strong>{props.workspace.plan}</strong>
      </button>
      <BreadcrumbSeparator />
      <A href={props.moduleHref}>{props.moduleLabel}</A>
      <BreadcrumbSeparator />
      <A href={props.tabHref} class="core-breadcrumb__muted">{props.tabLabel}</A>
    </div>
  )
}

function HistoryNav(props: { onBack: () => void; onForward: () => void }) {
  return (
    <div class="core-history-nav">
      <NavbarActionButton label="Go back" tooltip="Go back" onClick={props.onBack}>
        <ChevronLeft class="size-4" strokeWidth={2.1} />
      </NavbarActionButton>
      <NavbarActionButton label="Go forward" tooltip="Go forward" onClick={props.onForward}>
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

function BadgeButton(props: { children: JSX.Element; count: number; label: string }) {
  return (
    <div class="core-badge-button">
      {props.children}
      <Show when={props.count > 0}>
        <span aria-label={`${props.count} unread ${props.label.toLowerCase()}`}>
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

function WorkspaceSwitcher(props: { canManageWorkspace: boolean; onClose: () => void; workspace: WorkspaceIdentity }) {
  const [active, setActive] = createSignal<'org' | 'personal'>('org')

  return (
    <div class="core-workspace-switcher">
      <button
        type="button"
        aria-pressed={active() === 'org'}
        onClick={() => {
          setActive('org')
          props.onClose()
        }}
        class="core-workspace-switcher__row core-workspace-switcher__row--active"
      >
        <span class="core-workspace-mark">{props.workspace.initial}</span>
        <span>
          <strong>{props.workspace.name}</strong>
          <small>Organization workspace · {props.workspace.plan}</small>
        </span>
        <Show when={active() === 'org'}>
          <Check class="size-3.5" aria-hidden="true" />
        </Show>
      </button>

      <button
        type="button"
        aria-pressed={active() === 'personal'}
        onClick={() => {
          setActive('personal')
          props.onClose()
        }}
        class="core-workspace-switcher__row"
      >
        <span class="core-user-workspace-mark">{props.workspace.initial}</span>
        <span>
          <strong>{props.workspace.userName ?? 'Personal workspace'}</strong>
          <small>{props.workspace.userEmail ?? 'Signed in'}</small>
        </span>
        <Show when={active() === 'personal'}>
          <Check class="size-3.5" aria-hidden="true" />
        </Show>
      </button>

      <Show when={props.canManageWorkspace}>
        <A href="/settings/workspace" onClick={props.onClose} class="core-workspace-switcher__manage">
          <Building2 class="size-3.5" />
          Manage workspaces and members
        </A>
      </Show>
    </div>
  )
}
