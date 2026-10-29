'use client';

import React, { useState } from 'react';
import { cn } from '../utils';
import { useCompatibleLanguage } from '@/components/shared/contexts/GlobalLanguageContext';
import { 
  Settings as SettingsIcon, 
  User, 
  Bell, 
  Shield, 
  Palette, 
  HelpCircle,
  LogOut,
  ChevronUp,
  ChevronDown,
  Monitor,
  Smartphone,
  Mail
} from 'lucide-react';

interface SettingsProps {
  onSettingClick: (setting: string) => void;
  className?: string;
  isCollapsed?: boolean;
}

// i18n translations
const translations = {
  en: {
    settings: 'Settings',
    profile: 'Profile Settings',
    notifications: 'Notifications', 
    privacy: 'Privacy & Security',
    appearance: 'Appearance',
    language: 'Language & Region',
    help: 'Help & Support',
    signOut: 'Sign Out',
    collapseMenu: 'Collapse menu',
    expandMenu: 'Expand menu',
    english: 'English',
    norwegian: 'Norsk'
  },
  no: {
    settings: 'Innstillinger',
    profile: 'Profilinnstillinger',
    notifications: 'Varsler',
    privacy: 'Personvern og sikkerhet', 
    appearance: 'Utseende',
    language: 'Språk og region',
    help: 'Hjelp og støtte',
    signOut: 'Logg ut',
    collapseMenu: 'Skjul meny',
    expandMenu: 'Vis meny',
    english: 'English',
    norwegian: 'Norsk'
  }
};

// Settings items following Miller's Law - grouped logically with no more than 7 items
const getSettingsItems = (t: (key: string) => string) => [
  { 
    id: 'profile', 
    label: t('profile'), 
    icon: User,
    description: 'Personal information and preferences'
  },
  { 
    id: 'notifications', 
    label: t('notifications'), 
    icon: Bell,
    description: 'Alert settings and preferences'
  },
  { 
    id: 'privacy', 
    label: t('privacy'), 
    icon: Shield,
    description: 'Security and data protection'
  },
  { 
    id: 'appearance', 
    label: t('appearance'), 
    icon: Palette,
    description: 'Theme and display options'
  },
  { 
    id: 'help', 
    label: t('help'), 
    icon: HelpCircle,
    description: 'Documentation and support'
  }
];

