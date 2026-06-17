import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import type { CalendarEvent, Notification } from '../types';
import {
  normalizeSidebarCalendarEvents,
  normalizeSidebarNotifications,
} from '../lib/adapters';

function getApiBaseUrl(): string {
  return '';
}

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
    refetchInterval: false,
  });
};

export const useCalendarEvents = (options?: {
  enabled?: boolean;
  initialData?: CalendarEvent[];
}) => {
  const apiBaseUrl = getApiBaseUrl();
  const enabled = options?.enabled !== false;

  return useQuery({
    queryKey: ['calendar', 'events'],
    queryFn: async () => {
      const response = await fetch(`${apiBaseUrl}/api/user/calendar/events`, {
        credentials: 'include',
      });
      if (!response.ok) {
        throw new Error('Failed to fetch calendar events');
      }
      return normalizeSidebarCalendarEvents(await response.json());
    },
    enabled: enabled && typeof window !== 'undefined',
    initialData: options?.initialData,
    retry: (failureCount, error: unknown) => {
      const errorStatus = (error as { status?: number })?.status;
      if (errorStatus === 401 || errorStatus === 403) {
        return false;
      }
      return failureCount < 3;
    },
    refetchOnWindowFocus: false,
    refetchOnMount: false,
    refetchInterval: false,
  });
};

export const useNotifications = (options?: {
  enabled?: boolean;
  initialData?: Notification[];
}) => {
  const enabled = options?.enabled !== false;

  return useQuery({
    queryKey: ['notifications'],
    queryFn: async () => {
      const response = await fetch('/api/notifications', { credentials: 'include' });
      if (!response.ok) {
        throw new Error('Failed to fetch notifications');
      }

      return normalizeSidebarNotifications(await response.json());
    },
    enabled: enabled && typeof window !== 'undefined',
    initialData: options?.initialData,
    retry: (failureCount, error: unknown) => {
      const errorStatus = (error as { status?: number })?.status;
      if (errorStatus === 401 || errorStatus === 403) {
        return false;
      }
      return failureCount < 3;
    },
    refetchOnWindowFocus: false,
    refetchOnMount: false,
    refetchInterval: false,
  });
};

export const useMarkNotificationAsRead = () => {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: async (notificationId: string) => {
      const response = await fetch(`/api/notifications/${notificationId}/read`, {
        method: 'POST',
        credentials: 'include',
      });
      if (!response.ok) {
        throw new Error('Failed to mark notification as read');
      }
      return response.json();
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['notifications'] });
      queryClient.invalidateQueries({ queryKey: ['notifications', 'unread'] });
      queryClient.invalidateQueries({ queryKey: ['notifications', 'unread', 'count'] });
    },
  });
};

export const useClearAllNotifications = () => {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: async () => {
      const response = await fetch('/api/notifications/clear', {
        method: 'POST',
        credentials: 'include',
      });
      if (!response.ok) {
        throw new Error('Failed to clear notifications');
      }
      return response.json();
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['notifications'] });
      queryClient.invalidateQueries({ queryKey: ['notifications', 'unread'] });
      queryClient.invalidateQueries({ queryKey: ['notifications', 'unread', 'count'] });
    },
  });
};
