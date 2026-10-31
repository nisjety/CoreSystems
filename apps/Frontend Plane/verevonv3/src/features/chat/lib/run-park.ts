/**
 * Display-only readers of the frames of a PARKED run (build plan WS03 S8-A).
 *
 * When a pinned run stops on a typed, recoverable failure (an exhausted subscription window, an
 * unreachable provider, a credential that must be renewed), the executor may park it instead of
 * failing it: the run stays open and can be resumed later. The gateway then says so on the
 * owner's turn stream with a `run_parked` frame (`{run_id, request_id, status: 'parked', reason,
 * resume_after}`), and the orchestration subject carries `run_parked {run_id}` (no reason: it is
 * never broadcast) and `run_resumed {run_id, resumed_segment}`.
 *
 * This module only READS those frames and words the reason. It makes no request, offers no
 * retry and holds no credential: the resume of a parked run is the park form of the continue
 * call in `@/shared/api/orchestration-client`, and nothing calls it yet. A reason this build does not
 * know is not displayed as if it were understood: [runParkedFrameOf] gives null, and the stream
 * reader reports the frame as an unknown event.
 */

import type { Locale } from '@/shared/i18n/locales'

/**
 * The park reasons the executor writes today, a closed set mirrored from model-gateway's
 * `run_parked_event`. `crash_interrupted` exists in the Rust enum but is reserved for a later
 * slice (no executor writes it), so it is deliberately not here.
 */
export const RUN_PARK_REASONS = [
  'credential_window',
  'provider_exhausted',
  'upstream_unavailable',
  'reauth_required',
] as const

export type RunParkReason = (typeof RUN_PARK_REASONS)[number]

/** A `run_parked` frame of the owner's turn stream. Ids, a closed reason and a time only. */
export type RunParkedFrame = {
  runId: string
  requestId?: string
  reason: RunParkReason
  /** When the run can be resumed (RFC 3339), or null when the frame names no readable time. */
  resumeAfter: string | null
}

/** A `run_resumed` frame: the run and the segment its resume started. */
export type RunResumedFrame = {
  runId: string
  resumedSegment: number
}

function asRecord(value: unknown): Record<string, unknown> | null {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null
}

function nonEmptyString(value: unknown): string | undefined {
  return typeof value === 'string' && value.length > 0 ? value : undefined
}

/**
 * An RFC 3339 timestamp, the one shape model-gateway's `run_parked_event` writes (it re-formats
 * the value with `to_rfc3339_opts`). `Date.parse` alone is not a guard: V8's legacy parser
 * accepts free text such as 'hello 1'.
 */
const RFC3339 = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d+)?(Z|[+-]\d{2}:\d{2})$/

function rfc3339Of(value: unknown): string | null {
  return typeof value === 'string' && RFC3339.test(value) && !Number.isNaN(Date.parse(value)) ? value : null
}

function isParkReason(value: unknown): value is RunParkReason {
  return typeof value === 'string' && (RUN_PARK_REASONS as readonly string[]).includes(value)
}

/**
 * Reads the `run_parked` frame of a turn stream. Null unless the frame names its run, says
 * `status: 'parked'` exactly and carries one of [RUN_PARK_REASONS]: the orchestration subject's
 * `run_parked` (a run id and nothing else) is therefore not read here. `resume_after` is kept
 * only when it is an RFC 3339 timestamp that also parses; any other string is dropped, never
 * echoed. Nothing else of the frame is carried.
 */
export function runParkedFrameOf(payload: unknown): RunParkedFrame | null {
  const record = asRecord(payload)
  if (!record) return null
  const runId = nonEmptyString(record.run_id)
  if (!runId || record.status !== 'parked' || !isParkReason(record.reason)) return null
  const requestId = nonEmptyString(record.request_id)
  return {
    runId,
    ...(requestId ? { requestId } : {}),
    reason: record.reason,
    resumeAfter: rfc3339Of(record.resume_after),
  }
}

/**
 * Reads a `run_resumed` frame (the resume of a parked run, not of an approval). Null without a
 * run id, or when the segment is not a non-negative integer: the same rule the run console
 * applies to the same frame.
 */
export function runResumedFrameOf(payload: unknown): RunResumedFrame | null {
  const record = asRecord(payload)
  if (!record) return null
  const runId = nonEmptyString(record.run_id)
  const segment = record.resumed_segment
  if (!runId || typeof segment !== 'number' || !Number.isSafeInteger(segment) || segment < 0) return null
  return { runId, resumedSegment: segment }
}

const PARK_REASON_LABELS: Record<RunParkReason, { no: string; en: string }> = {
  credential_window: {
    no: 'Satt på pause: tilgangen til kjøringen gikk ut.',
    en: 'Paused: the access for this run ran out.',
  },
  provider_exhausted: {
    no: 'Satt på pause: leverandørens bruksgrense er nådd.',
    en: "Paused: the provider's usage limit was reached.",
  },
  upstream_unavailable: {
    no: 'Satt på pause: leverandøren er midlertidig utilgjengelig.',
    en: 'Paused: the provider is temporarily unavailable.',
  },
  reauth_required: {
    no: 'Satt på pause: kontoen må kobles til på nytt.',
    en: 'Paused: the account needs to be reconnected.',
  },
}

/**
 * The words for a park reason, in Norwegian (`no`) or English (`en`). It says that the run is
 * paused and why, and never offers or implies a retry. A reason this build does not know gets
 * the generic label, never the raw token.
 */
export function parkReasonLabel(reason: string, locale: Locale): string {
  const labels = isParkReason(reason) ? PARK_REASON_LABELS[reason] : undefined
  if (!labels) return locale === 'no' ? 'Satt på pause.' : 'Paused.'
  return locale === 'no' ? labels.no : labels.en
}
