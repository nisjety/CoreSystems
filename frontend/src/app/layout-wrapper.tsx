'use client';

import { Sidebar } from '../components/core/sidebar';
import { SIDEBAR_EXPANDED_WIDTH, SIDEBAR_MINIMIZED_WIDTH } from '../components/core/sidebar/constants';
import { useSidebar } from '../components/core/shared/SidebarContext';
import { usePathname } from 'next/navigation';

export function LayoutWrapper({ children }: { children: React.ReactNode }) {
  const { isMinimized } = useSidebar();
  const pathname = usePathname();
  
  // Don't show sidebar on auth pages
  const isAuthPage = pathname?.startsWith('/intro') || 
                     pathname?.startsWith('/sign-in') || 
                     pathname?.startsWith('/sign-up') ||
                     pathname?.startsWith('/verify-email') ||
                     pathname?.startsWith('/forgot-password');

  if (isAuthPage) {
    return <>{children}</>;
  }

  return (
    <div className="flex min-h-screen bg-gray-50">
      <Sidebar />
      <main
        className="overflow-x-hidden transition-all duration-300 relative min-h-screen"
        style={{
          width: `calc(100% - ${isMinimized ? SIDEBAR_MINIMIZED_WIDTH : SIDEBAR_EXPANDED_WIDTH}px)`,
          marginLeft: isMinimized ? `${SIDEBAR_MINIMIZED_WIDTH}px` : `${SIDEBAR_EXPANDED_WIDTH}px`,
        }}
      >
        <div className="dashboard-full-bleed w-full h-full">
          {children}
        </div>
      </main>
    </div>
  );
}
