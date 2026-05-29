'use client';

import { usePathname } from 'next/navigation';
import dynamic from 'next/dynamic';
import { Sidebar } from '@/components/core/sidebar';
import { SIDEBAR_EXPANDED_WIDTH, SIDEBAR_MINIMIZED_WIDTH } from '@/components/core/sidebar/constants';
import { SidebarProvider, useSidebar } from '@/components/core/shared/SidebarContext';
import { ChatProvider } from '@/components/chat/providers/ChatProvider';
import { ChatWorkspaceProvider } from '@/components/chat/providers/ChatWorkspaceProvider';
import { DashboardSearchProvider } from '@/components/dashboard/DashboardSearchContext';
import { DashboardRouteWarmup } from '@/components/dashboard/DashboardRouteWarmup';
import { DashboardTopNavbar } from '@/components/core/navbar/navbar-dashboard';
import { ConnectorConsentPrompt } from '@/components/dashboard/ConnectorConsentPrompt';
import { EnterpriseTrustBanner } from '@/components/dashboard/EnterpriseTrustBanner';
import { OnboardingGuard } from '@/components/onboarding/guards';
import { useEntitlementToast } from '@/lib/notifications/useEntitlementToast';

const DASHBOARD_NAVBAR_HEIGHT = 56;
const GlobalSearchModal = dynamic(
  () => import('@/components/dashboard/GlobalSearchModal').then((mod) => mod.GlobalSearchModal),
  { ssr: false },
);

function DashboardLayoutContent({ children }: { children: React.ReactNode }) {
  const { isMinimized, isMobile } = useSidebar();
  const sidebarWidth = isMinimized ? SIDEBAR_MINIMIZED_WIDTH : SIDEBAR_EXPANDED_WIDTH;
  const dashboardContentHeight = `calc(100dvh - ${DASHBOARD_NAVBAR_HEIGHT}px)`;

  // G44 (velion-gap.md §8.30): pop a sonner toast whenever notification-core
  // delivers a `control_session.entitlements_changed` notification (plan
  // upgrade, org switch, billing webhook ack). The hook is mounted at the
  // layout level so the toast fires regardless of which dashboard sub-page
  // the user is on.
  useEntitlementToast();

  return (
    <div
      className="relative flex h-dvh flex-col overflow-hidden bg-[var(--linear-sidebar-bg)]"
      style={{
        ['--dashboard-navbar-height' as string]: `${DASHBOARD_NAVBAR_HEIGHT}px`,
        ['--dashboard-content-height' as string]: dashboardContentHeight,
      }}
    >
      {/* Dashboard canvas that hides the root-layout animated noise
          everywhere in the dashboard. */}
      <div className="dashboard-solid-canvas" aria-hidden="true" />

      <DashboardTopNavbar />
      <DashboardRouteWarmup />
      <GlobalSearchModal />
      <div aria-hidden="true" className="h-14 shrink-0 bg-[var(--linear-sidebar-bg)]" />
      {/* G21: enterprise trust banner — verifies the zero-input sign-in result on first dashboard load. */}
      <EnterpriseTrustBanner />
      {/* G45 (Slice F): connector consent — shown only after `FIRST_VALUE_DELAY_MS`
          on the dashboard AND when the user has no Microsoft connection yet.
          Self-positions as a bottom-right popover; no layout impact when hidden. */}
      <ConnectorConsentPrompt />
      <div className="flex min-h-0 flex-1 overflow-hidden bg-[var(--linear-sidebar-bg)]">
        <Sidebar />
        <div
          aria-hidden="true"
          className="shrink-0 transition-[width] duration-300"
          style={{ width: isMobile ? '0' : `${sidebarWidth}px` }}
        />
        <main
          className="dashboard-main-panel relative flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden bg-[var(--linear-main-bg)] md:rounded-tl-[24px]"
          style={{
            ['--dashboard-sidebar-offset' as string]: isMobile ? '0px' : `${sidebarWidth}px`,
            height: 'var(--dashboard-content-height)',
          }}
        >
          <div
            className="flex min-h-0 flex-1 flex-col"
            style={{
              width: 'calc(100% + var(--dashboard-sidebar-offset, 0px))',
              marginLeft: 'calc(-1 * var(--dashboard-sidebar-offset, 0px))',
              paddingLeft: 'var(--dashboard-sidebar-offset, 0px)',
            }}
          >
            <div className="min-h-0 flex-1">{children}</div>
          </div>
        </main>
      </div>
    </div>
  );
}

function DashboardScopedProviders({ children }: { children: React.ReactNode }) {
  const pathname = usePathname();
  const needsChatProvider = pathname === '/chat' || pathname.startsWith('/chat/');

  const workspace = (
    <ChatWorkspaceProvider>
      <DashboardLayoutContent>{children}</DashboardLayoutContent>
    </ChatWorkspaceProvider>
  );

  return needsChatProvider ? <ChatProvider>{workspace}</ChatProvider> : workspace;
}

export default function DashboardLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  return (
    <OnboardingGuard>
      <SidebarProvider>
        <DashboardSearchProvider>
          <DashboardScopedProviders>{children}</DashboardScopedProviders>
        </DashboardSearchProvider>
      </SidebarProvider>
    </OnboardingGuard>
  );
}
