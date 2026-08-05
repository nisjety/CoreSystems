import { requestJson } from './http'

// Ops/Quality read model (Phase 7 B6). Calls the gateway `/api/v1/eval/quality`
// proxy, which aggregates the caller's run history from session-core's
// RunService (accuracy = success rate, drift = recent-vs-prior trend). Every
// figure is real run metadata; an org with no runs returns a null-accuracy,
// zero-count rollup — never a fabricated quality score.

export type QualityByStatus = {
  completed: number
  failed: number
  cancelled: number
  running: number
  other: number
}

export type QualityDrift = {
  recentAccuracy: number | null
  priorAccuracy: number | null
  accuracyDelta: number | null
  window: string
}

export type QualityRollup = {
  accuracy: number | null
  totalRuns: number
  terminalRuns: number
  byStatus: QualityByStatus
  avgInputTokens: number
  avgOutputTokens: number
  threadsSampled: number
  drift: QualityDrift
}

const EMPTY: QualityRollup = {
  accuracy: null,
  totalRuns: 0,
  terminalRuns: 0,
  byStatus: { completed: 0, failed: 0, cancelled: 0, running: 0, other: 0 },
  avgInputTokens: 0,
  avgOutputTokens: 0,
  threadsSampled: 0,
  drift: { recentAccuracy: null, priorAccuracy: null, accuracyDelta: null, window: '' },
}

/** Org/user run-quality rollup derived from RunService run history. */
export async function getQualityRollup(): Promise<QualityRollup> {
  const data = await requestJson<Partial<QualityRollup> | null>('/api/v1/eval/quality')
  return { ...EMPTY, ...(data ?? {}) }
}
