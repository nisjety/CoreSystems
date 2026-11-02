import type { Page } from '@playwright/test'

/**
 * Synthetic workload and in-page instruments for `chat-transcript-performance.spec.ts`.
 *
 * WS09 S1 measures the transcript before anything is optimised. Everything here
 * is generated: no real content, no real account data, no model call. The page
 * is served by the dev stack (it needs a signed-in session to render `/chat`),
 * but every chat endpoint the workload touches is answered inside the page by a
 * `window.fetch` shim, so the numbers do not depend on a model or on session-core.
 *
 * Why a fetch shim and not `page.route`: `route.fulfill` hands the browser the
 * whole body at once, and the thing under test is a stream arriving at 40 frames
 * per second. The shim returns a `Response` over a `ReadableStream` fed by
 * timers, the same technique the demo runtime uses (`src/demo/install.ts`).
 *
 * Privacy: the instruments record counts, lengths and timestamps. The storage
 * wrapper never keeps a value, only its length.
 */

export const PERF_THREAD_ID = 'perf-thread-200'
export const PERF_EMPTY_THREAD_ID = 'perf-thread-empty'
/** The local transcript cache: the key the per-flush rewrite lands on. */
export const TRANSCRIPTS_KEY = 'verevon.chat.threadTranscripts.v1'
export const ACTIVE_THREAD_KEY = 'verevon.chat.threadId'

/** The stress profile of the implementation plan, section 21.1. */
export const WORKLOAD = {
  coldOpenTurns: 200,
  streamParagraphs: 20,
  frameRateHz: 40,
  /** Words per streamed delta: the size of a typical provider chunk. */
  wordsPerDelta: 6,
  /** Server latency the shim adds to each request it answers, so a sequential waterfall shows as one. */
  shimLatencyMs: 30,
  heavyTurnEvidence: { citations: 12, toolCalls: 8 },
} as const

const FRAME_INTERVAL_MS = Math.round(1000 / WORKLOAD.frameRateHz)

const WORDS = [
  'omsetningen', 'kvartalet', 'marginen', 'kunder', 'offentlig', 'sektor', 'veksten', 'kostnader',
  'leveranser', 'avtalen', 'prosjektet', 'rapporten', 'budsjettet', 'risikoen', 'planen', 'teamet',
]

/** Deterministic text: the same index always yields the same words. */
function words(seed: number, count: number): string {
  const out: string[] = []
  for (let index = 0; index < count; index += 1) {
    out.push(WORDS[(seed * 7 + index * 3 + (index >> 2)) % WORDS.length] as string)
  }
  return out.join(' ')
}

function paragraph(seed: number): string {
  return `Avsnitt ${seed}: ${words(seed, 34)}. ${words(seed + 5, 12)}.`
}

/** An assistant answer of a few paragraphs, with a list on every third one. */
function answerText(turn: number): string {
  const blocks = [`## Svar ${turn}`, paragraph(turn), paragraph(turn + 1)]
  if (turn % 3 === 0) blocks.push(`- ${words(turn, 6)}\n- ${words(turn + 1, 6)}\n- ${words(turn + 2, 6)}`)
  blocks.push(paragraph(turn + 2))
  return blocks.join('\n\n')
}

const isoAt = (turn: number) => new Date(Date.UTC(2026, 9, 1, 8, 0, 0) + turn * 60_000).toISOString()

type ThreadFixture = {
  session: { threadId: string; title: string; preview: string; updatedAt: string; pinned: boolean; origin: string }
  messages: Array<{ id: string; role: 'user' | 'assistant'; content: string; created_at: string }>
  transcript: { thread_id: string; updated_at: string; turns: Array<Record<string, unknown>> }
}

/**
 * A synthetic thread of `turnCount` messages, alternating user and assistant.
 * Every tenth assistant turn carries the heavy evidence of the stress profile
 * (12 citations, 8 tool calls) in the transcript the shim serves, so the
 * derivation's optional branches are populated on a realistic share of turns.
 */
