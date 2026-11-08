import type { CronSchedule } from '@/shared/api/cron-client'

/**
 * The automations hub's view of a schedule (EP-6-S1, OD-3). The hub filters on
 * Active, Paused and Expired, and a paused schedule says why it stopped: by its
 * owner, or after three consecutive failed fires. Pure: the clock is passed in and
 * nothing is fetched, so every state is a unit test.
 */

export type AutomationState = 'active' | 'paused_by_owner' | 'paused_failures' | 'expired'
export type AutomationFilter = 'active' | 'paused' | 'expired'

export type AutomationRow = {
  readonly schedule: CronSchedule
  readonly state: AutomationState
}

/** The expiry of a stored approval's `expires_at`, or null when it cannot be read. */
function approvalExpiry(text: unknown): Date | null {
  if (typeof text !== 'string') return null
  try {
    const parsed: unknown = JSON.parse(text)
    const raw = parsed && typeof parsed === 'object' ? (parsed as { expires_at?: unknown }).expires_at : undefined
    if (typeof raw !== 'string') return null
    const expires = new Date(raw)
    return Number.isNaN(expires.getTime()) ? null : expires
  } catch {
    return null
  }
}

/**
 * The consent contract a schedule carries (WS20-S4), read from its stored template.
 * An approval that cannot be read or has passed its expiry cannot fire, which is
 * the same rule capability-core applies (WS20-S8).
 */
export function approvalOf(schedule: CronSchedule, now: number): { expiresAt: Date | null; lapsed: boolean } | null {
  const template = schedule.task_template
  if (!template || typeof template !== 'object' || !('approval_contract' in template)) return null
  const expiresAt = approvalExpiry(template.approval_contract)
  return { expiresAt, lapsed: !expiresAt || expiresAt.getTime() <= now }
}

export function approvalLapsed(schedule: CronSchedule, now: number = Date.now()): boolean {
  return approvalOf(schedule, now)?.lapsed ?? false
}

/** The goal a schedule was created with, which a renewal or a copy starts from. */
export function goalOf(schedule: CronSchedule): string {
  const template = schedule.task_template
  if (!template || typeof template !== 'object' || !('description' in template)) return ''
  return typeof template.description === 'string' ? template.description : ''
}

/**
 * Where a schedule stands. A lapsed approval is Expired whatever its enabled flag
 * says: renewal is the way back, not a switch (WS20-S8). A pause's reason is the
 * `last_outcome` the list carries from the newest fire, which records
 * `paused_failures` when the guards stopped it (EP-6-S3).
 */
export function automationStateOf(schedule: CronSchedule, now: number): AutomationState {
  if (approvalLapsed(schedule, now)) return 'expired'
  if (schedule.enabled) return 'active'
  return schedule.last_outcome === 'paused_failures' ? 'paused_failures' : 'paused_by_owner'
}

/** The filter a state is listed under. Both pauses share one filter; the row says which pause it is. */
export function filterOf(state: AutomationState): AutomationFilter {
  if (state === 'active') return 'active'
  if (state === 'expired') return 'expired'
  return 'paused'
}

export function filterAutomations(rows: readonly AutomationRow[], filter: AutomationFilter): AutomationRow[] {
  return rows.filter((row) => filterOf(row.state) === filter)
}

export function countAutomations(rows: readonly AutomationRow[]): Record<AutomationFilter, number> {
  const counts: Record<AutomationFilter, number> = { active: 0, paused: 0, expired: 0 }
  for (const row of rows) counts[filterOf(row.state)] += 1
  return counts
}
