'use client'

// U2-2 / U3-2 (ui-ux-velion-gap.md §10): live-catalog hook for the model
// picker. Fetches `/api/models` (proxy to capability-core's
// `/api/v1/capabilities?kind=model`) and falls back to the static
// `SUPPORTED_MODELS` map in `app/api/chat/_lib/models.ts` so the picker
// keeps working when capability-core is unreachable.
//
// The static fallback is intentionally narrow — same 4 models the chat
// path already supports (`gpt-4o-mini`, `gpt-5-mini`, `claude-sonnet-4-5`,
// `claude-opus-4-1`) — so a live registry that adds a new model surfaces
// automatically while a degraded backend never crashes the UI.

import { useEffect, useState } from 'react'

import { SUPPORTED_MODELS, type SupportedModel } from '@/app/api/chat/_lib/models'

export interface ModelOption {
  id: string
  label: string
  provider?: string
  tier?: 'low' | 'high'
  /** True when sourced from capability-core, false when fallback. */
  live: boolean
}

interface ApiModelsResponse {
  status: 'ok' | 'service_unavailable'
  models?: Array<{
    id: string
    name: string
    provider: string
    label: string
    tier: 'low' | 'high'
    description?: string
  }>
  detail?: string
}

function staticFallback(): ModelOption[] {
  return (Object.keys(SUPPORTED_MODELS) as SupportedModel[]).map((id) => ({
    id,
    label: SUPPORTED_MODELS[id].label,
    provider: SUPPORTED_MODELS[id].provider,
    tier: SUPPORTED_MODELS[id].tier,
    live: false,
  }))
}

/**
 * React hook that returns the live model catalog. While loading it returns
 * the static fallback so the picker never renders empty. When the fetch
 * fails the hook stays on the fallback and exposes `status='fallback'`
 * for callers that want to render a degraded-state banner.
 */
export function useModels(): {
  models: ModelOption[]
  status: 'loading' | 'live' | 'fallback'
} {
  const [models, setModels] = useState<ModelOption[]>(() => staticFallback())
  const [status, setStatus] = useState<'loading' | 'live' | 'fallback'>(
    'loading',
  )

  useEffect(() => {
    let cancelled = false

    async function load(): Promise<void> {
      try {
        const res = await fetch('/api/models', { cache: 'no-store' })
        if (!res.ok) {
          if (!cancelled) setStatus('fallback')
          return
        }
        const data = (await res.json()) as ApiModelsResponse
        if (data.status !== 'ok' || !data.models || data.models.length === 0) {
          if (!cancelled) setStatus('fallback')
          return
        }

        const live: ModelOption[] = data.models.map((m) => ({
          id: m.name, // gateway expects the deployment / Anthropic name, not the UUID
          label: m.label || m.name,
          provider: m.provider,
          tier: m.tier,
          live: true,
        }))

        // Merge: prefer live entries; keep any static fallback id that the
        // live catalog doesn't yet include so a registry-misconfiguration
        // doesn't strand a model the chat path actually supports.
        const liveIds = new Set(live.map((m) => m.id))
        const merged = [
          ...live,
          ...staticFallback().filter((m) => !liveIds.has(m.id)),
        ]

        if (!cancelled) {
          setModels(merged)
          setStatus('live')
        }
      } catch {
        if (!cancelled) setStatus('fallback')
      }
    }

    void load()
    return () => {
      cancelled = true
    }
  }, [])

  return { models, status }
}
