'use client';

import React, { useState } from 'react';
import { createPortal } from 'react-dom';
import { usePathname, useRouter } from 'next/navigation';
import { ChevronLeft, X, Menu, Search as SearchIcon } from 'lucide-react';
import { cn } from './utils';
import { SIDEBAR_EXPANDED_WIDTH, SIDEBAR_MINIMIZED_WIDTH } from './constants';
import { Navigation } from './components/Navigation';
import { MessagesSidebarPanel } from './components/MessagesSidebarPanel';
import { UserMenuPopover } from './components/UserMenuPopover';
import { MinimizedLogo } from './components/MinimizedLogo';
import { MinimizedNavigation } from './components/MinimizedNavigation';
import { MinimizedFooter } from './components/MinimizedFooter';
import { useAuth } from '@/components/auth/hooks/use-auth';
import { useSidebar } from '../shared/SidebarContext';
import { useCompatibleLanguage } from '../contexts/GlobalLanguageContext';
import {
  getActiveSidebarItem,
  getActiveSidebarSection,
  getNavLabel,
  sharedNavItems,
  type SharedNavPanelItem,
} from './config/nav-items';
import { useUser } from './hooks/useRealData';
import type { NavigationItem, SidebarProps } from './types';

const translations = {
  en: {
    searchPlaceholder: 'Filter this section',
    showMenu: 'Show menu',
    hideMenu: 'Hide menu',
  },
  no: {
    searchPlaceholder: 'Filtrer denne seksjonen',
    showMenu: 'Vis meny',
    hideMenu: 'Skjul meny',
  },
};

