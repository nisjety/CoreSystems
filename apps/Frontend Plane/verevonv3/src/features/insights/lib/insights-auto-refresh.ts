export const INSIGHTS_REFRESH_INTERVAL_MS = 60_000

/** Poll visible pages only, and refresh on reconnect/resume without overlap. */
export function startInsightsAutoRefresh(refresh: () => void, canRefresh: () => boolean): () => void {
  let lastRefresh = Date.now()
  const check = () => {
    if (document.visibilityState === 'hidden' || !canRefresh()) return
    // Visibility + focus often arrive together; don't send duplicate reads.
    if (Date.now() - lastRefresh < 5_000) return
    lastRefresh = Date.now()
    refresh()
  }
  const timer = window.setInterval(check, INSIGHTS_REFRESH_INTERVAL_MS)
  document.addEventListener('visibilitychange', check)
  window.addEventListener('focus', check)
  window.addEventListener('online', check)
  return () => {
    window.clearInterval(timer)
    document.removeEventListener('visibilitychange', check)
    window.removeEventListener('focus', check)
    window.removeEventListener('online', check)
  }
}
