'use client';

import { Sidebar } from '../components/core/Sidebar';
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
        className="flex-1 overflow-x-hidden transition-all duration-300 relative"
        style={{ 
          marginLeft: isMinimized ? '80px' : '260px' 
        }}
      >
        {children}
      </main>
    </div>
  );
}