export function buildThreadFixture(turnCount: number, threadId = PERF_THREAD_ID): ThreadFixture {
  const messages: ThreadFixture['messages'] = []
  const turns: ThreadFixture['transcript']['turns'] = []
  for (let index = 0; index < turnCount; index += 1) {
    const role = index % 2 === 0 ? 'user' : 'assistant'
    const content = role === 'user' ? `Spørsmål ${index}: ${words(index, 14)}?` : answerText(index)
    const id = `perf-msg-${index}`
    const createdAt = isoAt(index)
    messages.push({ id, role, content, created_at: createdAt })
    const heavy = role === 'assistant' && index % 20 === 1
    turns.push({
      id,
      role,
      content,
      createdAt,
      ...(heavy
        ? {
            citations: Array.from({ length: WORKLOAD.heavyTurnEvidence.citations }, (_, n) => ({
              id: `c-${index}-${n}`,
              title: `Kvartalsrapport ${n}`,
              url: `https://example.invalid/rapport-${index}-${n}`,
              snippet: 'Omsetning og margin per segment.',
            })),
            toolCalls: Array.from({ length: WORKLOAD.heavyTurnEvidence.toolCalls }, (_, n) => ({
              id: `t-${index}-${n}`,
              name: 'knowledge_search',
              status: 'done',
              args: { query: `omsetning ${n}` },
              output: 'Fant 4 dokumenter.',
            })),
          }
        : {}),
    })
  }
  const last = messages.at(-1)
  const updatedAt = last?.created_at ?? isoAt(0)
  return {
    session: {
      threadId,
      title: 'Kvartalsoversikt',
      preview: (last?.content ?? '').slice(0, 120),
      updatedAt,
      pinned: false,
      origin: 'chat',
    },
    messages,
    transcript: { thread_id: threadId, updated_at: updatedAt, turns },
  }
}

/** The text the streamed answer ends on; the spec waits for it to be on screen. */
export const STREAM_END_MARKER = 'Slutt på svaret.'

/**
 * The SSE script of one streamed answer: `connected`, then deltas of about six
 * words across `paragraphs` paragraphs, then `done`. Frames are pre-serialised so
 * the shim does no work but timing.
 */
export function buildStreamFrames(threadId: string, paragraphs = WORKLOAD.streamParagraphs): string[] {
  const frame = (id: number, event: string, data: object) => `id: ${id}\nevent: ${event}\ndata: ${JSON.stringify(data)}\n\n`
  const text: string[] = []
  for (let index = 0; index < paragraphs; index += 1) text.push(paragraph(1000 + index))
  text[text.length - 1] = `${text[text.length - 1]} ${STREAM_END_MARKER}`
  const tokens = text.join('\n\n').split(/(?<=\s)/u)
  const frames = [frame(1, 'connected', { ok: true, request_id: 'perf-req-1', thread_id: threadId })]
  for (let start = 0; start < tokens.length; start += WORKLOAD.wordsPerDelta) {
    frames.push(frame(frames.length + 1, 'chunk', { delta: tokens.slice(start, start + WORKLOAD.wordsPerDelta).join('') }))
  }
  frames.push(frame(frames.length + 1, 'done', { request_id: 'perf-req-1', done: true, model_used: 'perf-fixture' }))
  return frames
}

/** One request the page made, stamped with the page's own clock. */
export type PerfRequest = {
  method: string
  path: string
  search: string
  start: number
  end: number
  /** Answered by the shim (true) or passed through to the real stack (false). */
  shimmed: boolean
  status: number
}

export type PerfSnapshot = {
  now: number
  requests: PerfRequest[]
  marks: Record<string, number>
  /** `setItem` calls and value lengths per `storage:key`, for the watched keys only. */
  setItem: Record<string, { count: number; bytes: number }>
  longTasks: Array<{ start: number; duration: number }>
  loaf: Array<{ start: number; duration: number; blocking: number }>
  /** requestAnimationFrame timestamps; gaps between them are frame times. */
  frames: number[]
  mutations: { records: number; added: number; removed: number }
  domNodes: number
  heapUsed: number | null
}

