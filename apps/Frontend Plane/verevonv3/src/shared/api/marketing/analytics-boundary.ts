import { normalizePulseSite } from './pulse-site-stack.ts'
import { isWebsiteId } from './website-client.ts'

export const analyticsRecord = (value: unknown): value is Record<string, unknown> => typeof value === 'object' && value !== null && !Array.isArray(value)
export const analyticsTimestamp = (value: unknown): value is string => typeof value === 'string' && value.length <= 40 && /^\d{4}-\d{2}-\d{2}T/.test(value) && Number.isFinite(Date.parse(value))
export const analyticsFinite = (value: unknown): value is number => typeof value === 'number' && Number.isFinite(value) && value >= 0
export const analyticsNumber = (value: unknown): value is number => analyticsFinite(value) && value <= Number.MAX_SAFE_INTEGER
export const analyticsCount = (value: unknown): value is number => analyticsNumber(value) && Number.isSafeInteger(value)

export function validateAnalyticsContext(site: string, websiteId: string): void {
  if (normalizePulseSite(site) !== site || !isWebsiteId(websiteId)) throw new Error('Invalid website context')
}
export function analyticsSite(value: unknown): string {
  if (typeof value !== 'string' || normalizePulseSite(value) !== value) throw new Error('Invalid analytics website')
  return value
}
export function analyticsUpdatedAt(value: Record<string, unknown>): { updatedAt?: string } {
  if (value.updatedAt === undefined) return {}
  if (!analyticsTimestamp(value.updatedAt)) throw new Error('Invalid analytics date')
  return { updatedAt: value.updatedAt }
}
export type AnalyticsPanelState = { websiteId: string; site: string; revision: number; status: 'loading' | 'connected' | 'unavailable'; reason?: string; quarter?: string }
