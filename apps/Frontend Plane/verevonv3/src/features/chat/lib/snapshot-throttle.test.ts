// @vitest-environment jsdom

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import {
  DEFAULT_SNAPSHOT_THROTTLE_MS,
  createSnapshotWriter,
  flushOnPageLifecycle,
  parseSnapshotThrottleMs,
} from './snapshot-throttle'

beforeEach(() => {
  vi.useFakeTimers()
})

afterEach(() => {
  vi.useRealTimers()
})

describe('parseSnapshotThrottleMs', () => {
  it('defaults to one second', () => {
    expect(parseSnapshotThrottleMs(undefined)).toBe(DEFAULT_SNAPSHOT_THROTTLE_MS)
    expect(DEFAULT_SNAPSHOT_THROTTLE_MS).toBe(1000)
  })

  it('accepts whole milliseconds, and 0 for per-flush writes', () => {
    expect(parseSnapshotThrottleMs('0')).toBe(0)
    expect(parseSnapshotThrottleMs('250')).toBe(250)
  })

  it('keeps the default for anything that is not whole milliseconds', () => {
    for (const raw of ['', ' ', '-1', '1.5', '1e3', 'true', '1000ms', '0x10', '9999999']) {
      expect(parseSnapshotThrottleMs(raw)).toBe(DEFAULT_SNAPSHOT_THROTTLE_MS)
    }
  })

  it('caps the window so a typo cannot park a stream for minutes', () => {
    expect(parseSnapshotThrottleMs('600000')).toBe(60_000)
  })
})

