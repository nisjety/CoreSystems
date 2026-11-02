import { useLocation } from '@solidjs/router'
import { createEffect, createMemo, createSignal, Show, untrack } from 'solid-js'
import type { JSX } from '@solidjs/web'
import { createResource } from '@/shared/lib/create-resource-compat'
import { AgentsProvider } from '@/features/agents/lib/use-agent-selection'
import { CoreNavbar } from '@/features/core/components/CoreNavbar'
import { CoreSidebar, SIDEBAR_EXPANDED_WIDTH, SIDEBAR_MINIMIZED_WIDTH } from '@/features/core/components/CoreSidebar'
import { FeedbackWidget } from '@/features/core/components/FeedbackWidget'
import { AnnouncerRegions } from '@/shared/a11y/announcer'
import { chatAppearanceActive, chatAppearanceTokens } from '@/features/chat/lib/chat-appearance-preview'
import { OrgDeletionBanner } from '@/features/core/components/OrgDeletionBanner'
import {
  fallbackWorkspaceIdentity,
  formatPlanLabel,
  routeFromPath,
  type WorkspaceIdentity,
} from '@/features/core/lib/shell-data'
import { CoreWorkspaceContext } from '@/features/core/lib/workspace-context'
import { getNavbarData, type NavbarPayload } from '@/shared/api/navbar-client'
import { getDeletionStatus } from '@/shared/api/org-deletion-client'
import { getSession, type SessionState } from '@/shared/session/session-store'
import { RegionBoundary } from '@/shared/ui/resilience/RegionBoundary'

/** Extra height the fixed org-deletion banner (Flow C) occupies below the
 * navbar — added to `--dashboard-navbar-height` so `.core-main`'s top
 * padding reflows content below the banner instead of it being covered. */
const DELETION_BANNER_HEIGHT_PX = 40

