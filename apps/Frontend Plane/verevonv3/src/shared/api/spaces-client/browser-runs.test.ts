import { afterEach, describe, expect, it, vi } from 'vitest'

import {
  listSpaceBrowserRuns,
  readSpaceBrowserTimeline,
  spaceBrowserHostProblem,
  startSpaceBrowserRun,
  stopSpaceBrowserRunAsStarter,
} from './browser-runs'

type FetchCall = { url: string; init: RequestInit | undefined }

function stubFetch(status: number, body: unknown): FetchCall[] {
  const calls: FetchCall[] = []
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string, init?: RequestInit) => {
      calls.push({ url: String(url), init })
      return new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } })
    }),
  )
  return calls
}

afterEach(() => {
  vi.unstubAllGlobals()
})

const header = {
  run_id: 'run_1',
  thread_id: 'thr_1',
  started_by: { subject_id: 'user_1' },
  phase: 'running',
  started_at: '2026-11-02T10:00:00Z',
  finished_at: null,
  control: { mode: 'owner' },
  handoff: { open: false },
  view: { scope: 'owner' },
}

const runRow = { ...header, hosts: ['docs.example.test', 7, ''], step_count: 3 }

describe('spaceBrowserHostProblem (WS20-S5)', () => {
  it.each([
    ['', 'empty'],
    ['192.168.0.1', 'ip_literal'],
    ['example.123', 'ip_literal'],
    ['localhost', 'localhost'],
    ['docs.localhost', 'localhost'],
    ['*.example.test', 'wildcard'],
    ['https://example.test', 'scheme'],
    ['example.test/login', 'path'],
    ['example.test:8443', 'port'],
    ['intranet', 'format'],
  ])('reads "%s" as %s', (host, problem) => {
    expect(spaceBrowserHostProblem(host)).toBe(problem)
  })

  it('accepts a bare hostname in any case and checks it lowercased', () => {
    expect(spaceBrowserHostProblem('  Docs.Example.TEST  ')).toBeNull()
    expect(spaceBrowserHostProblem('LOCALHOST')).toBe('localhost')
    expect(spaceBrowserHostProblem('HTTPS://Docs.example.test')).toBe('scheme')
  })
})

