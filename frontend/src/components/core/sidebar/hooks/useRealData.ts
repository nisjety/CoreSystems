import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { chatApiClient } from '@/components/chat/api/orpc/chat';
import { chatQueryKeys, removeChatHistorySession } from '@/components/chat/lib/query-state';

// Type definitions for settings
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

const DEFAULT_APPEARANCE_SETTINGS: AppearanceSettings = {
  theme: 'light',
  colorScheme: 'blue',
  fontSize: 'medium',
  compactMode: false,
};

const DEFAULT_LANGUAGE_SETTINGS: LanguageSettings = {
  language: 'en-US',
  region: 'US',
  dateFormat: 'MM/DD/YYYY',
  timeFormat: '12h',
};

const DEFAULT_PRIVACY_SETTINGS: PrivacySettings = {
  shareStatus: true,
  shareActivity: false,
  allowAnalytics: true,
  dataRetention: '1year',
};

const DEFAULT_NOTIFICATION_SETTINGS: NotificationSettings = {
  emailNotifications: true,
  pushNotifications: true,
  teamsNotifications: true,
  calendarReminders: true,
  quietHours: {
    enabled: false,
    start: '22:00',
    end: '08:00',
  },
};

async function fetchSettingsWithFallback<T>(url: string, fallback: T): Promise<T> {
  const response = await fetch(url, { credentials: 'include' });

  if (response.ok) {
    return response.json();
  }

  if (response.status === 404) {
    return fallback;
  }

  const error = new Error(`Request failed: ${response.status}`) as Error & { status?: number };
  error.status = response.status;
  throw error;
}

// Utility function to get the correct API base URL based on environment
function getApiBaseUrl(): string {
  // In Docker environment, use API routes that proxy to internal services
  // In local development, could also use API routes or direct service URLs
  return '';  // Use relative URLs to hit Next.js API routes
}

