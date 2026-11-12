import { requestJson } from './http'

// ── Wire contract ───────────────────────────────────────────────────────────
// The person's read and stop of one addressable research run (Quarry v2 E4.6, Q4-062b). The gateway
// (`apps/gateway/src/domains/chat/research_runs.rs`) answers both routes under the usual `{ data }` envelope,
// camelCased, and every refusal as `{ error: { code, message, details } }`. The routes exist only while the
// gateway's `CHAT_RESEARCH_RUNS` is on: off they answer 404. Ids, status and counters only: a zero-retention run
// keeps no report id, and nothing in this module carries answer text.

/**
 * The closed words a research run can carry. `cancelled` is what the person's own stop records; `expired` is what a
 * run gets when its deadline passes before it finishes.
 */
export const RESEARCH_RUN_STATUS_WORDS = ['running', 'completed', 'stopped', 'cancelled', 'failed', 'expired'] as const

export type ResearchRunStatusWord = (typeof RESEARCH_RUN_STATUS_WORDS)[number]

/** `rsr_` and a 26-character ULID, the shape Session Core issues and the gateway checks. */
export const RESEARCH_RUN_ID_SHAPE = /^rsr_[0-9A-HJKMNP-TV-Z]{26}$/

/** One research run as the gateway reads it. */
export type ResearchRun = {
  id: string
  /** Null when the run is not tied to a thread. */
  threadId: string | null
  status: ResearchRunStatusWord
  /** Why the run ended, as the run record words it (`deadline`, `budget_usd`, ...). Null while it runs. */
  stopReason: string | null
  pagesRead: number
  sourcesFound: number
  spentUsd: number
  /** Present only for a run that keeps a report; a zero-retention or waived run has none. */
  reportId?: string
}

/** The gateway's answer to a stop (202): whether the stop reached the run's live stream. */
export type ResearchRunCancelAck = {
  id: string
  stopRequested: boolean
}

export type ResearchRunRequestOptions = {
  signal?: AbortSignal
}

/** The route path of one run. A malformed id is refused here, as the gateway would refuse it. */
function researchRunPath(id: string): string {
  if (!RESEARCH_RUN_ID_SHAPE.test(id)) throw new Error('Invalid research run id')
  return `/api/v1/chat/research-runs/${encodeURIComponent(id)}`
}

/** Reads one research run: `GET /api/v1/chat/research-runs/{id}`. A run of another owner reads as not found. */
export async function getResearchRun(id: string, options: ResearchRunRequestOptions = {}): Promise<ResearchRun> {
  return requestJson<ResearchRun>(researchRunPath(id), { signal: options.signal })
}

/**
 * Asks a run's live stream to stop: `POST /api/v1/chat/research-runs/{id}/cancel`. The gateway answers 202, and
 * the run then ends as `cancelled` with what it found so far. A run of another owner reads as not found.
 */
export async function cancelResearchRun(
  id: string,
  options: ResearchRunRequestOptions = {},
): Promise<ResearchRunCancelAck> {
  return requestJson<ResearchRunCancelAck>(`${researchRunPath(id)}/cancel`, {
    method: 'POST',
    signal: options.signal,
  })
}
