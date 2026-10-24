import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import {
  SESSION_KEEPALIVE_INTERVAL_MS,
  sessionHasHumanBrowsing,
  startSessionKeepAlive,
  type KeepAliveOutcome,
} from './session-keepalive'

describe('session keep-alive timer (WS8.D)', () => {
  beforeEach(() => {
    vi.useFakeTimers()
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('calls once per interval while active', async () => {
    const tick = vi.fn<() => Promise<KeepAliveOutcome>>(() => Promise.resolve('alive'))
    const stop = startSessionKeepAlive({ isActive: () => true, tick })

    await vi.advanceTimersByTimeAsync(SESSION_KEEPALIVE_INTERVAL_MS * 2)
    expect(tick).toHaveBeenCalledTimes(2)
    stop()
  })

  it('makes no call while the tab is hidden or the session is not live', async () => {
    const tick = vi.fn<() => Promise<KeepAliveOutcome>>(() => Promise.resolve('alive'))
    let active = false
    const stop = startSessionKeepAlive({ isActive: () => active, tick })

    await vi.advanceTimersByTimeAsync(SESSION_KEEPALIVE_INTERVAL_MS)
    expect(tick).not.toHaveBeenCalled()

    active = true
    await vi.advanceTimersByTimeAsync(SESSION_KEEPALIVE_INTERVAL_MS)
    expect(tick).toHaveBeenCalledTimes(1)
    stop()
  })

  it('does not overlap calls while one is still in flight', async () => {
    let finish: (() => void) | undefined
    const tick = vi.fn<() => Promise<KeepAliveOutcome>>(
      () => new Promise<KeepAliveOutcome>((resolve) => {
        finish = () => resolve('alive')
      }),
    )
    const stop = startSessionKeepAlive({ isActive: () => true, tick })

    await vi.advanceTimersByTimeAsync(SESSION_KEEPALIVE_INTERVAL_MS)
    await vi.advanceTimersByTimeAsync(SESSION_KEEPALIVE_INTERVAL_MS)
    expect(tick).toHaveBeenCalledTimes(1)

    finish?.()
    await vi.advanceTimersByTimeAsync(SESSION_KEEPALIVE_INTERVAL_MS)
    expect(tick).toHaveBeenCalledTimes(2)
    stop()
  })

  it('stops once and reports expiry when the grant can no longer be renewed', async () => {
    const tick = vi.fn<() => Promise<KeepAliveOutcome>>(() => Promise.resolve('expired'))
    const onExpired = vi.fn()
    startSessionKeepAlive({ isActive: () => true, tick, onExpired })

    await vi.advanceTimersByTimeAsync(SESSION_KEEPALIVE_INTERVAL_MS)
    await vi.advanceTimersByTimeAsync(SESSION_KEEPALIVE_INTERVAL_MS * 3)
    expect(tick).toHaveBeenCalledTimes(1)
    expect(onExpired).toHaveBeenCalledTimes(1)
  })

  it('a rejected call is a failed tick and the timer keeps running', async () => {
    const tick = vi
      .fn<() => Promise<KeepAliveOutcome>>()
      .mockRejectedValueOnce(new Error('network'))
      .mockResolvedValue('alive')
    const stop = startSessionKeepAlive({ isActive: () => true, tick })

    await vi.advanceTimersByTimeAsync(SESSION_KEEPALIVE_INTERVAL_MS)
    await vi.advanceTimersByTimeAsync(SESSION_KEEPALIVE_INTERVAL_MS)
    expect(tick).toHaveBeenCalledTimes(2)
    stop()
  })

  it('stop is idempotent and prevents further calls', async () => {
    const tick = vi.fn<() => Promise<KeepAliveOutcome>>(() => Promise.resolve('alive'))
    const stop = startSessionKeepAlive({ isActive: () => true, tick })
    stop()
    stop()
    await vi.advanceTimersByTimeAsync(SESSION_KEEPALIVE_INTERVAL_MS * 3)
    expect(tick).not.toHaveBeenCalled()
  })
})

describe('sessionHasHumanBrowsing', () => {
  it('reads only the human_browsing capability', () => {
    expect(sessionHasHumanBrowsing(['navigate', 'human_browsing'])).toBe(true)
    expect(sessionHasHumanBrowsing(['navigate'])).toBe(false)
    expect(sessionHasHumanBrowsing(undefined)).toBe(false)
  })
})
