import { expect, test, type Page, type TestInfo } from '@playwright/test'
import { mkdirSync, readdirSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
import {
  PERF_EMPTY_THREAD_ID,
  PERF_THREAD_ID,
  STREAM_END_MARKER,
  TRANSCRIPTS_KEY,
  WORKLOAD,
  accountRequests,
  buildStreamFrames,
  buildThreadFixture,
  coefficientOfVariation,
  frameGaps,
  installPerf,
  percentile,
  readPerf,
  shimConfig,
  summarise,
  waitForMark,
  type PerfRequest,
  type PerfSnapshot,
} from './chat-perf-fixtures'

/**
 * Transcript performance, measured in the browser (WS09 S1, "measure first").
 *
 * Four scenarios on a synthetic workload (the stress profile of implementation
 * plan section 21.1; no real content, no model call):
 *
 *   A  cold open of a 200-turn thread: time to all turns mounted, DOM size, heap
 *   B  a ~20-paragraph answer streamed at 40 frames/s into that thread
 *   C  the same stream into an empty thread: the control B is read against
 *   D  cold-start request accounting: what the open asks for, twice, and serially
 *
 * The chat endpoints are answered inside the page (`chat-perf-fixtures.ts`), so
 * the dev stack is needed only to sign in and serve the SPA: run it under the
 * `chat` project (`local-setup` dependency) against a second dev server,
 *
 *   LOCAL_E2E_BASE_URL=http://localhost:5174 pnpm exec playwright test \
 *     --project=chat tests/e2e/chat-transcript-performance.spec.ts --repeat-each=3
 *
 * Each scenario writes its JSON report to `test-results/chat-perf/` and attaches
 * it to the run. The reports are the evidence; the gate is separate: budgets in
 * `chat-perf-budget.json` are asserted only when `PERF_ENFORCE=1`, because
 * milliseconds on a bind-mounted host are noisy and a budget nobody has measured
 * is a guess. Structural counts (mounts, writes, requests) are the stable part.
 *
 * The "fixture-only" block needs no stack and no sign-in:
 *
 *   pnpm exec playwright test --project=chat --no-deps tests/e2e/chat-transcript-performance.spec.ts -g "fixture-only"
 */

const MESSAGE = '.verevon-chat-message'
const COMPOSER = 'form textarea'
const REPORT_DIR = 'test-results/chat-perf'
const BUDGET = JSON.parse(readFileSync(fileURLToPath(new URL('./chat-perf-budget.json', import.meta.url)), 'utf8')) as {
  budget: Record<string, Record<string, number | null>>
}

type Metrics = Record<string, number | null>
const mb = (bytes: number | null) => (bytes === null ? null : Math.round((bytes / 1_048_576) * 10) / 10)
const sum = (values: number[]) => values.reduce((total, value) => total + value, 0)
const round = (value: number) => Math.round(value * 10) / 10

/** Writes the scenario's report next to the others and attaches it to the run. */
async function report(info: TestInfo, scenario: string, browserVersion: string, metrics: Metrics, extra: object = {}) {
  mkdirSync(REPORT_DIR, { recursive: true })
  const body = JSON.stringify({ scenario, run: info.repeatEachIndex, chromium: browserVersion, workload: WORKLOAD, metrics, ...extra }, null, 2)
  writeFileSync(join(REPORT_DIR, `${scenario}-${info.repeatEachIndex}.json`), body)
  await info.attach(`chat-perf-${scenario}.json`, { body, contentType: 'application/json' })
}

/** Asserted only under `PERF_ENFORCE=1`; a null budget is "not measured yet" and fails rather than passes. */
function enforce(scenario: string, metrics: Metrics) {
  if (process.env.PERF_ENFORCE !== '1') return
  const budget = BUDGET.budget[scenario] ?? {}
  for (const [key, limit] of Object.entries(budget)) {
    expect(limit, `${scenario}.${key} has no budget: copy a measured baseline into chat-perf-budget.json first`).not.toBeNull()
    const value = metrics[key]
    expect(value, `${scenario}.${key} was not measured`).not.toBeNull()
    expect(value as number, `${scenario}.${key} over budget`).toBeLessThanOrEqual(limit as number)
  }
}

function coldOpenMetrics(snapshot: PerfSnapshot): Metrics {
  const mounted = snapshot.marks.allTurnsMounted ?? null
  return {
    allTurnsMountedMs: mounted === null ? null : round(mounted),
    composerReadyMs: snapshot.marks.composerReady === undefined ? null : round(snapshot.marks.composerReady),
    domNodes: snapshot.domNodes,
    heapUsedMb: mb(snapshot.heapUsed),
    longTaskTotalMs: round(sum(snapshot.longTasks.filter((task) => mounted === null || task.start <= mounted).map((task) => task.duration))),
  }
}

/** The stream window: from the second frame (the first delta) to the stream closing. */
function streamingMetrics(before: PerfSnapshot, after: PerfSnapshot): Metrics {
  const from = after.marks.streamStart ?? 0
  const to = after.marks.streamEnd ?? after.now
  const gaps = frameGaps(after.frames, from, to)
  const watched = (snapshot: PerfSnapshot, field: 'count' | 'bytes') => snapshot.setItem[`local:${TRANSCRIPTS_KEY}`]?.[field] ?? 0
  return {
    streamMs: round(to - from),
    p95FrameGapMs: round(percentile(gaps, 95)),
    maxFrameGapMs: round(Math.max(0, ...gaps)),
    longTaskTotalMs: round(sum(after.longTasks.filter((task) => task.start >= from && task.start <= to).map((task) => task.duration))),
    loafBlockingMs: round(sum(after.loaf.filter((entry) => entry.start >= from && entry.start <= to).map((entry) => entry.blocking))),
    transcriptSetItemCount: watched(after, 'count') - watched(before, 'count'),
    transcriptSetItemBytes: watched(after, 'bytes') - watched(before, 'bytes'),
    mutationRecords: after.mutations.records - before.mutations.records,
    domNodesAdded: after.domNodes - before.domNodes,
    heapGrowthMb: after.heapUsed !== null && before.heapUsed !== null ? mb(after.heapUsed - before.heapUsed) : null,
  }
}

async function sendMessage(page: Page) {
  const composer = page.locator(COMPOSER)
  await composer.click()
  await composer.fill('Lag en lang oppsummering av kvartalet.')
  await page.locator('form button[type="submit"]').click()
}

async function coldOpen(page: Page, kind: 'populated' | 'empty') {
  await installPerf(page, shimConfig(kind), { selectThread: kind === 'populated' })
  await page.goto('/chat')
  await expect(page.locator(COMPOSER)).toBeVisible()
  if (kind === 'populated') await waitForMark(page, 'allTurnsMounted')
}

async function streamOnce(page: Page): Promise<{ before: PerfSnapshot; after: PerfSnapshot }> {
  const before = await readPerf(page)
  await sendMessage(page)
  await waitForMark(page, 'streamEnd')
  await expect(page.locator(`${MESSAGE}--assistant`).last()).toContainText(STREAM_END_MARKER)
  // A settle, so the last flushes and the terminal snapshot are inside the window.
  await page.waitForTimeout(500)
  return { before, after: await readPerf(page) }
}

test.describe('chat transcript performance: browser benchmark', () => {
  test.describe.configure({ mode: 'serial' })

  test('A cold open of a 200-turn thread', async ({ page, browser }, info) => {
    await coldOpen(page, 'populated')
    await page.waitForTimeout(1_500)
    const metrics = coldOpenMetrics(await readPerf(page))
    await report(info, 'coldOpen', browser.version(), metrics)
    enforce('coldOpen', metrics)
  })

  test('B a 20-paragraph answer at 40 frames/s into the 200-turn thread', async ({ page, browser }, info) => {
    await coldOpen(page, 'populated')
    const { before, after } = await streamOnce(page)
    const metrics = streamingMetrics(before, after)
    await report(info, 'streaming', browser.version(), metrics, { turnsBefore: WORKLOAD.coldOpenTurns, domNodesBefore: before.domNodes })
    enforce('streaming', metrics)
  })

  test('C the same stream into an empty thread (control)', async ({ page, browser }, info) => {
    await coldOpen(page, 'empty')
    const { before, after } = await streamOnce(page)
    const metrics = streamingMetrics(before, after)
    await report(info, 'streamingControl', browser.version(), metrics, { threadId: PERF_EMPTY_THREAD_ID })
    enforce('streamingControl', metrics)
  })

  test('D cold-start request accounting for the 200-turn thread', async ({ page, browser }, info) => {
    // Real-network /api/ calls, counted by the browser itself: the shim answers the chat endpoints,
    // so anything seen here is a hydrator that fell through to the stack.
    let passThrough = 0
    page.on('request', (request) => {
      if (new URL(request.url()).pathname.startsWith('/api/')) passThrough += 1
    })
    await coldOpen(page, 'populated')
    await page.waitForTimeout(1_500)
    const snapshot = await readPerf(page)
    const accounting = accountRequests(snapshot.requests)
    const metrics: Metrics = {
      total: accounting.total,
      duplicateGetCount: accounting.duplicateGets.reduce((total, [, count]) => total + count - 1, 0),
      sequentialWaves: accounting.sequentialWaves,
      passThroughApiRequests: passThrough,
    }
    await report(info, 'requests', browser.version(), metrics, { byBucket: accounting.byBucket, duplicateGets: accounting.duplicateGets })
    enforce('requests', metrics)
  })

  test('E coefficient of variation across the repeated runs', async ({ browser }, info) => {
    const files = (() => {
      try {
        return readdirSync(REPORT_DIR).filter((name) => /^(coldOpen|streaming|streamingControl|requests)-\d+\.json$/u.test(name))
      } catch {
        return []
      }
    })()
    const byScenario = new Map<string, Metrics[]>()
    for (const name of files) {
      const parsed = JSON.parse(readFileSync(join(REPORT_DIR, name), 'utf8')) as { scenario: string; metrics: Metrics }
      byScenario.set(parsed.scenario, [...(byScenario.get(parsed.scenario) ?? []), parsed.metrics])
    }
    test.skip(byScenario.size === 0, 'run scenarios A-D first (with --repeat-each=3 for a meaningful spread)')
    const summary = Object.fromEntries([...byScenario.entries()].map(([scenario, runs]) => [scenario, summarise(runs)]))
    await report(info, 'summary', browser.version(), {}, { summary })
  })
})

test.describe('chat transcript performance: fixture-only (no stack, no sign-in)', () => {
  // No session: these never reach the app, only the fixtures and the in-page instruments.
  test.use({ storageState: { cookies: [], origins: [] } })

  test('the synthetic thread is deterministic, alternating and sized to the profile', () => {
    const first = buildThreadFixture(WORKLOAD.coldOpenTurns)
    expect(JSON.stringify(buildThreadFixture(WORKLOAD.coldOpenTurns))).toBe(JSON.stringify(first))
    expect(first.messages).toHaveLength(WORKLOAD.coldOpenTurns)
    expect(first.messages.map((message) => message.role).slice(0, 4)).toEqual(['user', 'assistant', 'user', 'assistant'])
    expect(first.transcript.turns.map((turn) => turn.id)).toEqual(first.messages.map((message) => message.id))
    // One heavy assistant turn in ten, each with the profile's evidence.
    const heavy = first.transcript.turns.filter((turn) => Array.isArray(turn.citations))
    expect(heavy).toHaveLength(WORKLOAD.coldOpenTurns / 20)
    expect(heavy[0]?.citations).toHaveLength(WORKLOAD.heavyTurnEvidence.citations)
    expect(heavy[0]?.toolCalls).toHaveLength(WORKLOAD.heavyTurnEvidence.toolCalls)
    // Synthetic only: no address, no real host.
    const serialised = JSON.stringify(first)
    expect(serialised).not.toMatch(/@|https?:\/\/(?!example\.invalid)/u)
    expect(serialised.length).toBeLessThan(2 * 1024 * 1024)
    expect(first.session.threadId).toBe(PERF_THREAD_ID)
  })

  test('the stream script is connected, deltas, then done, and ends on the marker', () => {
    const frames = buildStreamFrames(PERF_THREAD_ID)
    expect(frames[0]).toContain('event: connected')
    expect(frames.at(-1)).toContain('event: done')
    const deltas = frames.filter((frame) => frame.includes('event: chunk')).map((frame) => (JSON.parse(frame.split('data: ')[1] as string) as { delta: string }).delta)
    const text = deltas.join('')
    expect(text.split('\n\n')).toHaveLength(WORKLOAD.streamParagraphs)
    expect(text.endsWith(STREAM_END_MARKER)).toBe(true)
    expect(deltas.length).toBeGreaterThan(100)
    // Ids are strictly increasing, as the client's resume cursor expects.
    const ids = frames.map((frame) => Number(/^id: (\d+)/u.exec(frame)?.[1]))
    expect(ids).toEqual(ids.map((_, index) => index + 1))
  })

  test('request accounting counts duplicates and sequential waves', () => {
    const request = (path: string, start: number, end: number, method = 'GET', search = ''): PerfRequest => ({ method, path, search, start, end, shimmed: true, status: 200 })
    const accounting = accountRequests([
      request('/api/v1/chat/threads', 0, 30),
      request('/api/v1/chat/threads', 40, 70),
      request('/api/v1/chat/threads/t1/transcript', 80, 110),
      request('/api/v1/chat/threads/t1/messages', 80, 120),
      request('/health', 0, 5),
    ])
    expect(accounting.total).toBe(4)
    expect(accounting.duplicateGets).toEqual([['/api/v1/chat/threads', 2]])
    expect(accounting.sequentialWaves).toBe(3)
    expect(accounting.byBucket['GET /api/v1/chat/threads/:id/transcript']).toBe(1)
  })

  test('percentile, frame gaps and coefficient of variation', () => {
    expect(percentile([], 95)).toBe(0)
    expect(percentile([1, 2, 3, 4, 100], 95)).toBe(100)
    expect(percentile([1, 2, 3, 4, 100], 50)).toBe(3)
    expect(frameGaps([0, 16, 32, 80, 96], 10, 90)).toEqual([16, 48])
    expect(coefficientOfVariation([10])).toBe(0)
    expect(coefficientOfVariation([10, 10, 10])).toBe(0)
    expect(coefficientOfVariation([9, 11])).toBeCloseTo(0.1, 5)
    expect(summarise([{ a: 1, b: null }, { a: 3, b: null }])).toEqual({ a: { mean: 2, cv: 0.5, runs: 2 } })
  })

  test('the in-page shim serves the fixtures, streams at cadence and records its instruments', async ({ page }) => {
    const config = { ...shimConfig('populated'), expectTurns: 1 }
    await page.route('http://perf-harness.invalid/**', (route) => route.fulfill({
      status: 200,
      contentType: 'text/html',
      body: '<!doctype html><title>harness</title><form><textarea></textarea></form><div class="verevon-chat-message"></div>',
    }))
    await installPerf(page, config, { selectThread: false })
    await page.goto('http://perf-harness.invalid/chat')
    await waitForMark(page, 'allTurnsMounted', 10_000)

    const served = await page.evaluate(async ([threadId, key]) => {
      const listing = await (await fetch('/api/v1/chat/threads')).json() as { sessions: unknown[] }
      const transcript = await (await fetch(`/api/v1/chat/threads/${threadId}/transcript`)).json() as { transcript: { turns: unknown[] } }
      const other = await (await fetch('/api/v1/chat/threads/other/messages')).json() as { messages: unknown[] }
      const response = await fetch('/api/v1/chat/stream', { method: 'POST', body: '{}' })
      const reader = (response.body as ReadableStream<Uint8Array>).getReader()
      const arrivals: number[] = []
      for (;;) {
        const { done } = await reader.read()
        if (done) break
        arrivals.push(performance.now())
      }
      localStorage.setItem(key as string, 'abcd')
      sessionStorage.setItem(key as string, 'abcdef')
      localStorage.setItem('not-watched', 'x')
      await new Promise((resolve) => setTimeout(resolve, 100))
      return {
        sessions: listing.sessions.length,
        turns: transcript.transcript.turns.length,
        unknownMessages: other.messages.length,
        arrivals,
      }
    }, [PERF_THREAD_ID, TRANSCRIPTS_KEY] as const)

    expect(served.sessions).toBe(1)
    expect(served.turns).toBe(WORKLOAD.coldOpenTurns)
    expect(served.unknownMessages).toBe(0)
    // One read per frame, spaced by the frame interval (timers may only run late, never early).
    expect(served.arrivals.length).toBe(config.streamFrames.length)
    const gaps = served.arrivals.slice(2).map((at, index) => at - (served.arrivals[index + 1] as number))
    expect(percentile(gaps, 50)).toBeGreaterThanOrEqual(config.frameIntervalMs - 2)

    const snapshot = await readPerf(page)
    expect(snapshot.requests.filter((request) => request.shimmed)).toHaveLength(4)
    expect(snapshot.requests.every((request) => request.end >= request.start)).toBe(true)
    expect(snapshot.setItem[`local:${TRANSCRIPTS_KEY}`]).toEqual({ count: 1, bytes: 4 })
    expect(snapshot.setItem[`session:${TRANSCRIPTS_KEY}`]).toEqual({ count: 1, bytes: 6 })
    expect(Object.keys(snapshot.setItem)).toHaveLength(2)
    expect(snapshot.marks.streamStart).toBeDefined()
    expect(snapshot.marks.streamEnd).toBeGreaterThan(snapshot.marks.streamStart as number)
    expect(snapshot.marks.firstTurnVisible).toBeDefined()
    expect(snapshot.marks.composerReady).toBeDefined()
    expect(snapshot.frames.length).toBeGreaterThan(0)
    expect(snapshot.mutations.records).toBeGreaterThan(0)
    expect(snapshot.domNodes).toBeGreaterThan(0)
  })
})
