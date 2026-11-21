// capability-core answers an org with no schedules with `"schedules": null`
// (an empty Go slice); the client must hand callers an empty list, not null.

import { afterEach, describe, expect, it, vi } from 'vitest'

vi.mock('./http', async (importOriginal) => {
  const actual = await importOriginal<typeof import('./http')>()
  return { ...actual, requestJson: vi.fn() }
})

import { requestJson } from './http'
import { ApiError } from './api-error'
import {
  listCronSchedules,
  listDelegableScheduleTools,
  listScheduleFires,
  renewCronScheduleApproval,
  runScheduleNow,
} from './cron-client'

const mockedRequest = vi.mocked(requestJson)

afterEach(() => {
  vi.clearAllMocks()
})

describe('listCronSchedules', () => {
  it('turns the backend’s null list into an empty array', async () => {
    mockedRequest.mockResolvedValue({ schedules: null })
    await expect(listCronSchedules('org-1')).resolves.toEqual({ schedules: [] })
  })

  it('passes a real list through unchanged and sends the org header', async () => {
    const schedule = { id: 'cron_1', creator_subject_id: 'u1' }
    mockedRequest.mockResolvedValue({ schedules: [schedule] })
    await expect(listCronSchedules('org-1')).resolves.toEqual({ schedules: [schedule] })
    expect(mockedRequest).toHaveBeenCalledWith('/api/v1/cron', { headers: { 'x-verevon-org-id': 'org-1' } })
  })
})

describe('runScheduleNow', () => {
  it('posts only the request id for one schedule and unwraps the data envelope', async () => {
    const fire = {
      fire_id: 'cronfire_1',
      fire_key: 'manual:req-0123456789abcdef',
      task_id: 'task_1',
      status: 'pending',
      deduplicated: false,
    }
    mockedRequest.mockResolvedValue({ data: fire })
    await expect(runScheduleNow('org-1', 'cron 1', 'req-0123456789abcdef')).resolves.toEqual(fire)
    expect(mockedRequest).toHaveBeenCalledWith('/api/v1/cron/cron%201/run', {
      method: 'POST',
      body: JSON.stringify({ request_id: 'req-0123456789abcdef' }),
      headers: { 'x-verevon-org-id': 'org-1' },
    })
  })
})

describe('listCronSchedules features', () => {
  it('passes the server feature flags and the last fire fields through', async () => {
    const schedule = { id: 'cron_1', last_fire_status: 'paused', last_outcome: 'paused_failures' }
    mockedRequest.mockResolvedValue({ schedules: [schedule], features: { run_now: true } })
    await expect(listCronSchedules('org-1')).resolves.toEqual({
      schedules: [schedule],
      features: { run_now: true },
    })
  })
})

describe('listCronSchedules creator scope', () => {
  it('asks only for the caller’s own schedules when told to (creator=self)', async () => {
    mockedRequest.mockResolvedValue({ schedules: [] })
    await listCronSchedules('org-1', { creator: 'self' })
    expect(mockedRequest).toHaveBeenLastCalledWith('/api/v1/cron?creator=self', {
      headers: { 'x-verevon-org-id': 'org-1' },
    })
  })

  it('sends no creator filter for the org list, so the default request is unchanged', async () => {
    mockedRequest.mockResolvedValue({ schedules: [] })
    await listCronSchedules('org-1')
    expect(mockedRequest).toHaveBeenLastCalledWith('/api/v1/cron', { headers: { 'x-verevon-org-id': 'org-1' } })
  })
})

describe('listScheduleFires', () => {
  it('asks for the last five fires of one schedule and unwraps the data envelope', async () => {
    const fire = { id: 'fire_1', status: 'completed', withheld_effects: 0 }
    mockedRequest.mockResolvedValue({ data: [fire] })
    await expect(listScheduleFires('org-1', 'cron 1')).resolves.toEqual([fire])
    expect(mockedRequest).toHaveBeenCalledWith('/api/v1/cron/cron%201/fires?limit=5', {
      headers: { 'x-verevon-org-id': 'org-1' },
    })
  })
})

describe('renewCronScheduleApproval', () => {
  it('renews the existing schedule in place: one update of that id with the contract text and resume', async () => {
    mockedRequest.mockResolvedValue({ id: 'cron 1' })
    await renewCronScheduleApproval('org-1', 'cron 1', '{"v":1}')
    expect(mockedRequest).toHaveBeenCalledTimes(1)
    expect(mockedRequest).toHaveBeenCalledWith('/api/v1/cron/cron%201', {
      method: 'POST',
      body: JSON.stringify({ approval_contract: '{"v":1}', enabled: true }),
      headers: { 'x-verevon-org-id': 'org-1' },
    })
  })

  it('can leave the schedule paused when asked not to resume it', async () => {
    mockedRequest.mockResolvedValue({ id: 'cron_1' })
    await renewCronScheduleApproval('org-1', 'cron_1', '{"v":1}', { resume: false })
    expect(mockedRequest).toHaveBeenCalledWith('/api/v1/cron/cron_1', {
      method: 'POST',
      body: JSON.stringify({ approval_contract: '{"v":1}' }),
      headers: { 'x-verevon-org-id': 'org-1' },
    })
  })
})

describe('listDelegableScheduleTools', () => {
  it('asks the delegable route with no org header, since the BFF takes the org from the session', async () => {
    mockedRequest.mockResolvedValue([])
    await listDelegableScheduleTools()
    expect(mockedRequest).toHaveBeenCalledWith('/api/v1/cron/delegable-tools')
  })

  it('keeps each well-formed tool once, trimmed, with only its two fields', async () => {
    mockedRequest.mockResolvedValue([
      { tool: ' web_search ', capability: 'web', class: 'read' },
      { tool: 'web_search', capability: 'web' },
      { tool: 'fs_read' },
      { tool: 'calendar_read', capability: ' calendar ' },
    ])
    await expect(listDelegableScheduleTools()).resolves.toEqual([
      { tool: 'web_search', capability: 'web' },
      { tool: 'calendar_read', capability: 'calendar' },
    ])
  })

  it('also reads the rows under a tools key', async () => {
    mockedRequest.mockResolvedValue({ tools: [{ tool: 'web_search', capability: 'web' }] })
    await expect(listDelegableScheduleTools()).resolves.toEqual([{ tool: 'web_search', capability: 'web' }])
  })

  it('answers an empty list for a 404 while the switch is off', async () => {
    mockedRequest.mockRejectedValue(new ApiError('not found', 404, null))
    await expect(listDelegableScheduleTools()).resolves.toEqual([])
  })

  it('answers an empty list for a network error', async () => {
    mockedRequest.mockRejectedValue(new TypeError('Failed to fetch'))
    await expect(listDelegableScheduleTools()).resolves.toEqual([])
  })

  it('answers an empty list for a malformed answer', async () => {
    mockedRequest.mockResolvedValue({ tool: 'web_search', capability: 'web' })
    await expect(listDelegableScheduleTools()).resolves.toEqual([])
    mockedRequest.mockResolvedValue(null)
    await expect(listDelegableScheduleTools()).resolves.toEqual([])
    mockedRequest.mockResolvedValue({ tools: 'web_search' })
    await expect(listDelegableScheduleTools()).resolves.toEqual([])
  })
})
