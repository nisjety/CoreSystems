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
  MessageCircle,
  Heart,
  Plus,
  Box,
} from 'lucide-react';
import { cn } from '@/lib/utils';
import { useAuth } from '@/components/auth/hooks/use-auth';
import { useCompatibleLanguage } from '@/components/core/contexts/GlobalLanguageContext';
import { useNotificationCount, useNotifications as useCorebarNotifications } from '@/components/core/navbar/hooks/useCorebar';
import { useNotificationWS } from '@/lib/notifications/ws';
import { useUser } from '@/components/core/sidebar/hooks/useRealData';
import { useSidebar } from '@/components/core/shared/SidebarContext';
import { useDashboardSearch } from '../../../dashboard/DashboardSearchContext';
import { resolveDashboardNavbarContext } from '../../../dashboard/dashboard-navbar-context';
import {
  NavbarActionButton,
  AIChatModal,
  MessagesDropdown,
  NotificationsDropdown,
  CalendarDropdown,
  ProfileDropdown,
} from './modals';

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
  const [shortcutModifier, setShortcutModifier] = React.useState<'Cmd' | 'Ctrl'>('Cmd');
  const safeActiveQueryIndex = rotatingQueries.length > 0 ? activeQueryIndex % rotatingQueries.length : 0;

  React.useEffect(() => {
    if (typeof window === 'undefined') {
      return;
    }

    setShortcutModifier(/Mac|iPhone|iPad|iPod/.test(window.navigator.userAgent) ? 'Cmd' : 'Ctrl');
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
                    key={recentQuery}
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

function BreadcrumbNav({
  moduleLabel,
  moduleHref,
  tabLabel,
  tabHref,
}: {
  moduleLabel: string;
  moduleHref: string;
  tabLabel: string;
  tabHref: string;
}) {
  return (
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
  );
}

function HistoryNav({
  onBack,
  onForward,
}: {
  onBack: () => void;
  onForward: () => void;
}) {
  return (
    <div className="flex items-center gap-0.5 rounded-[12px] bg-[#F4F5F1] px-1 py-1">
      <NavbarActionButton
        label="Go back"
        tooltip="Go back"
        onClick={onBack}
      >
        <ChevronLeft className="h-4 w-4" strokeWidth={2.1} />
      </NavbarActionButton>
      <NavbarActionButton
        label="Go forward"
        tooltip="Go forward"
        onClick={onForward}
      >
        <ChevronRight className="h-4 w-4" strokeWidth={2.1} />
      </NavbarActionButton>
    </div>
  );
}

export function DashboardTopNavbar() {
  const pathname = usePathname();
  const router = useRouter();
  const { user: authUser } = useAuth();
  const { sidebarLocale } = useCompatibleLanguage();
  const { data: userData } = useUser({ enabled: !!authUser });
  const { data: unreadNotificationCount } = useNotificationCount();
  const { data: corebarNotifications = [] } = useCorebarNotifications();

  // Real-time updates via WebSocket; falls back to 5-min polling when WS is unavailable
  useNotificationWS(authUser?.id);
  const { isMinimized, toggleMinimize } = useSidebar();
  const { globalSearchQuery, recentQueries, openGlobalSearch } = useDashboardSearch();

  // Modal states
  const [isAIChatOpen, setIsAIChatOpen] = React.useState(false);
  const [isMessagesOpen, setIsMessagesOpen] = React.useState(false);
  const [isNotificationsOpen, setIsNotificationsOpen] = React.useState(false);
  const [isCalendarOpen, setIsCalendarOpen] = React.useState(false);
  const [isProfileOpen, setIsProfileOpen] = React.useState(false);

  // Mock data for messages
  const mockMessages = [
    {
      id: '1',
      senderName: 'Sarah Chen',
      senderAvatar: 'SC',
      preview: 'Hey, can you review the latest design mockups?',
      timestamp: '2m ago',
      unread: true,
      category: 'message' as const,
    },
    {
      id: '2',
      senderName: 'Alex Rivera',
      senderAvatar: 'AR',
      preview: 'The API documentation is ready for review',
      timestamp: '15m ago',
      unread: true,
      category: 'message' as const,
    },
    {
      id: '3',
      senderName: 'Jordan Kim',
      senderAvatar: 'JK',
      preview: 'Meeting rescheduled to 3 PM',
      timestamp: '1h ago',
      unread: false,
      category: 'message' as const,
    },
    {
      id: '4',
      senderName: 'Casey Morgan',
      senderAvatar: 'CM',
      preview: 'Thanks for the feedback on the proposal!',
      timestamp: '2h ago',
      unread: false,
      category: 'message' as const,
    },
    {
      id: '5',
      senderName: 'Pixelwave',
      senderAvatar: 'PW',
      preview: '@you in Classic Car — these draggable sliders look really cool!',
      timestamp: '1h ago',
      unread: true,
      category: 'mention' as const,
    },
    {
      id: '6',
      senderName: 'Marcus T.',
      senderAvatar: 'MT',
      preview: '@you nice work on the onboarding flow 🔥',
      timestamp: '3h ago',
      unread: false,
      category: 'mention' as const,
    },
  ];

  const formatRelativeTime = React.useCallback((timestamp: string) => {
    const parsed = new Date(timestamp);
    const diffMs = Date.now() - parsed.getTime();
    const diffMinutes = Math.max(0, Math.floor(diffMs / (1000 * 60)));

    if (diffMinutes < 1) {
      return 'now';
    }

    if (diffMinutes < 60) {
      return `${diffMinutes}m ago`;
    }

    const diffHours = Math.floor(diffMinutes / 60);
    if (diffHours < 24) {
      return `${diffHours}h ago`;
    }

    const diffDays = Math.floor(diffHours / 24);
    return `${diffDays}d ago`;
  }, []);

  const dropdownNotifications = React.useMemo(() => {
    return corebarNotifications.map((notification) => {
      const isTeamEvent =
        notification.event_type === 'team_invite_sent' ||
        notification.event_type === 'user_mentioned';
      return {
        id: notification.id,
        actor: notification.actor_name ?? notification.title,
        actorAvatar: notification.actor_avatar,
        actionText: notification.body,
        timestamp: formatRelativeTime(notification.created_at),
        badgeColor: isTeamEvent ? 'bg-[#7C3AED]' : 'bg-[#3578F6]',
        badgeIcon: isTeamEvent ? <MessageCircle /> : <Bell />,
        read: notification.read,
        category: isTeamEvent ? ('message' as const) : ('system' as const),
      };
    });
  }, [corebarNotifications, formatRelativeTime]);

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
    <>
      {/* Phase A · navbar gets the scoped noise field. The cream base
          colour matches the `dashboard-solid-canvas` veil so the visual
          edge between navbar and content is just the 1px hairline below,
          and the noise reads as a texture on the navbar surface rather
          than a separate panel. Auth pages still use the full-viewport
          noise from `app/layout.tsx`. */}
      <header className="navbar-noise-bg fixed inset-x-0 top-0 z-40 border-b border-[#E9EBF2]">
        <div className="flex h-14 items-center justify-between gap-4 px-5">
          <div className="flex min-w-0 items-center gap-3">
            <NavbarActionButton
              label={sidebarToggleLabel}
              tooltip={sidebarToggleLabel}
              onClick={toggleMinimize}
              className="hidden md:flex"
            >
              <SidebarToggleIcon className="h-4 w-4" strokeWidth={1.9} />
            </NavbarActionButton>

            <HistoryNav onBack={() => router.back()} onForward={() => router.forward()} />

            <BreadcrumbNav
              moduleLabel={moduleLabel}
              moduleHref={moduleHref}
              tabLabel={tabLabel}
              tabHref={tabHref}
            />
          </div>

          <div className="hidden min-w-0 max-w-[440px] flex-1 md:block">
            <SearchTrigger query={globalSearchQuery} recentQueries={recentQueries} onOpen={() => openGlobalSearch('navbar')} />
          </div>

          <div className="flex items-center gap-2">
            <NavbarActionButton
              label="Open global search"
              tooltip="Open global search"
              onClick={() => openGlobalSearch('navbar')}
              className="md:hidden"
            >
              <Search className="h-4 w-4" strokeWidth={1.9} />
            </NavbarActionButton>

            <div className="flex items-center gap-1 pl-2">
              <NavDivider />

              {/* Dark Mode Toggle - Tooltip only */}
              <NavbarActionButton
                label="Toggle dark mode"
                tooltip="Toggle dark mode"
                onClick={() => {
                  // TODO: Implement dark mode toggle
                }}
              >
                <MoonStar className="h-[18px] w-[18px]" strokeWidth={1.85} />
              </NavbarActionButton>

              <NavDivider />

              {/* AI Chat Modal */}
              <NavbarActionButton
                label="Open AI assistant"
                tooltip="AI assistant for current page"
                active={isAIChatOpen}
                onClick={() => setIsAIChatOpen(!isAIChatOpen)}
              >
                <Sparkles className="h-[18px] w-[18px]" strokeWidth={1.85} />
              </NavbarActionButton>

              {/* Messages Dropdown */}
              <MessagesDropdown
                isOpen={isMessagesOpen}
                onOpenChange={setIsMessagesOpen}
                messages={mockMessages}
                trigger={
                  <NavbarActionButton
                    label="Quick messages"
                    active={isMessagesOpen}
                  >
                    <MessageSquareMore className="h-[18px] w-[18px]" strokeWidth={1.85} />
                  </NavbarActionButton>
                }
              />

              {/* Notifications Dropdown */}
              <NotificationsDropdown
                isOpen={isNotificationsOpen}
                onOpenChange={setIsNotificationsOpen}
                notifications={dropdownNotifications}
                trigger={
                  <div className="relative">
                    <NavbarActionButton
                      label={unreadCount > 0 ? `${unreadCount} unread notifications` : 'Notifications'}
                      active={isNotificationsOpen}
                    >
                      <Bell className="h-[18px] w-[18px]" strokeWidth={1.85} />
                    </NavbarActionButton>
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
                }
              />

              {/* Calendar Dropdown */}
              <CalendarDropdown
                isOpen={isCalendarOpen}
                onOpenChange={setIsCalendarOpen}
                onDateSelect={(date) => {
                  // TODO: Handle date selection
                  console.log('Selected date:', date);
                }}
                trigger={
                  <NavbarActionButton
                    label="Calendar"
                    active={isCalendarOpen}
                  >
                    <CalendarDays className="h-[18px] w-[18px]" strokeWidth={1.85} />
                  </NavbarActionButton>
                }
              />

              <NavDivider />

              {/* Profile Dropdown */}
              <ProfileDropdown
                isOpen={isProfileOpen}
                onOpenChange={setIsProfileOpen}
                displayName={profileDisplayName}
                onSignOut={() => {
                  setIsProfileOpen(false);
                  router.push('/login');
                }}
                trigger={
                  <button
                    type="button"
                    aria-label="Open profile menu"
                    className="relative flex h-8 w-8 shrink-0 items-center justify-center rounded-full focus:outline-none focus-visible:ring-2 focus-visible:ring-[#111111]/20"
                  >
                    {/* Gradient border ring */}
                    <span
                      aria-hidden="true"
                      className="pointer-events-none absolute inset-0 rounded-full"
                      style={{
                        background: 'linear-gradient(135deg, #F97316, #EC4899, #8B5CF6)',
                        padding: '2px',
                        WebkitMask: 'linear-gradient(#fff 0 0) content-box, linear-gradient(#fff 0 0)',
                        WebkitMaskComposite: 'xor',
                        maskComposite: 'exclude',
                      }}
                    />
                    <span className="flex h-[26px] w-[26px] items-center justify-center rounded-full bg-[linear-gradient(135deg,#F2DFC2,#E6B783)] text-[10px] font-bold text-[#4A341A]">
                      {profileInitial}
                    </span>
                  </button>
                }
              />
            </div>

          </div>
        </div>
      </header>

      {/* Modals */}
      <AIChatModal
        isOpen={isAIChatOpen}
        onOpenChange={setIsAIChatOpen}
        title="AI Assistant"
        description={`Help with ${pathname.split('/').pop() || 'this page'}`}
      />
    </>
  );
}