export const useRealData = (options?: { enabled?: boolean }) => {
  const apiBaseUrl = getApiBaseUrl();
  const enabled = options?.enabled !== false;

  const { data: users, isLoading: usersLoading, error: usersError } = useQuery({
    queryKey: ['users'],
    queryFn: async () => {
      const response = await fetch(`${apiBaseUrl}/user/users`);
      if (!response.ok) {
        throw new Error('Failed to fetch users');
      }
      return response.json();
    },
    enabled: enabled && typeof window !== 'undefined',
    retry: (failureCount, error: unknown) => {
      const errorStatus = (error as { status?: number })?.status;
      if (errorStatus === 401 || errorStatus === 403) {
        return false;
      }
      return failureCount < 3;
    },
    refetchOnWindowFocus: false,
    refetchOnMount: false,
    refetchInterval: false
  });

  const { data: hrData, isLoading: hrLoading, error: hrError } = useQuery({
    queryKey: ['hr'],
    queryFn: async () => {
      const response = await fetch(`${apiBaseUrl}/hr/overview`);
      if (!response.ok) {
        throw new Error('Failed to fetch HR data');
      }
      return response.json();
    },
    enabled: enabled && typeof window !== 'undefined',
    retry: (failureCount, error: unknown) => {
      const errorStatus = (error as { status?: number })?.status;
      if (errorStatus === 401 || errorStatus === 403) {
        return false;
      }
      return failureCount < 3;
    },
    refetchOnWindowFocus: false,
    refetchOnMount: false,
    refetchInterval: false
  });

  const { data: assets, isLoading: assetsLoading, error: assetsError } = useQuery({
    queryKey: ['assets'],
    queryFn: async () => {
      const response = await fetch(`${apiBaseUrl}/assets/overview`);
      if (!response.ok) {
        throw new Error('Failed to fetch assets data');
      }
      return response.json();
    },
    enabled: enabled && typeof window !== 'undefined',
    retry: (failureCount, error: unknown) => {
      const errorStatus = (error as { status?: number })?.status;
      if (errorStatus === 401 || errorStatus === 403) {
        return false;
      }
      return failureCount < 3;
    },
    refetchOnWindowFocus: false,
    refetchOnMount: false,
    refetchInterval: false
  });

  const { data: procurementData, isLoading: procurementLoading, error: procurementError } = useQuery({
    queryKey: ['procurement'],
    queryFn: async () => {
      const response = await fetch(`${apiBaseUrl}/procurement/overview`);
      if (!response.ok) {
        throw new Error('Failed to fetch procurement data');
      }
      return response.json();
    },
    enabled: enabled && typeof window !== 'undefined',
    retry: (failureCount, error: unknown) => {
      const errorStatus = (error as { status?: number })?.status;
      if (errorStatus === 401 || errorStatus === 403) {
        return false;
      }
      return failureCount < 3;
    },
    refetchOnWindowFocus: false,
    refetchOnMount: false,
    refetchInterval: false
  });

  const { data: timebankData, isLoading: timebankLoading, error: timebankError } = useQuery({
    queryKey: ['timebank'],
    queryFn: async () => {
      const response = await fetch(`${apiBaseUrl}/timebank/overview`);
      if (!response.ok) {
        throw new Error('Failed to fetch timebank data');
      }
      return response.json();
    },
    enabled: enabled && typeof window !== 'undefined',
    retry: (failureCount, error: unknown) => {
      const errorStatus = (error as { status?: number })?.status;
      if (errorStatus === 401 || errorStatus === 403) {
        return false;
      }
      return failureCount < 3;
    },
    refetchOnWindowFocus: false,
    refetchOnMount: false,
    refetchInterval: false
  });

  const { data: corebarData, isLoading: corebarLoading, error: corebarError } = useQuery({
    queryKey: ['corebar'],
    queryFn: async () => {
      const response = await fetch(`${apiBaseUrl}/corebar/overview`);
      if (!response.ok) {
        throw new Error('Failed to fetch corebar data');
      }
      return response.json();
    },
    enabled: enabled && typeof window !== 'undefined',
    retry: (failureCount, error: unknown) => {
      const errorStatus = (error as { status?: number })?.status;
      if (errorStatus === 401 || errorStatus === 403) {
        return false;
      }
      return failureCount < 3;
    },
    refetchOnWindowFocus: false,
    refetchOnMount: false,
    refetchInterval: false
  });

  return {
    users,
    usersLoading,
    usersError,
    hrData,
    hrLoading,
    hrError,
    assets,
    assetsLoading,
    assetsError,
    procurementData,
    procurementLoading,
    procurementError,
    timebankData,
    timebankLoading,
    timebankError,
    corebarData,
    corebarLoading,
    corebarError,
  };
};

// User hooks
export const useUser = (options?: { enabled?: boolean }) => {
  const apiBaseUrl = getApiBaseUrl();
  const enabled = options?.enabled !== false;
  
  return useQuery({
    queryKey: ['user', 'current'],
    queryFn: async () => {
      const response = await fetch(`${apiBaseUrl}/api/user/current`, {
        credentials: 'include',
      });
      if (!response.ok) {
        throw new Error('Failed to fetch user');
      }
      return response.json();
    },
    enabled: enabled && typeof window !== 'undefined',
    retry: (failureCount, error: unknown) => {
      const errorStatus = (error as { status?: number })?.status;
      if (errorStatus === 401 || errorStatus === 403) {
        return false;
      }
      return failureCount < 3;
    },
    refetchOnWindowFocus: false,
    refetchOnMount: false,
    refetchInterval: false
  });
};

// Message hooks
export const useMessages = (options?: { enabled?: boolean }) => {
  const enabled = options?.enabled !== false;
  
  return useQuery({
    queryKey: chatQueryKeys.legacyMessages,
    queryFn: async () => {
      return chatApiClient.getSessions();
    },
    enabled: enabled && typeof window !== 'undefined',
    staleTime: 0,
    refetchOnMount: 'always',
    retry: (failureCount, error: unknown) => {
      const errorStatus = (error as { status?: number })?.status;
      if (errorStatus === 401 || errorStatus === 403) {
        return false;
      }
      return failureCount < 3;
    },
    refetchOnWindowFocus: false,
    refetchOnMount: false,
    refetchInterval: false
  });
};

