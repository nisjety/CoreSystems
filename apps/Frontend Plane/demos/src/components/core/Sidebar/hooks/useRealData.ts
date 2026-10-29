import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';

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
      const response = await fetch(`${apiBaseUrl}/user/me`);
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
  const apiBaseUrl = getApiBaseUrl();
  const enabled = options?.enabled !== false;
  
  return useQuery({
    queryKey: ['messages'],
    queryFn: async () => {
      const response = await fetch(`${apiBaseUrl}/chat/sessions`);
      if (!response.ok) {
        throw new Error('Failed to fetch messages');
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
      queryClient.invalidateQueries({ queryKey: ['messages'] });
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
  const apiBaseUrl = getApiBaseUrl();
  const enabled = options?.enabled !== false;
  
  return useQuery({
    queryKey: ['chat', 'history'],
    queryFn: async () => {
      const response = await fetch(`${apiBaseUrl}/chat/sessions`);
      if (!response.ok) {
        throw new Error('Failed to fetch chat history');
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
      queryClient.invalidateQueries({ queryKey: ['chat', 'history'] });
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
      queryClient.invalidateQueries({ queryKey: ['chat', 'history'] });
    }
  });
};

export const useDeleteChat = () => {
  const queryClient = useQueryClient();
  
  return useMutation({
    mutationFn: async (chatId: string) => {
      const response = await fetch(`${getApiBaseUrl()}/chat/sessions/${chatId}`, {
        method: 'DELETE',
      });
      if (!response.ok) {
        throw new Error('Failed to delete chat');
      }
      return response.json();
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['chat', 'history'] });
    }
  });
};

// Settings hooks
export const useAppearanceSettings = (options?: { enabled?: boolean }) => {
  const apiBaseUrl = getApiBaseUrl();
  const enabled = options?.enabled !== false;
  
  return useQuery({
    queryKey: ['settings', 'appearance'],
    queryFn: async () => {
      const response = await fetch(`${apiBaseUrl}/user/settings/appearance`);
      if (!response.ok) {
        throw new Error('Failed to fetch appearance settings');
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

export const useUpdateAppearanceSettings = () => {
  const queryClient = useQueryClient();
  
  return useMutation({
    mutationFn: async (settings: AppearanceSettings) => {
      const response = await fetch(`${getApiBaseUrl()}/user/settings/appearance`, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(settings),
      });
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
    queryFn: async () => {
      const response = await fetch(`${apiBaseUrl}/user/settings/language`);
      if (!response.ok) {
        throw new Error('Failed to fetch language settings');
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

export const useUpdateLanguageSettings = () => {
  const queryClient = useQueryClient();
  
  return useMutation({
    mutationFn: async (settings: LanguageSettings) => {
      const response = await fetch(`${getApiBaseUrl()}/user/settings/language`, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(settings),
      });
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
    queryFn: async () => {
      const response = await fetch(`${apiBaseUrl}/user/settings/privacy`);
      if (!response.ok) {
        throw new Error('Failed to fetch privacy settings');
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

export const useUpdatePrivacySettings = () => {
  const queryClient = useQueryClient();
  
  return useMutation({
    mutationFn: async (settings: PrivacySettings) => {
      const response = await fetch(`${getApiBaseUrl()}/user/settings/privacy`, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(settings),
      });
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
    queryFn: async () => {
      const response = await fetch(`${apiBaseUrl}/user/settings/notifications`);
      if (!response.ok) {
        throw new Error('Failed to fetch notification settings');
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

export const useUpdateNotificationSettings = () => {
  const queryClient = useQueryClient();
  
  return useMutation({
    mutationFn: async (settings: NotificationSettings) => {
      const response = await fetch(`${getApiBaseUrl()}/user/settings/notifications`, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(settings),
      });
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
