import { requestJson } from './http'

/**
 * Change-monitoring client — Phase 1 Track C (the "monitor" leg).
 *
 * Talks to the gateway `monitoring` domain (`/api/v1/monitoring/*`), which
 * proxies quarry-edge's `/v1/change/*` routes. Every value here traces to a
 * live edge response: a `check` runs a real fetch + baseline comparison, and
 * `latest` / `history` read whatever baselines the org has actually accrued.
 * There is no scheduling — this is on-demand only.
 */

/** A captured version of a URL (quarry `BaselineSnapshot`, camelCased shape). */
export interface BaselineSnapshot {
  baselineId: string
  orgId: string
  sourceUrl: string
  fingerprint: string
  artifactId?: string | null
  prevBaselineId?: string | null
  runId?: string | null
  capturedAt: string
}

export type ChangeStatus = 'new' | 'unchanged' | 'changed' | 'unreachable'

/** Result of comparing a fresh fetch against the latest baseline. */
export interface ChangeRecord {
  sourceUrl: string
  orgId: string
  status: ChangeStatus
  newBaseline?: BaselineSnapshot | null
  prevBaseline?: BaselineSnapshot | null
  diffId?: string | null
  checkedAt: string
}

/**
 * Run an on-demand check of `url`: the gateway fetches the page, fingerprints
 * it, and compares against the org's latest baseline. Returns the change status.
 */
export function checkUrlNow(url: string, signal?: AbortSignal): Promise<ChangeRecord> {
  return requestJson<ChangeRecord>('/api/v1/monitoring/check', {
    method: 'POST',
    body: JSON.stringify({ url }),
    signal,
  })
}

/**
 * The most recent baseline for `url`, or `null` when the org has never captured
 * it. (The gateway normalizes the edge's 404 to a `null` payload.)
 */
export function getLatestBaseline(url: string, signal?: AbortSignal): Promise<BaselineSnapshot | null> {
  return requestJson<BaselineSnapshot | null>(
    `/api/v1/monitoring/latest?url=${encodeURIComponent(url)}`,
    { signal },
  )
}

/**
 * Newest-first baseline chain for `url`. Empty until baselines accrue — never a
 * synthesized entry.
 */
export function getChangeHistory(
  url: string,
  limit = 50,
  signal?: AbortSignal,
): Promise<BaselineSnapshot[]> {
  return requestJson<BaselineSnapshot[]>(
    `/api/v1/monitoring/history?url=${encodeURIComponent(url)}&limit=${limit}`,
    { signal },
  )
}