export const useMarkMessageAsRead = () => {
  const queryClient = useQueryClient();
  
  return useMutation({
    mutationFn: async (messageId: string) => {
      const response = await fetch(`${getApiBaseUrl()}/chat/sessions/${messageId}/read`, {
        method: 'POST',
      });
      if (!response.ok) {
        throw new Error('Failed to mark message as read');
      }
      return response.json();
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: chatQueryKeys.legacyMessages });
    }
  });
};

// Calendar hooks
export const useCalendarEvents = (options?: { enabled?: boolean }) => {
  const apiBaseUrl = getApiBaseUrl();
  const enabled = options?.enabled !== false;
  
  return useQuery({
    queryKey: ['calendar', 'events'],
    queryFn: async () => {
      const response = await fetch(`${apiBaseUrl}/user/calendar/events`);
      if (!response.ok) {
        throw new Error('Failed to fetch calendar events');
      }
      return response.json();
    },
    enabled: enabled && typeof window !== 'undefined',
    retry: (failureCount, error: unknown) => {
      const errorStatus = (error as { status?: number })?.status;
      if (errorStatus === 401 || errorStatus === 403) {
        return false;
      }
      return failureCount < 3;
    },
    refetchOnWindowFocus: false,
    refetchOnMount: false,
    refetchInterval: false
  });
};

// Notification hooks
export const useNotifications = (options?: { enabled?: boolean }) => {
  const apiBaseUrl = getApiBaseUrl();
  const enabled = options?.enabled !== false;
  
  return useQuery({
    queryKey: ['notifications'],
    queryFn: async () => {
      const response = await fetch(`${apiBaseUrl}/user/notifications`);
      if (!response.ok) {
        throw new Error('Failed to fetch notifications');
      }
      return response.json();
    },
    enabled: enabled && typeof window !== 'undefined',
    retry: (failureCount, error: unknown) => {
      const errorStatus = (error as { status?: number })?.status;
      if (errorStatus === 401 || errorStatus === 403) {
        return false;
      }
      return failureCount < 3;
    },
    refetchOnWindowFocus: false,
    refetchOnMount: false,
    refetchInterval: false
  });
};

export const useUnreadNotifications = (options?: { enabled?: boolean }) => {
  const apiBaseUrl = getApiBaseUrl();
  const enabled = options?.enabled !== false;
  
  return useQuery({
    queryKey: ['notifications', 'unread'],
    queryFn: async () => {
      const response = await fetch(`${apiBaseUrl}/user/notifications/unread`);
      if (!response.ok) {
        throw new Error('Failed to fetch unread notifications');
      }
      return response.json();
    },
    enabled: enabled && typeof window !== 'undefined',
    retry: (failureCount, error: unknown) => {
      const errorStatus = (error as { status?: number })?.status;
      if (errorStatus === 401 || errorStatus === 403) {
        return false;
      }
      return failureCount < 3;
    },
    refetchOnWindowFocus: false,
    refetchOnMount: false,
    refetchInterval: false
  });
};

export const useMarkNotificationAsRead = () => {
  const queryClient = useQueryClient();
  
  return useMutation({
    mutationFn: async (notificationId: string) => {
      const response = await fetch(`${getApiBaseUrl()}/user/notifications/${notificationId}/read`, {
        method: 'POST',
      });
      if (!response.ok) {
        throw new Error('Failed to mark notification as read');
      }
      return response.json();
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['notifications'] });
      queryClient.invalidateQueries({ queryKey: ['notifications', 'unread'] });
    }
  });
};

export const useClearAllNotifications = () => {
  const queryClient = useQueryClient();
  
  return useMutation({
    mutationFn: async () => {
      const response = await fetch(`${getApiBaseUrl()}/user/notifications/clear`, {
        method: 'POST',
      });
      if (!response.ok) {
        throw new Error('Failed to clear notifications');
      }
      return response.json();
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['notifications'] });
      queryClient.invalidateQueries({ queryKey: ['notifications', 'unread'] });
    }
  });
};

