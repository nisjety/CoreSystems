'use client';

import { Sidebar } from '@/components/core/sidebar';
import { SIDEBAR_EXPANDED_WIDTH, SIDEBAR_MINIMIZED_WIDTH } from '@/components/core/sidebar/constants';
import { SidebarProvider, useSidebar } from '@/components/core/shared/SidebarContext';
import { ChatProvider } from '@/components/chat/providers/ChatProvider';
import { ChatWorkspaceProvider } from '@/components/chat/providers/ChatWorkspaceProvider';
import { DashboardSearchProvider } from '@/components/dashboard/DashboardSearchContext';
import { DashboardFooter } from '@/components/dashboard/DashboardFooter';
import { DashboardTopNavbar } from '@/components/dashboard/DashboardTopNavbar';
import { GlobalSearchModal } from '@/components/dashboard/GlobalSearchModal';
import { OnboardingGuard } from '@/components/onboarding/guards';

const DASHBOARD_NAVBAR_HEIGHT = 56;

function DashboardLayoutContent({ children }: { children: React.ReactNode }) {
  const { isMinimized, isMobile } = useSidebar();
  const sidebarWidth = isMinimized ? SIDEBAR_MINIMIZED_WIDTH : SIDEBAR_EXPANDED_WIDTH;
  const dashboardContentHeight = `calc(100dvh - ${DASHBOARD_NAVBAR_HEIGHT}px)`;

  return (
    <div
      className="flex h-dvh flex-col overflow-hidden"
      style={{
        ['--dashboard-navbar-height' as string]: `${DASHBOARD_NAVBAR_HEIGHT}px`,
        ['--dashboard-content-height' as string]: dashboardContentHeight,
      }}
    >
      <DashboardTopNavbar />
      <GlobalSearchModal />
      <div aria-hidden="true" className="h-14 shrink-0" />
      <div className="flex min-h-0 flex-1 overflow-hidden">
        <Sidebar />
        <div
          aria-hidden="true"
          className="shrink-0 transition-[width] duration-300"
          style={{ width: isMobile ? '0' : `${sidebarWidth}px` }}
        />
        <main
          className="relative flex min-h-0 min-w-0 flex-1 flex-col"
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
            <DashboardFooter />
          </div>
        </main>
      </div>
    </div>
  );
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
          <ChatProvider>
            <ChatWorkspaceProvider>
              <DashboardLayoutContent>{children}</DashboardLayoutContent>
            </ChatWorkspaceProvider>
          </ChatProvider>
        </DashboardSearchProvider>
      </SidebarProvider>
    </OnboardingGuard>
  );
}
