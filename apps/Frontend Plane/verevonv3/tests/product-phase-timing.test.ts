import { mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, describe, expect, it } from 'vitest'
import {
  MIN_P95_SAMPLES,
  nearestRank,
  phaseWorkload,
  productPhaseReport,
  splitTimedFrames,
  summarizeCohort,
  type PhaseReport,
  type StreamChunk,
} from './e2e/product-phase-timing'
import { collectPhaseReports, formatCohort } from './e2e/product-phase-report'

const frame = (event: string, data: unknown, id?: string) =>
  `${id === undefined ? '' : `id: ${id}\n`}event: ${event}\ndata: ${JSON.stringify(data)}\n\n`

/** A checked-document turn, split across reads the way a network delivers it. */
function documentTurn(click = 1_000): StreamChunk[] {
  const connected = frame('connected', { ok: true, request_id: 'req-1', run_id: 'run-1' })
  const running = frame('step_update', { id: 'call-1:source-check', title: 'Kontrollerer utkastet mot kildene', detail: 'x', status: 'running' }, '1')
  const done = frame('step_update', { id: 'call-1:source-check', title: 'Kildegjennomgang fullført', detail: 'y', status: 'done' }, '2')
  const artifact = frame('artifact', { id: 'doc', kind: 'document', title: 'Plan', content: '# Plan', version: 1 }, '3')
  const timing = frame('turn_timing', { request_id: 'req-1', outcome: 'completed', server_ms: 43_000, provider_ms: 42_800, orchestration_ms: 200 }, '4')
  const finished = frame('done', { request_id: 'req-1', model_used: 'gpt-5.6-terra' }, '5')
  return [
    { at: click + 300, text: connected.slice(0, 20) },
    // `connected` completes only here, together with half of the next frame.
    { at: click + 320, text: connected.slice(20) + running.slice(0, 10) },
    { at: click + 20_000, text: running.slice(10) },
    { at: click + 43_000, text: done.replace(/\n/g, '\r\n') },
    { at: click + 43_050, text: artifact + timing },
    { at: click + 43_100, text: finished },
  ]
}

describe('splitTimedFrames', () => {
  it('re-splits reads into frames like the app reader and stamps each with the read that completed it', () => {
    const frames = splitTimedFrames(documentTurn())
    expect(frames.map((entry) => [entry.event, entry.at])).toEqual([
      ['connected', 1_320],
      ['step_update', 21_000],
      ['step_update', 44_000],
      ['artifact', 44_050],
      ['turn_timing', 44_050],
      ['done', 44_100],
    ])
    expect(frames[1]?.id).toBe('1')
    // CRLF line endings are read the same as LF.
    expect(JSON.parse(frames[2]!.data).status).toBe('done')
  })

  it('skips comments, joins multi-line data, and never dispatches an unterminated frame', () => {
    const frames = splitTimedFrames([
      { at: 5, text: ': keep-alive\n\nevent: chunk\ndata: {"delta":\ndata: "a"}\n\n' },
      { at: 9, text: 'event: done\ndata: {}' },
    ])
    expect(frames).toEqual([{ event: 'chunk', data: '{"delta":\n"a"}', at: 5 }])
  })

  it('reassembles exactly the text the old capture produced', () => {
    const chunks = documentTurn()
    expect(chunks.map((chunk) => chunk.text).join('')).toContain('event: turn_timing')
  })
})

describe('productPhaseReport', () => {
  it('derives the Q14 phase marks and browser-clock durations from the frames', () => {
    const report = productPhaseReport({
      label: 'initial',
      workload: 'w',
      capture: {
        clickAt: 1_000,
        fetchStartedAt: 1_010,
        headersAt: 1_300,
        endAt: 44_120,
        acknowledgementAt: 1_050,
        firstAnswerAt: 44_150,
        artifactRenderedAt: 44_200,
        chunks: documentTurn(),
      },
    })
    expect(report.origin).toBe('submit-click')
    expect(report.outcome).toBe('completed')
    expect(report.marks).toMatchObject({
      headers: 300,
      connected: 320,
      generationEnded: 20_000,
      validationEnded: 43_000,
      firstArtifact: 43_050,
      artifactRendered: 43_200,
      acknowledgement: 50,
      firstAnswer: 43_150,
      terminal: 43_100,
      streamEnd: 43_120,
    })
    expect(report.durations).toEqual({
      toHeaders: 300,
      toConnected: 320,
      generation: 19_680,
      validation: 23_000,
      publication: 50,
      rendering: 150,
      total: 43_100,
    })
    expect(report.turnTiming).toMatchObject({ server_ms: 43_000, provider_ms: 42_800 })
    // Frames keep ids, names and states only — never titles, details or content.
    expect(report.frames[1]).toEqual({ event: 'step_update', id: '1', atMs: 20_000, step: { id: 'call-1:source-check', status: 'running' } })
    expect(JSON.stringify(report.frames)).not.toContain('Kontrollerer')
    expect(JSON.stringify(report.frames)).not.toContain('# Plan')
  })

  it('keeps a failed turn measurable and falls back to the request when the click was not seen', () => {
    const report = productPhaseReport({
      label: 'initial',
      workload: 'w',
      capture: {
        fetchStartedAt: 200,
        headersAt: 400,
        endAt: 900,
        chunks: [
          { at: 410, text: frame('connected', { ok: true }) },
          { at: 880, text: frame('error', { code: 'response_validation_failed', message: 'Ikke publisert.' }) },
        ],
      },
    })
    expect(report.origin).toBe('stream-request')
    expect(report.outcome).toBe('failed')
    expect(report.durations.total).toBe(680)
    expect(report.durations.validation).toBeNull()
    expect(report.frames.at(-1)).toMatchObject({ event: 'error', code: 'response_validation_failed' })
    expect(report.turnTiming).toBeNull()
  })

  it('reports a stream that ended without a terminal frame as incomplete, timed to its last byte', () => {
    const report = productPhaseReport({
      label: 'x',
      workload: 'w',
      capture: { clickAt: 0, endAt: 1_234, chunks: [{ at: 100, text: frame('connected', {}) }] },
    })
    expect(report.outcome).toBe('incomplete')
    expect(report.durations.total).toBe(1_234)
  })
})