// Chat history hooks
export const useChatHistory = (options?: { enabled?: boolean }) => {
  const enabled = options?.enabled !== false;
  
  return useQuery({
    queryKey: chatQueryKeys.history,
    queryFn: async () => {
      return chatApiClient.getSessions();
    },
    enabled: enabled && typeof window !== 'undefined',
    staleTime: 0,
    refetchOnMount: 'always',
    retry: (failureCount, error: unknown) => {
      const errorStatus = (error as { status?: number })?.status;
      if (errorStatus === 401 || errorStatus === 403) {
        return false;
      }
      return failureCount < 3;
    },
    refetchOnWindowFocus: false,
    refetchOnMount: false,
    refetchInterval: false
  });
};

export const usePinChat = () => {
  const queryClient = useQueryClient();
  
  return useMutation({
    mutationFn: async ({ chatId, isPinned }: { chatId: string; isPinned: boolean }) => {
      const response = await fetch(`${getApiBaseUrl()}/chat/sessions/${chatId}/pin`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ isPinned }),
      });
      if (!response.ok) {
        throw new Error('Failed to pin chat');
      }
      return response.json();
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: chatQueryKeys.history });
    }
  });
};

export const useArchiveChat = () => {
  const queryClient = useQueryClient();
  
  return useMutation({
    mutationFn: async (chatId: string) => {
      const response = await fetch(`${getApiBaseUrl()}/chat/sessions/${chatId}/archive`, {
        method: 'POST',
      });
      if (!response.ok) {
        throw new Error('Failed to archive chat');
      }
      return response.json();
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: chatQueryKeys.history });
    }
  });
};

export const useDeleteChat = () => {
  const queryClient = useQueryClient();
  
  return useMutation({
    mutationFn: async (chatId: string) => {
      await chatApiClient.deleteSession(chatId);
      return { deleted: true };
    },
    onSuccess: (_result, chatId) => {
      removeChatHistorySession(queryClient, chatId);
      queryClient.invalidateQueries({ queryKey: chatQueryKeys.history });
    }
  });
};

// Settings hooks
export const useAppearanceSettings = (options?: { enabled?: boolean }) => {
  const apiBaseUrl = getApiBaseUrl();
  const enabled = options?.enabled !== false;
  
  return useQuery({
    queryKey: ['settings', 'appearance'],
    queryFn: async () => fetchSettingsWithFallback(`${apiBaseUrl}/api/user/settings/appearance`, DEFAULT_APPEARANCE_SETTINGS),
    enabled: enabled && typeof window !== 'undefined',
    retry: (failureCount, error: unknown) => {
      const errorStatus = (error as { status?: number })?.status;
      if (errorStatus === 401 || errorStatus === 403) {
        return false;
      }
      return failureCount < 3;
    },
    refetchOnWindowFocus: false,
    refetchOnMount: false,
    refetchInterval: false
  });
};

export const useUpdateAppearanceSettings = () => {
  const queryClient = useQueryClient();
  
  return useMutation({
    mutationFn: async (settings: AppearanceSettings) => {
      const response = await fetch(`${getApiBaseUrl()}/api/user/settings/appearance`, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        credentials: 'include',
        body: JSON.stringify(settings),
      });
      if (response.status === 404) {
        return settings;
      }
      if (!response.ok) {
        throw new Error('Failed to update appearance settings');
      }
      return response.json();
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['settings', 'appearance'] });
    }
  });
};

export const useLanguageSettings = (options?: { enabled?: boolean }) => {
  const apiBaseUrl = getApiBaseUrl();
  const enabled = options?.enabled !== false;
  
  return useQuery({
    queryKey: ['settings', 'language'],
    queryFn: async () => fetchSettingsWithFallback(`${apiBaseUrl}/api/user/settings/language`, DEFAULT_LANGUAGE_SETTINGS),
    enabled: enabled && typeof window !== 'undefined',
    retry: (failureCount, error: unknown) => {
      const errorStatus = (error as { status?: number })?.status;
      if (errorStatus === 401 || errorStatus === 403) {
        return false;
      }
      return failureCount < 3;
    },
    refetchOnWindowFocus: false,
    refetchOnMount: false,
    refetchInterval: false
  });
};

