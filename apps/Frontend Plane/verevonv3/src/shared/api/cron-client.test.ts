// capability-core answers an org with no schedules with `"schedules": null`
// (an empty Go slice); the client must hand callers an empty list, not null.

import { afterEach, describe, expect, it, vi } from 'vitest'

vi.mock('./http', async (importOriginal) => {
  const actual = await importOriginal<typeof import('./http')>()
  return { ...actual, requestJson: vi.fn() }
})

import { requestJson } from './http'
import { listCronSchedules, listScheduleFires, runScheduleNow } from './cron-client'

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
