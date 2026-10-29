'use client';

import React, { useState, useEffect } from 'react';
import { cn } from '../utils';
import { 
  useAppearanceSettings,
  useUpdateAppearanceSettings,
  useLanguageSettings,
  useUpdateLanguageSettings,
  usePrivacySettings,
  useUpdatePrivacySettings,
  useNotificationSettings,
  useUpdateNotificationSettings
} from '../hooks/useRealData';
import { 
  Settings as SettingsIcon,
  User,
  Bell,
  Shield,
  Globe,
  Palette,
  Monitor,
  Moon,
  Sun,
  Smartphone,
  Mail,
  MessageSquare,
  Calendar,
  ChevronRight,
  Check,
  Save,
  RotateCcw,
  X,
  Loader2
} from 'lucide-react';

interface SettingsPanelProps {
  onClose?: () => void;
}

interface AppearanceSettings {
  theme: 'light' | 'dark' | 'auto';
  colorScheme: 'blue' | 'green' | 'purple' | 'orange';
  fontSize: 'small' | 'medium' | 'large';
  compactMode: boolean;
}

interface LanguageSettings {
  language: string;
  region: string;
  dateFormat: 'MM/DD/YYYY' | 'DD/MM/YYYY' | 'YYYY-MM-DD';
  timeFormat: '12h' | '24h';
}

interface PrivacySettings {
  shareStatus: boolean;
  shareActivity: boolean;
  allowAnalytics: boolean;
  dataRetention: '30days' | '90days' | '1year' | 'forever';
}

interface NotificationSettings {
  emailNotifications: boolean;
  pushNotifications: boolean;
  teamsNotifications: boolean;
  calendarReminders: boolean;
  quietHours: {
    enabled: boolean;
    start: string;
    end: string;
  };
}