export const useUpdateLanguageSettings = () => {
  const queryClient = useQueryClient();
  
  return useMutation({
    mutationFn: async (settings: LanguageSettings) => {
      const response = await fetch(`${getApiBaseUrl()}/api/user/settings/language`, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        credentials: 'include',
        body: JSON.stringify(settings),
      });
      if (response.status === 404) {
        return settings;
      }
      if (!response.ok) {
        throw new Error('Failed to update language settings');
      }
      return response.json();
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['settings', 'language'] });
    }
  });
};

export const usePrivacySettings = (options?: { enabled?: boolean }) => {
  const apiBaseUrl = getApiBaseUrl();
  const enabled = options?.enabled !== false;
  
  return useQuery({
    queryKey: ['settings', 'privacy'],
    queryFn: async () => fetchSettingsWithFallback(`${apiBaseUrl}/api/user/settings/privacy`, DEFAULT_PRIVACY_SETTINGS),
    enabled: enabled && typeof window !== 'undefined',
    retry: (failureCount, error: unknown) => {
      const errorStatus = (error as { status?: number })?.status;
      if (errorStatus === 401 || errorStatus === 403) {
        return false;
      }
      return failureCount < 3;
    },
    refetchOnWindowFocus: false,
    refetchOnMount: false,
    refetchInterval: false
  });
};

export const useUpdatePrivacySettings = () => {
  const queryClient = useQueryClient();
  
  return useMutation({
    mutationFn: async (settings: PrivacySettings) => {
      const response = await fetch(`${getApiBaseUrl()}/api/user/settings/privacy`, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        credentials: 'include',
        body: JSON.stringify(settings),
      });
      if (response.status === 404) {
        return settings;
      }
      if (!response.ok) {
        throw new Error('Failed to update privacy settings');
      }
      return response.json();
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['settings', 'privacy'] });
    }
  });
};

export const useNotificationSettings = (options?: { enabled?: boolean }) => {
  const apiBaseUrl = getApiBaseUrl();
  const enabled = options?.enabled !== false;
  
  return useQuery({
    queryKey: ['settings', 'notifications'],
    queryFn: async () => fetchSettingsWithFallback(`${apiBaseUrl}/api/user/settings/notifications`, DEFAULT_NOTIFICATION_SETTINGS),
    enabled: enabled && typeof window !== 'undefined',
    retry: (failureCount, error: unknown) => {
      const errorStatus = (error as { status?: number })?.status;
      if (errorStatus === 401 || errorStatus === 403) {
        return false;
      }
      return failureCount < 3;
    },
    refetchOnWindowFocus: false,
    refetchOnMount: false,
    refetchInterval: false
  });
};

export const useUpdateNotificationSettings = () => {
  const queryClient = useQueryClient();
  
  return useMutation({
    mutationFn: async (settings: NotificationSettings) => {
      const response = await fetch(`${getApiBaseUrl()}/api/user/settings/notifications`, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        credentials: 'include',
        body: JSON.stringify(settings),
      });
      if (response.status === 404) {
        return settings;
      }
      if (!response.ok) {
        throw new Error('Failed to update notification settings');
      }
      return response.json();
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['settings', 'notifications'] });
    }
  });
};

// Graph hooks for visualization data
// ─── Security settings ───────────────────────────────────────────────────────
export interface SecuritySettings {
  twoFactorEnabled: boolean;
  sessionTimeout: '15min' | '1hour' | '4hours' | '1day' | 'never';
  loginAlerts: boolean;
  trustedDevicesEnabled: boolean;
}

const DEFAULT_SECURITY_SETTINGS: SecuritySettings = {
  twoFactorEnabled: false,
  sessionTimeout: '4hours',
  loginAlerts: true,
  trustedDevicesEnabled: true,
};

export const useSecuritySettings = (options?: { enabled?: boolean }) => {
  const enabled = options?.enabled !== false;
  return useQuery({
    queryKey: ['settings', 'security'],
    queryFn: () => fetchSettingsWithFallback(`${getApiBaseUrl()}/api/user/settings/security`, DEFAULT_SECURITY_SETTINGS),
    enabled: enabled && typeof window !== 'undefined',
    retry: false,
    refetchOnWindowFocus: false,
    refetchOnMount: false,
  });
};

