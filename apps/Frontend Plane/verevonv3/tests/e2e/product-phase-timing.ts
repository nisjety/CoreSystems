/**
 * Q14 Phase 0: where a recorded chat turn's time went, measured in the browser.
 *
 * The acceptance helper used to capture the chat stream with
 * `response.clone().text()`, which keeps the bytes and loses when each frame
 * arrived. The page now records every network read of the stream with its
 * `performance.now()` stamp; this module turns those reads back into SSE
 * frames and the frames into phase marks. The frames the product already
 * emits are the boundaries:
 *
 *   submit click → `connected`            admission and context
 *   → first source-check `step_update` running   generation (private draft)
 *   → last source-check `step_update` done       source review and repair
 *   → first `artifact`                     durable completion (sent after the terminal ack)
 *   → document rendered                    rendering
 *
 * Every value is a difference of two readings of ONE document's
 * `performance.now()`. Nothing here subtracts a host timestamp from a browser
 * one, and nothing reads the gateway's clock: when the gateway reports its own
 * `turn_timing`, that payload is kept verbatim beside the browser marks, never
 * mixed into them.
 *
 * Pure and Playwright-free so vitest can test it (`tests/product-phase-timing.test.ts`)
 * and `product-phase-report.ts` can run it under plain Node.
 */

/** One network read of the chat stream, stamped with the page's `performance.now()`. */
export type StreamChunk = { at: number; text: string }

/** What the page recorded, as absolute `performance.now()` readings of one document. */
export type BrowserClockCapture = {
  /** The submit click, stamped in the capture phase before the app handles it. */
  clickAt?: number | null
  /** The chat stream request left the page. */
  fetchStartedAt?: number | null
  /** The stream's response headers arrived. */
  headersAt?: number | null
  /** The stream body ended. */
  endAt?: number | null
  /** The existing `-timing` DOM marks, as absolute readings rather than offsets. */
  acknowledgementAt?: number | null
  firstAnswerAt?: number | null
  /** First non-empty rendered document body that was not already on screen. */
  artifactRenderedAt?: number | null
  chunks: StreamChunk[]
}

/** One SSE frame, stamped with the read that completed it. */
export type TimedFrame = {
  event: string
  id?: string
  /** ms from the phase origin (normally the submit click). */
  atMs: number
  /** `step_update` only: the step's id and state — no title or detail text. */
  step?: { id: string; status: string }
  /** `error` only: the machine-readable code. */
  code?: string
}

export type PhaseOutcome = 'completed' | 'failed' | 'stopped' | 'incomplete'

export const PHASE_MARKS = [
  'headers',
  'connected',
  'firstChunk',
  'generationEnded',
  'validationEnded',
  'firstArtifact',
  'artifactRendered',
  'acknowledgement',
  'firstAnswer',
  'terminal',
  'streamEnd',
] as const
export type PhaseMark = (typeof PHASE_MARKS)[number]

export const PHASE_DURATIONS = [
  'toHeaders',
  'toConnected',
  'generation',
  'validation',
  'publication',
  'rendering',
  'total',
] as const
export type PhaseDuration = (typeof PHASE_DURATIONS)[number]

export type PhaseReport = {
  version: 1
  label: string
  /** Stable grouping key for a cohort: spec, test (without sample counters), label. */
  workload: string
  clock: 'browser-performance-now'
  origin: 'submit-click' | 'stream-request'
  outcome: PhaseOutcome
  /** ms from the origin; `null` when the turn never reached that point. */
  marks: Record<PhaseMark, number | null>
  /** ms between marks, browser clock only; `null` when either end is missing. */
  durations: Record<PhaseDuration, number | null>
  frameCount: number
  frames: TimedFrame[]
  /** The gateway's own `turn_timing` payload, verbatim, when it sent one. */
  turnTiming: Record<string, unknown> | null
}

type RawFrame = { event?: string; id?: string; data: string; at: number }

/**
 * Re-split timed stream reads into SSE frames, exactly as the app's reader
 * (`src/shared/api/sse.ts`) does: lines end in `\n` (a trailing `\r` is
 * dropped), a blank line dispatches the frame if it carried data, `:` lines
 * are comments, and an unterminated frame at end of stream is not dispatched.
 * Each frame gets the stamp of the read that delivered its terminating blank
 * line — the moment the app could first have acted on it.
 */
