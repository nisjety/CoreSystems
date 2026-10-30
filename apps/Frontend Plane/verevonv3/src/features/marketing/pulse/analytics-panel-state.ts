import { createTrackedEffect } from 'solid-js'
import { createSignal } from '@/shared/lib/create-immediate-signal'
import {  } from 'solid-js'
import type { PulseQuarter } from '../../../shared/api/marketing/pulse-client.ts'
import type { AnalyticsPanelState } from '../../../shared/api/marketing/analytics-boundary.ts'

type Context = { site: string; websiteId: string; revision: number; quarter?: PulseQuarter }
type Projection = { site: string; status: 'connected' | 'unavailable'; reason?: string }

/** Independent reads, cancellation and clearing are shared; source windows are not. */
export function createAnalyticsPanelState<T extends Projection>(context: () => Context, load: (context: Context, signal: AbortSignal) => Promise<T>, onState: (state: AnalyticsPanelState) => void) {
  const [snapshot, setSnapshot] = createSignal<T | null>(null)
  const [loading, setLoading] = createSignal(true)
  const [failed, setFailed] = createSignal(false)
  const [retry, setRetry] = createSignal(0)
  createTrackedEffect(() => { const cleanups: Array<() => void> = []; (() => {
    const selected = context()
    retry()
    const controller = new AbortController()
    setSnapshot(null); setLoading(true); setFailed(false)
    onState({ ...selected, status: 'loading' })
    void load(selected, controller.signal).then((value) => {
      if (controller.signal.aborted) return
      setSnapshot(() => value)
      onState({ ...selected, status: value.status, ...(value.reason ? { reason: value.reason } : {}) })
    }).catch(() => {
      if (!controller.signal.aborted) { setFailed(true); onState({ ...selected, status: 'unavailable', reason: 'source_unavailable' }) }
    }).finally(() => { if (!controller.signal.aborted) setLoading(false) })
    cleanups.push(() => controller.abort())
  })(); return () => { for (const cleanup of cleanups.reverse()) cleanup() } })
  return { snapshot, loading, failed, refresh: () => setRetry((value) => value + 1) }
}
