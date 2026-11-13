import { ApiError } from '@/shared/api/http'
import { isZdrCode, zdrRefusalPresentation, type Translate } from '@/shared/api/zdr-refusals'
import { translateApiError } from '@/shared/i18n'

/**
 * "Ingest selected" on a run's fetched pages (E4.1-5). The page list holds no page
 * content, so ingesting a picked page is a batch that fetches its URL again and persists it.
 */

/** The most URLs one batch takes. The gateway enforces the same cap (`MAX_BATCH_URLS`). */
export const INGEST_BATCH_LIMIT = 200

/** Read at call time, so a test can stub it. Off unless the build sets it to `true`. */
export function ingestSelectedEnabled(): boolean {
  return import.meta.env.VITE_INGEST_SELECTED === 'true'
}

/** A page row of a run's results: its sequence number and its URL. */
export type PickableRow = { seq: number; url: string }

/** The picked set after one row is clicked: a row is added while the batch has room. */
export function togglePick(picked: ReadonlySet<number>, seq: number, limit = INGEST_BATCH_LIMIT): ReadonlySet<number> {
  const next = new Set(picked)
  if (next.has(seq)) next.delete(seq)
  else if (next.size < limit) next.add(seq)
  return next
}

export type IngestPlan = {
  /** Each picked URL once, in page order, at most the batch limit. */
  urls: string[]
  /** Picked rows that repeat a URL already in the batch. */
  duplicates: number
}

/** Count first: what a batch of the picked rows would send. It sends nothing. */
export function planIngest(rows: readonly PickableRow[], picked: ReadonlySet<number>, limit = INGEST_BATCH_LIMIT): IngestPlan {
  const urls: string[] = []
  const seen = new Set<string>()
  let duplicates = 0
  for (const row of rows) {
    if (!picked.has(row.seq) || !row.url) continue
    if (seen.has(row.url)) duplicates += 1
    else if (urls.length < limit) {
      seen.add(row.url)
      urls.push(row.url)
    }
  }
  return { urls, duplicates }
}

/** The batch that re-fetches these URLs and persists their pages. */
export function ingestBatchRequest(urls: readonly string[]) {
  return { kind: 'batch' as const, urls: [...urls], ingest: true as const }
}

/** The ZDR refusal code a gateway error carries in `details.zdr_refusal`, or null. */
function zdrRefusalCode(error: unknown): string | null {
  if (!(error instanceof ApiError) || Array.isArray(error.details)) return null
  const refusal = error.details?.zdr_refusal
  if (!refusal || typeof refusal !== 'object') return null
  const code = (refusal as { code?: unknown }).code
  return typeof code === 'string' && isZdrCode(code) ? code : null
}

/** Why the batch was refused: the ZDR refusal in its own words, or the generic failure. */
export function ingestSelectedMessage(error: unknown, tr: Translate): string {
  const code = zdrRefusalCode(error)
  if (code) {
    const shown = zdrRefusalPresentation(code, null, tr)
    return shown.next === null ? shown.message : `${shown.message} ${shown.next}`
  }
  return translateApiError(error, tr, {
    no: 'Sidene kunne ikke hentes på nytt og lagres.',
    en: 'The pages could not be re-fetched and stored.',
  })
}
