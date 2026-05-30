import { useQuery } from '@tanstack/react-query';
import { apiClient } from '../lib/queryClient';
import type { NotificationFeed, UnreadCount } from '@/lib/notifications/types';

// User types based on corebar contract
interface User {
  id: string;
  name: string;
  email: string;
  position?: string;
  department?: string;
  status: 'online' | 'away' | 'busy' | 'offline';
  avatar?: string;
}

interface GraphProfile {
  id: string;
  email: string;
  name: string;
  avatar?: string;
  source: 'microsoft-graph' | 'local';
  profile?: {
    displayName: string;
    givenName?: string;
    surname?: string;
    mail?: string;
    userPrincipalName?: string;
    jobTitle?: string;
    officeLocation?: string;
    mobilePhone?: string;
    photo?: string;
  };
}

// Hooks for corebar API integration
function useCurrentUser(options?: { enabled?: boolean }) {
  return useQuery({
    queryKey: ['user', 'current'],
    queryFn: () => apiClient.get<User>('/api/user/current'),
    staleTime: 5 * 60 * 1000, // 5 minutes
    enabled: options?.enabled ?? false,
    retry: (failureCount, error: unknown) => {
      if (error && typeof error === 'object' && 'status' in error && (error.status === 401 || error.status === 403)) {
        return false;
      }
      return failureCount < 2;
    },
    refetchOnWindowFocus: false,
    refetchOnMount: false,
    refetchOnReconnect: false,
  });
}

function useGraphProfile(options?: { enabled?: boolean }) {
  return useQuery({
    queryKey: ['graph', 'user', 'profile'],
    queryFn: () => apiClient.get<GraphProfile>('/api/graph/user/profile'),
    staleTime: 10 * 60 * 1000,
    enabled: options?.enabled ?? false,
    retry: (failureCount, error: unknown) => {
      if (error && typeof error === 'object' && 'status' in error && (error.status === 401 || error.status === 403)) {
        return false;
      }
      return failureCount < 2;
    },
    refetchOnWindowFocus: false,
    refetchOnMount: false,
    refetchOnReconnect: false,
  });
}

export function useNotificationCount() {
  return useQuery({
    queryKey: ['notifications', 'unread', 'count'],
    queryFn: async () => {
      try {
        return await apiClient.get<UnreadCount>('/api/notifications/unread/count');
      } catch {
        return { count: 0 };
      }
    },
    // 5-minute polling as WS fallback; real-time updates come from useNotificationWS
    refetchInterval: 5 * 60 * 1000,
    retry: false,
  });
}

export function useNotifications() {
  return useQuery({
    queryKey: ['notifications'],
    queryFn: async () => {
      try {
        const feed = await apiClient.get<NotificationFeed>('/api/notifications');
        return feed?.notifications ?? [];
      } catch {
        return [];
      }
    },
    staleTime: 2 * 60 * 1000,
  });
}

// Keep unused exports to avoid breaking imports
export { useCurrentUser, useGraphProfile };