export const useUpdateSecuritySettings = () => {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async (settings: SecuritySettings) => {
      const response = await fetch(`${getApiBaseUrl()}/api/user/settings/security`, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        credentials: 'include',
        body: JSON.stringify(settings),
      });
      if (response.status === 404) return settings;
      if (!response.ok) throw new Error('Failed to update security settings');
      return response.json();
    },
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['settings', 'security'] }),
  });
};

// ─── Accessibility settings ───────────────────────────────────────────────────
export interface AccessibilitySettings {
  highContrast: boolean;
  reducedMotion: boolean;
  screenReaderOptimized: boolean;
  keyboardShortcutsEnabled: boolean;
}

const DEFAULT_ACCESSIBILITY_SETTINGS: AccessibilitySettings = {
  highContrast: false,
  reducedMotion: false,
  screenReaderOptimized: false,
  keyboardShortcutsEnabled: true,
};

export const useAccessibilitySettings = (options?: { enabled?: boolean }) => {
  const enabled = options?.enabled !== false;
  return useQuery({
    queryKey: ['settings', 'accessibility'],
    queryFn: () => fetchSettingsWithFallback(`${getApiBaseUrl()}/api/user/settings/accessibility`, DEFAULT_ACCESSIBILITY_SETTINGS),
    enabled: enabled && typeof window !== 'undefined',
    retry: false,
    refetchOnWindowFocus: false,
    refetchOnMount: false,
  });
};

export const useUpdateAccessibilitySettings = () => {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async (settings: AccessibilitySettings) => {
      const response = await fetch(`${getApiBaseUrl()}/api/user/settings/accessibility`, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        credentials: 'include',
        body: JSON.stringify(settings),
      });
      if (response.status === 404) return settings;
      if (!response.ok) throw new Error('Failed to update accessibility settings');
      return response.json();
    },
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['settings', 'accessibility'] }),
  });
};

// ─── AI settings ─────────────────────────────────────────────────────────────
export interface AISettings {
  aiEnabled: boolean;
  modelPreference: 'auto' | 'gpt-4' | 'gpt-4o' | 'claude';
  dataCollectionEnabled: boolean;
  personalizationEnabled: boolean;
  memoryEnabled: boolean;
}

const DEFAULT_AI_SETTINGS: AISettings = {
  aiEnabled: true,
  modelPreference: 'auto',
  dataCollectionEnabled: true,
  personalizationEnabled: true,
  memoryEnabled: false,
};

export const useAISettings = (options?: { enabled?: boolean }) => {
  const enabled = options?.enabled !== false;
  return useQuery({
    queryKey: ['settings', 'ai'],
    queryFn: () => fetchSettingsWithFallback(`${getApiBaseUrl()}/api/user/settings/ai`, DEFAULT_AI_SETTINGS),
    enabled: enabled && typeof window !== 'undefined',
    retry: false,
    refetchOnWindowFocus: false,
    refetchOnMount: false,
  });
};

export const useUpdateAISettings = () => {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async (settings: AISettings) => {
      const response = await fetch(`${getApiBaseUrl()}/api/user/settings/ai`, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        credentials: 'include',
        body: JSON.stringify(settings),
      });
      if (response.status === 404) return settings;
      if (!response.ok) throw new Error('Failed to update AI settings');
      return response.json();
    },
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['settings', 'ai'] }),
  });
};

// ─── Storage settings ─────────────────────────────────────────────────────────
export interface StorageSettings {
  autoSync: boolean;
  clearCacheOnLogout: boolean;
  compressionEnabled: boolean;
  offlineAccessEnabled: boolean;
}

const DEFAULT_STORAGE_SETTINGS: StorageSettings = {
  autoSync: true,
  clearCacheOnLogout: false,
  compressionEnabled: true,
  offlineAccessEnabled: false,
};