describe('browser run parsers (WS20-S5)', () => {
  it('starts a run from its receipt and throws when the receipt names no run', async () => {
    const calls = stubFetch(202, {
      data: { run_id: 'run_1', thread_id: 'thr_1', plan_id: 'plan_1', view: { scope: 'owner' } },
    })
    await expect(
      startSpaceBrowserRun('room', {
        threadId: 'thr_1',
        goal: 'Check the docs',
        allowedDomains: ['docs.example.test'],
        clientSendId: 'start-0000000000000001',
      }),
    ).resolves.toEqual({ runId: 'run_1', threadId: 'thr_1', planId: 'plan_1', viewScope: 'owner' })
    expect(calls[0]?.url.endsWith('/api/v1/spaces/room/browser-runs')).toBe(true)

    stubFetch(202, { data: { thread_id: 'thr_1', plan_id: 'plan_1', view: { scope: 'owner' } } })
    await expect(
      startSpaceBrowserRun('room', {
        threadId: 'thr_1',
        goal: 'Check the docs',
        allowedDomains: ['docs.example.test'],
        clientSendId: 'start-0000000000000002',
      }),
    ).rejects.toThrow('space_browser_run_unreadable')
  })

  // D4 (W3-DEC-BROWSER): no thread starts a new one, which the gateway does for an empty or absent thread_id.
  it('names the thread in the body only when there is one: a start with none asks for a new thread', async () => {
    const data = { run_id: 'run_2', thread_id: 'thr_new', plan_id: 'plan_2', view: { scope: 'owner' } }
    const bodyOf = (calls: FetchCall[]) => JSON.parse(String(calls[0]?.init?.body)) as Record<string, unknown>
    const start = { goal: 'Check the docs', allowedDomains: ['docs.example.test'], clientSendId: 'start-0000000000000003' }

    let calls = stubFetch(202, { data })
    await expect(startSpaceBrowserRun('room', start)).resolves.toEqual({
      runId: 'run_2',
      threadId: 'thr_new',
      planId: 'plan_2',
      viewScope: 'owner',
    })
    expect('thread_id' in bodyOf(calls)).toBe(false)

    calls = stubFetch(202, { data })
    await startSpaceBrowserRun('room', { ...start, threadId: '   ' })
    expect('thread_id' in bodyOf(calls)).toBe(false)

    calls = stubFetch(202, { data })
    await startSpaceBrowserRun('room', { ...start, threadId: ' thr_1 ' })
    expect(bodyOf(calls).thread_id).toBe('thr_1')
  })

  it('reads each run row with its hosts and step count, and the next cursor', async () => {
    const calls = stubFetch(200, { data: [runRow], meta: { next_cursor: 'cur_2' } })
    const page = await listSpaceBrowserRuns('room', { threadId: 'thr_1', limit: 5 })
    expect(calls[0]?.url.endsWith('/api/v1/spaces/room/browser-runs?thread_id=thr_1&limit=5')).toBe(true)
    expect(page.runs).toEqual([
      {
        runId: 'run_1',
        threadId: 'thr_1',
        startedBySubjectId: 'user_1',
        phase: 'running',
        startedAt: '2026-11-02T10:00:00Z',
        finishedAt: null,
        controlMode: 'owner',
        handoffOpen: false,
        viewScope: 'owner',
        hosts: ['docs.example.test'],
        stepCount: 3,
      },
    ])
    expect(page.nextCursor).toBe('cur_2')
  })

  it('throws for a malformed row rather than returning a shorter list, and for a malformed envelope', async () => {
    stubFetch(200, { data: [runRow, { ...runRow, step_count: -1 }] })
    await expect(listSpaceBrowserRuns('room', { threadId: 'thr_1' })).rejects.toThrow('space_browser_run_unreadable')
    stubFetch(200, { data: [{ ...runRow, started_by: undefined }] })
    await expect(listSpaceBrowserRuns('room', { threadId: 'thr_1' })).rejects.toThrow('space_browser_run_unreadable')
    stubFetch(200, { data: { runs: [runRow] } })
    await expect(listSpaceBrowserRuns('room', { threadId: 'thr_1' })).rejects.toThrow('space_browser_run_unreadable')
  })

  it('reads the run header, the events in order and the next event id', async () => {
    const calls = stubFetch(200, {
      data: {
        run: header,
        events: [
          {
            event_id: 'ev_1',
            at: '2026-11-02T10:00:01Z',
            kind: 'step',
            step: 'open',
            action_type: 'navigate',
            host: 'docs.example.test',
            status: 'ok',
          },
        ],
        next_event_id: 'ev_1',
        valid_until: '2026-11-02T10:05:00Z',
      },
    })
    const timeline = await readSpaceBrowserTimeline('room', 'run_1', { after: 'ev_0', limit: 50 })
    expect(calls[0]?.url.endsWith('/api/v1/spaces/room/browser-runs/run_1/timeline?after=ev_0&limit=50')).toBe(true)
    expect(timeline.run.runId).toBe('run_1')
    expect(timeline.events).toEqual([
      {
        eventId: 'ev_1',
        at: '2026-11-02T10:00:01Z',
        kind: 'step',
        step: 'open',
        actionType: 'navigate',
        host: 'docs.example.test',
        status: 'ok',
      },
    ])
    expect(timeline.nextEventId).toBe('ev_1')
    expect(timeline.validUntil).toBe('2026-11-02T10:05:00Z')
  })

  it('throws for a malformed event or a malformed timeline envelope', async () => {
    stubFetch(200, {
      data: { run: header, events: [{ event_id: 'ev_2', kind: 'step' }], valid_until: '2026-11-02T10:05:00Z' },
    })
    await expect(readSpaceBrowserTimeline('room', 'run_1')).rejects.toThrow('space_browser_run_unreadable')
    stubFetch(200, { data: { run: header, events: [] } })
    await expect(readSpaceBrowserTimeline('room', 'run_1')).rejects.toThrow('space_browser_run_unreadable')
    stubFetch(200, { data: { events: [], valid_until: '2026-11-02T10:05:00Z' } })
    await expect(readSpaceBrowserTimeline('room', 'run_1')).rejects.toThrow('space_browser_run_unreadable')
  })
})

describe('stopSpaceBrowserRunAsStarter (D21h)', () => {
  it('posts to the starter stop route with no body and reads the closed status', async () => {
    const calls = stubFetch(200, { data: { run_id: 'run 1', status: 'already_terminal' } })
    await expect(stopSpaceBrowserRunAsStarter('room/a', 'run 1')).resolves.toEqual({
      runId: 'run 1',
      status: 'already_terminal',
    })
    expect(calls[0]?.url).toBe('/api/v1/spaces/room%2Fa/browser-runs/run%201/stop')
    expect(calls[0]?.init?.method).toBe('POST')
    expect(calls[0]?.init?.body).toBeUndefined()
  })

  it('throws for an answer about another run or an unknown status', async () => {
    stubFetch(200, { data: { run_id: 'run_2', status: 'cancelled' } })
    await expect(stopSpaceBrowserRunAsStarter('room', 'run_1')).rejects.toThrow('space_browser_run_unreadable')
    stubFetch(200, { data: { run_id: 'run_1', status: 'paused' } })
    await expect(stopSpaceBrowserRunAsStarter('room', 'run_1')).rejects.toThrow('space_browser_run_unreadable')
  })
})
