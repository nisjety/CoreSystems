import { beforeEach, describe, expect, it, vi } from 'vitest'

import { createCronSchedule } from './cron-client'
import {
  buildScheduleContractText,
  createScheduleFromChat,
  scheduleContractExpiry,
  type ScheduleFromChatInput,
} from './schedule-from-chat'

vi.mock('./cron-client', () => ({
  createCronSchedule: vi.fn(async () => ({ id: 'cron-1' })),
  listCronSchedules: vi.fn(),
}))

const base: ScheduleFromChatInput = {
  orgId: 'org-1',
  name: 'Daily',
  scheduleExpr: '0 9 * * *',
  timezone: 'Europe/Oslo',
  goalText: 'Summarise the week',
  threadOriginHint: 'composer',
}

const readTool = { tool: 'web_search', capability: 'cap.tool.web_search' }

function sentTemplate(): Record<string, unknown> {
  const body = vi.mocked(createCronSchedule).mock.calls[0]?.[1] as { task_template: Record<string, unknown> }
  return body.task_template
}

describe('scheduleContractExpiry', () => {
  it('gives a canonical UTC instant and clamps to the maximum window', () => {
    const now = new Date('2026-10-24T10:00:00.123Z')

    expect(scheduleContractExpiry(30, now)).toBe('2026-11-23T10:00:00Z')
    expect(scheduleContractExpiry(400, now)).toBe('2027-01-22T10:00:00Z')
    expect(scheduleContractExpiry(0, now)).toBe('2026-10-25T10:00:00Z')
  })
})

describe('buildScheduleContractText', () => {
  it('writes read-only JSON text in the shape the Model Plane verifies', () => {
    const text = buildScheduleContractText(
      { tools: [readTool], expiresAt: '2026-11-23T10:00:00Z' },
      '2026-10-24T10:00:00Z',
    )

    expect(JSON.parse(text)).toEqual({
      v: 1,
      tools: [{ tool: 'web_search', capability: 'cap.tool.web_search', class: 'read', approved_at: '2026-10-24T10:00:00Z' }],
      expires_at: '2026-11-23T10:00:00Z',
      max_rounds: 6,
      max_tokens_per_round: 2048,
    })
  })
})

describe('createScheduleFromChat contract', () => {
  beforeEach(() => {
    vi.mocked(createCronSchedule).mockClear()
  })

  it('sends no approval contract when no tool is approved, so the schedule stays draft only', async () => {
    await createScheduleFromChat(base)

    expect(sentTemplate()).not.toHaveProperty('approval_contract')
  })

  it('sends an empty tool selection as draft only', async () => {
    await createScheduleFromChat({
      ...base,
      contract: { tools: [], expiresAt: '2026-11-23T10:00:00Z' },
    })

    expect(sentTemplate()).not.toHaveProperty('approval_contract')
  })

  it('sends the approval contract as a JSON text value when a read tool is approved', async () => {
    await createScheduleFromChat({
      ...base,
      contract: { tools: [readTool], expiresAt: '2026-11-23T10:00:00Z' },
    })

    const text = sentTemplate().approval_contract
    expect(typeof text).toBe('string')
    expect(JSON.parse(text as string)).toMatchObject({ v: 1, expires_at: '2026-11-23T10:00:00Z' })
  })
})