export function splitTimedFrames(chunks: readonly StreamChunk[]): RawFrame[] {
  const frames: RawFrame[] = []
  let buffer = ''
  let current: { event?: string; id?: string } = {}
  const data: string[] = []
  for (const chunk of chunks) {
    buffer += chunk.text
    const lines = buffer.split('\n')
    buffer = lines.pop() ?? ''
    for (const raw of lines) {
      const line = raw.endsWith('\r') ? raw.slice(0, -1) : raw
      if (line === '') {
        if (data.length > 0) frames.push({ ...current, data: data.join('\n'), at: chunk.at })
        current = {}
        data.length = 0
        continue
      }
      if (line.startsWith(':')) continue
      const colon = line.indexOf(':')
      const field = colon === -1 ? line : line.slice(0, colon)
      const value = colon === -1 ? '' : line.slice(colon + 1).replace(/^ /, '')
      if (field === 'data') data.push(value)
      else if (field === 'event') current.event = value
      else if (field === 'id') current.id = value
    }
  }
  return frames
}

function parseJson(data: string): Record<string, unknown> | null {
  try {
    const value: unknown = JSON.parse(data)
    return value && typeof value === 'object' && !Array.isArray(value) ? value as Record<string, unknown> : null
  } catch {
    return null
  }
}

const round = (ms: number) => Math.round(ms * 10) / 10

