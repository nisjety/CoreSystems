import { afterEach, describe, expect, it, vi } from 'vitest'

const http = vi.hoisted(() => ({ requestJson: vi.fn() }))

vi.mock('./http', async (importOriginal) => ({
  ...(await importOriginal<typeof import('./http')>()),
  requestJson: http.requestJson,
}))

import { listDelegableScheduleTools, loadDelegableScheduleTools, scheduleDelegableToolsEnabled } from './schedule-from-chat'

afterEach(() => {
  http.requestJson.mockReset()
  vi.unstubAllEnvs()
})

// F-f02-15: the consent card's list is the BFF's answer, checked for shape only.
describe('listDelegableScheduleTools', () => {
  it('asks the BFF route and keeps each well-formed tool once, trimmed', async () => {
    http.requestJson.mockResolvedValue([
      { tool: ' web_search ', capability: 'web' },
      { tool: 'web_search', capability: 'web' },
      { tool: '', capability: 'web' },
      { tool: 'fs_read' },
      { tool: 3, capability: 'x' },
      null,
      { tool: 'calendar_read', capability: 'calendar' },
    ])

    await expect(listDelegableScheduleTools()).resolves.toEqual([
      { tool: 'web_search', capability: 'web' },
      { tool: 'calendar_read', capability: 'calendar' },
    ])
    expect(http.requestJson).toHaveBeenCalledWith('/api/v1/cron/delegable-tools')
  })

  it('takes at most 64 rows', async () => {
    http.requestJson.mockResolvedValue(
      Array.from({ length: 100 }, (_, n) => ({ tool: `tool_${n}`, capability: 'read' })),
    )

    const tools = await listDelegableScheduleTools()
    expect(tools).toHaveLength(64)
    expect(tools[63]?.tool).toBe('tool_63')
  })

  it('answers an empty list when the request fails or the answer is not a list', async () => {
    http.requestJson.mockRejectedValue(new Error('upstream unavailable'))
    await expect(listDelegableScheduleTools()).resolves.toEqual([])

    http.requestJson.mockResolvedValue({ tool: 'web_search', capability: 'web' })
    await expect(listDelegableScheduleTools()).resolves.toEqual([])
  })
})

describe('VITE_SCHEDULE_DELEGABLE_TOOLS_ENABLED', () => {
  it('is on only for the literal true', () => {
    for (const value of ['', 'TRUE', '1', 'yes']) {
      vi.stubEnv('VITE_SCHEDULE_DELEGABLE_TOOLS_ENABLED', value)
      expect(scheduleDelegableToolsEnabled(), value).toBe(false)
    }
    vi.stubEnv('VITE_SCHEDULE_DELEGABLE_TOOLS_ENABLED', 'true')
    expect(scheduleDelegableToolsEnabled()).toBe(true)
  })
})

// F-f05-20: one request per tab and org. The answer is kept in module memory; an empty answer is asked again.
describe('loadDelegableScheduleTools', () => {
  it('asks the BFF once for two opens of one org, and reuses the answer', async () => {
    http.requestJson.mockResolvedValue([{ tool: 'web_search', capability: 'web' }])

    const first = await loadDelegableScheduleTools('org-cache-reuse')
    const second = await loadDelegableScheduleTools('org-cache-reuse')

    expect(first).toEqual([{ tool: 'web_search', capability: 'web' }])
    expect(second).toEqual(first)
    expect(http.requestJson).toHaveBeenCalledTimes(1)
  })

  it('shares one pending request when two opens overlap', async () => {
    http.requestJson.mockResolvedValue([{ tool: 'calendar_read', capability: 'calendar' }])

    const [first, second] = await Promise.all([
      loadDelegableScheduleTools('org-cache-overlap'),
      loadDelegableScheduleTools('org-cache-overlap'),
    ])

    expect(second).toEqual(first)
    expect(http.requestJson).toHaveBeenCalledTimes(1)
  })

  it('never keeps a failure or an empty answer, so the next open asks again', async () => {
    http.requestJson.mockRejectedValueOnce(new Error('upstream unavailable'))
    await expect(loadDelegableScheduleTools('org-cache-retry')).resolves.toEqual([])

    http.requestJson.mockResolvedValueOnce([])
    await expect(loadDelegableScheduleTools('org-cache-retry')).resolves.toEqual([])

    http.requestJson.mockResolvedValueOnce([{ tool: 'web_search', capability: 'web' }])
    await expect(loadDelegableScheduleTools('org-cache-retry')).resolves.toEqual([
      { tool: 'web_search', capability: 'web' },
    ])
    expect(http.requestJson).toHaveBeenCalledTimes(3)
  })

  it('keeps one list per org, and asks nothing without an org', async () => {
    http.requestJson.mockResolvedValue([{ tool: 'web_search', capability: 'web' }])

    await loadDelegableScheduleTools('org-cache-a')
    await loadDelegableScheduleTools('org-cache-b')
    expect(http.requestJson).toHaveBeenCalledTimes(2)

    http.requestJson.mockClear()
    await expect(loadDelegableScheduleTools('   ')).resolves.toEqual([])
    expect(http.requestJson).not.toHaveBeenCalled()
  })
})
