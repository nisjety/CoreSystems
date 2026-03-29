'use client';

import React, { useState, useEffect } from 'react';
import { cn } from '../../sidebar/utils';
import { 
  useAppearanceSettings,
  useUpdateAppearanceSettings,
  useLanguageSettings,
  useUpdateLanguageSettings,
  usePrivacySettings,
  useUpdatePrivacySettings,
  useNotificationSettings,
  useUpdateNotificationSettings,
  useSecuritySettings,
  useUpdateSecuritySettings,
  useAccessibilitySettings,
  useUpdateAccessibilitySettings,
  useAISettings,
  useUpdateAISettings,
  useStorageSettings,
  useUpdateStorageSettings,
  type SecuritySettings,
  type AccessibilitySettings,
  type AISettings,
  type StorageSettings,
} from '../../sidebar/hooks/useRealData';
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
  Loader2,
  Lock,
  Eye,
  Cpu,
  HardDrive,
  Clock,
  Zap,
  Database,
  RefreshCw,
  WifiOff
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

type SettingsTab = 'appearance' | 'language' | 'privacy' | 'notifications' | 'security' | 'accessibility' | 'ai' | 'storage';

export function SettingsPanel({ onClose }: SettingsPanelProps) {
  const [activeTab, setActiveTab] = useState<SettingsTab>('appearance');
  const [hasChanges, setHasChanges] = useState(false);

  // Real data hooks
  const { data: appearanceData, isLoading: appearanceLoading } = useAppearanceSettings();
  const { data: languageData, isLoading: languageLoading } = useLanguageSettings();
  const { data: privacyData, isLoading: privacyLoading } = usePrivacySettings();
  const { data: notificationData, isLoading: notificationLoading } = useNotificationSettings();
  const { data: securityData, isLoading: securityLoading } = useSecuritySettings();
  const { data: accessibilityData, isLoading: accessibilityLoading } = useAccessibilitySettings();
  const { data: aiData, isLoading: aiLoading } = useAISettings();
  const { data: storageData, isLoading: storageLoading } = useStorageSettings();

  // Mutations
  const updateAppearanceMutation = useUpdateAppearanceSettings();
  const updateLanguageMutation = useUpdateLanguageSettings();
  const updatePrivacyMutation = useUpdatePrivacySettings();
  const updateNotificationMutation = useUpdateNotificationSettings();
  const updateSecurityMutation = useUpdateSecuritySettings();
  const updateAccessibilityMutation = useUpdateAccessibilitySettings();
  const updateAIMutation = useUpdateAISettings();
  const updateStorageMutation = useUpdateStorageSettings();

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

  const [securitySettings, setSecuritySettings] = useState<SecuritySettings>({
    twoFactorEnabled: false,
    sessionTimeout: '4hours',
    loginAlerts: true,
    trustedDevicesEnabled: true,
  });

  const [accessibilitySettings, setAccessibilitySettings] = useState<AccessibilitySettings>({
    highContrast: false,
    reducedMotion: false,
    screenReaderOptimized: false,
    keyboardShortcutsEnabled: true,
  });

  const [aiSettings, setAISettings] = useState<AISettings>({
    aiEnabled: true,
    modelPreference: 'auto',
    dataCollectionEnabled: true,
    personalizationEnabled: true,
    memoryEnabled: false,
  });

  const [storageSettings, setStorageSettings] = useState<StorageSettings>({
    autoSync: true,
    clearCacheOnLogout: false,
    compressionEnabled: true,
    offlineAccessEnabled: false,
  });

  // Sync server data → local state
  useEffect(() => {
    if (securityData) setSecuritySettings(securityData);
  }, [securityData]);

  useEffect(() => {
    if (accessibilityData) setAccessibilitySettings(accessibilityData);
  }, [accessibilityData]);

  useEffect(() => {
    if (aiData) setAISettings(aiData);
  }, [aiData]);

  useEffect(() => {
    if (storageData) setStorageSettings(storageData);
  }, [storageData]);

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
    { id: 'appearance' as const, label: 'Appearance', icon: Palette, description: 'Theme & display' },
    { id: 'language' as const, label: 'Language', icon: Globe, description: 'Region & formats' },
    { id: 'privacy' as const, label: 'Privacy', icon: Shield, description: 'Data & retention' },
    { id: 'notifications' as const, label: 'Notifications', icon: Bell, description: 'Alerts & hours' },
    { id: 'security' as const, label: 'Security', icon: Lock, description: '2FA & sessions' },
    { id: 'accessibility' as const, label: 'Accessibility', icon: Eye, description: 'Motion & contrast' },
    { id: 'ai' as const, label: 'AI & Copilot', icon: Cpu, description: 'Model & memory' },
    { id: 'storage' as const, label: 'Storage & Sync', icon: HardDrive, description: 'Cache & offline' },
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
      } else if (activeTab === 'security') {
        await updateSecurityMutation.mutateAsync(securitySettings);
      } else if (activeTab === 'accessibility') {
        await updateAccessibilityMutation.mutateAsync(accessibilitySettings);
      } else if (activeTab === 'ai') {
        await updateAIMutation.mutateAsync(aiSettings);
      } else if (activeTab === 'storage') {
        await updateStorageMutation.mutateAsync(storageSettings);
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
    } else if (activeTab === 'security' && securityData) {
      setSecuritySettings(securityData);
    } else if (activeTab === 'accessibility' && accessibilityData) {
      setAccessibilitySettings(accessibilityData);
    } else if (activeTab === 'ai' && aiData) {
      setAISettings(aiData);
    } else if (activeTab === 'storage' && storageData) {
      setStorageSettings(storageData);
    }
    setHasChanges(false);
  };

  const renderAppearanceSettings = () => (
    <div className="space-y-6">
      <div>
        <h4 className="text-sm font-medium text-black mb-3">Theme</h4>
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
                'flex flex-col items-center gap-2 p-3 rounded-xl border-2 transition-all text-black hover:bg-black/5 group',
                appearanceSettings.theme === value
                  ? 'border-black/20 bg-black/5 text-black'
                  : 'border-black/10 hover:border-black/25'
              )}
            >
              <Icon className="w-5 h-5" />
              <span className="text-xs font-medium">{label}</span>
              {appearanceSettings.theme === value && (
                <Check className="w-4 h-4 text-black" />
              )}
            </button>
          ))}
        </div>
      </div>

      <div>
        <h4 className="text-sm font-medium text-black mb-3">Color Scheme</h4>
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
                'flex items-center justify-center h-10 rounded-lg border-2 transition-all hover:bg-black/5',
                appearanceSettings.colorScheme === value
                  ? 'border-black/25 bg-black/5'
                  : 'border-black/10 hover:border-black/25'
              )}
            >
              <div className={cn('w-6 h-6 rounded-full', color)} />
            </button>
          ))}
        </div>
      </div>

      <div>
        <h4 className="text-sm font-medium text-black mb-3">Font Size</h4>
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
                'p-3 rounded-lg border-2 transition-all text-center text-black hover:bg-black/5',
                appearanceSettings.fontSize === value
                  ? 'border-black/25 bg-black/5'
                  : 'border-black/10 hover:border-black/25'
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
          <h4 className="text-sm font-medium text-black">Compact Mode</h4>
          <p className="text-xs text-black/60">Reduce spacing for more content</p>
        </div>
        <button
          onClick={() => {
            setAppearanceSettings(prev => ({ ...prev, compactMode: !prev.compactMode }));
            setHasChanges(true);
          }}
          className={cn(
            'relative w-11 h-6 rounded-full transition-colors',
            appearanceSettings.compactMode ? 'bg-black/70' : 'bg-black/15'
          )}
        >
          <div
            className={cn(
              'absolute top-0.5 w-5 h-5 bg-white rounded-full transition-transform',
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
        <h4 className="text-sm font-medium text-black mb-3">Language</h4>
        <select
          value={languageSettings.language}
          onChange={(e) => {
            setLanguageSettings(prev => ({ ...prev, language: e.target.value }));
            setHasChanges(true);
          }}
          className="w-full p-3 border border-black/10 rounded-lg bg-white text-black"
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
        <h4 className="text-sm font-medium text-black mb-3">Date Format</h4>
        <div className="space-y-2">
          {[
            { value: 'MM/DD/YYYY', example: '12/31/2024' },
            { value: 'DD/MM/YYYY', example: '31/12/2024' },
            { value: 'YYYY-MM-DD', example: '2024-12-31' },
          ].map(({ value, example }) => (
            <label key={value} className="flex items-center gap-3 p-3 rounded-lg border border-black/10 hover:bg-black/5 cursor-pointer text-black transition-colors group">
              <input
                type="radio"
                name="dateFormat"
                value={value}
                checked={languageSettings.dateFormat === value}
                onChange={(e) => {
                  setLanguageSettings(prev => ({ ...prev, dateFormat: e.target.value as 'MM/DD/YYYY' | 'DD/MM/YYYY' | 'YYYY-MM-DD' }));
                  setHasChanges(true);
                }}
                className="w-4 h-4 text-black"
              />
              <div className="flex-1">
                <div className="text-sm font-medium">{value}</div>
                <div className="text-xs text-black/60">{example}</div>
              </div>
            </label>
          ))}
        </div>
      </div>

      <div>
        <h4 className="text-sm font-medium text-black mb-3">Time Format</h4>
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
                'p-3 rounded-lg border-2 transition-all text-center text-black hover:bg-black/5',
                languageSettings.timeFormat === value
                  ? 'border-black/25 bg-black/5'
                  : 'border-black/10 hover:border-black/25'
              )}
            >
              <div className="text-sm font-medium">{label}</div>
              <div className="text-xs text-black/60">{example}</div>
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
        <div key={key} className="flex items-center justify-between p-4 rounded-lg border border-black/10 bg-white">
          <div className="flex items-center gap-3">
            <div className="w-8 h-8 rounded-lg bg-black/5 flex items-center justify-center">
              <Icon className="w-4 h-4 text-black/70" />
            </div>
            <div>
              <h4 className="text-sm font-medium text-black">{label}</h4>
              <p className="text-xs text-black/60">{description}</p>
            </div>
          </div>
          <button
            onClick={() => {
              setPrivacySettings(prev => ({ ...prev, [key]: !prev[key] }));
              setHasChanges(true);
            }}
            className={cn(
              'relative w-11 h-6 rounded-full transition-colors',
              privacySettings[key] ? 'bg-black/70' : 'bg-black/15'
            )}
          >
            <div
              className={cn(
                'absolute top-0.5 w-5 h-5 bg-white rounded-full transition-transform',
                privacySettings[key] ? 'translate-x-5' : 'translate-x-0.5'
              )}
            />
          </button>
        </div>
      ))}

      <div>
        <h4 className="text-sm font-medium text-black mb-3">Data Retention</h4>
        <div className="space-y-2">
          {[
            { value: '30days', label: '30 Days' },
            { value: '90days', label: '90 Days' },
            { value: '1year', label: '1 Year' },
            { value: 'forever', label: 'Forever' },
          ].map(({ value, label }) => (
            <label key={value} className="flex items-center gap-3 p-3 rounded-lg border border-black/10 hover:bg-black/5 cursor-pointer text-black transition-colors group">
              <input
                type="radio"
                name="dataRetention"
                value={value}
                checked={privacySettings.dataRetention === value}
                onChange={(e) => {
                  setPrivacySettings(prev => ({ ...prev, dataRetention: e.target.value as '30days' | '90days' | '1year' | 'forever' }));
                  setHasChanges(true);
                }}
                className="w-4 h-4 text-black"
              />
              <span className="text-sm font-medium">{label}</span>
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
        <div key={key} className="flex items-center justify-between p-4 rounded-lg border border-black/10 bg-white">
          <div className="flex items-center gap-3">
            <div className="w-8 h-8 rounded-lg bg-black/5 flex items-center justify-center">
              <Icon className="w-4 h-4 text-black/70" />
            </div>
            <div>
              <h4 className="text-sm font-medium text-black">{label}</h4>
              <p className="text-xs text-black/60">{description}</p>
            </div>
          </div>
          <button
            onClick={() => {
              setNotificationSettings(prev => ({ ...prev, [key]: !prev[key] }));
              setHasChanges(true);
            }}
            className={cn(
              'relative w-11 h-6 rounded-full transition-colors',
              notificationSettings[key] ? 'bg-black/70' : 'bg-black/15'
            )}
          >
            <div
              className={cn(
                'absolute top-0.5 w-5 h-5 bg-white rounded-full transition-transform',
                notificationSettings[key] ? 'translate-x-5' : 'translate-x-0.5'
              )}
            />
          </button>
        </div>
      ))}

      <div className="p-4 rounded-lg border border-black/10 bg-white">
        <div className="flex items-center justify-between mb-4">
          <div>
            <h4 className="text-sm font-medium text-black">Quiet Hours</h4>
            <p className="text-xs text-black/60">Disable notifications during these hours</p>
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
              notificationSettings.quietHours.enabled ? 'bg-black/70' : 'bg-black/15'
            )}
          >
            <div
              className={cn(
                'absolute top-0.5 w-5 h-5 bg-white rounded-full transition-transform',
                notificationSettings.quietHours.enabled ? 'translate-x-5' : 'translate-x-0.5'
              )}
            />
          </button>
        </div>

        {notificationSettings.quietHours.enabled && (
          <div className="grid grid-cols-2 gap-3">
            <div>
              <label className="block text-xs font-medium text-black/60 mb-1">Start Time</label>
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
                className="w-full p-2 border border-black/10 rounded-lg text-sm bg-white text-black"
              />
            </div>
            <div>
              <label className="block text-xs font-medium text-black/60 mb-1">End Time</label>
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
                className="w-full p-2 border border-black/10 rounded-lg text-sm bg-white text-black"
              />
            </div>
          </div>
        )}
      </div>
    </div>
  );

  const renderSecuritySettings = () => (
    <div className="space-y-6">
      {/* Two-Factor Auth */}
      <div className="flex items-center justify-between p-4 rounded-lg border border-black/10 bg-white">
        <div className="flex items-center gap-3">
          <div className="w-8 h-8 rounded-lg bg-black/5 flex items-center justify-center">
            <Lock className="w-4 h-4 text-black/70" />
          </div>
          <div>
            <h4 className="text-sm font-medium text-black">Two-Factor Authentication</h4>
            <p className="text-xs text-black/60">Require a second verification step on login</p>
          </div>
        </div>
        <button
          onClick={() => { setSecuritySettings(p => ({ ...p, twoFactorEnabled: !p.twoFactorEnabled })); setHasChanges(true); }}
          className={cn('relative w-11 h-6 rounded-full transition-colors', securitySettings.twoFactorEnabled ? 'bg-black/70' : 'bg-black/15')}
        >
          <div className={cn('absolute top-0.5 w-5 h-5 bg-white rounded-full transition-transform', securitySettings.twoFactorEnabled ? 'translate-x-5' : 'translate-x-0.5')} />
        </button>
      </div>

      {/* Session Timeout */}
      <div>
        <div className="flex items-center gap-2 mb-3">
          <Clock className="w-4 h-4 text-black/60" />
          <h4 className="text-sm font-medium text-black">Session Timeout</h4>
        </div>
        <div className="grid grid-cols-3 gap-2">
          {(['15min', '1hour', '4hours', '1day', 'never'] as const).map(v => (
            <button key={v}
              onClick={() => { setSecuritySettings(p => ({ ...p, sessionTimeout: v })); setHasChanges(true); }}
              className={cn('p-2 rounded-lg border-2 text-xs font-medium transition-all text-black hover:bg-black/5',
                securitySettings.sessionTimeout === v ? 'border-black/25 bg-black/5' : 'border-black/10')}
            >
              {v === '15min' ? '15 min' : v === '1hour' ? '1 hour' : v === '4hours' ? '4 hours' : v === '1day' ? '1 day' : 'Never'}
            </button>
          ))}
        </div>
      </div>

      {/* Toggles */}
      {[
        { key: 'loginAlerts' as const, label: 'Login Alerts', desc: 'Get notified when a new login is detected', icon: Bell },
        { key: 'trustedDevicesEnabled' as const, label: 'Trusted Devices', desc: 'Skip 2FA on devices you trust', icon: Smartphone },
      ].map(({ key, label, desc, icon: Icon }) => (
        <div key={key} className="flex items-center justify-between p-4 rounded-lg border border-black/10 bg-white">
          <div className="flex items-center gap-3">
            <div className="w-8 h-8 rounded-lg bg-black/5 flex items-center justify-center">
              <Icon className="w-4 h-4 text-black/70" />
            </div>
            <div>
              <h4 className="text-sm font-medium text-black">{label}</h4>
              <p className="text-xs text-black/60">{desc}</p>
            </div>
          </div>
          <button
            onClick={() => { setSecuritySettings(p => ({ ...p, [key]: !p[key] })); setHasChanges(true); }}
            className={cn('relative w-11 h-6 rounded-full transition-colors', securitySettings[key] ? 'bg-black/70' : 'bg-black/15')}
          >
            <div className={cn('absolute top-0.5 w-5 h-5 bg-white rounded-full transition-transform', securitySettings[key] ? 'translate-x-5' : 'translate-x-0.5')} />
          </button>
        </div>
      ))}
    </div>
  );

  const renderAccessibilitySettings = () => (
    <div className="space-y-4">
      {[
        { key: 'highContrast' as const, label: 'High Contrast', desc: 'Increase contrast for better readability', icon: Eye },
        { key: 'reducedMotion' as const, label: 'Reduced Motion', desc: 'Minimise animations and transitions', icon: Zap },
        { key: 'screenReaderOptimized' as const, label: 'Screen Reader Mode', desc: 'Optimise layout for assistive technology', icon: Monitor },
        { key: 'keyboardShortcutsEnabled' as const, label: 'Keyboard Shortcuts', desc: 'Enable keyboard navigation shortcuts', icon: ChevronRight },
      ].map(({ key, label, desc, icon: Icon }) => (
        <div key={key} className="flex items-center justify-between p-4 rounded-lg border border-black/10 bg-white">
          <div className="flex items-center gap-3">
            <div className="w-8 h-8 rounded-lg bg-black/5 flex items-center justify-center">
              <Icon className="w-4 h-4 text-black/70" />
            </div>
            <div>
              <h4 className="text-sm font-medium text-black">{label}</h4>
              <p className="text-xs text-black/60">{desc}</p>
            </div>
          </div>
          <button
            onClick={() => { setAccessibilitySettings(p => ({ ...p, [key]: !p[key] })); setHasChanges(true); }}
            className={cn('relative w-11 h-6 rounded-full transition-colors', accessibilitySettings[key] ? 'bg-black/70' : 'bg-black/15')}
          >
            <div className={cn('absolute top-0.5 w-5 h-5 bg-white rounded-full transition-transform', accessibilitySettings[key] ? 'translate-x-5' : 'translate-x-0.5')} />
          </button>
        </div>
      ))}
    </div>
  );

  const renderAISettings = () => (
    <div className="space-y-6">
      {/* AI enabled */}
      <div className="flex items-center justify-between p-4 rounded-lg border border-black/10 bg-white">
        <div className="flex items-center gap-3">
          <div className="w-8 h-8 rounded-lg bg-black/5 flex items-center justify-center">
            <Cpu className="w-4 h-4 text-black/70" />
          </div>
          <div>
            <h4 className="text-sm font-medium text-black">AI Assistant</h4>
            <p className="text-xs text-black/60">Enable Copilot AI features across the platform</p>
          </div>
        </div>
        <button
          onClick={() => { setAISettings(p => ({ ...p, aiEnabled: !p.aiEnabled })); setHasChanges(true); }}
          className={cn('relative w-11 h-6 rounded-full transition-colors', aiSettings.aiEnabled ? 'bg-black/70' : 'bg-black/15')}
        >
          <div className={cn('absolute top-0.5 w-5 h-5 bg-white rounded-full transition-transform', aiSettings.aiEnabled ? 'translate-x-5' : 'translate-x-0.5')} />
        </button>
      </div>

      {/* Model preference */}
      <div>
        <div className="flex items-center gap-2 mb-3">
          <Cpu className="w-4 h-4 text-black/60" />
          <h4 className="text-sm font-medium text-black">Model Preference</h4>
        </div>
        <div className="grid grid-cols-2 gap-2">
          {(['auto', 'gpt-4', 'gpt-4o', 'claude'] as const).map(v => (
            <button key={v}
              onClick={() => { setAISettings(p => ({ ...p, modelPreference: v })); setHasChanges(true); }}
              className={cn('p-3 rounded-lg border-2 text-sm font-medium transition-all text-black hover:bg-black/5',
                aiSettings.modelPreference === v ? 'border-black/25 bg-black/5' : 'border-black/10')}
            >
              {v === 'auto' ? 'Auto (recommended)' : v === 'gpt-4' ? 'GPT-4' : v === 'gpt-4o' ? 'GPT-4o' : 'Claude'}
              {aiSettings.modelPreference === v && <Check className="w-3 h-3 inline ml-2" />}
            </button>
          ))}
        </div>
      </div>

      {/* AI toggles */}
      {[
        { key: 'dataCollectionEnabled' as const, label: 'Data Collection', desc: 'Allow AI to learn from your interactions', icon: Database },
        { key: 'personalizationEnabled' as const, label: 'Personalisation', desc: 'Tailor AI responses to your work style', icon: User },
        { key: 'memoryEnabled' as const, label: 'AI Memory', desc: 'Remember context across sessions', icon: RefreshCw },
      ].map(({ key, label, desc, icon: Icon }) => (
        <div key={key} className="flex items-center justify-between p-4 rounded-lg border border-black/10 bg-white">
          <div className="flex items-center gap-3">
            <div className="w-8 h-8 rounded-lg bg-black/5 flex items-center justify-center">
              <Icon className="w-4 h-4 text-black/70" />
            </div>
            <div>
              <h4 className="text-sm font-medium text-black">{label}</h4>
              <p className="text-xs text-black/60">{desc}</p>
            </div>
          </div>
          <button
            onClick={() => { setAISettings(p => ({ ...p, [key]: !p[key] })); setHasChanges(true); }}
            className={cn('relative w-11 h-6 rounded-full transition-colors', aiSettings[key] ? 'bg-black/70' : 'bg-black/15')}
          >
            <div className={cn('absolute top-0.5 w-5 h-5 bg-white rounded-full transition-transform', aiSettings[key] ? 'translate-x-5' : 'translate-x-0.5')} />
          </button>
        </div>
      ))}
    </div>
  );

  const renderStorageSettings = () => (
    <div className="space-y-4">
      {[
        { key: 'autoSync' as const, label: 'Auto Sync', desc: 'Automatically sync data in the background', icon: RefreshCw },
        { key: 'clearCacheOnLogout' as const, label: 'Clear Cache on Logout', desc: 'Remove cached data when you sign out', icon: Database },
        { key: 'compressionEnabled' as const, label: 'Data Compression', desc: 'Compress data to reduce storage usage', icon: HardDrive },
        { key: 'offlineAccessEnabled' as const, label: 'Offline Access', desc: 'Enable limited functionality without network', icon: WifiOff },
      ].map(({ key, label, desc, icon: Icon }) => (
        <div key={key} className="flex items-center justify-between p-4 rounded-lg border border-black/10 bg-white">
          <div className="flex items-center gap-3">
            <div className="w-8 h-8 rounded-lg bg-black/5 flex items-center justify-center">
              <Icon className="w-4 h-4 text-black/70" />
            </div>
            <div>
              <h4 className="text-sm font-medium text-black">{label}</h4>
              <p className="text-xs text-black/60">{desc}</p>
            </div>
          </div>
          <button
            onClick={() => { setStorageSettings(p => ({ ...p, [key]: !p[key] })); setHasChanges(true); }}
            className={cn('relative w-11 h-6 rounded-full transition-colors', storageSettings[key] ? 'bg-black/70' : 'bg-black/15')}
          >
            <div className={cn('absolute top-0.5 w-5 h-5 bg-white rounded-full transition-transform', storageSettings[key] ? 'translate-x-5' : 'translate-x-0.5')} />
          </button>
        </div>
      ))}
    </div>
  );

  const renderTabContent = () => {
    switch (activeTab) {
      case 'appearance': return renderAppearanceSettings();
      case 'language': return renderLanguageSettings();
      case 'privacy': return renderPrivacySettings();
      case 'notifications': return renderNotificationSettings();
      case 'security': return renderSecuritySettings();
      case 'accessibility': return renderAccessibilitySettings();
      case 'ai': return renderAISettings();
      case 'storage': return renderStorageSettings();
      default: return null;
    }
  };

  return (
    <div className="h-full min-h-0 flex flex-col bg-[#FAFBFD] text-black">
      {/* Header */}
      <div className="flex items-center justify-between p-4 border-b border-black/10 bg-white">
        <div className="flex items-center gap-3">
          <div className="w-8 h-8 rounded-lg bg-black/5 flex items-center justify-center">
            <SettingsIcon className="w-5 h-5 text-black/80" />
          </div>
          <div>
            <h2 className="text-lg font-semibold text-black">Settings</h2>
            <p className="text-sm text-black/60">Manage your preferences</p>
          </div>
        </div>
        {onClose && (
          <button
            onClick={onClose}
            className="w-8 h-8 rounded-lg bg-black/5 flex items-center justify-center hover:bg-black/10 transition-colors text-black/70"
          >
            <X className="w-4 h-4" />
          </button>
        )}
      </div>

      <div className="flex-1 min-h-0 flex overflow-hidden">
        {/* Sidebar */}
        <div className="w-48 border-r border-black/10 p-4 bg-white">
          <div className="space-y-1">
            {settingsTabs.map((tab) => (
              <button
                key={tab.id}
                onClick={() => setActiveTab(tab.id)}
                className={cn(
                  'w-full flex items-center gap-3 p-3 rounded-lg text-left transition-colors text-black hover:bg-black/5 group',
                  activeTab === tab.id
                    ? 'bg-black/5 text-black'
                    : 'hover:bg-black/5'
                )}
              >
                <tab.icon className="w-4 h-4 text-black/70" />
                <div className="flex-1 min-w-0">
                  <div className="text-sm font-medium">{tab.label}</div>
                  <div className="text-xs text-black/55 truncate">{tab.description}</div>
                </div>
                {activeTab === tab.id && <ChevronRight className="w-4 h-4 text-black/60" />}
              </button>
            ))}
          </div>
        </div>

        {/* Content */}
        <div className="flex-1 min-h-0 flex flex-col overflow-hidden">
          <div className="flex-1 p-6 overflow-y-auto">
            <div className="max-w-2xl">
              {/* Loading state */}
              {((activeTab === 'appearance' && appearanceLoading) ||
                (activeTab === 'language' && languageLoading) ||
                (activeTab === 'privacy' && privacyLoading) ||
                (activeTab === 'notifications' && notificationLoading) ||
                (activeTab === 'security' && securityLoading) ||
                (activeTab === 'accessibility' && accessibilityLoading) ||
                (activeTab === 'ai' && aiLoading) ||
                (activeTab === 'storage' && storageLoading)) && (
                <div className="flex items-center justify-center py-12">
                  <Loader2 className="w-6 h-6 animate-spin text-black/40" />
                  <span className="ml-2 text-sm text-black/60">Loading settings...</span>
                </div>
              )}

              {/* Content */}
              {!((activeTab === 'appearance' && appearanceLoading) ||
                 (activeTab === 'language' && languageLoading) ||
                 (activeTab === 'privacy' && privacyLoading) ||
                 (activeTab === 'notifications' && notificationLoading) ||
                 (activeTab === 'security' && securityLoading) ||
                 (activeTab === 'accessibility' && accessibilityLoading) ||
                 (activeTab === 'ai' && aiLoading) ||
                 (activeTab === 'storage' && storageLoading)) &&
                 renderTabContent()}
            </div>
          </div>

          {/* Footer Actions */}
          {hasChanges && (
            <div className="border-t border-black/10 p-4 bg-white">
              <div className="flex items-center justify-between">
                <p className="text-sm text-black/60">You have unsaved changes</p>
                <div className="flex items-center gap-3">
                  <button
                    onClick={handleReset}
                    disabled={updateAppearanceMutation.isPending || updateLanguageMutation.isPending ||
                             updatePrivacyMutation.isPending || updateNotificationMutation.isPending ||
                             updateSecurityMutation.isPending || updateAccessibilityMutation.isPending ||
                             updateAIMutation.isPending || updateStorageMutation.isPending}
                    className="flex items-center gap-2 px-3 py-2 text-sm text-black/60 hover:text-black hover:bg-black/5 rounded-lg transition-colors disabled:opacity-50"
                  >
                    <RotateCcw className="w-4 h-4" />
                    Reset
                  </button>
                  <button
                    onClick={handleSave}
                    disabled={updateAppearanceMutation.isPending || updateLanguageMutation.isPending ||
                             updatePrivacyMutation.isPending || updateNotificationMutation.isPending ||
                             updateSecurityMutation.isPending || updateAccessibilityMutation.isPending ||
                             updateAIMutation.isPending || updateStorageMutation.isPending}
                    className="flex items-center gap-2 px-4 py-2 bg-black text-white text-sm font-medium rounded-lg hover:bg-black/85 transition-colors disabled:opacity-50"
                  >
                    {(updateAppearanceMutation.isPending || updateLanguageMutation.isPending ||
                      updatePrivacyMutation.isPending || updateNotificationMutation.isPending ||
                      updateSecurityMutation.isPending || updateAccessibilityMutation.isPending ||
                      updateAIMutation.isPending || updateStorageMutation.isPending) ? (
                      <Loader2 className="w-4 h-4 animate-spin" />
                    ) : (
                      <Save className="w-4 h-4" />
                    )}
                    {(updateAppearanceMutation.isPending || updateLanguageMutation.isPending ||
                      updatePrivacyMutation.isPending || updateNotificationMutation.isPending ||
                      updateSecurityMutation.isPending || updateAccessibilityMutation.isPending ||
                      updateAIMutation.isPending || updateStorageMutation.isPending) ? 'Saving...' : 'Save Changes'}
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