export function CoreShell(props: { children?: JSX.Element }) {
  const location = useLocation()
  const session = getSession()
  // One-time peek to seed the initial signal value; the reactive read for
  // ongoing updates is activeRoute (below), a proper createMemo.
  const initialRoute = untrack(() => routeFromPath(location.pathname))
  const [sidebarExpanded, setSidebarExpanded] = createSignal(
    defaultSidebarExpandedForRoute(initialRoute, { pathname: untrack(() => location.pathname), phone: isPhoneViewport() }),
  )
  const [searchOpen, setSearchOpen] = createSignal(false)
  const [navbarData, { refetch: refetchNavbar }] = createResource(fetchNavbarData)
  const orgId = createMemo(() => session.activeOrg?.id ?? null)
  const [deletionStatus, { refetch: refetchDeletionStatus }] = createResource(
    orgId,
    fetchDeletionStatus,
  )
  const deletionBannerVisible = createMemo(() => Boolean(deletionStatus.latest?.pending ?? deletionStatus()?.pending))
  const activeRoute = createMemo(() => routeFromPath(location.pathname))
  // Support uses a compact navigation column so conversation and ticket work
  // retain the horizontal space they need.
  const expandedSidebarWidth = () => activeRoute() === '/support' ? 303 : SIDEBAR_EXPANDED_WIDTH
  const sidebarWidth = () => (sidebarExpanded() ? expandedSidebarWidth() : SIDEBAR_MINIMIZED_WIDTH)
  const workspace = createMemo(() => resolveWorkspaceIdentity(session, navbarData.latest ?? navbarData()))

  // At phone width the expanded sidebar is a drawer laid over the page. Picking a room or a
  // conversation from it (or from the rooms index) navigates to an open room; the drawer has done
  // its job then, and left open it hides the room the person just chose. Only a change of
  // location counts (the first run is the page the person arrived on, whose default is decided
  // above), only a destination that is an open room, and only at phone width: desktop keeps its
  // column, and a rail tap to another section keeps the drawer open for that section's list.
  let lastNavigationKey = untrack(() => `${location.pathname}${location.search}`)
  createEffect(
    () => `${location.pathname}${location.search}`,
    (key) => {
      if (key === lastNavigationKey) return
      lastNavigationKey = key
      if (isPhoneViewport() && isOpenRoomPath(location.pathname)) setSidebarExpanded(false)
    },
  )

  createEffect(
    () => {
      if (navbarData.latest?.theme?.configured === false) return undefined
      return navbarData.latest?.theme?.theme
    },
    (theme) => {
      if (theme) applyThemePreference(theme)
    },
  )

  // No tracked reads here — this effect only sets up an imperative interval/
  // visibility listener once on mount, so compute is a constant and all the
  // side-effect work lives in the effect function.
  createEffect(
    () => undefined,
    () => {
      let refreshTimer: number | undefined
      const scheduleRefresh = () => {
        window.clearTimeout(refreshTimer)
        refreshTimer = window.setTimeout(() => {
          void refetchNavbar()
          scheduleRefresh()
        }, document.hidden ? 180_000 : 60_000)
      }
      const handleVisibilityChange = () => {
        if (!document.hidden) void refetchNavbar()
      }

      scheduleRefresh()
      document.addEventListener('visibilitychange', handleVisibilityChange)
      return () => {
        window.clearTimeout(refreshTimer)
        document.removeEventListener('visibilitychange', handleVisibilityChange)
      }
    },
  )

  return (
    <AgentsProvider routeLocation={location}>
      <div
        class="core-product-shell"
        data-chat-appearance={activeRoute() === '/chat' ? chatAppearanceTokens() : chatAppearanceActive('navigation') ? 'navigation' : undefined}
        data-sidebar-expanded={sidebarExpanded() ? 'true' : 'false'}
        style={{
          '--dashboard-navbar-height': deletionBannerVisible()
            ? `${56 + DELETION_BANNER_HEIGHT_PX}px`
            : '56px',
          '--dashboard-rail-width': `${sidebarWidth()}px`,
          // Only override when the workspace has its own accent; otherwise fall
          // through to the :root default (--verevon-accent, warm) to match v2.
          '--verevon-sidebar-accent': workspace().accentColor || undefined,
        }}
      >
        <RegionBoundary region="navbar" compact>
          <CoreNavbar
            activeRoute={activeRoute()}
            navbarData={navbarData.latest ?? navbarData()}
            onNavbarRefresh={() => void refetchNavbar()}
            searchOpen={searchOpen()}
            onSearchOpenChange={setSearchOpen}
            workspace={workspace()}
            sidebarExpanded={sidebarExpanded()}
            onSidebarToggle={() => setSidebarExpanded((expanded) => !expanded)}
          />
        </RegionBoundary>
        <Show when={deletionBannerVisible()}>
          <OrgDeletionBanner
            status={deletionStatus.latest ?? deletionStatus()}
            onRefetch={() => void refetchDeletionStatus()}
          />
        </Show>
        <Show when={chatAppearanceActive('navigation') && sidebarExpanded()}>
          <button type="button" class="core-sidebar-mobile-scrim" aria-label="Close navigation" onClick={() => setSidebarExpanded(false)} />
        </Show>
        <RegionBoundary region="sidebar" compact>
          <CoreSidebar
            activeRoute={activeRoute()}
            expanded={sidebarExpanded()}
            expandedWidth={expandedSidebarWidth()}
            onExpandedChange={setSidebarExpanded}
            onOpenSearch={() => setSearchOpen(true)}
          />
        </RegionBoundary>
        <main class="core-main">
          <div class="verevon-workspace-panel dashboard-main-panel core-main__panel">
            <CoreWorkspaceContext value={workspace}>
              <RegionBoundary region="outlet" resetKey={() => location.pathname}>
                {props.children}
              </RegionBoundary>
            </CoreWorkspaceContext>
          </div>
        </main>
        <FeedbackWidget />
        <AnnouncerRegions />
      </div>
    </AgentsProvider>
  )
}

/** `null` orgId (no active org yet) or a transient org-core failure both
 * resolve to `null` — the banner must never block the shell from rendering. */
async function fetchDeletionStatus(orgId: string | null) {
  if (!orgId) return null
  try {
    return await getDeletionStatus(orgId)
  } catch {
    return null
  }
}

async function fetchNavbarData(): Promise<NavbarPayload | null> {
  try {
    return await getNavbarData()
  } catch {
    // Keep shell rendering even when optional navbar integrations are offline.
    return null
  }
}

function resolveWorkspaceIdentity(
  session: SessionState,
  navbarData: NavbarPayload | null | undefined,
): WorkspaceIdentity {
  const organization = session.activeOrg
  const profile = navbarData?.profile ?? null
  const userName = profile?.name ?? session.user?.name ?? null
  const userEmail = profile?.email ?? session.user?.email ?? null
  const personalName = resolvePersonalName(userName, organization?.name, userEmail)
  // An org-style account name (e.g. "AQUATIQ AS") is the workspace label, not a
  // person — surface it as the workspace name when no explicit org name exists.
  const orgStyleName = userName && isOrgStyleName(userName.trim()) ? userName.trim() : null
  // Use `||` (not `??`) so an empty-string org name falls through to the
  // org-style account name instead of collapsing to the "Workspace" fallback.
  const name = organization?.name || orgStyleName || personalName || fallbackWorkspaceIdentity.name
  const plan = formatPlanLabel(navbarData?.plan ?? fallbackWorkspaceIdentity.plan)

  return {
    accentColor: navbarData?.theme?.colorScheme ?? null,
    initial: firstInitial(organization?.name, orgStyleName, personalName, userEmail),
    name: cleanOrganizationName(name),
    plan,
    planTrial: navbarData?.planTrial === true,
    role: organization?.role ?? null,
    userAvatar: profile?.avatar ?? session.user?.image ?? null,
    userEmail,
    userName: personalName,
  }
}