export type PerfShimConfig = {
  threadId: string
  /** Sessions the listing answers with; empty for the empty-thread control. */
  sessions: unknown[]
  messages: unknown[]
  transcript: unknown | null
  streamFrames: string[]
  frameIntervalMs: number
  latencyMs: number
  /** Marks `allTurnsMounted` once this many `.verevon-chat-message` elements exist. */
  expectTurns: number
  watchedKeys: string[]
}

declare global {
  interface Window {
    __chatPerf?: { snapshot: () => PerfSnapshot; hasMark: (name: string) => boolean }
  }
}

/**
 * Installed with `page.addInitScript`, so it runs in the page before any app
 * code. Self-contained on purpose: Playwright serialises the function source, so
 * it may not close over anything in this module.
 */
export function installPerfShim(config: PerfShimConfig): void {
  const requests: PerfRequest[] = []
  const marks: Record<string, number> = {}
  const setItem: PerfSnapshot['setItem'] = {}
  const longTasks: PerfSnapshot['longTasks'] = []
  const loaf: PerfSnapshot['loaf'] = []
  const frames: number[] = []
  const mutations = { records: 0, added: 0, removed: 0 }
  const mark = (name: string) => {
    if (marks[name] === undefined) marks[name] = performance.now()
  }

  // Storage writes: count and length per watched key, never the value.
  const originalSetItem = Storage.prototype.setItem
  Storage.prototype.setItem = function patched(this: Storage, key: string, value: string) {
    if (config.watchedKeys.includes(key)) {
      const label = `${this === window.localStorage ? 'local' : 'session'}:${key}`
      const entry = (setItem[label] ??= { count: 0, bytes: 0 })
      entry.count += 1
      entry.bytes += String(value).length
    }
    return originalSetItem.call(this, key, value)
  }

  // Long tasks and long animation frames, each only where the browser has them.
  const supported = PerformanceObserver.supportedEntryTypes ?? []
  if (supported.includes('longtask')) {
    new PerformanceObserver((list) => {
      for (const entry of list.getEntries()) longTasks.push({ start: entry.startTime, duration: entry.duration })
    }).observe({ type: 'longtask', buffered: true })
  }
  if (supported.includes('long-animation-frame')) {
    new PerformanceObserver((list) => {
      for (const entry of list.getEntries()) {
        const blocking = (entry as unknown as { blockingDuration?: number }).blockingDuration ?? 0
        loaf.push({ start: entry.startTime, duration: entry.duration, blocking })
      }
    }).observe({ type: 'long-animation-frame', buffered: true })
  }

  // Frame times: the gap between consecutive animation frames.
  const sampleFrame = (at: number) => {
    if (frames.length < 50_000) frames.push(at)
    requestAnimationFrame(sampleFrame)
  }
  requestAnimationFrame(sampleFrame)

  // DOM churn and the cold-open marks, from one observer.
  const turns = document.getElementsByClassName('verevon-chat-message')
  const watchDom = () => {
    new MutationObserver((records) => {
      mutations.records += records.length
      for (const record of records) {
        mutations.added += record.addedNodes.length
        mutations.removed += record.removedNodes.length
      }
      if (turns.length >= 1) mark('firstTurnVisible')
      if (turns.length >= config.expectTurns) mark('allTurnsMounted')
      if (document.querySelector('form textarea')) mark('composerReady')
    }).observe(document, { childList: true, subtree: true })
  }
  watchDom()

  const json = (body: unknown, status = 200) =>
    new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } })
  const delay = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms))

  const streamResponse = (signal: AbortSignal | null | undefined) => {
    const encoder = new TextEncoder()
    let next = 0
    let aborted = false
    signal?.addEventListener('abort', () => { aborted = true }, { once: true })
    mark('streamRequest')
    return new Response(
      new ReadableStream<Uint8Array>({
        async pull(controller) {
          if (aborted || next >= config.streamFrames.length) {
            mark('streamEnd')
            controller.close()
            return
          }
          if (next > 0) await delay(config.frameIntervalMs)
          if (next === 1) mark('streamStart')
          controller.enqueue(encoder.encode(config.streamFrames[next] as string))
          next += 1
        },
      }),
      { status: 200, headers: { 'content-type': 'text/event-stream' } },
    )
  }

  const realFetch = window.fetch.bind(window)
  window.fetch = async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = new URL(input instanceof Request ? input.url : String(input), location.href)
    const method = (init?.method ?? (input instanceof Request ? input.method : 'GET')).toUpperCase()
    const { pathname } = url
    const start = performance.now()
    const record = (shimmed: boolean, status: number) => {
      if (pathname.startsWith('/api/')) requests.push({ method, path: pathname, search: url.search, start, end: performance.now(), shimmed, status })
    }
    const thread = /^\/api\/v1\/chat\/threads\/([^/]+)(?:\/(transcript|messages|events))?$/u.exec(pathname)
    const known = thread && decodeURIComponent(thread[1] as string) === config.threadId
    let response: Response | null = null
    if (pathname === '/api/v1/chat/stream' && method === 'POST') {
      response = streamResponse(init?.signal)
    } else if (pathname === '/api/v1/chat/threads' && method === 'GET') {
      await delay(config.latencyMs)
      response = json({ sessions: config.sessions, retention: { zdr: false } })
    } else if (thread && method === 'GET') {
      await delay(config.latencyMs)
      const part = thread[2]
      if (part === 'transcript') response = json({ transcript: known ? config.transcript : null })
      else if (part === 'messages') response = json({ messages: known ? config.messages : [] })
      else if (part === 'events') response = json({ thread_id: config.threadId, events: [], truncated: false })
    } else if (thread && !thread[2] && method === 'PUT') {
      await delay(config.latencyMs)
      response = json({ session: config.sessions[0] ?? null })
    }
    if (response) {
      record(true, response.status)
      return response
    }
    const real = await realFetch(input, init)
    record(false, real.status)
    return real
  }

  window.__chatPerf = {
    // Polled while waiting; cheap, unlike a full snapshot.
    hasMark: (name: string) => marks[name] !== undefined,
    snapshot: () => ({
      now: performance.now(),
      requests: requests.slice(),
      marks: { ...marks },
      setItem: JSON.parse(JSON.stringify(setItem)) as PerfSnapshot['setItem'],
      longTasks: longTasks.slice(),
      loaf: loaf.slice(),
      frames: frames.slice(),
      mutations: { ...mutations },
      domNodes: document.getElementsByTagName('*').length,
      heapUsed: (performance as unknown as { memory?: { usedJSHeapSize: number } }).memory?.usedJSHeapSize ?? null,
    }),
  }
}

