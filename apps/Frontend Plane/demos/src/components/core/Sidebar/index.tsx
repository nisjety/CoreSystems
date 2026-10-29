'use client';

import React, { useState, useRef, useEffect } from 'react';
import { cn } from './utils';
import { SimpleTooltip } from './ui/simple-tooltip';
import UserProfile from './components/UserProfile';
import { Navigation } from './components/Navigation';
import { MessageTab } from './components/MessageTab';
import { NotificationPanel } from './components/NotificationPanel';
import { Calendar } from './components/Calendar';
import { ChatHistoryTab } from './components/ChatHistoryTab';
import { Settings } from './components/Settings';
import { SettingsPanel } from './components/SettingsPanel';
import { MinimizedLogo } from './components/MinimizedLogo';
import { MinimizedNavigation } from './components/MinimizedNavigation';
import { MinimizedFooter } from './components/MinimizedFooter';
import { SharedLogo } from '../shared/SharedLogo';
import { useSidebar } from '../shared/SidebarContext';
import { useCompatibleLanguage } from '@/components/shared/contexts/GlobalLanguageContext';
import { useAuth } from '@/components/auth/hooks/use-auth';
import { 
  useUser, 
  useMessages, 
  useCalendarEvents, 
  useNotifications, 
  useUnreadNotifications,
  useMarkNotificationAsRead,
  useClearAllNotifications,
  useMarkMessageAsRead
} from './hooks/useRealData';
import type { Message, CalendarEvent, Notification, NavigationItem, SidebarProps } from './types';
import { 
  MessageSquare as MessageSquareIcon, 
  Calendar as CalendarIcon,
  Search as SearchIcon,
  Bell as BellIcon,
  MessageCircle,
  X,
  Menu,
  Globe,
  ChevronDown,
  Check,
  FolderSearch,
  HelpCircle,
  User
} from 'lucide-react';

type ActivePanel = 'navigation' | 'messages' | 'notifications' | 'calendar' | 'chat-history' | 'settings';

type SessionLike = {
  id?: string;
  sessionId?: string;
  title?: string;
  createdAt?: string;
  updatedAt?: string;
};

// i18n translations for sidebar
const translations = {
  en: {
    messages: 'Messages',
    notifications: 'Notifications',
    calendar: 'Calendar',
    chatHistory: 'Chat History',
    settings: 'Settings',
    searchPlaceholder: 'Search everything...',
    expandSidebar: 'Expand sidebar',
    collapseSidebar: 'Collapse sidebar',
    showMenu: 'Show menu',
    hideMenu: 'Hide menu',
    smartWorkspace: 'Smart Workspace',
    switchLanguage: 'Switch language',
    language: 'Language',
    english: 'English',
    norwegian: 'Norwegian'
  },
  no: {
    messages: 'Meldinger',
    notifications: 'Varsler',
    calendar: 'Kalender', 
    chatHistory: 'Samtalehistorikk',
    settings: 'Innstillinger',
    searchPlaceholder: 'Søk i alt...',
    expandSidebar: 'Utvid sidepanel',
    collapseSidebar: 'Skjul sidepanel',
    showMenu: 'Vis meny',
    hideMenu: 'Skjul meny',
    smartWorkspace: 'Smart Arbeidsområde',
    switchLanguage: 'Bytt språk',
    language: 'Språk',
    english: 'English',
    norwegian: 'Norsk'
  }
};

