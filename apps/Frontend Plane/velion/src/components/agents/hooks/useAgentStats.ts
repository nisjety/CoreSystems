'use client';

import { useCallback, useEffect, useState } from 'react';

/**
 * U3-9 (ui-ux-verevon-gap.md §14): client hook for per-agent run stats.
 *
 * The Convex side (`agentRuns:statsByAgent`) is reactive — but verevon's
 * `convexQuery` server helper isn't subscribed from the browser, so we
 * poll every 30 s here and refetch on demand. The cost is small (one
 * Convex query) and the UX is "live enough" for a metrics panel.
 *
 * The hook never throws: a network error returns the empty-stats shape
 * so the UI degrades gracefully to "No runs yet" instead of an error
 * banner that obscures the rest of the tab.
 */
export interface AgentRunStats {
  total: number
  started: number
  completed: number
  failed: number
  cancelled: number
  successRate: number
  avgDurationMs: number
  lookbackDays: number
  recent: Array<{
    runId: string
    status: 'started' | 'completed' | 'failed' | 'cancelled'
    startedAt: number
    completedAt: number | null
    error: string | null
  }>
}

const EMPTY_STATS: AgentRunStats = {
  total: 0,
  started: 0,
  completed: 0,
  failed: 0,
  cancelled: 0,
  successRate: 0,
  avgDurationMs: 0,
  lookbackDays: 30,
  recent: [],
}

const POLL_INTERVAL_MS = 30_000

export function useAgentStats(agentId: string | undefined): {
  data: AgentRunStats
  isLoading: boolean
  refetch: () => Promise<void>
} {
  const [data, setData] = useState<AgentRunStats>(EMPTY_STATS)
  const [isLoading, setIsLoading] = useState<boolean>(true)

  const refetch = useCallback(async () => {
    if (!agentId) return
    try {
      const res = await fetch(`/api/agents/${agentId}/stats`, {
        cache: 'no-store',
      })
      if (!res.ok) {
        setData(EMPTY_STATS)
        return
      }
      const payload = (await res.json()) as AgentRunStats
      setData(payload)
    } catch {
      setData(EMPTY_STATS)
    } finally {
      setIsLoading(false)
    }
  }, [agentId])

  useEffect(() => {
    void refetch()
    const interval = setInterval(refetch, POLL_INTERVAL_MS)
    return () => clearInterval(interval)
  }, [refetch])

  return { data, isLoading, refetch }
}
