import { requestJson } from './http'

/**
 * Change-monitoring client — Phase 1 Track C (the "monitor" leg).
 *
 * Talks to the gateway `monitoring` domain (`/api/v1/monitoring/*`), which
 * proxies quarry-edge's `/v1/change/*` routes. Every value here traces to a
 * live edge response: a `check` runs a real fetch + baseline comparison, and
 * `latest` / `history` read whatever baselines the org has actually accrued.
 * Recurring monitors (`/schedules`) are Quarry `change_monitor` schedules with a
 * fixed hourly/daily/weekly cadence.
 */

/** A captured version of a URL (quarry `BaselineSnapshot`, camelCased shape). */
export interface BaselineSnapshot {
  baselineId: string
  orgId: string
  sourceUrl: string
  fingerprint: string
  artifactId?: string | null
  prevBaselineId?: string | null
  runId?: string | null
  capturedAt: string
}

export type ChangeStatus = 'new' | 'unchanged' | 'changed' | 'unreachable'

/** Result of comparing a fresh fetch against the latest baseline. */
export interface ChangeRecord {
  sourceUrl: string
  orgId: string
  status: ChangeStatus
  newBaseline?: BaselineSnapshot | null
  prevBaseline?: BaselineSnapshot | null
  diffId?: string | null
  checkedAt: string
}

/**
 * Run an on-demand check of `url`: the gateway fetches the page, fingerprints
 * it, and compares against the org's latest baseline. Returns the change status.
 */
export function checkUrlNow(url: string, signal?: AbortSignal): Promise<ChangeRecord> {
  return requestJson<ChangeRecord>('/api/v1/monitoring/check', {
    method: 'POST',
    body: JSON.stringify({ url }),
    signal,
  })
}

/**
 * The most recent baseline for `url`, or `null` when the org has never captured
 * it. (The gateway normalizes the edge's 404 to a `null` payload.)
 */
export function getLatestBaseline(url: string, signal?: AbortSignal): Promise<BaselineSnapshot | null> {
  return requestJson<BaselineSnapshot | null>(
    `/api/v1/monitoring/latest?url=${encodeURIComponent(url)}`,
    { signal },
  )
}

/** Fixed cadences a recurring monitor accepts; Quarry derives the cron from them. */
export type MonitorPreset = 'hourly' | 'daily' | 'weekly'

/** A recurring change monitor (a Quarry `change_monitor` schedule), camelCased. */
export interface ChangeMonitor {
  id: string
  url: string
  cron: string
  enabled: boolean
  createdAt: string | null
  lastRunAt: string | null
  nextRunAt: string | null
}

/** Quarry stamps schedule times as unix seconds; tolerate ISO strings too. */
function monitorTime(value: unknown): string | null {
  if (typeof value === 'number' && Number.isFinite(value) && value > 0) {
    return new Date(value < 1e12 ? value * 1000 : value).toISOString()
  }
  return typeof value === 'string' && value.trim() ? value : null
}

function toChangeMonitor(row: Record<string, unknown>): ChangeMonitor {
  const text = (...keys: string[]) => {
    for (const key of keys) {
      const value = row[key]
      if (typeof value === 'string' && value.trim()) return value
    }
    return ''
  }
  return {
    id: text('id', 'schedule_id'),
    url: text('target_ref', 'targetRef', 'url'),
    cron: text('cron'),
    enabled: row.enabled !== false,
    createdAt: monitorTime(row.created_at ?? row.createdAt),
    lastRunAt: monitorTime(row.last_run_at ?? row.lastRunAt),
    nextRunAt: monitorTime(row.next_run_at ?? row.nextRunAt),
  }
}

/** The org's recurring change monitors. Empty until one is created. */
export async function listMonitors(signal?: AbortSignal): Promise<ChangeMonitor[]> {
  const rows = await requestJson<unknown>('/api/v1/monitoring/schedules', { signal })
  if (!Array.isArray(rows)) return []
  return rows
    .filter((row): row is Record<string, unknown> => typeof row === 'object' && row !== null)
    .map(toChangeMonitor)
    .filter((monitor) => monitor.id.length > 0)
}

/** Create a recurring monitor for `url` (contract: `monitoring.create_monitor`). */
export async function createMonitor(
  url: string,
  preset: MonitorPreset,
  signal?: AbortSignal,
): Promise<ChangeMonitor | null> {
  const row = await requestJson<unknown>('/api/v1/monitoring/schedules', {
    method: 'POST',
    body: JSON.stringify({ url, preset }),
    signal,
  })
  return typeof row === 'object' && row !== null ? toChangeMonitor(row as Record<string, unknown>) : null
}

/** Stop and remove a recurring monitor (contract: `monitoring.delete_monitor`). */
export async function deleteMonitor(id: string, signal?: AbortSignal): Promise<void> {
  await requestJson<unknown>(`/api/v1/monitoring/schedules/${encodeURIComponent(id)}`, {
    method: 'DELETE',
    signal,
  })
}

/**
 * Newest-first baseline chain for `url`. Empty until baselines accrue — never a
 * synthesized entry.
 */
export function getChangeHistory(
  url: string,
  limit = 50,
  signal?: AbortSignal,
): Promise<BaselineSnapshot[]> {
  return requestJson<BaselineSnapshot[]>(
    `/api/v1/monitoring/history?url=${encodeURIComponent(url)}&limit=${limit}`,
    { signal },
  )
}