export const Sidebar: React.FC<SidebarProps> = ({
  user: propUser,
  onNavigate,
  className,
}) => {
  const [searchQuery, setSearchQuery] = useState('');

  const {
    isMinimized,
    isMobile,
    showMobileMenu,
    setIsMinimized,
    setShowMobileMenu,
  } = useSidebar();
  const router = useRouter();
  const pathname = usePathname();
  const { user: authUser } = useAuth();
  const { sidebarLocale } = useCompatibleLanguage();
  const language = sidebarLocale;
  const deferredSearchQuery = React.useDeferredValue(searchQuery);
  const { data: userData } = useUser({ enabled: !!authUser });
  const avatarButtonRef = React.useRef<HTMLButtonElement>(null);
  const [isUserMenuOpen, setIsUserMenuOpen] = React.useState(false);
  const { signOut } = useAuth();

  const t = React.useCallback((key: keyof typeof translations.en) => {
    const currentTranslations = translations[language as keyof typeof translations] ?? translations.en;
    return currentTranslations[key] || key;
  }, [language]);

  const authenticatedUser = authUser
    ? {
        id: authUser.id,
        name: authUser.name,
        email: authUser.email,
        position: '',
        department: '',
        status: 'online' as const,
      }
    : null;

  const user = userData || propUser || authenticatedUser || {
    id: '1',
    name: 'Loading...',
    email: '',
    position: 'Loading...',
    department: '',
    status: 'offline' as const,
  };

  const activeSection = React.useMemo(
    () => getActiveSidebarSection(pathname) ?? sharedNavItems[0],
    [pathname]
  );

  const activeNavItem = React.useMemo(
    () => getActiveSidebarItem(pathname)?.id,
    [pathname]
  );
  const isMessagesSection = activeSection.id === 'messages';

  React.useEffect(() => {
    setSearchQuery('');
  }, [activeSection.id]);

  const sectionLabel = getNavLabel(activeSection.labelKey, activeSection.defaultLabel, language);

  const handleNavigationClick = React.useCallback((item: SharedNavPanelItem) => {
    if (!item.href) {
      return;
    }

    onNavigate?.({
      id: item.id,
      label: getNavLabel(item.labelKey, item.defaultLabel, language),
      icon: item.icon,
      href: item.href,
      description: item.description,
      status: item.status,
      aliases: item.aliases,
    } as NavigationItem);

    if (pathname !== item.href) {
      router.push(item.href);
    }

    if (isMobile) {
      setShowMobileMenu(false);
    }
  }, [isMobile, language, onNavigate, pathname, router, setShowMobileMenu]);

  const showAsMinimized = isMobile ? false : isMinimized;
  const showExpandedContent = isMobile ? showMobileMenu : !isMinimized;
  const sidebarViewportTop = 'var(--dashboard-navbar-height, 56px)';
  const sidebarViewportHeight = 'var(--dashboard-content-height, calc(100dvh - var(--dashboard-navbar-height, 56px)))';
  const mobileMenuButtonTop = 'calc(var(--dashboard-navbar-height, 56px) + 16px)';

  return (
    <>
      {isMobile && showMobileMenu ? (
        <div
          className="fixed inset-0 z-30 bg-black/30"
          onClick={() => setShowMobileMenu(false)}
        />
      ) : null}

      <div
        className={cn(
          'fixed left-0 z-40 overflow-hidden border-r border-[#E9EBF2] bg-white transition-all duration-300 ease-in-out',
          isMobile ? (showMobileMenu ? 'translate-x-0' : '-translate-x-full') : '',
          showAsMinimized ? 'w-[60px]' : 'w-[280px]',
          className,
        )}
        style={{
          top: sidebarViewportTop,
          height: sidebarViewportHeight,
        }}
      >
        <div className={cn('relative z-10 flex h-full', showAsMinimized ? '' : 'gap-0 px-0 py-0')}>
          {/* Icon rail */}
          <div className="flex h-full w-[60px] flex-col border-r border-[#E9EBF2] bg-[#F6F5F3]">
            <div className="flex items-center justify-center px-2 pb-1 pt-3">
              <MinimizedLogo />
            </div>

            <div className="flex-1 overflow-hidden py-3">
              <MinimizedNavigation />
            </div>

            <div className="flex flex-col items-center gap-3 px-2 pb-5">
              <MinimizedFooter
                userName={user.name}
                avatarButtonRef={avatarButtonRef}
                onProfileClick={() => setIsUserMenuOpen(v => !v)}
              />
            </div>
          </div>

          {showExpandedContent ? (
            <div className="flex min-w-0 flex-1 flex-col bg-white px-5 pb-5 pt-4">
              <div className="mb-4 flex items-center justify-between gap-3">
                <div className="text-[17px] font-semibold tracking-[-0.025em] text-[#1C1C1E]">{sectionLabel}</div>

                {isMobile && showMobileMenu ? (
                  <button
                    onClick={() => setShowMobileMenu(false)}
                    className="flex h-8 w-8 items-center justify-center rounded-[10px] text-[#7A7D87] transition-colors hover:bg-[#F4F5F8] hover:text-[#2E3038]"
                    title={t('hideMenu')}
                    aria-label={t('hideMenu')}
                  >
                    <X className="h-4 w-4" />
                  </button>
                ) : (
                  <button
                    type="button"
                    onClick={() => setIsMinimized(true)}
                    className="hidden h-8 w-8 items-center justify-center rounded-[10px] bg-[#F4F5F1] text-[#8A8D96] transition-colors hover:bg-[#EBECE7] hover:text-[#383B43] md:flex"
                    title="Collapse sidebar"
                    aria-label="Collapse sidebar"
                  >
                    <ChevronLeft className="h-4 w-4" strokeWidth={1.9} />
                  </button>
                )}
              </div>

              <div className="relative mb-4">
                <SearchIcon className="absolute left-3 top-1/2 h-[15px] w-[15px] -translate-y-1/2 text-[#9B9EA8]" strokeWidth={1.8} />
                <input
                  type="text"
                  placeholder={t('searchPlaceholder')}
                  value={searchQuery}
                  onChange={(event) => setSearchQuery(event.target.value)}
                  className="h-9 w-full rounded-[10px] border border-[#E2E3E9] bg-white pl-9 pr-3 text-[14px] text-[#3A3C44] placeholder:text-[#B0B3BC] transition-all focus:outline-none focus:ring-2 focus:ring-[#DD7A1F]/20"
                />
              </div>

              <div className="min-h-0 flex-1 overflow-y-auto">
                {isMessagesSection ? (
                  <MessagesSidebarPanel
                    searchQuery={deferredSearchQuery}
                    onNavigate={() => {
                      if (isMobile) {
                        setShowMobileMenu(false);
                      }
                    }}
                  />
                ) : (
                  <Navigation
                    section={activeSection}
                    activeItem={activeNavItem}
                    onNavigate={handleNavigationClick}
                    searchQuery={deferredSearchQuery}
                  />
                )}
              </div>
            </div>
          ) : null}
        </div>
      </div>

      {isMobile && !showMobileMenu && typeof window !== 'undefined' ? createPortal(
        <button
          onClick={() => setShowMobileMenu(true)}
          className="fixed left-4 z-30 rounded-xl border border-black/10 bg-white/90 p-2.5 text-black/70 shadow-md backdrop-blur-sm transition-all hover:bg-white hover:text-black"
          style={{ top: mobileMenuButtonTop }}
          aria-label={t('showMenu')}
        >
          <Menu className="h-5 w-5" />
        </button>,
        document.body,
      ) : null}

      <UserMenuPopover
        anchorRef={avatarButtonRef}
        isOpen={isUserMenuOpen}
        onClose={() => setIsUserMenuOpen(false)}
        userName={user.name}
        userEmail={user.email ?? ''}
        onLogout={() => {
          setIsUserMenuOpen(false);
          signOut();
        }}
      />
    </>
  );
};
