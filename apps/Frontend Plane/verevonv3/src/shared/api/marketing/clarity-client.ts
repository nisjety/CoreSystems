import { analyticsRecord as record, analyticsCount as count, analyticsNumber as number, analyticsTimestamp as timestamp, analyticsSite, analyticsUpdatedAt, validateAnalyticsContext } from './analytics-boundary.ts'

export const clarityUXKeys = ['deadClickPercent', 'rageClickPercent', 'excessiveScrollPercent', 'quickBackPercent', 'scriptErrorPercent', 'errorClickPercent', 'averageScrollDepth', 'activeSeconds', 'totalSeconds'] as const
export type ClarityUXMetrics = Record<typeof clarityUXKeys[number], number | null>
export type ClarityMetrics = { sessions: number | null; botSessions: number | null; users: number | null; pagesPerSession: number | null; ux?: ClarityUXMetrics }
export type ClarityReason = 'not_connected' | 'pending_verification' | 'not_configured' | 'quota_limited' | 'source_unavailable' | 'authorization_required' | 'no_data' | 'snapshot_pending' | 'snapshot_expired'
export type ClaritySnapshot = {
  site: string; source: 'microsoft_clarity'; status: 'connected' | 'unavailable'; reason?: ClarityReason
  projectId?: string; scope: 'clarity_project'; window: { numOfDays: 3; timeZone: 'UTC'; observedAt?: string }
  metrics: ClarityMetrics | null; updatedAt?: string
}
const reasons: readonly ClarityReason[] = ['not_connected', 'pending_verification', 'not_configured', 'quota_limited', 'source_unavailable', 'authorization_required', 'no_data', 'snapshot_pending', 'snapshot_expired']
function metric(value: unknown, whole = true): number | null {
  if (value === undefined || value === null) return null
  if (!(whole ? count(value) : number(value))) throw new Error('Invalid Clarity metric')
  return value as number
}
function metrics(value: unknown): ClarityMetrics {
  if (!record(value)) throw new Error('Invalid Clarity metrics')
  let ux: ClarityUXMetrics | undefined
  if (value.ux !== undefined && value.ux !== null) {
    if (!record(value.ux)) throw new Error('Invalid Clarity UX')
    const source = value.ux
    ux = Object.fromEntries(clarityUXKeys.map((key) => {
      const parsed = metric(source[key], false)
      if (!['activeSeconds', 'totalSeconds'].includes(key) && parsed !== null && parsed > 100) throw new Error('Invalid Clarity percentage')
      return [key, parsed]
    })) as ClarityUXMetrics
    if (ux.activeSeconds !== null && ux.totalSeconds !== null && ux.activeSeconds > ux.totalSeconds) throw new Error('Invalid Clarity time')
  }
  return { sessions: metric(value.sessions), botSessions: metric(value.botSessions), users: metric(value.users), pagesPerSession: metric(value.pagesPerSession, false), ...(ux ? { ux } : {}) }
}
/** Only documented project aggregates; no raw recordings, user IDs or provider URLs. */
export function sanitizeClaritySnapshot(value: unknown): ClaritySnapshot {
  if (!record(value) || value.source !== 'microsoft_clarity' || value.scope !== 'clarity_project' || !['connected', 'unavailable'].includes(String(value.status)) || !record(value.window) || value.window.numOfDays !== 3 || value.window.timeZone !== 'UTC') throw new Error('Invalid Clarity response')
  const site = analyticsSite(value.site)
  if (value.projectId !== undefined && (typeof value.projectId !== 'string' || !/^[a-z0-9]{4,32}$/.test(value.projectId))) throw new Error('Invalid Clarity project')
  if (value.window.observedAt !== undefined && !timestamp(value.window.observedAt)) throw new Error('Invalid Clarity observation time')
  if (value.status === 'unavailable' ? !reasons.includes(value.reason as ClarityReason) || value.metrics !== null : value.reason !== undefined) throw new Error('Invalid Clarity state')
  return {
    site, source: 'microsoft_clarity', scope: 'clarity_project', status: value.status as ClaritySnapshot['status'],
    window: { numOfDays: 3, timeZone: 'UTC', ...(value.window.observedAt === undefined ? {} : { observedAt: value.window.observedAt }) },
    metrics: value.status === 'unavailable' ? null : metrics(value.metrics),
    ...(value.projectId === undefined ? {} : { projectId: value.projectId as string }),
    ...(value.reason === undefined ? {} : { reason: value.reason as ClarityReason }),
    ...analyticsUpdatedAt(value),
  }
}
export async function loadClarity(site: string, websiteId: string, signal?: AbortSignal): Promise<ClaritySnapshot> {
  validateAnalyticsContext(site, websiteId)
  const response = await fetch(`/api/v1/visibility/clarity?site=${encodeURIComponent(site)}&websiteId=${websiteId}`, { credentials: 'include', signal, headers: { Accept: 'application/json' } })
  if (!response.ok) throw new Error('Clarity is unavailable')
  const payload: unknown = await response.json()
  const data = sanitizeClaritySnapshot(record(payload) ? payload.data : undefined)
  if (data.site !== site) throw new Error('Unexpected Clarity website')
  return data
}
