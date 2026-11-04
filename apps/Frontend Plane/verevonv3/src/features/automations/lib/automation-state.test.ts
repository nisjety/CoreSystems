import { describe, expect, it } from 'vitest'
import type { CronSchedule } from '@/shared/api/cron-client'
import {
  approvalOf,
  approvalLapsed,
  automationStateOf,
  countAutomations,
  filterAutomations,
  filterOf,
  goalOf,
  type AutomationRow,
  type AutomationState,
} from './automation-state'

const NOW = Date.parse('2026-11-03T12:00:00Z')

function schedule(overrides: Partial<CronSchedule>): CronSchedule {
  return {
    id: 'cron_1',
    org_id: 'org-1',
    name: 'Daglig rapport',
    description: '',
    schedule_expr: '0 9 * * *',
    timezone: 'Europe/Oslo',
    task_template: {},
    enabled: true,
    creator_subject_id: 'user-1',
    ...overrides,
  }
}

/** A stored approval the way WS20-S4 writes it: JSON text in `approval_contract`. */
function approval(expiresAt: string): Record<string, unknown> {
  return {
    kind: 'cron',
    title: 'Daglig rapport',
    description: 'summarise the week',
    approval_contract: JSON.stringify({ v: 1, tools: [], expires_at: expiresAt, max_rounds: 6, max_tokens_per_round: 2048 }),
  }
}

function row(id: string, state: AutomationState): AutomationRow {
  return { schedule: schedule({ id }), state }
}

describe('automationStateOf', () => {
  it('shows an enabled schedule with no approval as active', () => {
    expect(automationStateOf(schedule({ enabled: true }), NOW)).toBe('active')
  })

  it('shows a disabled schedule with no fire yet as paused by its owner', () => {
    expect(automationStateOf(schedule({ enabled: false }), NOW)).toBe('paused_by_owner')
    expect(automationStateOf(schedule({ enabled: false, last_outcome: '' }), NOW)).toBe('paused_by_owner')
  })

  it('shows a disabled schedule whose last fire the guards paused for failures as paused after failures', () => {
    expect(automationStateOf(schedule({ enabled: false, last_outcome: 'paused_failures' }), NOW)).toBe('paused_failures')
  })

  it('keeps the owner pause for any other reason recorded on the last fire', () => {
    expect(automationStateOf(schedule({ enabled: false, last_outcome: 'paused_expired' }), NOW)).toBe('paused_by_owner')
  })

  it('shows a schedule switched back on as active, even when its last fire paused it', () => {
    expect(automationStateOf(schedule({ enabled: true, last_outcome: 'paused_failures' }), NOW)).toBe('active')
  })

  it('expires a schedule whose approval has lapsed, whether or not it is still switched on', () => {
    const lapsed = { task_template: approval('2026-01-01T00:00:00Z') }
    expect(automationStateOf(schedule({ enabled: true, ...lapsed }), NOW)).toBe('expired')
    expect(automationStateOf(schedule({ enabled: false, ...lapsed }), NOW)).toBe('expired')
  })

  it('keeps a schedule with a live approval active or paused by its owner', () => {
    const live = { task_template: approval('2099-01-01T00:00:00Z') }
    expect(automationStateOf(schedule({ enabled: true, ...live }), NOW)).toBe('active')
    expect(automationStateOf(schedule({ enabled: false, ...live }), NOW)).toBe('paused_by_owner')
  })

  it('never expires a contract-free schedule, however old the clock says it is', () => {
    const contractFree = schedule({ enabled: false, task_template: { kind: 'cron', description: 'x' } })
    expect(approvalLapsed(contractFree, NOW)).toBe(false)
    expect(automationStateOf(contractFree, Date.parse('2100-01-01T00:00:00Z'))).toBe('paused_by_owner')
  })

  it('treats an approval that cannot be read as lapsed', () => {
    const unreadable = schedule({ enabled: true, task_template: { approval_contract: 'not json' } })
    expect(automationStateOf(unreadable, NOW)).toBe('expired')
  })
})

describe('filters and counts', () => {
  it('lists both pauses under Paused, and each state under its own filter', () => {
    expect(filterOf('active')).toBe('active')
    expect(filterOf('paused_by_owner')).toBe('paused')
    expect(filterOf('paused_failures')).toBe('paused')
    expect(filterOf('expired')).toBe('expired')
  })

  it('counts every filter, including the empty ones', () => {
    const rows = [
      row('a', 'active'),
      row('b', 'active'),
      row('c', 'paused_by_owner'),
      row('d', 'paused_failures'),
      row('e', 'expired'),
    ]
    expect(countAutomations(rows)).toEqual({ active: 2, paused: 2, expired: 1 })
    expect(countAutomations([])).toEqual({ active: 0, paused: 0, expired: 0 })
  })

  it('returns the rows of one filter in their original order', () => {
    const rows = [row('a', 'paused_failures'), row('b', 'active'), row('c', 'paused_by_owner')]
    expect(filterAutomations(rows, 'paused').map((item) => item.schedule.id)).toEqual(['a', 'c'])
    expect(filterAutomations(rows, 'expired')).toEqual([])
  })
})

describe('schedule text and approval reads', () => {
  it('reads the goal a schedule was created with, and nothing when it has none', () => {
    expect(goalOf(schedule({ task_template: { description: 'summarise the week' } }))).toBe('summarise the week')
    expect(goalOf(schedule({ task_template: { description: 42 } }))).toBe('')
    expect(goalOf(schedule({ task_template: null }))).toBe('')
  })

  it('reports no approval for a schedule that carries none, and the expiry of one that does', () => {
    expect(approvalOf(schedule({}), NOW)).toBeNull()
    const expires = approvalOf(schedule({ task_template: approval('2099-01-01T00:00:00Z') }), NOW)
    expect(expires?.lapsed).toBe(false)
    expect(expires?.expiresAt?.toISOString()).toBe('2099-01-01T00:00:00.000Z')
  })
})
