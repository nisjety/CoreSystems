'use client';

import { useCallback, useEffect, useState } from 'react';

import type { PersistedAgent } from '../types';

/**
 * U3-12 (ui-ux-velion-gap.md §14): per-agent cron schedules.
 *
 * Wraps `/api/cron` (which proxies to model-gateway `/v1/cron`, which
 * proxies to capability-core's cron table). Each cron row owns an
 * `agent_id` reference so the operator can schedule recurring agent
 * runs — e.g. "every weekday at 09:00, ask the news-summarizer agent
 * for an industry digest and post to Slack".
 *
 * The hook scopes the org-wide list to the current agent by filtering
 * client-side. Capability-core doesn't index by agent_id (cron tables
 * stay small — usually <100 rows per org), so the cost is negligible.
 *
 * Cron payload shape (matches the gateway's pass-through):
 *   {
 *     id?: string,                 // present on existing rows
 *     agent_id: string,
 *     name: string,                // human label, e.g. "Daily digest"
 *     schedule: string,            // standard 5-field cron expression
 *     payload: { prompt: string }, // the message to send into the agent
 *     enabled: boolean,
 *   }
 */

export interface CronEntry {
  id: string
  agent_id: string
  name: string
  schedule: string
  payload: { prompt?: string }
  enabled: boolean
  created_at?: string
  updated_at?: string
}

export interface CronDraft {
  name: string
  schedule: string
  prompt: string
  enabled?: boolean
}

interface UseAgentCronReturn {
  entries: CronEntry[]
  isLoading: boolean
  error: string | null
  create: (draft: CronDraft) => Promise<void>
  remove: (id: string) => Promise<void>
  toggle: (id: string, enabled: boolean) => Promise<void>
  reload: () => Promise<void>
}

export function useAgentCron(agent: PersistedAgent | null): UseAgentCronReturn {
  const [entries, setEntries] = useState<CronEntry[]>([])
  const [isLoading, setIsLoading] = useState<boolean>(true)
  const [error, setError] = useState<string | null>(null)

  const reload = useCallback(async (): Promise<void> => {
    if (!agent) {
      setEntries([])
      setIsLoading(false)
      return
    }
    try {
      const res = await fetch('/api/cron', { cache: 'no-store' })
      if (!res.ok) {
        throw new Error(`list failed (${res.status})`)
      }
      const payload = (await res.json()) as { items?: CronEntry[] } | CronEntry[]
      const all: CronEntry[] = Array.isArray(payload) ? payload : (payload.items ?? [])
      setEntries(all.filter((e) => e.agent_id === agent.id))
      setError(null)
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : 'unknown error')
      setEntries([])
    } finally {
      setIsLoading(false)
    }
  }, [agent])

  useEffect(() => {
    void reload()
  }, [reload])

  const create = useCallback(
    async (draft: CronDraft): Promise<void> => {
      if (!agent) return
      try {
        const res = await fetch('/api/cron', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({
            agent_id: agent.id,
            name: draft.name.trim() || 'Untitled schedule',
            schedule: draft.schedule.trim(),
            payload: { prompt: draft.prompt },
            enabled: draft.enabled ?? true,
          }),
        })
        if (!res.ok) {
          throw new Error(`create failed (${res.status})`)
        }
        await reload()
      } catch (e: unknown) {
        setError(e instanceof Error ? e.message : 'create failed')
      }
    },
    [agent, reload],
  )

  const remove = useCallback(
    async (id: string): Promise<void> => {
      try {
        const res = await fetch(`/api/cron/${encodeURIComponent(id)}`, {
          method: 'DELETE',
        })
        if (!res.ok && res.status !== 404) {
          throw new Error(`delete failed (${res.status})`)
        }
        await reload()
      } catch (e: unknown) {
        setError(e instanceof Error ? e.message : 'delete failed')
      }
    },
    [reload],
  )

  const toggle = useCallback(
    async (id: string, enabled: boolean): Promise<void> => {
      try {
        const res = await fetch(`/api/cron/${encodeURIComponent(id)}`, {
          method: 'PATCH',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ enabled }),
        })
        if (!res.ok) {
          throw new Error(`patch failed (${res.status})`)
        }
        await reload()
      } catch (e: unknown) {
        setError(e instanceof Error ? e.message : 'patch failed')
      }
    },
    [reload],
  )

  return { entries, isLoading, error, create, remove, toggle, reload }
}
