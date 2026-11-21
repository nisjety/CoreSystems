import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { createCronSchedule, renewCronScheduleApproval } from './cron-client'
import {
  buildScheduleContractText,
  createScheduleFromChat,
  renewalToolsOf,
  renewScheduleApproval,
  scheduleContractExpiry,
  scheduleRenewInPlaceEnabled,
  type ScheduleFromChatInput,
} from './schedule-from-chat'

vi.mock('./cron-client', () => ({
  createCronSchedule: vi.fn(async () => ({ id: 'cron-1' })),
  listCronSchedules: vi.fn(),
  renewCronScheduleApproval: vi.fn(async () => ({})),
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
  // Through unknown: the body is the typed CronTaskTemplate, which an interface does not widen to a record.
  const body = vi.mocked(createCronSchedule).mock.calls[0]?.[1] as unknown as { task_template: Record<string, unknown> }
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

describe('renewScheduleApproval', () => {
  beforeEach(() => {
    vi.mocked(createCronSchedule).mockClear()
    vi.mocked(renewCronScheduleApproval).mockClear()
  })

  it('renews the named schedule with the contract text and never creates a schedule', async () => {
    await renewScheduleApproval({
      orgId: 'org-1',
      cronId: 'cron-9',
      contract: { tools: [readTool], expiresAt: '2026-12-20T10:00:00Z' },
    })

    expect(createCronSchedule).not.toHaveBeenCalled()
    expect(renewCronScheduleApproval).toHaveBeenCalledTimes(1)
    const [orgId, cronId, text] = vi.mocked(renewCronScheduleApproval).mock.calls[0] as [string, string, string]
    expect(orgId).toBe('org-1')
    expect(cronId).toBe('cron-9')
    const contract = JSON.parse(text) as { v: number; expires_at: string; tools: { tool: string; class: string; approved_at: string }[] }
    expect(contract.v).toBe(1)
    expect(contract.expires_at).toBe('2026-12-20T10:00:00Z')
    expect(contract.tools.map((tool) => [tool.tool, tool.class])).toEqual([['web_search', 'read']])
    expect(contract.tools[0]?.approved_at).toMatch(/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$/)
  })
})

describe('renewalToolsOf', () => {
  it('reads the tools the lapsed approval named, once each', () => {
    const text = buildScheduleContractText(
      { tools: [readTool, { tool: 'web_fetch', capability: 'cap.tool.web_fetch' }], expiresAt: '2026-01-01T00:00:00Z' },
      '2025-12-01T10:00:00Z',
    )

    expect(renewalToolsOf({ approval_contract: text })).toEqual([
      readTool,
      { tool: 'web_fetch', capability: 'cap.tool.web_fetch' },
    ])
  })

  it('gives nothing to approve for a draft, a malformed contract or a non-object template', () => {
    expect(renewalToolsOf({ kind: 'cron' })).toEqual([])
    expect(renewalToolsOf({ approval_contract: 'not json' })).toEqual([])
    expect(renewalToolsOf({ approval_contract: '{"tools":"web_search"}' })).toEqual([])
    expect(renewalToolsOf({ approval_contract: '{"tools":[{"tool":1,"capability":"x"},{"tool":"a"}]}' })).toEqual([])
    expect(renewalToolsOf(null)).toEqual([])
    expect(renewalToolsOf('x')).toEqual([])
  })
})

describe('scheduleRenewInPlaceEnabled', () => {
  afterEach(() => vi.unstubAllEnvs())

  it('is on only for the literal true', () => {
    expect(scheduleRenewInPlaceEnabled()).toBe(false)
    vi.stubEnv('VITE_SCHEDULE_DELEGABLE_TOOLS_ENABLED', '1')
    expect(scheduleRenewInPlaceEnabled()).toBe(false)
    vi.stubEnv('VITE_SCHEDULE_DELEGABLE_TOOLS_ENABLED', 'true')
    expect(scheduleRenewInPlaceEnabled()).toBe(true)
  })
})
