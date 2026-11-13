import { gatewayBaseUrl } from './config'
import { apiErrorFromResponse, signalSessionExpiry } from './http'

/**
 * The owner's read of one background process's output in a run (EP-1 W4-P4, F-t3-035): the BFF route
 * `GET /api/v1/chat/runs/{run_id}/processes/{pid}/output` (W4-P3), which relays Model Gateway's page unchanged.
 *
 * The page sits under `data`, and its cursor and `has_more` flag sit under `meta`. The envelope is therefore read
 * whole here: `requestJson` keeps `data` alone and would drop the cursor. The route and Model Gateway decide what the
 * caller may read; this client decides nothing. Output comes back as text and is never parsed as markup.
 */

export type RunProcessOutputChunk = {
  /** The chunk's sequence number in the process's output. */
  readonly seq: number
  /** The stream the text came from, as Model Gateway names it: `stdout`, `stderr` or `unspecified`. */
  readonly stream: string
  /** Plain text, exactly as the process wrote it. */
  readonly text: string
  readonly endsWithNewline: boolean
}

export type RunProcessOutput = {
  /** The process state as Model Gateway names it: `STARTING`, `RUNNING`, `EXITED`, `KILLED`, `EXPIRED`, `LOST` or `UNSPECIFIED`. */
  readonly state: string
  /** The exit code once the process has exited; `null` while it has not. */
  readonly exitCode: number | null
  readonly chunks: readonly RunProcessOutputChunk[]
  /** The cursor to pass as `afterSeq` on the next read. Never below the cursor this read was made with. */
  readonly nextCursor: number
  /** More output is retained after `nextCursor`, so the next read can follow at once. */
  readonly hasMore: boolean
  /** Output that was produced and then trimmed lies before this page. */
  readonly gapBefore: boolean
}

type Envelope = { data?: unknown; meta?: unknown }

function record(value: unknown): Record<string, unknown> | undefined {
  return value && typeof value === 'object' && !Array.isArray(value) ? (value as Record<string, unknown>) : undefined
}

function wholeNumber(value: unknown): number | undefined {
  return typeof value === 'number' && Number.isInteger(value) && value >= 0 ? value : undefined
}

function chunkFrom(value: unknown): RunProcessOutputChunk | undefined {
  const raw = record(value)
  const seq = wholeNumber(raw?.seq)
  if (!raw || seq === undefined || typeof raw.text !== 'string') return undefined
  return {
    seq,
    stream: typeof raw.stream === 'string' ? raw.stream : 'unspecified',
    text: raw.text,
    endsWithNewline: raw.ends_with_newline === true,
  }
}

/**
 * The page an envelope holds. A body without a `data` record reads as an unknown page with no output: it is never
 * read as output.
 */
function pageFrom(body: Envelope | null, afterSeq: number): RunProcessOutput {
  const data = record(body?.data)
  const meta = record(body?.meta)
  const rawChunks = data?.chunks
  const chunks: RunProcessOutputChunk[] = []
  if (Array.isArray(rawChunks)) {
    for (const entry of rawChunks) {
      const chunk = chunkFrom(entry)
      if (chunk) chunks.push(chunk)
    }
  }
  const reported = wholeNumber(meta?.next_cursor) ?? wholeNumber(data?.next_cursor) ?? afterSeq
  const furthest = chunks.reduce((latest, chunk) => Math.max(latest, chunk.seq), afterSeq)
  const stateValue = data?.state
  const state = typeof stateValue === 'string' ? stateValue.trim().toUpperCase() : ''
  const exit = data?.exit_code
  return {
    state: state || 'UNSPECIFIED',
    exitCode: typeof exit === 'number' && Number.isInteger(exit) ? exit : null,
    chunks,
    nextCursor: Math.max(afterSeq, reported, furthest),
    hasMore: meta?.has_more === true || data?.has_more === true,
    gapBefore: data?.gap_before === true,
  }
}

/** The route's path and query for one page of a process's output, read after `afterSeq`. */
export function runProcessOutputPath(runId: string, pid: string, spaceRef: string, afterSeq: number): string {
  const query = new URLSearchParams({ space_ref: spaceRef, after_seq: String(afterSeq) })
  return `/api/v1/chat/runs/${encodeURIComponent(runId)}/processes/${encodeURIComponent(pid)}/output?${query.toString()}`
}

/**
 * One page of a process's output in a run, read after `afterSeq` (0 reads from the start of what is retained).
 *
 * Resolves `null` when the read is refused with 404 (the route is off, or the process is not in this run) or 403 (the
 * read was declined): there is no output this person may read, and that is not an error. Every other failure is thrown
 * as the typed `ApiError` the other clients throw.
 */
export async function readRunProcessOutput(
  runId: string,
  pid: string,
  spaceRef: string,
  afterSeq: number,
  signal?: AbortSignal,
): Promise<RunProcessOutput | null> {
  const path = runProcessOutputPath(runId, pid, spaceRef, afterSeq)
  const headers = new Headers({ Accept: 'application/json' })
  // The same local dev bypass `requestJson` applies, so this read behaves like the other reads in development.
  if (import.meta.env.VITE_ALLOW_DEV_AUTH_BYPASS === 'true') headers.set('Authorization', 'Bearer dev-bypass')

  const response = await fetch(`${gatewayBaseUrl()}${path}`, { credentials: 'include', headers, signal })
  const body = (await response.json().catch(() => null)) as Envelope | null
  if (!response.ok) {
    const failure = apiErrorFromResponse(body, response)
    signalSessionExpiry(failure, path)
    if (failure.status === 404 || failure.status === 403) return null
    throw failure
  }
  return pageFrom(body, afterSeq)
}
