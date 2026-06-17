/**
 * RPC barrel export — unified import for both server and client code.
 *
 * Server components: import { serviceClient, orgApi } from '@/lib/rpc'
 * Client components: import { useDashboardStats, queryKeys } from '@/lib/rpc'
 *
 * Note: server.ts is behind 'server-only' so client bundler will tree-shake it.
 */

// Contract types (shared)
export type {
  DashboardStats,
  Notification,
  NotificationFeed,
  UserProfile,
  Organization,
  SessionContext,
} from './contract'

export {
  DashboardStatsSchema,
  NotificationSchema,
  NotificationFeedSchema,
  UnreadCountSchema,
  UnseenCountSchema,
  UserProfileSchema,
  OrganizationSchema,
  SessionContextSchema,
} from './contract'

// Client-side hooks
export {
  queryKeys,
  useDashboardStats,
  useNotificationFeed,
  useUnreadCount,
  useUnseenCount,
  useMarkNotificationRead,
  useMarkAllRead,
  useMarkAllSeen,
  useCurrentUser,
  useOrganizations,
} from './hooks'
