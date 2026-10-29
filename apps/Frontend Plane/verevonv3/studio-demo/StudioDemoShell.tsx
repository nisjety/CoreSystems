import { useLocation, useNavigate } from '@solidjs/router'
import { createMemo, createSignal, onMount } from 'solid-js'
import { CoreNavbar } from '@/features/core/components/CoreNavbar'
import { CoreSidebar, SIDEBAR_EXPANDED_WIDTH } from '@/features/core/components/CoreSidebar'
import { routeFromPath, type WorkspaceIdentity } from '@/features/core/lib/shell-data'
import { sidebarSections, type SidebarSection } from '@/features/core/lib/sidebar-navigation'
import StudioPage from '@/features/studio/components/StudioPage'
import { MarketingPulse } from './MarketingPulse'

const studioDemoWorkspace: WorkspaceIdentity = {
  accentColor: '#111111',
  initial: 'S',
  name: 'Studio demo',
  plan: 'Local',
}

/**
 * The demo deliberately exposes only routes that this static Studio server
 * implements. The labels, icons and hierarchy come from the live SPA's shared
 * navigation contract, so visual changes remain aligned without exposing an
 * account, global search, or API-backed destination.
 */
const studioDemoNavigation = sidebarSections
  .filter((section) => section.id === 'marketing' || section.id === 'studio')
  .map(restrictToStudioRoutes)

function restrictToStudioRoutes(section: SidebarSection): SidebarSection {
  const panelGroups = section.panelGroups
    .map((group) => ({
      ...group,
      items: group.items.filter((item) => item.href.startsWith('/studio')),
    }))
    .filter((group) => group.items.length > 0)

  return {
    ...section,
    panelGroups,
  }
}

function sectionForPath(pathname: string): 'pulse' | 'canvas' | 'campaigns' | 'templates' {
  if (pathname.startsWith('/studio/pulse')) return 'pulse'
  if (pathname.startsWith('/studio/campaigns')) return 'campaigns'
  if (pathname.startsWith('/studio/templates')) return 'templates'
  return 'canvas'
}

export function StudioDemoShell() {
  const location = useLocation()
  const navigate = useNavigate()
  const [sidebarExpanded, setSidebarExpanded] = createSignal(true)
  const activeRoute = createMemo(() => routeFromPath(location.pathname))
  const activeSection = createMemo(() => sectionForPath(location.pathname))
  const activeStudioSection = createMemo(() => {
    const section = activeSection()
    return section === 'pulse' ? undefined : section
  })
  const sidebarWidth = () => `${sidebarExpanded() ? SIDEBAR_EXPANDED_WIDTH : 60}px`

  onMount(() => {
    if (!location.pathname.startsWith('/studio')) {
      navigate('/studio/canvas', { replace: true })
    }
  })

  return (
    <div
      class="core-product-shell studio-demo-shell"
      style={{
        '--dashboard-navbar-height': '56px',
        '--dashboard-rail-width': sidebarWidth(),
        '--verevon-sidebar-accent': studioDemoWorkspace.accentColor ?? undefined,
      }}
    >
      <CoreNavbar
        activeRoute={activeRoute()}
        homeHref="/studio/canvas"
        offline
        workspace={studioDemoWorkspace}
      />
      <CoreSidebar
        activeRoute={activeRoute()}
        expanded={sidebarExpanded()}
        navigationSections={studioDemoNavigation}
        onExpandedChange={setSidebarExpanded}
        onOpenSearch={() => undefined}
        showAccountActions={false}
        showSearchAction={false}
      />
      <main class="core-main">
        <div class="verevon-workspace-panel dashboard-main-panel core-main__panel">
          {activeSection() === 'pulse'
            ? <MarketingPulse />
            : <StudioPage mode="demo" section={activeStudioSection()} />}
        </div>
      </main>
    </div>
  )
}