export function SettingsPanel({ onClose }: SettingsPanelProps) {
  const [activeTab, setActiveTab] = useState<'appearance' | 'language' | 'privacy' | 'notifications'>('appearance');
  const [hasChanges, setHasChanges] = useState(false);

  // Real data hooks
  const { data: appearanceData, isLoading: appearanceLoading } = useAppearanceSettings();
  const { data: languageData, isLoading: languageLoading } = useLanguageSettings();
  const { data: privacyData, isLoading: privacyLoading } = usePrivacySettings();
  const { data: notificationData, isLoading: notificationLoading } = useNotificationSettings();

  // Mutations
  const updateAppearanceMutation = useUpdateAppearanceSettings();
  const updateLanguageMutation = useUpdateLanguageSettings();
  const updatePrivacyMutation = useUpdatePrivacySettings();
  const updateNotificationMutation = useUpdateNotificationSettings();

  // Local state that syncs with server data
  const [appearanceSettings, setAppearanceSettings] = useState<AppearanceSettings>({
    theme: 'light',
    colorScheme: 'blue',
    fontSize: 'medium',
    compactMode: false,
  });

  const [languageSettings, setLanguageSettings] = useState<LanguageSettings>({
    language: 'en-US',
    region: 'US',
    dateFormat: 'MM/DD/YYYY',
    timeFormat: '12h',
  });

  const [privacySettings, setPrivacySettings] = useState<PrivacySettings>({
    shareStatus: true,
    shareActivity: false,
    allowAnalytics: true,
    dataRetention: '1year',
  });

  const [notificationSettings, setNotificationSettings] = useState<NotificationSettings>({
    emailNotifications: true,
    pushNotifications: true,
    teamsNotifications: true,
    calendarReminders: true,
    quietHours: {
      enabled: false,
      start: '22:00',
      end: '08:00',
    },
  });

  // Sync server data with local state
  useEffect(() => {
    if (appearanceData) {
      // eslint-disable-next-line react-hooks/set-state-in-effect
      setAppearanceSettings({
        theme: (appearanceData.theme as 'light' | 'dark' | 'auto') || 'light',
        colorScheme: (appearanceData.colorScheme as 'blue' | 'green' | 'purple' | 'orange') || 'blue',
        fontSize: (appearanceData.fontSize as 'small' | 'medium' | 'large') || 'medium',
        compactMode: appearanceData.compactMode || false,
      });
    }
  }, [appearanceData]);

  useEffect(() => {
    if (languageData) {
      // eslint-disable-next-line react-hooks/set-state-in-effect
      setLanguageSettings({
        language: languageData.language || 'en-US',
        region: languageData.region || 'US',
        dateFormat: (languageData.dateFormat as 'MM/DD/YYYY' | 'DD/MM/YYYY' | 'YYYY-MM-DD') || 'MM/DD/YYYY',
        timeFormat: (languageData.timeFormat as '12h' | '24h') || '12h',
      });
    }
  }, [languageData]);

  useEffect(() => {
    if (privacyData) {
      // eslint-disable-next-line react-hooks/set-state-in-effect
      setPrivacySettings({
        shareStatus: privacyData.shareStatus ?? true,
        shareActivity: privacyData.shareActivity ?? false,
        allowAnalytics: privacyData.allowAnalytics ?? true,
        dataRetention: (privacyData.dataRetention as '30days' | '90days' | '1year' | 'forever') || '1year',
      });
    }
  }, [privacyData]);

  useEffect(() => {
    if (notificationData) {
      // eslint-disable-next-line react-hooks/set-state-in-effect
      setNotificationSettings({
        emailNotifications: notificationData.emailNotifications ?? true,
        pushNotifications: notificationData.pushNotifications ?? true,
        teamsNotifications: notificationData.teamsNotifications ?? true,
        calendarReminders: notificationData.calendarReminders ?? true,
        quietHours: {
          enabled: notificationData.quietHours?.enabled ?? false,
          start: notificationData.quietHours?.start || '22:00',
          end: notificationData.quietHours?.end || '08:00',
        },
      });
    }
  }, [notificationData]);

  const settingsTabs = [
    {
      id: 'appearance' as const,
      label: 'Appearance',
      icon: Palette,
      description: 'Theme, colors, and display',
    },
    {
      id: 'language' as const,
      label: 'Language & Region',
      icon: Globe,
      description: 'Language, timezone, and formats',
    },
    {
      id: 'privacy' as const,
      label: 'Privacy & Security',
      icon: Shield,
      description: 'Data sharing and security',
    },
    {
      id: 'notifications' as const,
      label: 'Notifications',
      icon: Bell,
      description: 'Alerts and quiet hours',
    },
  ];

  const handleSave = async () => {
    try {
      setHasChanges(false);
      
      // Save settings based on active tab
      if (activeTab === 'appearance') {
        await updateAppearanceMutation.mutateAsync(appearanceSettings);
      } else if (activeTab === 'language') {
        await updateLanguageMutation.mutateAsync(languageSettings);
      } else if (activeTab === 'privacy') {
        await updatePrivacyMutation.mutateAsync(privacySettings);
      } else if (activeTab === 'notifications') {
        await updateNotificationMutation.mutateAsync(notificationSettings);
      }
      
      console.log('Settings saved successfully');
    } catch (error) {
      console.error('Failed to save settings:', error);
      setHasChanges(true); // Restore changes flag on error
    }
  };

  const handleReset = () => {
    // Reset to server defaults based on active tab
    if (activeTab === 'appearance' && appearanceData) {
      setAppearanceSettings({
        theme: (appearanceData.theme as 'light' | 'dark' | 'auto') || 'light',
        colorScheme: (appearanceData.colorScheme as 'blue' | 'green' | 'purple' | 'orange') || 'blue',
        fontSize: (appearanceData.fontSize as 'small' | 'medium' | 'large') || 'medium',
        compactMode: appearanceData.compactMode || false,
      });
    } else if (activeTab === 'language' && languageData) {
      setLanguageSettings({
        language: languageData.language || 'en-US',
        region: languageData.region || 'US',
        dateFormat: (languageData.dateFormat as 'MM/DD/YYYY' | 'DD/MM/YYYY' | 'YYYY-MM-DD') || 'MM/DD/YYYY',
        timeFormat: (languageData.timeFormat as '12h' | '24h') || '12h',
      });
    } else if (activeTab === 'privacy' && privacyData) {
      setPrivacySettings({
        shareStatus: privacyData.shareStatus ?? true,
        shareActivity: privacyData.shareActivity ?? false,
        allowAnalytics: privacyData.allowAnalytics ?? true,
        dataRetention: (privacyData.dataRetention as '30days' | '90days' | '1year' | 'forever') || '1year',
      });
    } else if (activeTab === 'notifications' && notificationData) {
      setNotificationSettings({
        emailNotifications: notificationData.emailNotifications ?? true,
        pushNotifications: notificationData.pushNotifications ?? true,
        teamsNotifications: notificationData.teamsNotifications ?? true,
        calendarReminders: notificationData.calendarReminders ?? true,
        quietHours: {
          enabled: notificationData.quietHours?.enabled ?? false,
          start: notificationData.quietHours?.start || '22:00',
          end: notificationData.quietHours?.end || '08:00',
        },
      });
    }
    setHasChanges(false);
  };

  const renderAppearanceSettings = () => (
    <div className="space-y-6">
      <div>
        <h4 className="text-sm font-medium text-white mb-3">Theme</h4>
        <div className="grid grid-cols-3 gap-3">
          {[
            { value: 'light', label: 'Light', icon: Sun },
            { value: 'dark', label: 'Dark', icon: Moon },
            { value: 'auto', label: 'Auto', icon: Monitor },
          ].map(({ value, label, icon: Icon }) => (
            <button
              key={value}
              onClick={() => {
                setAppearanceSettings(prev => ({ ...prev, theme: value as 'light' | 'dark' | 'auto' }));
                setHasChanges(true);
              }}
              className={cn(
                'flex flex-col items-center gap-2 p-3 rounded-xl border-2 transition-all text-white hover:text-[#151F6C] hover:bg-white group',
                appearanceSettings.theme === value
                  ? 'border-white bg-white/10 text-white'
                  : 'border-white/20 hover:border-white'
              )}
            >
              <Icon className="w-5 h-5 group-hover:text-[#151F6C]" />
              <span className="text-xs font-medium group-hover:text-[#151F6C]">{label}</span>
              {appearanceSettings.theme === value && (
                <Check className="w-4 h-4 text-white group-hover:text-[#151F6C]" />
              )}
            </button>
          ))}
        </div>
      </div>

      <div>
        <h4 className="text-sm font-medium text-white mb-3">Color Scheme</h4>
        <div className="grid grid-cols-4 gap-3">
          {[
            { value: 'blue', color: 'bg-blue-500' },
            { value: 'green', color: 'bg-green-500' },
            { value: 'purple', color: 'bg-purple-500' },
            { value: 'orange', color: 'bg-orange-500' },
          ].map(({ value, color }) => (
            <button
              key={value}
              onClick={() => {
                setAppearanceSettings(prev => ({ ...prev, colorScheme: value as 'blue' | 'green' | 'purple' | 'orange' }));
                setHasChanges(true);
              }}
              className={cn(
                'flex items-center justify-center h-10 rounded-lg border-2 transition-all hover:bg-white',
                appearanceSettings.colorScheme === value
                  ? 'border-white'
                  : 'border-white/20 hover:border-white'
              )}
            >
              <div className={cn('w-6 h-6 rounded-full', color)} />
            </button>
          ))}
        </div>
      </div>

      <div>
        <h4 className="text-sm font-medium text-white mb-3">Font Size</h4>
        <div className="grid grid-cols-3 gap-3">
          {[
            { value: 'small', label: 'Small' },
            { value: 'medium', label: 'Medium' },
            { value: 'large', label: 'Large' },
          ].map(({ value, label }) => (
            <button
              key={value}
              onClick={() => {
                setAppearanceSettings(prev => ({ ...prev, fontSize: value as 'small' | 'medium' | 'large' }));
                setHasChanges(true);
              }}
              className={cn(
                'p-3 rounded-lg border-2 transition-all text-center text-white hover:text-[#151F6C] hover:bg-white',
                appearanceSettings.fontSize === value
                  ? 'border-white bg-white/10'
                  : 'border-white/20 hover:border-white'
              )}
            >
              <span className={cn(
                'font-medium',
                value === 'small' && 'text-xs',
                value === 'medium' && 'text-sm',
                value === 'large' && 'text-base'
              )}>
                {label}
              </span>
            </button>
          ))}
        </div>
      </div>

      <div className="flex items-center justify-between">
        <div>
          <h4 className="text-sm font-medium text-white">Compact Mode</h4>
          <p className="text-xs text-white/70">Reduce spacing for more content</p>
        </div>
        <button
          onClick={() => {
            setAppearanceSettings(prev => ({ ...prev, compactMode: !prev.compactMode }));
            setHasChanges(true);
          }}
          className={cn(
            'relative w-11 h-6 rounded-full transition-colors',
            appearanceSettings.compactMode ? 'bg-white' : 'bg-white/20'
          )}
        >
          <div
            className={cn(
              'absolute top-0.5 w-5 h-5 bg-[#151F6C] rounded-full transition-transform',
              appearanceSettings.compactMode ? 'translate-x-5' : 'translate-x-0.5'
            )}
          />
        </button>
      </div>
    </div>
  );

  const renderLanguageSettings = () => (
    <div className="space-y-6">
      <div>
        <h4 className="text-sm font-medium text-white mb-3">Language</h4>
        <select
          value={languageSettings.language}
          onChange={(e) => {
            setLanguageSettings(prev => ({ ...prev, language: e.target.value }));
            setHasChanges(true);
          }}
          className="w-full p-3 border border-white/20 rounded-lg bg-white/10 text-white"
        >
          <option value="en-US">English (US)</option>
          <option value="en-GB">English (UK)</option>
          <option value="es-ES">Español</option>
          <option value="fr-FR">Français</option>
          <option value="de-DE">Deutsch</option>
          <option value="pt-BR">Português</option>
          <option value="ja-JP">日本語</option>
          <option value="zh-CN">中文</option>
        </select>
      </div>

      <div>
        <h4 className="text-sm font-medium text-white mb-3">Date Format</h4>
        <div className="space-y-2">
          {[
            { value: 'MM/DD/YYYY', example: '12/31/2024' },
            { value: 'DD/MM/YYYY', example: '31/12/2024' },
            { value: 'YYYY-MM-DD', example: '2024-12-31' },
          ].map(({ value, example }) => (
            <label key={value} className="flex items-center gap-3 p-3 rounded-lg border border-white/20 hover:bg-white hover:text-[#151F6C] cursor-pointer text-white transition-colors group">
              <input
                type="radio"
                name="dateFormat"
                value={value}
                checked={languageSettings.dateFormat === value}
                onChange={(e) => {
                  setLanguageSettings(prev => ({ ...prev, dateFormat: e.target.value as 'MM/DD/YYYY' | 'DD/MM/YYYY' | 'YYYY-MM-DD' }));
                  setHasChanges(true);
                }}
                className="w-4 h-4 text-white"
              />
              <div className="flex-1">
                <div className="text-sm font-medium group-hover:text-[#151F6C]">{value}</div>
                <div className="text-xs text-white/70 group-hover:text-[#151F6C]/70">{example}</div>
              </div>
            </label>
          ))}
        </div>
      </div>

      <div>
        <h4 className="text-sm font-medium text-white mb-3">Time Format</h4>
        <div className="grid grid-cols-2 gap-3">
          {[
            { value: '12h', label: '12 Hour', example: '2:30 PM' },
            { value: '24h', label: '24 Hour', example: '14:30' },
          ].map(({ value, label, example }) => (
            <button
              key={value}
              onClick={() => {
                setLanguageSettings(prev => ({ ...prev, timeFormat: value as '12h' | '24h' }));
                setHasChanges(true);
              }}
              className={cn(
                'p-3 rounded-lg border-2 transition-all text-center text-white hover:text-[#151F6C] hover:bg-white',
                languageSettings.timeFormat === value
                  ? 'border-white bg-white/10'
                  : 'border-white/20 hover:border-white'
              )}
            >
              <div className="text-sm font-medium">{label}</div>
              <div className="text-xs text-white/70 group-hover:text-[#151F6C]/70">{example}</div>
            </button>
          ))}
        </div>
      </div>
    </div>
  );

  const renderPrivacySettings = () => (
    <div className="space-y-6">
      {[
        {
          key: 'shareStatus' as keyof PrivacySettings,
          label: 'Share Presence Status',
          description: 'Allow others to see when you\'re online',
          icon: User,
        },
        {
          key: 'shareActivity' as keyof PrivacySettings,
          label: 'Share Calendar Information',
          description: 'Allow others to see your availability',
          icon: Calendar,
        },
        {
          key: 'allowAnalytics' as keyof PrivacySettings,
          label: 'Allow External Invites',
          description: 'Receive meeting invites from external users',
          icon: Mail,
        },
      ].map(({ key, label, description, icon: Icon }) => (
        <div key={key} className="flex items-center justify-between p-4 rounded-lg border border-white/20">
          <div className="flex items-center gap-3">
            <div className="w-8 h-8 rounded-lg bg-white/10 flex items-center justify-center">
              <Icon className="w-4 h-4 text-white" />
            </div>
            <div>
              <h4 className="text-sm font-medium text-white">{label}</h4>
              <p className="text-xs text-white/70">{description}</p>
            </div>
          </div>
          <button
            onClick={() => {
              setPrivacySettings(prev => ({ ...prev, [key]: !prev[key] }));
              setHasChanges(true);
            }}
            className={cn(
              'relative w-11 h-6 rounded-full transition-colors',
              privacySettings[key] ? 'bg-white' : 'bg-white/20'
            )}
          >
            <div
              className={cn(
                'absolute top-0.5 w-5 h-5 bg-[#151F6C] rounded-full transition-transform',
                privacySettings[key] ? 'translate-x-5' : 'translate-x-0.5'
              )}
            />
          </button>
        </div>
      ))}

      <div>
        <h4 className="text-sm font-medium text-white mb-3">Data Retention</h4>
        <div className="space-y-2">
          {[
            { value: '30days', label: '30 Days' },
            { value: '90days', label: '90 Days' },
            { value: '1year', label: '1 Year' },
            { value: 'forever', label: 'Forever' },
          ].map(({ value, label }) => (
            <label key={value} className="flex items-center gap-3 p-3 rounded-lg border border-white/20 hover:bg-white hover:text-[#151F6C] cursor-pointer text-white transition-colors group">
              <input
                type="radio"
                name="dataRetention"
                value={value}
                checked={privacySettings.dataRetention === value}
                onChange={(e) => {
                  setPrivacySettings(prev => ({ ...prev, dataRetention: e.target.value as '30days' | '90days' | '1year' | 'forever' }));
                  setHasChanges(true);
                }}
                className="w-4 h-4 text-white"
              />
              <span className="text-sm font-medium group-hover:text-[#151F6C]">{label}</span>
            </label>
          ))}
        </div>
      </div>
    </div>
  );

  const renderNotificationSettings = () => (
    <div className="space-y-6">
      {[
        {
          key: 'emailNotifications' as keyof NotificationSettings,
          label: 'Email Notifications',
          description: 'Receive notifications via email',
          icon: Mail,
        },
        {
          key: 'pushNotifications' as keyof NotificationSettings,
          label: 'Push Notifications',
          description: 'Receive notifications on this device',
          icon: Smartphone,
        },
        {
          key: 'teamsNotifications' as keyof NotificationSettings,
          label: 'Teams Notifications',
          description: 'Notifications for Teams messages',
          icon: MessageSquare,
        },
        {
          key: 'calendarReminders' as keyof NotificationSettings,
          label: 'Calendar Reminders',
          description: 'Reminders for upcoming events',
          icon: Calendar,
        },
      ].map(({ key, label, description, icon: Icon }) => (
        <div key={key} className="flex items-center justify-between p-4 rounded-lg border border-white/20">
          <div className="flex items-center gap-3">
            <div className="w-8 h-8 rounded-lg bg-white/10 flex items-center justify-center">
              <Icon className="w-4 h-4 text-white" />
            </div>
            <div>
              <h4 className="text-sm font-medium text-white">{label}</h4>
              <p className="text-xs text-white/70">{description}</p>
            </div>
          </div>
          <button
            onClick={() => {
              setNotificationSettings(prev => ({ ...prev, [key]: !prev[key] }));
              setHasChanges(true);
            }}
            className={cn(
              'relative w-11 h-6 rounded-full transition-colors',
              notificationSettings[key] ? 'bg-white' : 'bg-white/20'
            )}
          >
            <div
              className={cn(
                'absolute top-0.5 w-5 h-5 bg-[#151F6C] rounded-full transition-transform',
                notificationSettings[key] ? 'translate-x-5' : 'translate-x-0.5'
              )}
            />
          </button>
        </div>
      ))}

      <div className="p-4 rounded-lg border border-white/20">
        <div className="flex items-center justify-between mb-4">
          <div>
            <h4 className="text-sm font-medium text-white">Quiet Hours</h4>
            <p className="text-xs text-white/70">Disable notifications during these hours</p>
          </div>
          <button
            onClick={() => {
              setNotificationSettings(prev => ({
                ...prev,
                quietHours: { ...prev.quietHours, enabled: !prev.quietHours.enabled }
              }));
              setHasChanges(true);
            }}
            className={cn(
              'relative w-11 h-6 rounded-full transition-colors',
              notificationSettings.quietHours.enabled ? 'bg-white' : 'bg-white/20'
            )}
          >
            <div
              className={cn(
                'absolute top-0.5 w-5 h-5 bg-[#151F6C] rounded-full transition-transform',
                notificationSettings.quietHours.enabled ? 'translate-x-5' : 'translate-x-0.5'
              )}
            />
          </button>
        </div>

        {notificationSettings.quietHours.enabled && (
          <div className="grid grid-cols-2 gap-3">
            <div>
              <label className="block text-xs font-medium text-white/70 mb-1">Start Time</label>
              <input
                type="time"
                value={notificationSettings.quietHours.start}
                onChange={(e) => {
                  setNotificationSettings(prev => ({
                    ...prev,
                    quietHours: { ...prev.quietHours, start: e.target.value }
                  }));
                  setHasChanges(true);
                }}
                className="w-full p-2 border border-white/20 rounded-lg text-sm bg-white/10 text-white"
              />
            </div>
            <div>
              <label className="block text-xs font-medium text-white/70 mb-1">End Time</label>
              <input
                type="time"
                value={notificationSettings.quietHours.end}
                onChange={(e) => {
                  setNotificationSettings(prev => ({
                    ...prev,
                    quietHours: { ...prev.quietHours, end: e.target.value }
                  }));
                  setHasChanges(true);
                }}
                className="w-full p-2 border border-white/20 rounded-lg text-sm bg-white/10 text-white"
              />
            </div>
          </div>
        )}
      </div>
    </div>
  );

  const renderTabContent = () => {
    switch (activeTab) {
      case 'appearance':
        return renderAppearanceSettings();
      case 'language':
        return renderLanguageSettings();
      case 'privacy':
        return renderPrivacySettings();
      case 'notifications':
        return renderNotificationSettings();
      default:
        return null;
    }
  };

  return (
    <div className="h-full flex flex-col bg-[#151F6C]">
      {/* Header */}
      <div className="flex items-center justify-between p-4 border-b border-white/10">
        <div className="flex items-center gap-3">
          <div className="w-8 h-8 rounded-lg bg-white/10 flex items-center justify-center">
            <SettingsIcon className="w-5 h-5 text-white" />
          </div>
          <div>
            <h2 className="text-lg font-semibold text-white">Settings</h2>
            <p className="text-sm text-white/70">Manage your preferences</p>
          </div>
        </div>
        {onClose && (
          <button
            onClick={onClose}
            className="w-8 h-8 rounded-lg bg-white/10 flex items-center justify-center hover:bg-white hover:text-[#151F6C] transition-colors text-white"
          >
            <X className="w-4 h-4" />
          </button>
        )}
      </div>

      <div className="flex-1 flex overflow-hidden">
        {/* Sidebar */}
        <div className="w-48 border-r border-white/10 p-4">
          <div className="space-y-1">
            {settingsTabs.map((tab) => (
              <button
                key={tab.id}
                onClick={() => setActiveTab(tab.id)}
                className={cn(
                  'w-full flex items-center gap-3 p-3 rounded-lg text-left transition-colors text-white hover:text-[#151F6C] hover:bg-white group',
                  activeTab === tab.id
                    ? 'bg-white/10 text-white'
                    : 'hover:bg-white'
                )}
              >
                <tab.icon className="w-4 h-4 group-hover:text-[#151F6C]" />
                <div className="flex-1 min-w-0">
                  <div className="text-sm font-medium group-hover:text-[#151F6C]">{tab.label}</div>
                  <div className="text-xs text-white/70 group-hover:text-[#151F6C]/70 truncate">{tab.description}</div>
                </div>
                {activeTab === tab.id && <ChevronRight className="w-4 h-4 group-hover:text-[#151F6C]" />}
              </button>
            ))}
          </div>
        </div>

        {/* Content */}
        <div className="flex-1 flex flex-col overflow-hidden">
          <div className="flex-1 p-6 overflow-y-auto">
            <div className="max-w-2xl">
              {/* Loading state */}
              {((activeTab === 'appearance' && appearanceLoading) ||
                (activeTab === 'language' && languageLoading) ||
                (activeTab === 'privacy' && privacyLoading) ||
                (activeTab === 'notifications' && notificationLoading)) && (
                <div className="flex items-center justify-center py-12">
                  <Loader2 className="w-6 h-6 animate-spin text-blue-500" />
                  <span className="ml-2 text-sm text-gray-600">Loading settings...</span>
                </div>
              )}
              
              {/* Content */}
              {!((activeTab === 'appearance' && appearanceLoading) ||
                 (activeTab === 'language' && languageLoading) ||
                 (activeTab === 'privacy' && privacyLoading) ||
                 (activeTab === 'notifications' && notificationLoading)) && 
                 renderTabContent()}
            </div>
          </div>

          {/* Footer Actions */}
          {hasChanges && (
            <div className="border-t border-white/10 p-4 bg-[#0f1654]">
              <div className="flex items-center justify-between">
                <p className="text-sm text-white/70">You have unsaved changes</p>
                <div className="flex items-center gap-3">
                  <button
                    onClick={handleReset}
                    disabled={updateAppearanceMutation.isPending || updateLanguageMutation.isPending || 
                             updatePrivacyMutation.isPending || updateNotificationMutation.isPending}
                    className="flex items-center gap-2 px-3 py-2 text-sm text-white/70 hover:text-white hover:bg-white/10 rounded-lg transition-colors disabled:opacity-50"
                  >
                    <RotateCcw className="w-4 h-4" />
                    Reset
                  </button>
                  <button
                    onClick={handleSave}
                    disabled={updateAppearanceMutation.isPending || updateLanguageMutation.isPending || 
                             updatePrivacyMutation.isPending || updateNotificationMutation.isPending}
                    className="flex items-center gap-2 px-4 py-2 bg-white text-[#151F6C] text-sm font-medium rounded-lg hover:bg-white/90 transition-colors disabled:opacity-50"
                  >
                    {(updateAppearanceMutation.isPending || updateLanguageMutation.isPending || 
                      updatePrivacyMutation.isPending || updateNotificationMutation.isPending) ? (
                      <Loader2 className="w-4 h-4 animate-spin" />
                    ) : (
                      <Save className="w-4 h-4" />
                    )}
                    {(updateAppearanceMutation.isPending || updateLanguageMutation.isPending || 
                      updatePrivacyMutation.isPending || updateNotificationMutation.isPending) ? 'Saving...' : 'Save Changes'}
                  </button>
                </div>
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
