'use client';

import Link from 'next/link';
import React from 'react';
import { usePathname, useRouter } from 'next/navigation';
import {
  Bell,
  CalendarDays,
  ChevronLeft,
  ChevronRight,
  MessageSquareMore,
  MoonStar,
  PanelLeftClose,
  PanelLeftOpen,
  Search,
  Sparkles,
  Slash,
} from 'lucide-react';
import { cn } from '@/lib/utils';
import { useAuth } from '@/components/auth/hooks/use-auth';
import { useCompatibleLanguage } from '@/components/core/contexts/GlobalLanguageContext';
import { useNotificationCount } from '@/components/core/navbar/hooks/useCorebar';
import { useUser } from '@/components/core/sidebar/hooks/useRealData';
import { useSidebar } from '@/components/core/shared/SidebarContext';
import { useDashboardSearch } from './DashboardSearchContext';
import { resolveDashboardNavbarContext } from './dashboard-navbar-context';

function ActionButton({
  label,
  onClick,
  children,
  className,
}: {
  label: string;
  onClick?: () => void;
  children: React.ReactNode;
  className?: string;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-label={label}
      title={label}
      className={cn(
        'flex h-9 w-9 items-center justify-center rounded-[10px] bg-transparent text-[#6F737C] shadow-none transition-[background-color,color,box-shadow] duration-200 motion-safe:transition-transform motion-safe:hover:scale-[1.04] hover:bg-white hover:text-[#383B43] hover:shadow-[0_10px_24px_rgba(17,17,17,0.12)] active:bg-[#F5F5F5] active:shadow-[0_4px_12px_rgba(17,17,17,0.08)] motion-safe:active:scale-[0.97] focus:outline-none motion-safe:focus-visible:scale-[1.04] focus-visible:bg-white focus-visible:text-[#383B43] focus-visible:ring-2 focus-visible:ring-[#111111]/30 focus-visible:shadow-[0_10px_24px_rgba(17,17,17,0.12)]',
        className,
      )}
    >
      {children}
    </button>
  );
}

function NavDivider() {
  return <div className="mx-2 h-7 w-px bg-[#E4E0D8]" aria-hidden="true" />;
}

function SearchTrigger({
  query,
  recentQueries,
  onOpen,
}: {
  query: string;
  recentQueries: string[];
  onOpen: () => void;
}) {
  const rotatingQueries = React.useMemo(() => recentQueries.slice(0, 3), [recentQueries]);
  const [activeQueryIndex, setActiveQueryIndex] = React.useState(0);
  const safeActiveQueryIndex = rotatingQueries.length > 0 ? activeQueryIndex % rotatingQueries.length : 0;
  const shortcutModifier = React.useMemo(() => {
    if (typeof window === 'undefined') {
      return 'Cmd';
    }

    return /Mac|iPhone|iPad|iPod/.test(window.navigator.userAgent) ? 'Cmd' : 'Ctrl';
  }, []);

  React.useEffect(() => {
    if (rotatingQueries.length <= 1) {
      setActiveQueryIndex(0);
      return;
    }

    if (window.matchMedia('(prefers-reduced-motion: reduce)').matches) {
      setActiveQueryIndex(0);
      return;
    }

    const intervalId = window.setInterval(() => {
      setActiveQueryIndex((currentIndex) => (currentIndex + 1) % rotatingQueries.length);
    }, 2400);

    return () => window.clearInterval(intervalId);
  }, [rotatingQueries]);

  const activeRecentQuery = rotatingQueries[safeActiveQueryIndex];

  return (
    <button
      type="button"
      onClick={onOpen}
      className="flex h-10 w-full items-center gap-3 rounded-[14px] border border-[#E2E3E9] bg-[#F4F5F1] px-4 text-left text-[#7F7A72] transition-colors hover:bg-[#EBECE7] focus:outline-none focus-visible:ring-2 focus-visible:ring-[#111111]/10"
      aria-label="Open global search"
    >
      <Search className="h-4 w-4 shrink-0 text-[#989286]" strokeWidth={1.8} />
      <div className="min-w-0 flex-1 overflow-hidden">
        {activeRecentQuery ? (
          <div className="flex items-center gap-2 overflow-hidden whitespace-nowrap">
            <span className="shrink-0 text-[10px] uppercase tracking-[0.24em] text-[#A49B8E]">Recent</span>
            <span
              key={activeRecentQuery}
              className="block truncate rounded-full border border-[#E2DDD4] bg-white px-3 py-1 text-[11px] text-[#615B52] animate-in fade-in-0 slide-in-from-bottom-1 duration-300"
            >
              {activeRecentQuery}
            </span>
            {rotatingQueries.length > 1 ? (
              <span className="hidden items-center gap-1 lg:inline-flex">
                {rotatingQueries.map((recentQuery, index) => (
                  <span
                    key={index}
                    className={cn(
                      'h-1.5 w-1.5 rounded-full transition-colors',
                      index === safeActiveQueryIndex ? 'bg-[#8E867A]' : 'bg-[#D9D3C8]'
                    )}
                  />
                ))}
              </span>
            ) : null}
          </div>
        ) : (
          <span className="block truncate text-[13px] text-[#5F5A52]">
            {query.trim() ? query : 'Search across the whole system'}
          </span>
        )}
      </div>
      <span className="hidden items-center gap-2 lg:inline-flex">
        <span className="inline-flex items-center rounded-xl border border-[#D8D2C6] bg-white px-2 py-1 text-[10px] uppercase tracking-[0.18em] text-[#9A9387] shadow-[inset_0_0_0_1px_rgba(0,0,0,0.04)]">
          /
        </span>
        <span className="inline-flex items-center rounded-xl border border-[#D8D2C6] bg-white px-2 py-1 text-[10px] uppercase tracking-[0.18em] text-[#9A9387] shadow-[inset_0_0_0_1px_rgba(0,0,0,0.04)]">
          {shortcutModifier}+K
        </span>
      </span>
    </button>
  );
}

