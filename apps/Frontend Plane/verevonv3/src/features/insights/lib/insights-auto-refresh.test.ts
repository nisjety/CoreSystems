import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { INSIGHTS_REFRESH_INTERVAL_MS, startInsightsAutoRefresh } from './insights-auto-refresh'

describe('Insights automatic refresh', () => {
  let stop: () => void
  beforeEach(() => { vi.useFakeTimers(); stop = () => {} })
  afterEach(() => { stop(); vi.restoreAllMocks(); vi.useRealTimers() })

  it('polls every minute and stops all polling when the view is disposed', () => {
    vi.spyOn(document, 'visibilityState', 'get').mockReturnValue('visible')
    const refresh = vi.fn()
    stop = startInsightsAutoRefresh(refresh, () => true)
    vi.advanceTimersByTime(INSIGHTS_REFRESH_INTERVAL_MS - 1)
    expect(refresh).not.toHaveBeenCalled()
    vi.advanceTimersByTime(1)
    expect(refresh).toHaveBeenCalledTimes(1)
    stop()
    vi.advanceTimersByTime(INSIGHTS_REFRESH_INTERVAL_MS)
    window.dispatchEvent(new Event('online'))
    expect(refresh).toHaveBeenCalledTimes(1)
  })

  it('pauses hidden pages and deduplicates simultaneous resume/focus events', () => {
    const visibility = vi.spyOn(document, 'visibilityState', 'get').mockReturnValue('hidden')
    const refresh = vi.fn()
    stop = startInsightsAutoRefresh(refresh, () => true)
    vi.advanceTimersByTime(INSIGHTS_REFRESH_INTERVAL_MS)
    expect(refresh).not.toHaveBeenCalled()
    visibility.mockReturnValue('visible')
    document.dispatchEvent(new Event('visibilitychange'))
    window.dispatchEvent(new Event('focus'))
    expect(refresh).toHaveBeenCalledTimes(1)
  })

  it('skips refreshes while a previous read is in flight', () => {
    vi.spyOn(document, 'visibilityState', 'get').mockReturnValue('visible')
    let busy = true
    const refresh = vi.fn()
    stop = startInsightsAutoRefresh(refresh, () => !busy)
    vi.advanceTimersByTime(INSIGHTS_REFRESH_INTERVAL_MS)
    expect(refresh).not.toHaveBeenCalled()
    busy = false
    window.dispatchEvent(new Event('online'))
    expect(refresh).toHaveBeenCalledTimes(1)
  })
})
