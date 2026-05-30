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
import { AgentSidebarChevronIcon, getAgentSidebarContext } from '@/components/agents/sidebar';
import Link from 'next/link';

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
  const [isFilterInteractive, setIsFilterInteractive] = useState(false);

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
    () => getAgentSidebarContext(pathname)?.section ?? getActiveSidebarSection(pathname) ?? sharedNavItems[0],
    [pathname]
  );
  const agentSidebarContext = React.useMemo(
    () => getAgentSidebarContext(pathname),
    [pathname]
  );

  const activeNavItem = React.useMemo(
    () => {
      if (agentSidebarContext) {
        for (const group of agentSidebarContext.section.panelGroups) {
          const item = group.items.find((candidate) => {
            const normalizedPath = pathname.replace(/\/+$/, '') || '/';
            const hrefs = [candidate.href, ...(candidate.aliases ?? [])].filter(Boolean) as string[];

            return hrefs.some((href) => normalizedPath === href || normalizedPath.startsWith(`${href}/`));
          });

          if (item) {
            return item.id;
          }
        }

        return agentSidebarContext.section.panelGroups[0]?.items[0]?.id;
      }

      return getActiveSidebarItem(pathname)?.id;
    },
    [agentSidebarContext, pathname]
  );
  const isMessagesSection = activeSection.id === 'messages';
  const isAgentDetailSidebar = Boolean(agentSidebarContext);

  React.useEffect(() => {
    setSearchQuery('');
    setIsFilterInteractive(false);
  }, [activeSection.id]);

  const sectionLabel = isAgentDetailSidebar
    ? agentSidebarContext?.agentName ?? activeSection.defaultLabel
    : getNavLabel(activeSection.labelKey, activeSection.defaultLabel, language);

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

  const handleNavigationPrefetch = React.useCallback((href: string) => {
    router.prefetch(href);
  }, [router]);

  const showAsMinimized = isMobile ? false : isMinimized;
  const showExpandedContent = isMobile ? showMobileMenu : !isMinimized;
  const sidebarViewportTop = 'var(--dashboard-navbar-height, 56px)';
  const sidebarViewportHeight = 'var(--dashboard-content-height, calc(100dvh - var(--dashboard-navbar-height, 56px)))';
  const mobileMenuButtonTop = 'calc(var(--dashboard-navbar-height, 56px) + 16px)';

  return (
    <>
      {isMobile && showMobileMenu ? (
        <div
          className="fixed inset-0 z-[35] bg-black/30"
          onClick={() => setShowMobileMenu(false)}
          role="button"
          tabIndex={0}
          aria-label="Close menu"
          onKeyDown={(e) => { if (e.key === 'Enter' || e.key === ' ') setShowMobileMenu(false); }}
        />
      ) : null}

      <div
        className={cn(
          'fixed left-0 z-30 overflow-hidden bg-[var(--linear-sidebar-bg)] transition-all duration-300 ease-in-out',
          isMobile ? (showMobileMenu ? 'translate-x-0' : '-translate-x-full') : '',
          className,
        )}
        style={{
          top: sidebarViewportTop,
          height: sidebarViewportHeight,
          width: showAsMinimized ? `${SIDEBAR_MINIMIZED_WIDTH}px` : `${SIDEBAR_EXPANDED_WIDTH}px`,
        }}
      >
        <div className={cn('relative z-10 flex h-full', showAsMinimized ? '' : 'gap-0 px-0 py-0')}>
          {/* Icon rail */}
          <div className="flex h-full w-[60px] flex-col bg-transparent">
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
            <div
              aria-hidden="true"
              className="my-3 w-px shrink-0 self-stretch bg-gradient-to-b from-transparent via-[#D9DCE3]/80 to-transparent"
            />
          ) : null}

          {showExpandedContent ? (
            <div className="flex min-w-0 flex-1 flex-col bg-transparent px-5 pb-5 pt-4">
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

              {isAgentDetailSidebar && agentSidebarContext ? (
                <div className="mb-5 space-y-2">
                  <button
                    type="button"
                    className="flex w-full items-center gap-3 rounded-[12px] border border-[#E8E8E8] bg-white px-3 py-2.5 text-left transition-colors hover:bg-[#F8F8F8]"
                  >
                    <span className="grid size-8 shrink-0 place-items-center rounded-[9px] bg-[#111318] text-white">
                      {React.createElement(activeSection.icon, { className: 'h-3.5 w-3.5', strokeWidth: 1.9 })}
                    </span>
                    <span className="min-w-0 flex-1 truncate text-[14px] font-semibold text-[#1f2229]">
                      {agentSidebarContext.agentName}
                    </span>
                    <AgentSidebarChevronIcon className="h-3.5 w-3.5 shrink-0 text-[#9B9EA8]" strokeWidth={2} />
                  </button>

                  <Link
                    href={`/agents/${agentSidebarContext.agentId}`}
                    className="flex items-center gap-3 rounded-[12px] border border-[#E8E8E8] bg-white px-3 py-2.5 text-[#1f2229] transition-colors hover:bg-[#F8F8F8]"
                  >
                    <span className="grid size-8 shrink-0 place-items-center rounded-[9px] bg-[#111318] text-white">
                      <activeSection.icon className="h-3.5 w-3.5" strokeWidth={1.9} />
                    </span>
                    <span className="text-[14px] font-semibold">Get started</span>
                  </Link>

                  <div className="mt-3 h-px bg-[#ECECF1]" />
                </div>
              ) : (
                <div className="relative mb-4">
                  <SearchIcon className="absolute left-3 top-1/2 h-[15px] w-[15px] -translate-y-1/2 text-[#9B9EA8]" strokeWidth={1.8} />
                  <input
                    type="search"
                    name={`sidebar-filter-${activeSection.id}`}
                    placeholder={t('searchPlaceholder')}
                    value={searchQuery}
                    onChange={(event) => setSearchQuery(event.target.value)}
                    onPointerDown={() => setIsFilterInteractive(true)}
                    onFocus={() => setIsFilterInteractive(true)}
                    autoComplete="off"
                    autoCorrect="off"
                    autoCapitalize="none"
                    spellCheck={false}
                    readOnly={!isFilterInteractive}
                    data-lpignore="true"
                    data-1p-ignore="true"
                    data-bwignore="true"
                    data-form-type="other"
                    aria-autocomplete="none"
                    enterKeyHint="search"
                    className="h-9 w-full rounded-[10px] border border-[#E2E3E9] bg-white pl-9 pr-3 text-[14px] text-[#3A3C44] placeholder:text-[#B0B3BC] transition-all focus:outline-none focus:ring-2 focus:ring-[#DD7A1F]/20"
                  />
                </div>
              )}

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
                    onPrefetch={handleNavigationPrefetch}
                    searchQuery={deferredSearchQuery}
                  />
                )}
              </div>

              {isAgentDetailSidebar && agentSidebarContext ? (
                <div className="mt-3 border-t border-[#ECECF1] pt-3">
                  <Link
                    href={agentSidebarContext.footerCta.href}
                    className="flex items-center gap-3 rounded-[12px] border border-[#E8E8E8] bg-white px-3 py-2.5 transition-colors hover:bg-[#F8F8F8]"
                  >
                    <span className="grid size-8 shrink-0 place-items-center rounded-[9px] bg-[#111318] text-white">
                      <agentSidebarContext.footerCta.icon className="h-3.5 w-3.5" strokeWidth={1.9} />
                    </span>
                    <span className="flex-1 text-[14px] font-semibold text-[#1f2229]">
                      {agentSidebarContext.footerCta.label}
                    </span>
                    <ChevronLeft className="h-3.5 w-3.5 shrink-0 rotate-180 text-[#9B9EA8]" strokeWidth={2} />
                  </Link>
                </div>
              ) : null}
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