describe('createSnapshotWriter', () => {
  it('writes the first call at once and collapses the rest of the window into one trailing write', () => {
    const write = vi.fn()
    const writer = createSnapshotWriter<number>(write, { intervalMs: 1000 })

    writer.schedule(1)
    expect(write).toHaveBeenCalledTimes(1)
    for (let i = 2; i <= 40; i += 1) {
      writer.schedule(i)
      vi.advanceTimersByTime(20)
    }
    expect(write).toHaveBeenCalledTimes(1)
    expect(writer.hasPending()).toBe(true)

    vi.advanceTimersByTime(1000)
    expect(write).toHaveBeenCalledTimes(2)
    // The trailing write carries the LATEST call, not the first one it saw.
    expect(write).toHaveBeenLastCalledWith(40)
    expect(writer.hasPending()).toBe(false)
  })

  it('writes at most once per window across a long stream', () => {
    const write = vi.fn()
    const writer = createSnapshotWriter<number>(write, { intervalMs: 1000 })
    const seconds = 10
    for (let ms = 0; ms < seconds * 1000; ms += 25) {
      writer.schedule(ms)
      vi.advanceTimersByTime(25)
    }
    vi.advanceTimersByTime(1000)
    expect(write.mock.calls.length).toBeLessThanOrEqual(seconds + 1)
    expect(write.mock.calls.length).toBeGreaterThanOrEqual(seconds)
  })

  it('stays quiet when nothing is scheduled and writes a later one-off at once', () => {
    const write = vi.fn()
    const writer = createSnapshotWriter<number>(write, { intervalMs: 1000 })
    writer.schedule(1)
    vi.advanceTimersByTime(5000)
    expect(write).toHaveBeenCalledTimes(1)
    writer.schedule(2)
    expect(write).toHaveBeenCalledTimes(2)
  })

  it('flush writes the pending call now and nothing when none is pending', () => {
    const write = vi.fn()
    const writer = createSnapshotWriter<string>(write, { intervalMs: 1000 })
    writer.flush()
    expect(write).not.toHaveBeenCalled()

    writer.schedule('a')
    writer.schedule('b')
    expect(write).toHaveBeenCalledTimes(1)
    writer.flush()
    expect(write).toHaveBeenCalledTimes(2)
    expect(write).toHaveBeenLastCalledWith('b')

    writer.flush()
    vi.advanceTimersByTime(5000)
    expect(write).toHaveBeenCalledTimes(2)
  })

  it('a flush opens a new window, so the cadence holds after it', () => {
    const write = vi.fn()
    const writer = createSnapshotWriter<string>(write, { intervalMs: 1000 })
    writer.schedule('a')
    writer.schedule('b')
    writer.flush()
    writer.schedule('c')
    expect(write).toHaveBeenCalledTimes(2)
    vi.advanceTimersByTime(1000)
    expect(write).toHaveBeenCalledTimes(3)
    expect(write).toHaveBeenLastCalledWith('c')
  })

  it('cancel drops the pending call; dispose drops it and stops the timer', () => {
    const write = vi.fn()
    const writer = createSnapshotWriter<string>(write, { intervalMs: 1000 })
    writer.schedule('a')
    writer.schedule('b')
    writer.cancel()
    vi.advanceTimersByTime(2000)
    expect(write).toHaveBeenCalledTimes(1)

    writer.schedule('c')
    writer.schedule('d')
    writer.dispose()
    vi.advanceTimersByTime(2000)
    expect(write).toHaveBeenCalledTimes(2)
    expect(vi.getTimerCount()).toBe(0)
  })

  it('interval 0 writes on every call, as before the throttle', () => {
    const write = vi.fn()
    const writer = createSnapshotWriter<number>(write, { intervalMs: 0 })
    writer.schedule(1)
    writer.schedule(2)
    writer.schedule(3)
    expect(write).toHaveBeenCalledTimes(3)
    expect(vi.getTimerCount()).toBe(0)
    expect(writer.hasPending()).toBe(false)
  })

  it('a throwing write does not leave the stream unthrottled', () => {
    const write = vi.fn(() => {
      throw new Error('quota')
    })
    const writer = createSnapshotWriter<number>(write, { intervalMs: 1000 })
    expect(() => writer.schedule(1)).toThrow('quota')
    write.mockClear()
    writer.schedule(2)
    writer.schedule(3)
    expect(write).not.toHaveBeenCalled()
  })

  it('decides whether to write at write time: a guard that refuses at that moment drops the pending call', () => {
    // The controller's `write` runs its ZDR / temporary / foreign guards when the
    // write happens. The writer only moves the call in time, so a guard that
    // turns false between schedule and fire stops the write.
    let allowed = true
    const stored: string[] = []
    const writer = createSnapshotWriter<string>((value) => {
      if (allowed) stored.push(value)
    }, { intervalMs: 1000 })

    writer.schedule('first')
    writer.schedule('second')
    allowed = false
    vi.advanceTimersByTime(1000)
    writer.flush()
    expect(stored).toEqual(['first'])
  })
})

describe('flushOnPageLifecycle', () => {
  const setVisibility = (state: 'visible' | 'hidden') => {
    Object.defineProperty(document, 'visibilityState', { configurable: true, get: () => state })
  }

  afterEach(() => {
    setVisibility('visible')
  })

  it('flushes when the page is hidden, not when it becomes visible', () => {
    const flush = vi.fn()
    const unbind = flushOnPageLifecycle({ flush })
    setVisibility('visible')
    document.dispatchEvent(new Event('visibilitychange'))
    expect(flush).not.toHaveBeenCalled()
    setVisibility('hidden')
    document.dispatchEvent(new Event('visibilitychange'))
    expect(flush).toHaveBeenCalledTimes(1)
    unbind()
  })

  it('flushes on pagehide', () => {
    const flush = vi.fn()
    const unbind = flushOnPageLifecycle({ flush })
    window.dispatchEvent(new Event('pagehide'))
    expect(flush).toHaveBeenCalledTimes(1)
    unbind()
  })

  it('stops flushing after unbind', () => {
    const flush = vi.fn()
    flushOnPageLifecycle({ flush })()
    setVisibility('hidden')
    document.dispatchEvent(new Event('visibilitychange'))
    window.dispatchEvent(new Event('pagehide'))
    expect(flush).not.toHaveBeenCalled()
  })
})
