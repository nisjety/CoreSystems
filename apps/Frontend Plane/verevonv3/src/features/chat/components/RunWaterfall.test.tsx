// @vitest-environment jsdom

import { cleanup, fireEvent, render } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { RunTiming } from '@/shared/api/run-console-client'
import { assignLanes, isRunWaterfallEnabled, RunTimingPanel, RunWaterfall, waterfallGroups } from './RunWaterfall'

// Without a provider the page renders in its default locale, Norwegian.

afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

const turnTiming = (): RunTiming => ({
  timings: [{
    schema: 'turn-timing-v1',
    requestId: 'req-1',
    phasesDropped: 0,
    report: {
      server_ms: 900,
      provider_ms: 700,
      phases: [
        { phase: 'admission', start_ms: 0, end_ms: 20 },
        { phase: 'review_batch', start_ms: 100, end_ms: 400, batch: 0 },
        { phase: 'review_batch', start_ms: 150, end_ms: 500, batch: 1 },
        { phase: 'generation', start_ms: 500, end_ms: 900, round: 2 },
      ],
    },
  }],
  actions: [],
  truncated: false,
})

describe('waterfall rows', () => {
  it('stacks overlapping spans on separate lanes and reuses a lane once it is free', () => {
    const rows = assignLanes([
      { key: 'a', label: 'a', startMs: 0, endMs: 100 },
      { key: 'b', label: 'b', startMs: 50, endMs: 150 },
      { key: 'c', label: 'c', startMs: 100, endMs: 200 },
    ])
    expect(rows.map((row) => [row.key, row.lane])).toEqual([['a', 0], ['b', 1], ['c', 0]])
  })

  it('draws recorded spans only and never sums them', () => {
    const [group] = waterfallGroups(turnTiming())
    expect(group?.rows).toHaveLength(4)
    expect(group?.lanes).toBe(2)
    // The right edge is the latest recorded end, not the sum of the spans (1 120 ms here).
    expect(group?.extentMs).toBe(900)
    expect(group?.rows.find((row) => row.label === 'generation · 2')).toBeDefined()
  })

  it('splits a round into inference and dispatch, and leaves an answering round one bar', () => {
    const groups = waterfallGroups({
      timings: [{
        schema: 'round-timing-v1',
        requestId: 'req-2',
        phasesDropped: 0,
        report: {
          rounds: [
            { round: 1, inference_start_ms: 0, inference_end_ms: 400, dispatch_end_ms: 900, tool_count: 2 },
            { round: 2, inference_start_ms: 900, inference_end_ms: 1300, dispatch_end_ms: 1300, tool_count: 0 },
          ],
        },
      }],
      actions: [],
      truncated: false,
    })
    expect(groups[0]?.rows.map((row) => row.label)).toEqual(['inference · 1', 'dispatch · 1', 'inference · 2'])
  })

  it('places tool calls on their own clock and leaves an unfinished call open', () => {
    const [group] = waterfallGroups({
      timings: [],
      actions: [
        { actionId: 'a1', tool: 'web_search', status: 'completed', reservedAt: '2026-11-03T10:00:01.000Z', finalizedAt: '2026-11-03T10:00:03.000Z' },
        { actionId: 'a2', tool: 'read_file', status: 'reserved', reservedAt: '2026-11-03T10:00:02.000Z' },
      ],
      truncated: false,
    })
    expect(group?.rows.map((row) => [row.label, row.startMs, row.endMs])).toEqual([
      ['web_search', 0, 2000],
      ['read_file', 1000, undefined],
    ])
  })

  it('ignores a record of an unknown schema and a malformed span', () => {
    const groups = waterfallGroups({
      timings: [
        { schema: 'turn-timing-v9', requestId: 'r', phasesDropped: 0, report: { phases: [{ phase: 'x', start_ms: 0, end_ms: 1 }] } },
        { schema: 'turn-timing-v1', requestId: 'r', phasesDropped: 0, report: { phases: [{ phase: 'x', start_ms: 5, end_ms: 1 }, 'nonsense'] } },
      ],
      actions: [],
      truncated: false,
    })
    expect(groups).toEqual([])
  })
})

describe('RunWaterfall', () => {
  it('draws one bar per span and keeps a table with the same rows', () => {
    const { container } = render(() => <RunWaterfall timing={turnTiming()} />)
    expect(container.querySelectorAll('[data-waterfall-bar]')).toHaveLength(4)
    expect(container.querySelector('svg')?.getAttribute('aria-hidden')).toBe('true')
    const rows = container.querySelectorAll('table [data-waterfall-row]')
    expect(rows).toHaveLength(4)
    expect(rows[0]?.querySelector('th')?.textContent).toBe('admission')
  })

  it('says when nothing was recorded', () => {
    const { container } = render(() => <RunWaterfall timing={{ timings: [], actions: [], truncated: false }} />)
    expect(container.querySelector('svg')).toBeNull()
    expect(container.textContent).toContain('Ingen tidsdata')
  })
})

describe('RunTimingPanel', () => {
  it('fetches nothing until asked, then draws what the run kept', async () => {
    const load = vi.fn(async () => turnTiming())
    const { container, findAllByText } = render(() => <RunTimingPanel runId="run-1" load={load} />)
    expect(load).not.toHaveBeenCalled()
    fireEvent.click(container.querySelector('button') as HTMLButtonElement)
    expect(load).toHaveBeenCalledWith('run-1')
    await findAllByText('admission')
    expect(container.querySelectorAll('[data-waterfall-bar]')).toHaveLength(4)
  })

  it('says the timing is not available when the read fails', async () => {
    const load = vi.fn(async (): Promise<RunTiming> => {
      throw new Error('404')
    })
    const { container, findByText } = render(() => <RunTimingPanel runId="run-1" load={load} />)
    fireEvent.click(container.querySelector('button') as HTMLButtonElement)
    await findByText('Tidslinjen er ikke tilgjengelig for denne kjøringen.')
  })
})

describe('the switch', () => {
  it('takes only the literal true', () => {
    expect(isRunWaterfallEnabled(undefined)).toBe(false)
    expect(isRunWaterfallEnabled('1')).toBe(false)
    expect(isRunWaterfallEnabled('TRUE')).toBe(false)
    expect(isRunWaterfallEnabled('true')).toBe(true)
  })
})