/** Spec file, test title without a trailing sample counter (`3/20`, `#3`), and label. */
export function phaseWorkload(file: string, title: string, label: string): string {
  const spec = (file.split(/[\\/]/).at(-1) ?? file).replace(/\.spec\.[cm]?[jt]s$/, '')
  const test = title.trim().replace(/\s*(?:\(\s*)?(?:#\s*\d+|\d+\s*\/\s*\d+)(?:\s*\))?$/, '').trim()
  return [spec, test, label].filter(Boolean).join(' › ')
}

/** Turn the page's clock capture into the `-phases` attachment. */
export function productPhaseReport(input: { label: string; workload: string; capture: BrowserClockCapture }): PhaseReport {
  const { capture } = input
  const originAt = capture.clickAt ?? capture.fetchStartedAt ?? capture.chunks[0]?.at ?? 0
  const origin: PhaseReport['origin'] = capture.clickAt != null ? 'submit-click' : 'stream-request'
  const since = (at: number | null | undefined) => (at == null ? null : round(at - originAt))

  const raw = splitTimedFrames(capture.chunks)
  let turnTiming: Record<string, unknown> | null = null
  const frames: TimedFrame[] = raw.map((frame) => {
    const event = frame.event ?? 'message'
    const timed: TimedFrame = { event, atMs: since(frame.at) ?? 0 }
    if (frame.id !== undefined) timed.id = frame.id
    if (event === 'step_update') {
      const payload = parseJson(frame.data)
      const id = typeof payload?.id === 'string' ? payload.id : typeof payload?.step_id === 'string' ? payload.step_id : undefined
      if (id) timed.step = { id, status: typeof payload?.status === 'string' ? payload.status : '' }
    } else if (event === 'error') {
      const payload = parseJson(frame.data)
      const nested = payload?.error && typeof payload.error === 'object' ? payload.error as Record<string, unknown> : undefined
      const code = typeof payload?.code === 'string' ? payload.code : typeof nested?.code === 'string' ? nested.code : undefined
      if (code) timed.code = code
    } else if (event === 'turn_timing') {
      turnTiming = parseJson(frame.data) ?? turnTiming
    }
    return timed
  })

  const first = (match: (frame: TimedFrame) => boolean) => frames.find(match)?.atMs ?? null
  const last = (match: (frame: TimedFrame) => boolean) => frames.filter(match).at(-1)?.atMs ?? null
  const sourceCheck = (status: string) => (frame: TimedFrame) => frame.event === 'step_update'
    && Boolean(frame.step?.id.endsWith(':source-check'))
    && frame.step?.status === status
  const terminalFrame = frames.find((frame) => frame.event === 'done' || frame.event === 'error' || frame.event === 'stopped')

  const marks: Record<PhaseMark, number | null> = {
    headers: since(capture.headersAt),
    connected: first((frame) => frame.event === 'connected'),
    firstChunk: first((frame) => frame.event === 'chunk'),
    generationEnded: first(sourceCheck('running')),
    validationEnded: last(sourceCheck('done')),
    firstArtifact: first((frame) => frame.event === 'artifact'),
    artifactRendered: since(capture.artifactRenderedAt),
    acknowledgement: since(capture.acknowledgementAt),
    firstAnswer: since(capture.firstAnswerAt),
    terminal: terminalFrame?.atMs ?? null,
    streamEnd: since(capture.endAt),
  }
  const between = (from: number | null, to: number | null) => (from == null || to == null ? null : round(to - from))
  const durations: Record<PhaseDuration, number | null> = {
    toHeaders: marks.headers,
    toConnected: marks.connected,
    generation: between(marks.connected, marks.generationEnded),
    validation: between(marks.generationEnded, marks.validationEnded),
    publication: between(marks.validationEnded, marks.firstArtifact),
    rendering: between(marks.firstArtifact, marks.artifactRendered),
    // A failed or stopped turn still has a duration: its terminal frame. Only
    // a stream that never ended cleanly falls back to its last byte.
    total: marks.terminal ?? marks.streamEnd,
  }
  const outcome: PhaseOutcome = terminalFrame?.event === 'done'
    ? 'completed'
    : terminalFrame?.event === 'error'
      ? 'failed'
      : terminalFrame?.event === 'stopped'
        ? 'stopped'
        : 'incomplete'
  return {
    version: 1,
    label: input.label,
    workload: input.workload,
    clock: 'browser-performance-now',
    origin,
    outcome,
    marks,
    durations,
    frameCount: frames.length,
    frames,
    turnTiming,
  }
}

export function isPhaseReport(value: unknown): value is PhaseReport {
  if (!value || typeof value !== 'object') return false
  const record = value as Record<string, unknown>
  return record.version === 1
    && typeof record.workload === 'string'
    && typeof record.outcome === 'string'
    && Boolean(record.durations) && typeof record.durations === 'object'
}

/** Below this many samples a p95 is one or two runs, not a percentile. */
export const MIN_P95_SAMPLES = 20

export type MetricSummary = {
  n: number
  p50: number | null
  /** `null` with `p95Withheld` when the cohort is too small to state one. */
  p95: number | null
  p95Withheld?: string
  max: number | null
}

export type WorkloadSummary = {
  workload: string
  runs: number
  outcomes: Record<PhaseOutcome, number>
  metrics: Record<PhaseDuration, MetricSummary>
}

/** Nearest-rank percentile of an ascending list: the smallest value with at least q of the samples at or below it. */
export function nearestRank(sorted: readonly number[], q: number): number | null {
  if (sorted.length === 0) return null
  const rank = Math.max(1, Math.ceil(q * sorted.length))
  return sorted[Math.min(sorted.length, rank) - 1] ?? null
}

/**
 * Per-workload p50/p95/max for every duration.
 *
 * Failed, stopped and incomplete runs STAY in the cohort. Dropping them
 * would report the latency of the runs that happened to succeed — exactly the
 * survivorship Q14 exists to avoid (the earlier timeout takes were the slow
 * tail). Their outcome counts are reported beside the numbers.
 */
export function summarizeCohort(reports: readonly PhaseReport[]): WorkloadSummary[] {
  const byWorkload = new Map<string, PhaseReport[]>()
  for (const report of reports) {
    const group = byWorkload.get(report.workload) ?? []
    group.push(report)
    byWorkload.set(report.workload, group)
  }
  return [...byWorkload.entries()]
    .sort(([left], [right]) => left.localeCompare(right))
    .map(([workload, group]) => {
      const outcomes: Record<PhaseOutcome, number> = { completed: 0, failed: 0, stopped: 0, incomplete: 0 }
      for (const report of group) outcomes[report.outcome] = (outcomes[report.outcome] ?? 0) + 1
      const metrics = Object.fromEntries(PHASE_DURATIONS.map((name) => {
        const values = group
          .map((report) => report.durations[name])
          .filter((value): value is number => typeof value === 'number' && Number.isFinite(value))
          .sort((left, right) => left - right)
        const summary: MetricSummary = {
          n: values.length,
          p50: nearestRank(values, 0.5),
          p95: values.length >= MIN_P95_SAMPLES ? nearestRank(values, 0.95) : null,
          max: values.at(-1) ?? null,
        }
        if (values.length < MIN_P95_SAMPLES) {
          summary.p95Withheld = `needs at least ${MIN_P95_SAMPLES} samples, has ${values.length}`
        }
        return [name, summary]
      })) as Record<PhaseDuration, MetricSummary>
      return { workload, runs: group.length, outcomes, metrics }
    })
}
