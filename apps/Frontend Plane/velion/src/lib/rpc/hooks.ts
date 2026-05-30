/**
 * Client-side oRPC hooks — type-safe React Query wrappers for all API routes.
 *
 * Each hook:
 *  - Uses @tanstack/react-query with typed queryKey & queryFn
 *  - Configures staleTime / gcTime per-domain for optimal UX
 *  - Avoids waterfalls by supporting parallel prefetching
 *
 * Import these directly in client components instead of raw useQuery+fetch.
 */
'use client'

import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import type { UseQueryOptions, UseMutationOptions } from '@tanstack/react-query'
import type {
  DashboardStats,
  NotificationFeed,
  UserProfile,
  Organization,
} from './contract'

// ── Fetch helper ──────────────────────────────────────────────────────────────

async function rpcFetch<T>(url: string, init?: RequestInit): Promise<T> {
  const res = await fetch(url, {
    credentials: 'include',
    ...init,
    headers: {
      'Content-Type': 'application/json',
      ...init?.headers,
    },
  })
  if (!res.ok) {
    const text = await res.text().catch(() => res.statusText)
    throw Object.assign(new Error(text), { status: res.status })
  }
  if (res.status === 204) return null as T
  return res.json() as Promise<T>
}

// ── Query key factories (stable references, easy invalidation) ────────────────

export const queryKeys = {
  dashboard: {
    stats: ['dashboard', 'stats'] as const,
  },
  notifications: {
    all: ['notifications'] as const,
    feed: (page: number) => ['notifications', 'feed', page] as const,
    unreadCount: ['notifications', 'unread', 'count'] as const,
    unseenCount: ['notifications', 'unseen', 'count'] as const,
  },
  user: {
    current: ['user', 'current'] as const,
    profile: ['user', 'profile'] as const,
    session: ['user', 'session'] as const,
  },
  org: {
    list: ['org', 'list'] as const,
    current: ['org', 'current'] as const,
    members: (orgId: string) => ['org', 'members', orgId] as const,
  },
  knowledge: {
    sources: ['knowledge', 'sources'] as const,
    documents: (page?: number) => ['knowledge', 'documents', page ?? 0] as const,
    integrations: ['knowledge', 'integrations'] as const,
  },
  planner: {
    documents: ['planner', 'documents'] as const,
  },
} as const

// ── Dashboard ─────────────────────────────────────────────────────────────────

export function useDashboardStats(
  opts?: Partial<UseQueryOptions<DashboardStats | null>>,
) {
  return useQuery<DashboardStats | null>({
    queryKey: queryKeys.dashboard.stats,
    queryFn: () => rpcFetch<DashboardStats | null>('/api/dashboard/stats'),
    staleTime: 60_000,
    gcTime: 5 * 60_000,
    refetchOnWindowFocus: false,
    ...opts,
  })
}

// ── Notifications ─────────────────────────────────────────────────────────────

export function useNotificationFeed(page = 0) {
  return useQuery<NotificationFeed>({
    queryKey: queryKeys.notifications.feed(page),
    queryFn: () => rpcFetch<NotificationFeed>(`/api/notifications?page=${page}&limit=20`),
    staleTime: 30_000,
    gcTime: 5 * 60_000,
  })
}

export function useUnreadCount() {
  return useQuery<{ count: number }>({
    queryKey: queryKeys.notifications.unreadCount,
    queryFn: () => rpcFetch<{ count: number }>('/api/notifications/unread/count'),
    staleTime: 15_000,
    refetchInterval: 60_000, // poll every 60s as WS fallback
  })
}

export function useUnseenCount() {
  return useQuery<{ count: number }>({
    queryKey: queryKeys.notifications.unseenCount,
    queryFn: () => rpcFetch<{ count: number }>('/api/notifications/unseen/count'),
    staleTime: 15_000,
    refetchInterval: 60_000,
  })
}

export function useMarkNotificationRead() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (id: string) =>
      rpcFetch<void>(`/api/notifications/${id}/read`, { method: 'POST' }),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.notifications.unreadCount })
      void queryClient.invalidateQueries({ queryKey: queryKeys.notifications.all })
    },
  })
}

export function useMarkAllRead() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: () => rpcFetch<void>('/api/notifications/clear', { method: 'POST' }),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.notifications.unreadCount })
      void queryClient.invalidateQueries({ queryKey: queryKeys.notifications.all })
    },
  })
}

export function useMarkAllSeen() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: () => rpcFetch<void>('/api/notifications/mark-all-seen', { method: 'POST' }),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.notifications.unseenCount })
      void queryClient.invalidateQueries({ queryKey: queryKeys.notifications.all })
    },
  })
}

// ── User ──────────────────────────────────────────────────────────────────────

export function useCurrentUser() {
  return useQuery<UserProfile | null>({
    queryKey: queryKeys.user.current,
    queryFn: () => rpcFetch<UserProfile | null>('/api/user/users/me'),
    staleTime: 5 * 60_000,
    gcTime: 10 * 60_000,
    refetchOnWindowFocus: false,
  })
}

// ── Organizations ─────────────────────────────────────────────────────────────

export function useOrganizations() {
  return useQuery<Organization[]>({
    queryKey: queryKeys.org.list,
    queryFn: () => rpcFetch<Organization[]>('/api/org/orgs/me'),
    staleTime: 5 * 60_000,
    gcTime: 10 * 60_000,
    refetchOnWindowFocus: false,
  })
}
