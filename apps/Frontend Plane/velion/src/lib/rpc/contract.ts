/**
 * oRPC Contract — shared type-safe API contract between client and server.
 *
 * Each backend service exposes a namespace with typed procedures.
 * The contract is used by both the server-side route handlers and the
 * client-side React Query hooks via @orpc/tanstack-query.
 */
import { z } from 'zod'

// ── Shared schemas ────────────────────────────────────────────────────────────

export const DashboardStatsSchema = z.object({
  memberCount: z.number().nullable(),
  sourceCount: z.number().nullable(),
  documentCount: z.number().nullable(),
  crawledPages: z.number().nullable(),
  crawlStatus: z.enum(['idle', 'running', 'done', 'error']).nullable(),
  lastCrawlAt: z.string().nullable(),
})

export type DashboardStats = z.infer<typeof DashboardStatsSchema>

export const NotificationSchema = z.object({
  id: z.string(),
  userId: z.string(),
  eventType: z.string(),
  title: z.string(),
  body: z.string().optional(),
  link: z.string().optional(),
  read: z.boolean(),
  seen: z.boolean(),
  createdAt: z.string(),
})

export type Notification = z.infer<typeof NotificationSchema>

export const NotificationFeedSchema = z.object({
  notifications: z.array(NotificationSchema),
  total: z.number(),
  page: z.number(),
  hasMore: z.boolean(),
})

export type NotificationFeed = z.infer<typeof NotificationFeedSchema>

export const UnreadCountSchema = z.object({
  count: z.number(),
})

export const UnseenCountSchema = z.object({
  count: z.number(),
})

export const UserProfileSchema = z.object({
  id: z.string(),
  email: z.string().optional(),
  name: z.string().optional(),
  avatar: z.string().optional(),
  onboardingComplete: z.boolean().optional(),
  userTier: z.enum(['admin', 'premium', 'regular', 'trial']).optional(),
  languagePreference: z.string().optional(),
  timezone: z.string().optional(),
})

export type UserProfile = z.infer<typeof UserProfileSchema>

export const OrganizationSchema = z.object({
  id: z.string(),
  name: z.string(),
  slug: z.string(),
  plan: z.enum(['free', 'pro', 'enterprise']),
  status: z.enum(['active', 'suspended', 'deleted']),
})

export type Organization = z.infer<typeof OrganizationSchema>

export const SessionContextSchema = z.object({
  userId: z.string(),
  userEmail: z.string(),
  userName: z.string(),
  orgId: z.string().nullable(),
  role: z.string().nullable(),
})

export type SessionContext = z.infer<typeof SessionContextSchema>
