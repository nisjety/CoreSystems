import { beforeEach, describe, expect, it, vi } from 'vitest'

vi.mock('@/shared/api/browser-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/browser-client')>()),
  setBrowserControlMode: vi.fn(),
  getBrowserSession: vi.fn(),
}))

import { getBrowserSession, setBrowserControlMode, type BrowserSessionResponse } from '@/shared/api/browser-client'
import { ApiError } from '@/shared/api/http'
import { changeBrowserControl, isStaleControlEpoch } from './browser-control'

function projection(epoch: number, mode: 'agent_control' | 'human_takeover'): BrowserSessionResponse {
  return {
    session: {
      capabilities: [],
      control: { mode, epoch },
      id: 'run-1',
      profile: { scope: 'run_scoped', storage: 'isolated' },
      renderMode: 'chromium',
      status: 'live',
      title: 'Example',
      url: 'https://example.com/',
      viewport: { width: 1280, height: 800 },
    },
  }
}

beforeEach(() => {
  vi.mocked(setBrowserControlMode).mockReset()
  vi.mocked(getBrowserSession).mockReset()
})

describe('changeBrowserControl (WS9.I)', () => {
  it('sends the epoch the person saw and returns the projection after the change', async () => {
    vi.mocked(setBrowserControlMode).mockResolvedValue(projection(2, 'human_takeover'))

    const outcome = await changeBrowserControl('org-1', 'run-1', 'human_takeover', 1)

    expect(setBrowserControlMode).toHaveBeenCalledWith('org-1', 'run-1', 'human_takeover', { expectedEpoch: 1 })
    expect(outcome.stale).toBe(false)
    expect(outcome.response.session.control?.epoch).toBe(2)
    expect(getBrowserSession).not.toHaveBeenCalled()
  })

  it('a stale epoch refetches the projection exactly once and is not retried', async () => {
    vi.mocked(setBrowserControlMode).mockRejectedValue(
      new ApiError('Browser control changed elsewhere.', 409, 'browser_control_epoch_stale'),
    )
    vi.mocked(getBrowserSession).mockResolvedValue(projection(3, 'agent_control'))

    const outcome = await changeBrowserControl('org-1', 'run-1', 'human_takeover', 1)

    expect(outcome.stale).toBe(true)
    expect(outcome.response.session.control).toEqual({ mode: 'agent_control', epoch: 3 })
    expect(setBrowserControlMode).toHaveBeenCalledTimes(1)
    expect(getBrowserSession).toHaveBeenCalledTimes(1)
    expect(getBrowserSession).toHaveBeenCalledWith('org-1', 'run-1')
  })

  it('any other failure is rethrown unchanged, with no refetch', async () => {
    const failure = new ApiError('server error', 500, 'INTERNAL')
    vi.mocked(setBrowserControlMode).mockRejectedValue(failure)

    await expect(changeBrowserControl('org-1', 'run-1', 'agent_control', 2)).rejects.toBe(failure)
    expect(getBrowserSession).not.toHaveBeenCalled()
  })

  it('sends no epoch when none is known, so the change stays unconditional', async () => {
    vi.mocked(setBrowserControlMode).mockResolvedValue(projection(0, 'agent_control'))

    await changeBrowserControl('org-1', 'run-1', 'agent_control', null)

    expect(setBrowserControlMode).toHaveBeenCalledWith('org-1', 'run-1', 'agent_control', { expectedEpoch: null })
  })
})

describe('isStaleControlEpoch', () => {
  it('recognises only the typed stale-epoch code', () => {
    expect(isStaleControlEpoch(new ApiError('x', 409, 'browser_control_epoch_stale'))).toBe(true)
    expect(isStaleControlEpoch(new ApiError('x', 409, 'USER_TOOK_OVER'))).toBe(false)
    expect(isStaleControlEpoch(new Error('browser_control_epoch_stale'))).toBe(false)
  })
})