/** Builds the shim config for the populated thread or the empty-thread control. */
export function shimConfig(kind: 'populated' | 'empty'): PerfShimConfig {
  if (kind === 'empty') {
    return {
      threadId: PERF_EMPTY_THREAD_ID,
      sessions: [],
      messages: [],
      transcript: null,
      streamFrames: buildStreamFrames(PERF_EMPTY_THREAD_ID),
      frameIntervalMs: FRAME_INTERVAL_MS,
      latencyMs: WORKLOAD.shimLatencyMs,
      expectTurns: 1,
      watchedKeys: [TRANSCRIPTS_KEY],
    }
  }
  const thread = buildThreadFixture(WORKLOAD.coldOpenTurns)
  return {
    threadId: PERF_THREAD_ID,
    sessions: [thread.session],
    messages: thread.messages,
    transcript: thread.transcript,
    streamFrames: buildStreamFrames(PERF_THREAD_ID),
    frameIntervalMs: FRAME_INTERVAL_MS,
    latencyMs: WORKLOAD.shimLatencyMs,
    expectTurns: WORKLOAD.coldOpenTurns,
    watchedKeys: [TRANSCRIPTS_KEY],
  }
}

/** Installs the shim and, for the populated case, points the page at the thread; call before `goto`. */
export async function installPerf(page: Page, config: PerfShimConfig, options: { selectThread: boolean }) {
  await page.addInitScript(
    ([threadId, select, activeKey]) => {
      // A cold open: nothing cached locally, as on a device that never saw the thread.
      window.localStorage.setItem('verevon.locale', 'no')
      for (const key of ['verevon.chat.threadId', 'verevon.chat.threadHistory.v1', 'verevon.chat.threadTranscripts.v1', 'verevon.chat.pendingLaunch']) {
        window.localStorage.removeItem(key)
        window.sessionStorage.removeItem(key)
      }
      if (select) window.localStorage.setItem(activeKey as string, threadId as string)
    },
    [config.threadId, options.selectThread, ACTIVE_THREAD_KEY] as const,
  )
  await page.addInitScript(installPerfShim, config)
}

