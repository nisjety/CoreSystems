import { useLocation } from '@solidjs/router'
import { createEffect, createMemo, createResource, createSignal, onCleanup, Show, type JSX } from 'solid-js'
import { AgentsProvider } from '@/features/agents/lib/use-agent-selection'
import { CoreNavbar } from '@/features/core/components/CoreNavbar'
import { CoreSidebar, SIDEBAR_EXPANDED_WIDTH, SIDEBAR_MINIMIZED_WIDTH } from '@/features/core/components/CoreSidebar'
import { FeedbackWidget } from '@/features/core/components/FeedbackWidget'
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

/** Extra height the fixed org-deletion banner (Flow C) occupies below the
 * navbar — added to `--dashboard-navbar-height` so `.core-main`'s top
 * padding reflows content below the banner instead of it being covered. */
const DELETION_BANNER_HEIGHT_PX = 40

export function CoreShell(props: { children?: JSX.Element }) {
  const location = useLocation()
  const session = getSession()
  const initialRoute = routeFromPath(location.pathname)
  const [sidebarExpanded, setSidebarExpanded] = createSignal(defaultSidebarExpandedForRoute(initialRoute))
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

  createEffect(() => {
    if (navbarData.latest?.theme?.configured === false) return
    const theme = navbarData.latest?.theme?.theme
    if (theme) applyThemePreference(theme)
  })

  createEffect(() => {
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
    onCleanup(() => {
      window.clearTimeout(refreshTimer)
      document.removeEventListener('visibilitychange', handleVisibilityChange)
    })
  })

  return (
    <AgentsProvider routeLocation={location}>
      <div
        class="core-product-shell"
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
        <CoreNavbar
          activeRoute={activeRoute()}
          navbarData={navbarData.latest ?? navbarData()}
          onNavbarRefresh={() => void refetchNavbar()}
          searchOpen={searchOpen()}
          onSearchOpenChange={setSearchOpen}
          workspace={workspace()}
        />
        <Show when={deletionBannerVisible()}>
          <OrgDeletionBanner
            status={deletionStatus.latest ?? deletionStatus()}
            onRefetch={() => void refetchDeletionStatus()}
          />
        </Show>
        <CoreSidebar
          activeRoute={activeRoute()}
          expanded={sidebarExpanded()}
          expandedWidth={expandedSidebarWidth()}
          onExpandedChange={setSidebarExpanded}
          onOpenSearch={() => setSearchOpen(true)}
        />
        <main class="core-main">
          <div class="verevon-workspace-panel dashboard-main-panel core-main__panel">
            <CoreWorkspaceContext.Provider value={workspace}>
              {props.children}
            </CoreWorkspaceContext.Provider>
          </div>
        </main>
        <FeedbackWidget />
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

function defaultSidebarExpandedForRoute(route: ReturnType<typeof routeFromPath>) {
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