export const Sidebar: React.FC<SidebarProps> = ({ 
  user: propUser, 
  onNavigate,
  className 
}) => {
  const [activePanel, setActivePanel] = useState<ActivePanel>('navigation');
  const [searchQuery, setSearchQuery] = useState('');
  const [activeNavItem, setActiveNavItem] = useState('dashboard');
  const [expandedFloatingMenu, setExpandedFloatingMenu] = useState<string | null>(null);
  const [showLanguageDropdown, setShowLanguageDropdown] = useState(false);

  // Ref for language dropdown click outside detection
  const languageDropdownRef = useRef<HTMLDivElement>(null);

  // Use shared sidebar context
  const { 
    isMinimized, 
    isMobile, 
    showMobileMenu, 
    setShowMobileMenu,
    toggleMinimize 
  } = useSidebar();

  // Use global language context
  const { sidebarLocale, global } = useCompatibleLanguage();
  const language = sidebarLocale;

  // Handle click outside language dropdown
  useEffect(() => {
    const handleClickOutside = (event: MouseEvent) => {
      if (languageDropdownRef.current && !languageDropdownRef.current.contains(event.target as Node)) {
        setShowLanguageDropdown(false);
      }
    };

    if (showLanguageDropdown) {
      document.addEventListener('mousedown', handleClickOutside);
      return () => document.removeEventListener('mousedown', handleClickOutside);
    }
  }, [showLanguageDropdown]);

  // Get auth state to conditionally enable API calls
  const { user: authUser } = useAuth();
  const isAuthenticated = !!authUser;

  // Real data hooks - DISABLED until backend endpoints are implemented
  const { data: userData } = useUser({ enabled: false });
  const { data: messagesData, error: messagesError } = useMessages({ enabled: false });
  const { data: eventsData } = useCalendarEvents({ enabled: false });
  const { data: notificationsData, error: notificationsError } = useNotifications({ enabled: false });
  const { data: unreadNotificationsData, error: unreadError } = useUnreadNotifications({ enabled: false });

  // Real mutation hooks for interactions
  const markNotificationAsRead = useMarkNotificationAsRead();
  const clearAllNotifications = useClearAllNotifications();
  const markMessageAsRead = useMarkMessageAsRead();

  // Translation helper
  const t = (key: string) => {
    return translations[language][key as keyof typeof translations[typeof language]] || key;
  };

  // Use real data or fallback - no mock data
  const user = userData || propUser || {
    id: '1',
    name: 'Loading...',
    email: '',
    position: 'Loading...',
    department: '',
    status: 'offline' as const
  };
  const messages: Message[] = React.useMemo(() => {
    if (messagesError) return [];
    const data: unknown = messagesData ?? [];
    if (Array.isArray(data)) return data as Message[];
    if (data && typeof data === 'object') {
      const withMessages = data as { messages?: unknown };
      if (Array.isArray(withMessages.messages)) return withMessages.messages as Message[];

      const withSessions = data as { sessions?: unknown };
      if (Array.isArray(withSessions.sessions)) {
        return (withSessions.sessions as SessionLike[]).map((s, index): Message => ({
          id: String(s.id ?? s.sessionId ?? `session-${index}`),
          from: { name: 'Chat', email: '' },
          content: s.title ?? 'Chat session',
          timestamp: new Date(s.updatedAt ?? s.createdAt ?? new Date()),
          read: true,
          priority: 'low',
          type: 'chat',
        }));
      }
    }
    return [];
  }, [messagesData, messagesError]);
  const events = eventsData || [];
  const notifications = notificationsError ? [] : (notificationsData || []);
  const unreadNotifications = unreadError ? 0 : (unreadNotificationsData?.count || 0);

  const handleNavigationClick = (item: NavigationItem) => {
    setActiveNavItem(item.id);
    
    // Handle floating menu for minimized sidebar
    if (isMinimized && item.children && item.children.length > 0) {
      setExpandedFloatingMenu(expandedFloatingMenu === item.id ? null : item.id);
      return;
    }
    
    // Close any open panels when navigating
    setActivePanel('navigation');
    
    onNavigate?.(item);
    
    // Auto-close mobile menu - Following mobile UX patterns
    if (isMobile) {
      setShowMobileMenu(false);
    }
    
    // Close floating menu when navigating
    setExpandedFloatingMenu(null);
  };

  const handleSettingClick = (setting: string) => {
    console.log('Setting clicked:', setting);
    if (setting === 'logout') {
      console.log('Logout clicked');
    } else {
      setActivePanel('settings');
    }
  };

  const handleChatSelect = (chat: {
    id: string;
    title: string;
    lastMessage: string;
    timestamp: Date;
    unreadCount: number;
    isPinned: boolean;
    participants: string[];
  }) => {
    console.log('Chat selected:', chat);
  };

  const handleNewChat = () => {
    console.log('New chat clicked');
  };

  const handlePanelToggle = (panel: ActivePanel) => {
    // If clicking the same panel, close it
    if (activePanel === panel) {
      setActivePanel('navigation');
    } else {
      setActivePanel(panel);
    }
    
    // On mobile, show the mobile menu when switching to navigation
    if (isMobile && panel === 'navigation') {
      setShowMobileMenu(true);
    }
  };

  const handleMessageClick = (message: Message) => {
    console.log('Message clicked:', message);
  };

  const handleNotificationClick = (notification: Notification) => {
    console.log('Notification clicked:', notification);
  };

  const handleEventClick = (event: CalendarEvent) => {
    console.log('Event clicked:', event);
  };

  const handleMarkAsRead = (id: string) => {
    markNotificationAsRead.mutate(id);
  };

  const handleMarkMessageAsRead = (messageId: string) => {
    markMessageAsRead.mutate(messageId);
  };

  const handleClearAll = () => {
    clearAllNotifications.mutate();
  };

  // Handle title clicks to navigate to pages
  const handleTitleClick = (href: string, label: string, icon: React.ComponentType<{ className?: string }>) => {
    console.log(`Navigating to ${href} from title click`);
    onNavigate?.({ 
      id: href.replace('/', ''), 
      label: label, 
      icon: icon, 
      href: href 
    });
    // Close the panel after navigation
    setActivePanel('navigation');
  };

  const unreadMessages = Array.isArray(messages)
    ? messages.filter((m) => !m.read).length
    : 0;

  // Render floating panels for minimized mode
  const renderFloatingPanel = () => {
    // Only show floating panels when minimized and a panel is active (not navigation)
    if (!isMinimized || activePanel === 'navigation') return null;

    const panelContent = () => {
      switch (activePanel) {
        case 'messages':
          return (
            <MessageTab
              messages={messages}
              onMessageClick={handleMessageClick}
              onMarkAsRead={handleMarkMessageAsRead}
              onTitleClick={() => handleTitleClick('/messages', t('messages'), MessageSquareIcon)}
            />
          );
        case 'chat-history':
          return (
            <ChatHistoryTab
              onChatSelect={handleChatSelect}
              onNewChat={handleNewChat}
            />
          );
        case 'notifications':
          return (
            <NotificationPanel
              notifications={notifications}
              onNotificationClick={handleNotificationClick}
              onMarkAsRead={handleMarkAsRead}
              onClearAll={handleClearAll}
              onTitleClick={() => handleTitleClick('/notifications', t('notifications'), BellIcon)}
            />
          );
        case 'calendar':
          return (
            <Calendar
              events={events}
              onDateSelect={(date) => console.log('Date selected:', date)}
              onEventClick={handleEventClick}
              onTitleClick={() => handleTitleClick('/calendar', t('calendar'), CalendarIcon)}
            />
          );
        default:
          return null;
      }
    };

    return (
      <>
        {/* Backdrop */}
        <div 
          className="fixed inset-0 z-40" 
          onClick={() => setActivePanel('navigation')}
        />
        
        {/* Floating Panel */}
        <div 
          className="fixed z-50 bg-white rounded-2xl shadow-[0_8px_32px_rgba(0,0,0,0.12)] border border-gray-100 overflow-hidden"
          style={{ 
            left: '85px',
            top: '100px',
            width: '260px',
            height: '420px',
            maxWidth: 'calc(100vw - 100px)', // Prevent overflow on small screens
          }}
        >
          {panelContent()}
        </div>
      </>
    );
  };

  return (
    <div className="relative">
      <div className={cn(
        'flex flex-col shadow-[0_2px_20px_rgba(0,0,0,0.08)] fixed left-0 top-0 z-50 transition-all duration-300 ease-in-out overflow-x-hidden overflow-y-hidden',
        // When minimized, use white background like Sidebar.tsx, otherwise use Aquatiq blue gradient
        isMinimized ? 'bg-white border-r border-gray-200' : 'bg-gradient-to-br from-[#151F6D] to-[#0d1444]',
        isMinimized ? 'w-20' : 'w-[260px]',
        'h-screen',
        className
      )}>
        {/* Mobile Overlay - Following mobile UX patterns */}
        {isMobile && showMobileMenu && !isMinimized && (
          <div 
            className="fixed inset-0 bg-black bg-opacity-50 z-40"
            onClick={() => setShowMobileMenu(false)}
          />
        )}

        {/* Header - Following Law of Common Region */}
        <div className={cn(
          'transition-all duration-300 relative z-50',
          isMinimized ? 'p-4 border-b border-gray-200' : 'p-4 border-b border-white/10',
          isMinimized ? 'py-6' : ''
        )}>
          <div className={cn(
            'flex items-center gap-3',
            isMinimized ? 'mb-0 justify-center' : 'mb-6'
          )}>
            {/* Logo with Toggle */}
            {isMinimized ? (
              <MinimizedLogo />
            ) : (
              <SharedLogo 
                variant="sidebar"
                showTitle={!isMinimized}
                showToggle={true}
              />
            )}
            
            {!isMinimized && (
              <div className="flex-1">
                {/* Title is handled by SharedLogo now */}
              </div>
            )}

            {/* Mobile Menu Toggle */}
            {isMobile && !isMinimized && (
              <button
                onClick={() => setShowMobileMenu(!showMobileMenu)}
                className="p-2 rounded-xl text-white/60 hover:bg-white/10 transition-colors"
                title={showMobileMenu ? t('hideMenu') : t('showMenu')}
              >
                {showMobileMenu ? <X className="w-4 h-4" /> : <Menu className="w-4 h-4" />}
              </button>
            )}
          </div>

          {/* Search - Following Aesthetic-Usability Effect */}
          {!isMinimized && (!isMobile || showMobileMenu) && (
            <div className="relative mb-4">
              <SearchIcon className="absolute left-3 top-1/2 transform -translate-y-1/2 w-4 h-4 text-white/60" />
              <input
                type="text"
                placeholder={t('searchPlaceholder')}
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                className="w-full pl-10 pr-4 py-3 text-sm bg-white/10 rounded-xl focus:bg-white/20 focus:outline-none focus:ring-2 focus:ring-white/30 transition-all text-white placeholder-white/60 border border-white/10"
              />
            </div>
          )}

          {/* Language Dropdown */}
          {!isMinimized && (!isMobile || showMobileMenu) && (
            <div ref={languageDropdownRef} className="relative mb-4">
              <button
                onClick={() => setShowLanguageDropdown(!showLanguageDropdown)}
                className="w-full flex items-center justify-between gap-3 p-3 text-sm bg-white/10 rounded-xl hover:bg-white/20 focus:bg-white/20 focus:outline-none focus:ring-2 focus:ring-white/30 transition-all text-white border border-white/10"
              >
                <div className="flex items-center gap-2">
                  <Globe className="w-4 h-4 text-white/60" />
                  <span>{t('language')}</span>
                </div>
                <div className="flex items-center gap-2">
                  <span className="text-xs font-medium text-white/80">
                    {sidebarLocale === 'en' ? t('english') : t('norwegian')}
                  </span>
                  <ChevronDown className={`w-4 h-4 text-white/60 transition-transform duration-200 ${showLanguageDropdown ? 'rotate-180' : ''}`} />
                </div>
              </button>

              {/* Dropdown Menu */}
              {showLanguageDropdown && (
                <div className="absolute top-full left-0 right-0 mt-1 bg-[#1a2359] rounded-xl shadow-lg border border-white/20 py-2 z-50">
                  <button
                    onClick={() => {
                      if (sidebarLocale !== 'en') {
                        global.toggleLanguage();
                      }
                      setShowLanguageDropdown(false);
                    }}
                    className="w-full flex items-center justify-between px-3 py-2 text-sm hover:bg-white/10 transition-colors text-white"
                  >
                    <div className="flex items-center gap-2">
                      <span className="text-xs">🇺🇸</span>
                      <span className="text-white">{t('english')}</span>
                    </div>
                    {sidebarLocale === 'en' && <Check className="w-4 h-4 text-white" />}
                  </button>
                  <button
                    onClick={() => {
                      if (sidebarLocale !== 'no') {
                        global.toggleLanguage();
                      }
                      setShowLanguageDropdown(false);
                    }}
                    className="w-full flex items-center justify-between px-3 py-2 text-sm hover:bg-white/10 transition-colors text-white"
                  >
                    <div className="flex items-center gap-2">
                      <span className="text-xs">🇳🇴</span>
                      <span className="text-white">{t('norwegian')}</span>
                    </div>
                    {sidebarLocale === 'no' && <Check className="w-4 h-4 text-white" />}
                  </button>
                </div>
              )}
            </div>
          )}

          {/* Close Button for non-navigation panels */}
          {!isMinimized && (!isMobile || showMobileMenu) && activePanel !== 'navigation' && (
            <div className="mb-4 flex justify-end">
              <button
                onClick={() => setActivePanel('navigation')}
                className="w-8 h-8 rounded-lg bg-gray-100 hover:bg-gray-200 flex items-center justify-center transition-colors"
                title="Close panel"
                aria-label="Close panel"
              >
                <X className="w-4 h-4 text-gray-600" />
              </button>
            </div>
          )}

          {/* Quick Actions - Following Chunking principle */}
          {!isMinimized && (
            <div className="flex gap-2">
              <button
                onClick={() => handlePanelToggle('messages')}
                className={cn(
                  'flex items-center justify-center gap-2 py-3 px-4 rounded-xl transition-all duration-200 relative',
                  'min-h-[44px] flex-1',
                  activePanel === 'messages'
                    ? 'bg-white/20 text-white border border-white/30'
                    : 'bg-white/10 hover:bg-white/20 text-white/80 hover:text-white'
                )}
              >
                <MessageSquareIcon className="w-4 h-4" />
                <span className="text-sm font-medium">{t('messages')}</span>
                {unreadMessages > 0 && (
                  <div className="absolute -top-1 -right-1 w-5 h-5 bg-red-500 text-white text-xs rounded-full flex items-center justify-center font-medium">
                    {unreadMessages > 99 ? '99+' : unreadMessages}
                  </div>
                )}
              </button>

              <button
                onClick={() => handlePanelToggle('chat-history')}
                className={cn(
                  'w-12 h-12 rounded-xl flex items-center justify-center transition-all duration-200 min-h-[44px]',
                  activePanel === 'chat-history'
                    ? 'bg-white/20 text-white border border-white/30'
                    : 'bg-white/10 hover:bg-white/20 text-white/80 hover:text-white'
                )}
                title={t('chatHistory')}
              >
                <MessageCircle className="w-4 h-4" />
              </button>

              <button
                onClick={() => handlePanelToggle('notifications')}
                className={cn(
                  'w-12 h-12 rounded-xl flex items-center justify-center transition-all duration-200 relative min-h-[44px]',
                  activePanel === 'notifications'
                    ? 'bg-white/20 text-white border border-white/30'
                    : 'bg-white/10 hover:bg-white/20 text-white/80 hover:text-white'
                )}
                title={t('notifications')}
              >
                <BellIcon className="w-4 h-4" />
                {unreadNotifications > 0 && (
                  <div className="absolute -top-1 -right-1 w-5 h-5 bg-red-500 text-white text-xs rounded-full flex items-center justify-center font-medium">
                    {unreadNotifications > 99 ? '99+' : unreadNotifications}
                  </div>
                )}
              </button>

              <button
                onClick={() => handlePanelToggle('calendar')}
                className={cn(
                  'w-12 h-12 rounded-xl flex items-center justify-center transition-all duration-200 min-h-[44px]',
                  activePanel === 'calendar'
                    ? 'bg-white/20 text-white border border-white/30'
                    : 'bg-white/10 hover:bg-white/20 text-white/80 hover:text-white'
                )}
                title={t('calendar')}
              >
                <CalendarIcon className="w-4 h-4" />
              </button>
            </div>
          )}

          {/* Quick Actions for Minimized State - Simple styling like Sidebar.tsx */}
          {isMinimized && (
            <div className="flex flex-col items-center gap-4">
              <SimpleTooltip content={t('searchPlaceholder')} placement="right" delay={200}>
                <button
                  onClick={() => {
                    toggleMinimize();
                    // Focus search input after a brief delay to allow for animation
                    setTimeout(() => {
                      const searchInput = document.querySelector('input[type="text"]') as HTMLInputElement;
                      if (searchInput) searchInput.focus();
                    }, 300);
                  }}
                  className={cn(
                    'w-12 h-12 flex items-center justify-center rounded-lg transition-all',
                    'text-gray-400 hover:bg-gray-100 hover:text-aquatiq-blue'
                  )}
                >
                  <SearchIcon className="w-6 h-6" />
                </button>
              </SimpleTooltip>

              {/* Language Toggle for Minimized State */}
              <SimpleTooltip content={t('switchLanguage')} placement="right" delay={200}>
                <button
                  onClick={() => {
                    global.toggleLanguage();
                  }}
                  className={cn(
                    'w-12 h-12 flex items-center justify-center rounded-lg transition-all',
                    'text-gray-400 hover:bg-gray-100 hover:text-aquatiq-blue'
                  )}
                >
                  <Globe className="w-6 h-6" />
                </button>
              </SimpleTooltip>
            </div>
          )}
        </div>

        {/* Content Area - Following Miller's Law for information hierarchy */}
        {(!isMinimized && (!isMobile || showMobileMenu)) && (
          <div className="flex-1 overflow-x-hidden overflow-y-hidden">
            {activePanel === 'navigation' && (
              <div className="p-3 h-full overflow-y-auto overflow-x-hidden relative">
                <Navigation 
                  activeItem={activeNavItem}
                  onNavigate={handleNavigationClick}
                  isCollapsed={false}
                />
              </div>
            )}

            {activePanel === 'messages' && (
              <MessageTab
                messages={messages}
                onMessageClick={handleMessageClick}
                onMarkAsRead={handleMarkMessageAsRead}
                onTitleClick={() => handleTitleClick('/messages', t('messages'), MessageSquareIcon)}
              />
            )}

            {activePanel === 'chat-history' && (
              <ChatHistoryTab
                onChatSelect={handleChatSelect}
                onNewChat={handleNewChat}
              />
            )}

            {activePanel === 'notifications' && (
              <NotificationPanel
                notifications={notifications}
                onNotificationClick={handleNotificationClick}
                onMarkAsRead={handleMarkAsRead}
                onClearAll={handleClearAll}
                onTitleClick={() => handleTitleClick('/notifications', t('notifications'), BellIcon)}
              />
            )}

            {activePanel === 'calendar' && (
              <Calendar
                events={events}
                onDateSelect={(date) => console.log('Date selected:', date)}
                onEventClick={handleEventClick}
                onTitleClick={() => handleTitleClick('/calendar', t('calendar'), CalendarIcon)}
              />
            )}

            {activePanel === 'settings' && (
              <SettingsPanel
                onClose={() => setActivePanel('navigation')}
              />
            )}
          </div>
        )}

        {/* Minimized Navigation */}
        {isMinimized && !isMobile && (
          <div className="flex-1 flex flex-col items-center gap-8 overflow-x-hidden overflow-y-hidden py-8">
            <MinimizedNavigation />
          </div>
        )}

        {/* Settings Section - Following Law of Proximity */}
        {(!isMinimized && (!isMobile || showMobileMenu)) && (
          <div className="border-t border-white/10">
            <Settings 
              onSettingClick={handleSettingClick}
              isCollapsed={false}
            />
          </div>
        )}

        {/* User Profile Footer - Following Peak-End Rule */}
        <div className={cn(
          isMinimized ? 'border-t-0 p-4' : 'border-t border-white/10 p-4'
        )}>
          {isMinimized ? (
            <MinimizedFooter 
              onProfileClick={() => {
                onNavigate?.({ 
                  id: 'profile', 
                  label: 'Profile', 
                  icon: () => <div className="w-4 h-4" />,
                  href: '/profile' 
                });
              }}
              onHelpClick={() => {
                console.log('Help clicked');
              }}
              onSettingsClick={() => handlePanelToggle('settings')}
              isSettingsActive={activePanel === 'settings'}
            />
          ) : (
            <UserProfile 
              user={user}
              onClick={() => {
                onNavigate?.({ 
                  id: 'profile', 
                  label: 'Profile', 
                  icon: () => <div className="w-4 h-4" />, // Simple placeholder icon
                  href: '/profile' 
                });
              }}
              isMinimized={isMinimized}
            />
          )}
        </div>
        
        {/* Floating Panel for Collapsed Mode */}
        {renderFloatingPanel()}
      </div>
    </div>
  );
};