export const useStorageSettings = (options?: { enabled?: boolean }) => {
  const enabled = options?.enabled !== false;
  return useQuery({
    queryKey: ['settings', 'storage'],
    queryFn: () => fetchSettingsWithFallback(`${getApiBaseUrl()}/api/user/settings/storage`, DEFAULT_STORAGE_SETTINGS),
    enabled: enabled && typeof window !== 'undefined',
    retry: false,
    refetchOnWindowFocus: false,
    refetchOnMount: false,
  });
};

export const useUpdateStorageSettings = () => {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async (settings: StorageSettings) => {
      const response = await fetch(`${getApiBaseUrl()}/api/user/settings/storage`, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        credentials: 'include',
        body: JSON.stringify(settings),
      });
      if (response.status === 404) return settings;
      if (!response.ok) throw new Error('Failed to update storage settings');
      return response.json();
    },
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['settings', 'storage'] }),
  });
};

export const useGraphProfile = (options?: { enabled?: boolean }) => {
  const apiBaseUrl = getApiBaseUrl();
  const enabled = options?.enabled !== false;
  
  return useQuery({
    queryKey: ['graph', 'profile'],
    queryFn: async () => {
      const response = await fetch(`${apiBaseUrl}/user/graph/profile`);
      if (!response.ok) {
        throw new Error('Failed to fetch profile graph data');
      }
      return response.json();
    },
    enabled: enabled && typeof window !== 'undefined',
    retry: (failureCount, error: unknown) => {
      const errorStatus = (error as { status?: number })?.status;
      if (errorStatus === 401 || errorStatus === 403) {
        return false;
      }
      return failureCount < 3;
    },
    refetchOnWindowFocus: false,
    refetchOnMount: false,
    refetchInterval: false
  });
};

export const useGraphMessages = (options?: { enabled?: boolean }) => {
  const apiBaseUrl = getApiBaseUrl();
  const enabled = options?.enabled !== false;
  
  return useQuery({
    queryKey: ['graph', 'messages'],
    queryFn: async () => {
      const response = await fetch(`${apiBaseUrl}/chat/graph/messages`);
      if (!response.ok) {
        throw new Error('Failed to fetch messages graph data');
      }
      return response.json();
    },
    enabled: enabled && typeof window !== 'undefined',
    retry: (failureCount, error: unknown) => {
      const errorStatus = (error as { status?: number })?.status;
      if (errorStatus === 401 || errorStatus === 403) {
        return false;
      }
      return failureCount < 3;
    },
    refetchOnWindowFocus: false,
    refetchOnMount: false,
    refetchInterval: false
  });
};

export const useGraphCalendar = (options?: { enabled?: boolean }) => {
  const apiBaseUrl = getApiBaseUrl();
  const enabled = options?.enabled !== false;
  
  return useQuery({
    queryKey: ['graph', 'calendar'],
    queryFn: async () => {
      const response = await fetch(`${apiBaseUrl}/user/graph/calendar`);
      if (!response.ok) {
        throw new Error('Failed to fetch calendar graph data');
      }
      return response.json();
    },
    enabled: enabled && typeof window !== 'undefined',
    retry: (failureCount, error: unknown) => {
      const errorStatus = (error as { status?: number })?.status;
      if (errorStatus === 401 || errorStatus === 403) {
        return false;
      }
      return failureCount < 3;
    },
    refetchOnWindowFocus: false,
    refetchOnMount: false,
    refetchInterval: false
  });
};

export const useGraphChats = (options?: { enabled?: boolean }) => {
  const apiBaseUrl = getApiBaseUrl();
  const enabled = options?.enabled !== false;
  
  return useQuery({
    queryKey: ['graph', 'chats'],
    queryFn: async () => {
      const response = await fetch(`${apiBaseUrl}/chat/graph/chats`);
      if (!response.ok) {
        throw new Error('Failed to fetch chats graph data');
      }
      return response.json();
    },
    enabled: enabled && typeof window !== 'undefined',
    retry: (failureCount, error: unknown) => {
      const errorStatus = (error as { status?: number })?.status;
      if (errorStatus === 401 || errorStatus === 403) {
        return false;
      }
      return failureCount < 3;
    },
    refetchOnWindowFocus: false,
    refetchOnMount: false,
    refetchInterval: false
  });
};
