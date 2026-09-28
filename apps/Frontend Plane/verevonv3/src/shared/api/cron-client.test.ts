// capability-core answers an org with no schedules with `"schedules": null`
// (an empty Go slice); the client must hand callers an empty list, not null.

import { afterEach, describe, expect, it, vi } from 'vitest'

vi.mock('./http', async (importOriginal) => {
  const actual = await importOriginal<typeof import('./http')>()
  return { ...actual, requestJson: vi.fn() }
})

import { requestJson } from './http'
import { listCronSchedules } from './cron-client'

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
