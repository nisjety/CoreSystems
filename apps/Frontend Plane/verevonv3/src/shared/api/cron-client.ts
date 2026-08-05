import { requestJson } from './http'

/**
 * A durable cron schedule, mirroring capability-core's `cron_schedules` row
 * (fronted by model-gateway `/v1/cron`). The capability-core sweeper fires due
 * schedules: it creates a task from `task_template`, records a cron_fires row,
 * and advances `next_fire_at`.
 */
export interface CronSchedule {
  id: string
  org_id: string
  name: string
  description: string
  schedule_expr: string
  timezone: string
  task_template: unknown
  enabled: boolean
  last_fire_at?: string | null
  next_fire_at?: string | null
  created_at?: string
  updated_at?: string
}

export function listCronSchedules(orgId: string): Promise<{ schedules: CronSchedule[] }> {
  return requestJson<{ schedules: CronSchedule[] }>('/api/v1/cron', {
    headers: { 'x-verevon-org-id': orgId },
  })
}

/**
 * Create a cron schedule. capability-core validates the cron expression and
 * seeds next_fire_at, so an invalid expression returns 400. Org id is derived
 * server-side. Returns the new schedule id.
 */
export function createCronSchedule(
  orgId: string,
  body: {
    name: string
    schedule_expr: string
    timezone?: string
    description?: string
    task_template?: unknown
    enabled?: boolean
  },
): Promise<{ id: string }> {
  return requestJson<{ id: string }>('/api/v1/cron', {
    method: 'POST',
    body: JSON.stringify(body),
    headers: { 'x-verevon-org-id': orgId },
  })
}

/** Update a schedule's enabled flag, description, or expression. */
export function updateCronSchedule(
  orgId: string,
  cronId: string,
  body: { enabled?: boolean; description?: string; schedule_expr?: string },
): Promise<unknown> {
  return requestJson<unknown>(`/api/v1/cron/${encodeURIComponent(cronId)}`, {
    method: 'POST',
    body: JSON.stringify(body),
    headers: { 'x-verevon-org-id': orgId },
  })
}

export function deleteCronSchedule(orgId: string, cronId: string): Promise<void> {
  return requestJson<void>(`/api/v1/cron/${encodeURIComponent(cronId)}`, {
    method: 'DELETE',
    headers: { 'x-verevon-org-id': orgId },
  })
}