function applyThemePreference(theme: NonNullable<NavbarPayload['theme']>['theme']) {
  const prefersDark = window.matchMedia?.('(prefers-color-scheme: dark)').matches ?? false
  const dark = theme === 'dark' || (theme === 'system' && prefersDark)
  document.documentElement.classList.toggle('dark', dark)
}

function cleanOrganizationName(name: string): string {
  const trimmed = name.trim()
  if (!trimmed) return fallbackWorkspaceIdentity.name
  if (!/^[A-ZÆØÅ0-9 .&-]+$/.test(trimmed)) return trimmed

  return trimmed
    .toLowerCase()
    .split(' ')
    .map((word) => {
      if (['as', 'asa', 'ab', 'sa', 'ba', 'llc', 'inc'].includes(word)) return word.toUpperCase()
      return word.charAt(0).toUpperCase() + word.slice(1)
    })
    .join(' ')
}

function firstInitial(...values: Array<string | null | undefined>): string {
  const first = values.find((value) => value?.trim())
  return (first?.trim().charAt(0) || fallbackWorkspaceIdentity.initial).toUpperCase()
}

function normalizeIdentityName(value: string | null | undefined): string {
  return (value ?? '')
    .trim()
    .toLowerCase()
    .replace(/\b(as|asa|ab|sa|ba|llc|inc|ltd)\b/g, '')
    .replace(/[^a-z0-9æøå]+/g, '')
}

function sameIdentityName(first: string | null | undefined, second: string | null | undefined): boolean {
  const normalizedFirst = normalizeIdentityName(first)
  const normalizedSecond = normalizeIdentityName(second)
  return normalizedFirst.length > 0 && normalizedFirst === normalizedSecond
}

function displayNameFromEmail(email: string | null | undefined): string | null {
  const local = email?.split('@')[0]?.trim()
  if (!local) return null
  return local
    .split(/[._-]+/)
    .filter(Boolean)
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1).toLowerCase())
    .join(' ')
}

function resolvePersonalName(
  userName: string | null | undefined,
  organizationName: string | null | undefined,
  userEmail: string | null | undefined,
): string | null {
  const trimmed = userName?.trim()
  if (trimmed && !sameIdentityName(trimmed, organizationName) && !isOrgStyleName(trimmed)) return trimmed
  return displayNameFromEmail(userEmail)
}

// All-uppercase single word (no lowercase letters) = org identifier pattern, not a personal name.
// Matches "AQUATIQ", "VEREVON", "MICROSOFT" but not "Ima", "John Smith", "João".
function isOrgStyleName(name: string): boolean {
  return /^[A-ZÆØÅ0-9 ]{2,40}$/.test(name) && !/[a-zæøå]/.test(name)
}

/** Below this width the expanded sidebar is a drawer laid over the page, not a column beside it. */
const PHONE_MEDIA_QUERY = '(max-width: 767px)'

function isPhoneViewport(): boolean {
  try {
    return typeof window !== 'undefined' && window.matchMedia?.(PHONE_MEDIA_QUERY).matches === true
  } catch {
    return false
  }
}

/** An open room (`/spaces/:id`), as opposed to the rooms index at `/spaces`. */
function isOpenRoomPath(pathname: string): boolean {
  return /^\/spaces\/[^/]+/.test(pathname)
}

/**
 * Whether the sidebar starts expanded on a first visit. On a phone the expanded sidebar covers the
 * whole page, so a room opened by link or reload starts with the drawer closed: the person came for
 * the room, and the menu button is one tap away. The rooms index keeps its list open.
 */
export function defaultSidebarExpandedForRoute(
  route: ReturnType<typeof routeFromPath>,
  options: { readonly pathname?: string; readonly phone?: boolean } = {},
) {
  if (options.phone === true && isOpenRoomPath(options.pathname ?? '')) return false
  return route === '/spaces' ||
    route === '/support' ||
    route === '/social' ||
    route.startsWith('/studio') ||
    route.startsWith('/social') ||
    route.startsWith('/insights') ||
    route === '/social/calendar' ||
    route === '/agents' ||
    route === '/ingestions' ||
    route === '/knowledge' ||
    route === '/account' ||
    route === '/settings' ||
    route.startsWith('/settings/')
}