export function Settings({ 
  onSettingClick, 
  className, 
  isCollapsed = false
}: SettingsProps) {
  const [isExpanded, setIsExpanded] = useState(false);
  
  // Use global language context
  const { sidebarLocale } = useCompatibleLanguage();
  const language = sidebarLocale;

  const t = (key: string) => {
    return translations[language][key as keyof typeof translations[typeof language]] || key;
  };

  const settingsItems = getSettingsItems(t);

  const handleSettingsClick = () => {
    if (isCollapsed) {
      // In collapsed mode, directly open settings panel
      onSettingClick('settings');
    } else {
      // In expanded mode, toggle the dropdown
      setIsExpanded(!isExpanded);
    }
  };

  return (
    <div className={cn('border-t border-white/10', className)}>
      {/* Settings Section */}
      <div className={cn(
        'w-full flex items-center gap-3 p-4 hover:bg-white transition-colors group',
        isCollapsed && 'justify-center px-3'
      )}>
        {/* Settings Button */}
        <button
          onClick={handleSettingsClick}
          className="flex items-center gap-3 flex-1"
          title={isCollapsed ? t('settings') : (isExpanded ? t('collapseMenu') : t('expandMenu'))}
          aria-expanded={isExpanded}
          aria-label={t('settings')}
        >
          <div className={cn(
            'rounded-lg bg-white/10 flex items-center justify-center group-hover:bg-white/30 transition-colors',
            isCollapsed ? 'w-10 h-10' : 'w-8 h-8'
          )}>
            <SettingsIcon className="w-4 h-4 text-white group-hover:text-[#151F6C]" />
          </div>
          
          {!isCollapsed && (
            <>
              <span className="flex-1 text-sm font-medium text-white group-hover:text-[#151F6C] text-left transition-colors">
                {t('settings')}
              </span>
              <span className="text-white/60 group-hover:text-[#151F6C]/70 transition-all duration-200">
                {isExpanded ? (
                  <ChevronDown className="w-4 h-4" />
                ) : (
                  <ChevronUp className="w-4 h-4" />
                )}
              </span>
            </>
          )}
        </button>
      </div>

      {/* Settings Menu - Following Chunking principle */}
      {isExpanded && !isCollapsed && (
        <div className="px-2 pb-4 space-y-1 animate-in fade-in slide-in-from-top-2 duration-200">
          {settingsItems.map((item) => (
            <button
              key={item.id}
              onClick={() => {
                onSettingClick(item.id);
                setIsExpanded(false);
              }}
              className="w-full flex items-center gap-3 px-3 py-3 rounded-xl text-left transition-all duration-200 text-white hover:bg-white hover:text-[#151F6C] group"
              title={item.description}
            >
              <span className="flex-shrink-0 w-8 h-8 rounded-lg bg-white/10 flex items-center justify-center group-hover:bg-white/30 transition-colors">
                <item.icon className="w-4 h-4 text-white group-hover:text-[#151F6C]" />
              </span>
              <div className="flex-1 min-w-0">
                <div className="text-sm font-medium">{item.label}</div>
                {item.description && (
                  <div className="text-xs text-white/60 group-hover:text-[#151F6C]/70 truncate">
                    {item.description}
                  </div>
                )}
              </div>
            </button>
          ))}
          
          {/* Separator following Law of Common Region */}
          <div className="my-2 border-t border-white/10" />
          
          {/* Sign out button with distinctive styling - Von Restorff Effect */}
          <button
            onClick={() => {
              onSettingClick('logout');
              setIsExpanded(false);
            }}
            className="w-full flex items-center gap-3 px-3 py-3 rounded-xl text-left transition-all duration-200 text-red-300 hover:bg-red-50 hover:text-red-700 group"
            title="Sign out of your account"
          >
            <span className="flex-shrink-0 w-8 h-8 rounded-lg bg-red-500/20 flex items-center justify-center group-hover:bg-red-200 transition-colors">
              <LogOut className="w-4 h-4 text-red-300 group-hover:text-red-600" />
            </span>
            <div className="flex-1">
              <div className="text-sm font-medium">{t('signOut')}</div>
              <div className="text-xs text-red-200 group-hover:text-red-500">
                End your current session
              </div>
            </div>
          </button>
        </div>
      )}
    </div>
  );
}

// Additional Quick Settings Component for common actions
export function QuickSettings({
  onThemeChange,
  onNotificationToggle,
  currentTheme = 'light',
  notificationsEnabled = true,
  isCollapsed = false
}: {
  onThemeChange?: (theme: 'light' | 'dark' | 'auto') => void;
  onNotificationToggle?: () => void;
  currentTheme?: 'light' | 'dark' | 'auto';
  notificationsEnabled?: boolean;
  isCollapsed?: boolean;
}) {
  const themes = [
    { id: 'light', icon: Monitor, label: 'Light' },
    { id: 'dark', icon: Monitor, label: 'Dark' },
    { id: 'auto', icon: Smartphone, label: 'Auto' }
  ];

  return (
    <div className={cn(
      'border-t border-white/10 p-3',
      isCollapsed && 'px-2'
    )}>
      <div className={cn(
        'flex gap-2',
        isCollapsed ? 'flex-col' : 'justify-between items-center'
      )}>
        {/* Theme Toggle */}
        {!isCollapsed && (
          <div className="flex items-center gap-2">
            {themes.map((theme) => (
              <button
                key={theme.id}
                onClick={() => onThemeChange?.(theme.id as 'light' | 'dark' | 'auto')}
                className={cn(
                  'p-2 rounded-lg transition-colors',
                  currentTheme === theme.id
                    ? 'bg-blue-100 text-blue-600'
                    : 'hover:bg-white/10 text-white'
                )}
                title={`Switch to ${theme.label} theme`}
              >
                <theme.icon className="w-4 h-4" />
              </button>
            ))}
          </div>
        )}

        {/* Notification Toggle */}
        <button
          onClick={onNotificationToggle}
          className={cn(
            'p-2 rounded-lg transition-colors',
            notificationsEnabled
              ? 'bg-green-100 text-green-600'
              : 'bg-white/10 text-white',
            isCollapsed && 'w-full flex justify-center'
          )}
          title={notificationsEnabled ? 'Disable notifications' : 'Enable notifications'}
        >
          {notificationsEnabled ? <Bell className="w-4 h-4" /> : <Mail className="w-4 h-4" />}
        </button>
      </div>
    </div>
  );
}