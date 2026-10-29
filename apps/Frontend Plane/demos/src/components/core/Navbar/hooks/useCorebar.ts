import { useQuery } from '@tanstack/react-query';
import { apiClient } from '../lib/queryClient';

// User types based on corebar contract
export interface User {
  id: string;
  name: string;
  email: string;
  position?: string;
  department?: string;
  status: 'online' | 'away' | 'busy' | 'offline';
  avatar?: string;
}

export interface GraphProfile {
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

export interface NotificationCount {
  count: number;
}

export interface Notification {
  id: string;
  title: string;
  description: string;
  type: 'email' | 'teams' | 'calendar' | 'system';
  timestamp: string;
  read: boolean;
  priority: 'low' | 'normal' | 'high';
  actionUrl?: string;
}

// Hooks for corebar API integration
export function useCurrentUser(options?: { enabled?: boolean }) {
  return useQuery({
    queryKey: ['user', 'current'],
    queryFn: () => apiClient.get<User>('/api/user/current'),
    staleTime: 5 * 60 * 1000, // 5 minutes
    enabled: options?.enabled ?? false, // Default to disabled to prevent infinite loops
    retry: (failureCount, error: unknown) => {
      // Don't retry if we get 401/403 (not authenticated)
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

export function useGraphProfile(options?: { enabled?: boolean }) {
  return useQuery({
    queryKey: ['graph', 'user', 'profile'],
    queryFn: () => apiClient.get<GraphProfile>('/api/graph/user/profile'),
    staleTime: 10 * 60 * 1000, // 10 minutes
    enabled: options?.enabled ?? false, // Default to disabled to prevent unnecessary calls
    retry: (failureCount, error: unknown) => {
      // Don't retry if we get 401/403 (not authenticated)
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
    queryFn: () => apiClient.get<NotificationCount>('/api/notifications/unread/count'),
    refetchInterval: 30 * 1000, // Refetch every 30 seconds
  });
}

export function useNotifications() {
  return useQuery({
    queryKey: ['notifications'],
    queryFn: () => apiClient.get<Notification[]>('/api/notifications'),
    staleTime: 2 * 60 * 1000, // 2 minutes
  });
}
