import type { RunItem } from '@/shared/api/ingestions-client'

/**
 * Apify-style run bookkeeping for the Innhenting run history: one status family
 * per run, a derived duration, and the list filter. Quarry jobs carry no cost
 * field, so cost is deliberately absent here.
 */
export type RunStatusGroup = 'active' | 'succeeded' | 'failed' | 'other'

const ACTIVE = new Set(['running', 'queued', 'pending', 'active', 'in_progress', 'started'])
const SUCCEEDED = new Set(['completed', 'complete', 'success', 'succeeded', 'done', 'ok'])
const FAILED = new Set(['failed', 'error', 'timeout', 'timed_out', 'cancelled', 'canceled', 'aborted'])

export function runStatusGroup(status: string): RunStatusGroup {
  const value = status.trim().toLowerCase()
  if (ACTIVE.has(value)) return 'active'
  if (SUCCEEDED.has(value)) return 'succeeded'
  if (FAILED.has(value)) return 'failed'
  return 'other'
}

function parseTime(value: string | null | undefined): number | null {
  if (!value) return null
  const time = Date.parse(value)
  return Number.isFinite(time) ? time : null
}

/**
 * How long the run took (or has taken so far, while active). `null` when the
 * timestamps cannot tell: a finished run without a completion stamp has no
 * honest duration.
 */
export function runDurationMs(run: Pick<RunItem, 'status' | 'createdAt' | 'startedAt' | 'completedAt'>, now = Date.now()): number | null {
  const start = parseTime(run.startedAt) ?? parseTime(run.createdAt)
  if (start === null) return null
  const end = parseTime(run.completedAt) ?? (runStatusGroup(run.status) === 'active' ? now : null)
  if (end === null || end < start) return null
  return end - start
}

/** Compact duration: `850 ms`, `12 s`, `3 min 4 s`, `1 t 2 min`. */
export function formatDuration(ms: number | null, tr: (no: string, en: string) => string): string {
  if (ms === null) return '—'
  if (ms < 1000) return `${Math.round(ms)} ms`
  const seconds = Math.round(ms / 1000)
  if (seconds < 60) return `${seconds} s`
  const minutes = Math.floor(seconds / 60)
  if (minutes < 60) {
    const rest = seconds % 60
    return rest ? `${minutes} min ${rest} s` : `${minutes} min`
  }
  const hours = Math.floor(minutes / 60)
  const restMinutes = minutes % 60
  const hourUnit = tr('t', 'h')
  return restMinutes ? `${hours} ${hourUnit} ${restMinutes} min` : `${hours} ${hourUnit}`
}

export type RunStatusFilter = 'all' | RunStatusGroup

export type RunFilter = {
  status: RunStatusFilter
  kind: string
  text: string
}

export const defaultRunFilter: RunFilter = { status: 'all', kind: 'all', text: '' }

export function filterRuns(runs: RunItem[], filter: RunFilter): RunItem[] {
  const text = filter.text.trim().toLowerCase()
  return runs.filter((run) => {
    if (filter.status !== 'all' && runStatusGroup(run.status) !== filter.status) return false
    if (filter.kind !== 'all' && run.kind !== filter.kind) return false
    if (!text) return true
    return [run.target, run.id, run.runId ?? '', run.kind].some((value) => value.toLowerCase().includes(text))
  })
}

/** Per-group counts for the overview tiles and the filter chips. */
export function countRunsByGroup(runs: RunItem[]): Record<RunStatusGroup, number> {
  const counts: Record<RunStatusGroup, number> = { active: 0, succeeded: 0, failed: 0, other: 0 }
  for (const run of runs) counts[runStatusGroup(run.status)] += 1
  return counts
}

/** Progress as a 0..100 share, or `null` when the run reports no total. */
export function runProgressPercent(run: Pick<RunItem, 'progress'>): number | null {
  const total = run.progress.total ?? 0
  if (!total) return null
  return Math.min(100, Math.round(((run.progress.completed ?? 0) / total) * 100))
}
