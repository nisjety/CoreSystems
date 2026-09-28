// chat-wave2 "schedule" spec §2.1, §3 — `createScheduleFromChat` shapes the
// task_template correctly (no workflow_type, no space_ref); `listMySchedules`
// filters to the caller's own; `cronExprToLabel` reverse-maps the four fixed
// presets and falls back to the raw expression.

import { afterEach, describe, expect, it, vi } from 'vitest'

vi.mock('./cron-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('./cron-client')>()
  return {
    ...actual,
    createCronSchedule: vi.fn(),
    listCronSchedules: vi.fn(),
  }
})

import { createCronSchedule, listCronSchedules, type CronSchedule } from './cron-client'
import {
  cronExprToLabel,
  createScheduleFromChat,
  DEFAULT_SCHEDULE_TIMEZONE,
  listMySchedules,
  SCHEDULE_PRESETS,
} from './schedule-from-chat'

const mockedCreate = vi.mocked(createCronSchedule)
const mockedList = vi.mocked(listCronSchedules)

afterEach(() => {
  vi.clearAllMocks()
})

describe('createScheduleFromChat', () => {
  it('never sets workflow_type or space_ref, and derives the goal from the description', async () => {
    mockedCreate.mockResolvedValue({ id: 'cron_1' })

    const result = await createScheduleFromChat({
      orgId: 'org-1',
      name: '  Daglig oppsummering  ',
      scheduleExpr: '0 9 * * *',
      timezone: DEFAULT_SCHEDULE_TIMEZONE,
      goalText: 'Oppsummer gårsdagens saker',
      threadOriginHint: 'composer',
    })

    expect(result).toEqual({ id: 'cron_1' })
    expect(mockedCreate).toHaveBeenCalledTimes(1)
    const [orgId, body] = mockedCreate.mock.calls[0]!
    expect(orgId).toBe('org-1')
    expect(body.name).toBe('Daglig oppsummering')
    expect(body.schedule_expr).toBe('0 9 * * *')
    expect(body.timezone).toBe('Europe/Oslo')
    expect(body.enabled).toBe(true)
    expect(body.space_ref).toBeUndefined()
    const template = body.task_template as Record<string, unknown>
    expect(template.workflow_type).toBeUndefined()
    expect(template.workflow_input).toBeUndefined()
    expect(template.description).toBe('Oppsummer gårsdagens saker')
    expect(template.title).toBe('Daglig oppsummering')
  })

  it('falls back to Europe/Oslo when the timezone is blank', async () => {
    mockedCreate.mockResolvedValue({ id: 'cron_1' })

    await createScheduleFromChat({
      orgId: 'org-1',
      name: 'x',
      scheduleExpr: '0 9 * * *',
      timezone: '  ',
      goalText: 'goal',
      threadOriginHint: 'message-action',
    })

    const [, body] = mockedCreate.mock.calls[0]!
    expect(body.timezone).toBe('Europe/Oslo')
  })
})

function schedule(overrides: Partial<CronSchedule>): CronSchedule {
  return {
    id: 'cron_1',
    org_id: 'org-1',
    name: 'n',
    description: '',
    schedule_expr: '0 9 * * *',
    timezone: 'Europe/Oslo',
    task_template: {},
    enabled: true,
    ...overrides,
  }
}

describe('listMySchedules', () => {
  it('filters to schedules the caller created', async () => {
    mockedList.mockResolvedValue({
      schedules: [
        schedule({ id: 'a', creator_subject_id: 'user-1' }),
        schedule({ id: 'b', creator_subject_id: 'user-2' }),
        schedule({ id: 'c', creator_subject_id: 'user-1' }),
      ],
    })

    const mine = await listMySchedules('org-1', 'user-1')

    expect(mine.map((s) => s.id)).toEqual(['a', 'c'])
  })

  it('excludes every schedule when the caller created none', async () => {
    mockedList.mockResolvedValue({
      schedules: [schedule({ id: 'a', creator_subject_id: 'someone-else' })],
    })

    const mine = await listMySchedules('org-1', 'user-1')

    expect(mine).toHaveLength(0)
  })
})

describe('SCHEDULE_PRESETS', () => {
  it('has exactly the four owner-approved cadences plus the custom escape hatch', () => {
    expect(SCHEDULE_PRESETS.map((p) => p.cronExpr)).toEqual([
      '0 9 * * *',
      '0 8 * * 1-5',
      '0 9 * * 1',
      '0 9 1 * *',
      null,
    ])
  })
})

describe('cronExprToLabel', () => {
  it('maps a known preset expression back to its label', () => {
    expect(cronExprToLabel('0 9 * * 1', true)).toBe('Hver mandag kl. 09:00')
    expect(cronExprToLabel('0 9 * * 1', false)).toBe('Every Monday at 09:00')
  })

  it('falls back to the raw expression for anything else', () => {
    expect(cronExprToLabel('*/15 * * * *', true)).toBe('*/15 * * * *')
  })
})
