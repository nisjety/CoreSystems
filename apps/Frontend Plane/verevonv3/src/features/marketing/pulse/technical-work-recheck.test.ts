import { afterEach, describe, expect, it, vi } from 'vitest'
import { recheckTechnicalTask } from './technical-work-recheck.ts'
import { loadPageSpeed, startPageSpeed } from '../../../shared/api/marketing/pagespeed-client.ts'
import { verifyMarketingTask, type MarketingTask, type MarketingWorkSnapshot } from '../../../shared/api/marketing/marketing-work-client.ts'
vi.mock('../../../shared/api/marketing/pagespeed-client.ts', () => ({ startPageSpeed: vi.fn(), loadPageSpeed: vi.fn() }))
vi.mock('../../../shared/api/marketing/marketing-work-client.ts', () => ({ verifyMarketingTask: vi.fn() }))
const task: MarketingTask = { id: `task_${'1'.repeat(32)}`, websiteId: `web_${'2'.repeat(32)}`, site: 'https://example.com/', revision: 1, status: 'todo', createdAt: '2026-10-22T10:00:00Z', updatedAt: '2026-10-22T10:00:00Z', evidence: { kind: 'pagespeed_check', key: 'canonical', strategy: 'mobile', checkStatus: 'failed', checkScore: 0, observedAt: '2026-10-22T10:00:00Z', lighthouseVersion: '13.0.1' } }
const state = (status: 'running' | 'complete' | 'failed') => ({ site: task.site, strategy: 'mobile' as const, source: 'google_pagespeed' as const, status })
describe('explicit technical task recheck', () => {
  afterEach(() => { vi.resetAllMocks(); vi.useRealTimers() })
  it('starts once, polls cached status and verifies without a task status mutation', async () => {
    vi.useFakeTimers()
    vi.mocked(startPageSpeed).mockResolvedValue(state('running'))
    vi.mocked(loadPageSpeed).mockResolvedValue(state('complete'))
    vi.mocked(verifyMarketingTask).mockResolvedValue({ tasks: [task] } as MarketingWorkSnapshot)
    const result = recheckTechnicalTask(task, 4, new AbortController().signal)
    expect(startPageSpeed).toHaveBeenCalledTimes(1)
    await vi.advanceTimersByTimeAsync(5000)
    expect((await result).tasks[0]?.status).toBe('todo')
    expect(loadPageSpeed).toHaveBeenCalledTimes(1)
    expect(verifyMarketingTask).toHaveBeenCalledWith({ websiteId: task.websiteId, site: task.site, websiteRevision: 4, taskId: task.id, revision: 1 }, expect.any(AbortSignal))
  })
  it('does not retry verification writes when an old completed cache is rejected', async () => {
    vi.mocked(startPageSpeed).mockResolvedValue(state('complete'))
    vi.mocked(verifyMarketingTask).mockRejectedValue(new Error('work_conflict'))
    await expect(recheckTechnicalTask(task, 4, new AbortController().signal)).rejects.toThrow('work_conflict')
    expect(verifyMarketingTask).toHaveBeenCalledTimes(1)
    expect(loadPageSpeed).not.toHaveBeenCalled()
  })
  it('stops pending polling on context cancellation without verifying', async () => {
    vi.useFakeTimers()
    vi.mocked(startPageSpeed).mockResolvedValue(state('running'))
    const controller = new AbortController()
    const request = recheckTechnicalTask(task, 4, controller.signal)
    const result = expect(request).rejects.toThrow()
    await vi.advanceTimersByTimeAsync(1)
    controller.abort()
    await result
    expect(verifyMarketingTask).not.toHaveBeenCalled()
    expect(loadPageSpeed).not.toHaveBeenCalled()
  })
  it('bounds a running audit to two minutes and never verifies incomplete reports', async () => {
    vi.useFakeTimers()
    vi.mocked(startPageSpeed).mockResolvedValue(state('running'))
    vi.mocked(loadPageSpeed).mockResolvedValue(state('running'))
    const result = expect(recheckTechnicalTask(task, 4, new AbortController().signal)).rejects.toThrow()
    await vi.advanceTimersByTimeAsync(120_001)
    await result
    expect(vi.mocked(loadPageSpeed).mock.calls.length).toBeLessThanOrEqual(24)
    expect(startPageSpeed).toHaveBeenCalledTimes(1)
    expect(verifyMarketingTask).not.toHaveBeenCalled()
  })
})