describe('phaseWorkload', () => {
  it('groups samples of one benchmark while keeping specs and labels apart', () => {
    expect(phaseWorkload('C:\\repo\\tests\\e2e\\product-latency.spec.ts', 'Q08 latency microbenchmark 3/20', 'benchmark'))
      .toBe('product-latency › Q08 latency microbenchmark › benchmark')
    expect(phaseWorkload('/repo/tests/e2e/product-recording.spec.ts', 'customer recording: genuine draft', 'revision'))
      .toBe('product-recording › customer recording: genuine draft › revision')
  })
})

function sample(workload: string, total: number, outcome: PhaseReport['outcome'] = 'completed'): PhaseReport {
  return {
    version: 1,
    label: 'initial',
    workload,
    clock: 'browser-performance-now',
    origin: 'submit-click',
    outcome,
    marks: { headers: 1, connected: 2, firstChunk: null, generationEnded: null, validationEnded: null, firstArtifact: null, artifactRendered: null, acknowledgement: null, firstAnswer: null, terminal: total, streamEnd: total },
    durations: { toHeaders: 1, toConnected: 2, generation: null, validation: null, publication: null, rendering: null, total },
    frameCount: 2,
    frames: [],
    turnTiming: null,
  }
}

describe('summarizeCohort', () => {
  it('uses nearest-rank percentiles', () => {
    const values = Array.from({ length: 20 }, (_, index) => index + 1)
    expect(nearestRank(values, 0.5)).toBe(10)
    expect(nearestRank(values, 0.95)).toBe(19)
    expect(nearestRank([], 0.5)).toBeNull()
  })

  it('keeps failed runs in the cohort and withholds p95 below the sample floor', () => {
    const small = [sample('plan', 40_000), sample('plan', 90_000, 'failed'), sample('plan', 50_000)]
    const [summary] = summarizeCohort(small)
    expect(summary?.runs).toBe(3)
    expect(summary?.outcomes).toEqual({ completed: 2, failed: 1, stopped: 0, incomplete: 0 })
    expect(summary?.metrics.total).toMatchObject({ n: 3, p50: 50_000, p95: null, max: 90_000 })
    expect(summary?.metrics.total.p95Withheld).toBe(`needs at least ${MIN_P95_SAMPLES} samples, has 3`)
    expect(summary?.metrics.generation.n).toBe(0)
  })

  it('states a p95 once there are enough samples, per workload', () => {
    const reports = [
      ...Array.from({ length: 20 }, (_, index) => sample('note', (index + 1) * 1_000, index === 19 ? 'failed' : 'completed')),
      sample('plan', 1),
    ]
    const summaries = summarizeCohort(reports)
    expect(summaries.map((entry) => entry.workload)).toEqual(['note', 'plan'])
    expect(summaries[0]?.metrics.total).toMatchObject({ n: 20, p50: 10_000, p95: 19_000, max: 20_000 })
    expect(summaries[0]?.metrics.total.p95Withheld).toBeUndefined()
    expect(summaries[1]?.metrics.total.p95).toBeNull()
  })
})

describe('cohort report inputs', () => {
  let dir: string | undefined
  afterEach(() => {
    if (dir) rmSync(dir, { recursive: true, force: true })
    dir = undefined
  })

  it('reads attachment files and Playwright JSON reports, counting a run found twice once', () => {
    dir = mkdtempSync(join(tmpdir(), 'phase-report-'))
    const run = sample('note', 12_000)
    const failed = sample('note', 30_000, 'failed')
    writeFileSync(join(dir, 'initial-phases.json'), JSON.stringify(run))
    writeFileSync(join(dir, 'report.json'), JSON.stringify({
      suites: [{ specs: [{ tests: [{ results: [{ attachments: [
        { name: 'initial-stream', contentType: 'text/plain', body: Buffer.from('event: done').toString('base64') },
        { name: 'initial-phases', contentType: 'application/json', body: Buffer.from(JSON.stringify(run)).toString('base64') },
        { name: 'revision-phases', contentType: 'application/json', body: Buffer.from(JSON.stringify(failed)).toString('base64') },
      ] }] }] }] }],
    }))
    writeFileSync(join(dir, 'unrelated.json'), JSON.stringify({ hello: 'world' }))
    const reports = collectPhaseReports([dir])
    expect(reports).toHaveLength(2)
    const text = formatCohort(summarizeCohort(reports))
    expect(text).toContain('runs 2 (completed 1, failed 1, stopped 0, incomplete 0)')
    expect(text).toContain('withheld (needs at least 20 samples, has 2)')
  })
})