export async function waitForMark(page: Page, name: string, timeout = 60_000): Promise<void> {
  await page.waitForFunction((mark) => window.__chatPerf?.hasMark(mark) === true, name, { timeout })
}

export async function readPerf(page: Page): Promise<PerfSnapshot> {
  return page.evaluate(() => {
    const perf = window.__chatPerf
    if (!perf) throw new Error('the perf shim is not installed')
    return perf.snapshot()
  })
}

/** Nearest-rank percentile of a list of numbers; 0 for an empty list. */
export function percentile(values: number[], p: number): number {
  if (values.length === 0) return 0
  const sorted = [...values].sort((a, b) => a - b)
  const rank = Math.min(sorted.length - 1, Math.max(0, Math.ceil((p / 100) * sorted.length) - 1))
  return sorted[rank] as number
}

/** Standard deviation over mean, as a fraction; 0 when there is one sample or a zero mean. */
export function coefficientOfVariation(values: number[]): number {
  if (values.length < 2) return 0
  const mean = values.reduce((total, value) => total + value, 0) / values.length
  if (mean === 0) return 0
  const variance = values.reduce((total, value) => total + (value - mean) ** 2, 0) / values.length
  return Math.sqrt(variance) / mean
}

/** Frame gaps (ms) between consecutive animation frames that fall inside `[from, to]`. */
export function frameGaps(frames: number[], from: number, to: number): number[] {
  const inside = frames.filter((at) => at >= from && at <= to)
  return inside.slice(1).map((at, index) => at - (inside[index] as number))
}

export type RequestAccounting = {
  total: number
  byBucket: Record<string, number>
  /** GETs repeated with the same path and query: `[key, count]`. */
  duplicateGets: Array<[string, number]>
  /** Disjoint busy periods: a request that starts after every earlier one ended opens a new wave. */
  sequentialWaves: number
}

/** The thread-open request budget: what was asked, what was asked twice, and how serially. */
export function accountRequests(requests: PerfRequest[]): RequestAccounting {
  const api = requests.filter((request) => request.path.startsWith('/api/')).sort((a, b) => a.start - b.start)
  const byBucket: Record<string, number> = {}
  const gets = new Map<string, number>()
  for (const request of api) {
    const bucket = request.path.replace(/\/api\/v1\/chat\/threads\/[^/]+/u, '/api/v1/chat/threads/:id')
    byBucket[`${request.method} ${bucket}`] = (byBucket[`${request.method} ${bucket}`] ?? 0) + 1
    if (request.method === 'GET') {
      const key = `${request.path}${request.search}`
      gets.set(key, (gets.get(key) ?? 0) + 1)
    }
  }
  let waves = 0
  let waveEnd = Number.NEGATIVE_INFINITY
  for (const request of api) {
    if (request.start >= waveEnd) waves += 1
    waveEnd = Math.max(waveEnd, request.end)
  }
  return {
    total: api.length,
    byBucket,
    duplicateGets: [...gets.entries()].filter(([, count]) => count > 1).sort((a, b) => b[1] - a[1]),
    sequentialWaves: waves,
  }
}

/** Mean and coefficient of variation per numeric metric across repeated runs of one scenario. */
export function summarise(reports: Array<Record<string, number | null>>): Record<string, { mean: number; cv: number; runs: number }> {
  const out: Record<string, { mean: number; cv: number; runs: number }> = {}
  const keys = new Set(reports.flatMap((report) => Object.keys(report)))
  for (const key of keys) {
    const values = reports.map((report) => report[key]).filter((value): value is number => typeof value === 'number')
    if (values.length === 0) continue
    out[key] = { mean: values.reduce((total, value) => total + value, 0) / values.length, cv: coefficientOfVariation(values), runs: values.length }
  }
  return out
}