export function DashboardTopNavbar() {
  const pathname = usePathname();
  const router = useRouter();
  const { user: authUser } = useAuth();
  const { sidebarLocale } = useCompatibleLanguage();
  const { data: userData } = useUser({ enabled: !!authUser });
  const { data: unreadNotificationCount } = useNotificationCount();
  const { isMinimized, isMobile, toggleMinimize } = useSidebar();
  const { globalSearchQuery, recentQueries, openGlobalSearch } = useDashboardSearch();

  const { moduleLabel, moduleHref, tabLabel, tabHref } = React.useMemo(
    () => resolveDashboardNavbarContext(pathname, sidebarLocale),
    [pathname, sidebarLocale]
  );

  const profileDisplayName = userData?.name || authUser?.name || 'Account';
  const profileInitial = (profileDisplayName.trim().charAt(0) || 'A').toUpperCase();
  const unreadCount = unreadNotificationCount?.count ?? 0;

  const sidebarToggleLabel = isMinimized ? 'Expand sidebar' : 'Collapse sidebar';
  const SidebarToggleIcon = isMinimized ? PanelLeftOpen : PanelLeftClose;

  return (
    <header className="fixed inset-x-0 top-0 z-50 border-b border-[#E9EBF2] bg-white">
      <div className="flex h-14 items-center justify-between gap-4 px-5">
        <div className="flex min-w-0 items-center gap-3">
          {!isMobile ? (
            <ActionButton label={sidebarToggleLabel} onClick={toggleMinimize}>
              <SidebarToggleIcon className="h-4 w-4" strokeWidth={1.9} />
            </ActionButton>
          ) : null}

          <div className="flex items-center gap-0.5 rounded-[12px] bg-[#F4F5F1] px-1 py-1">
            <ActionButton label="Go back" onClick={() => router.back()}>
              <ChevronLeft className="h-4 w-4" strokeWidth={2.1} />
            </ActionButton>
            <ActionButton label="Go forward" onClick={() => router.forward()}>
              <ChevronRight className="h-4 w-4" strokeWidth={2.1} />
            </ActionButton>
          </div>

          <div className="hidden min-w-0 items-center gap-1 text-[14px] tracking-[-0.02em] text-[#6D717B] lg:flex">
            <Link
              href={moduleHref}
              className="truncate font-medium text-[#2A2D35] transition-colors hover:text-[#111111]"
            >
              {moduleLabel}
            </Link>
            <Slash className="h-3.5 w-3.5 shrink-0 text-[#C0C4CC]" strokeWidth={2} />
            <Link
              href={tabHref}
              className="truncate font-medium text-[#7A7F89] transition-colors hover:text-[#2A2D35]"
            >
              {tabLabel}
            </Link>
          </div>
        </div>

        <div className="hidden min-w-0 max-w-[440px] flex-1 md:block">
          <SearchTrigger query={globalSearchQuery} recentQueries={recentQueries} onOpen={() => openGlobalSearch('navbar')} />
        </div>

        <div className="flex items-center gap-2">
          {isMobile ? (
            <ActionButton label="Open global search" onClick={() => openGlobalSearch('navbar')}>
              <Search className="h-4 w-4" strokeWidth={1.9} />
            </ActionButton>
          ) : null}

          <div className="flex items-center gap-1 pl-2">
            <NavDivider />

            <div className="relative">
              <ActionButton
                label={unreadCount > 0 ? `Open overview updates (${unreadCount} unread)` : 'Open overview updates'}
                onClick={() => router.push('/overview')}
              >
                <MoonStar className="h-[18px] w-[18px]" strokeWidth={1.85} />
              </ActionButton>
              {unreadCount > 0 ? (
                <span
                  aria-label={`${unreadCount} unread notifications`}
                  className="pointer-events-none absolute right-0.5 top-0.5 inline-flex min-w-4 items-center justify-center rounded-full bg-[#E8F1FF] px-1 text-[9px] font-semibold leading-4 text-[#3578F6]"
                >
                  {Math.min(unreadCount, 9)}
                  {unreadCount > 9 ? '+' : null}
                </span>
              ) : null}
            </div>

            <NavDivider />

            <ActionButton label="Open quick actions" onClick={() => router.push('/helpdesk')}>
              <Sparkles className="h-[18px] w-[18px]" strokeWidth={1.85} />
            </ActionButton>

            <ActionButton label="Open conversations" onClick={() => router.push('/answers')}>
              <MessageSquareMore className="h-[18px] w-[18px]" strokeWidth={1.85} />
            </ActionButton>

            <ActionButton label="Open notifications" onClick={() => router.push('/notifications')}>
              <Bell className="h-[18px] w-[18px]" strokeWidth={1.85} />
            </ActionButton>

            <ActionButton label="Open calendar" onClick={() => router.push('/calendar')}>
              <CalendarDays className="h-[18px] w-[18px]" strokeWidth={1.85} />
            </ActionButton>
          </div>

          {isMobile ? (
            <>
              <NavDivider />

              <Link
                href="/profile"
                aria-label="Open profile"
                className="flex h-7 w-7 items-center justify-center rounded-full bg-[linear-gradient(135deg,#F2DFC2,#E6B783)] text-[11px] font-semibold text-[#4A341A] transition-transform hover:scale-[1.03] focus:outline-none focus-visible:ring-2 focus-visible:ring-[#111111]/10"
              >
                {profileInitial}
              </Link>
            </>
          ) : null}
        </div>
      </div>
    </header>
  );
}
