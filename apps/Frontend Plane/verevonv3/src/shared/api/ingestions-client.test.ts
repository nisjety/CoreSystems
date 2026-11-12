import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  cancelIngestionRun,
  createIngestionSchedule,
  getIngestionRunStatus,
  getScheduleCapabilities,
  NO_SCHEDULE_CAPABILITIES,
  pauseIngestionRun,
  resumeIngestionRun,
  runIngestionScheduleAction,
} from '@/shared/api/ingestions-client'

function stubFetch(body: unknown = { data: {} }, status = 200) {
  const fetchMock = vi.fn(
    async (_input: RequestInfo | URL, _init?: RequestInit) =>
      new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } }),
  )
  vi.stubGlobal('fetch', fetchMock)
  return fetchMock
}

function call(fetchMock: ReturnType<typeof stubFetch>) {
  const [input, init] = fetchMock.mock.calls[0] ?? []
  return { url: String(input), init: (init ?? {}) as RequestInit }
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('ingestions client job control (WS15.L5)', () => {
  it('reads one crawl run status', async () => {
    const fetchMock = stubFetch({ data: { id: 'job_1', kind: 'crawl', status: 'running', desiredState: 'run' } })
    const status = await getIngestionRunStatus('job_1')

    const { url, init } = call(fetchMock)
    expect(url).toMatch(/\/api\/v1\/knowledge\/crawl\/job_1$/)
    expect(init.method ?? 'GET').toBe('GET')
    expect(status).toMatchObject({ id: 'job_1', desiredState: 'run' })
  })

  it('pauses and resumes with a POST to the verb path', async () => {
    const pauseFetch = stubFetch()
    await pauseIngestionRun('job_1')
    expect(call(pauseFetch).url).toMatch(/\/api\/v1\/knowledge\/crawl\/job_1\/pause$/)
    expect(call(pauseFetch).init.method).toBe('POST')

    const resumeFetch = stubFetch()
    await resumeIngestionRun('job_1')
    expect(call(resumeFetch).url).toMatch(/\/api\/v1\/knowledge\/crawl\/job_1\/resume$/)
    expect(call(resumeFetch).init.method).toBe('POST')
  })

  it('cancels with a DELETE on the run and encodes the id', async () => {
    const fetchMock = stubFetch()
    await cancelIngestionRun('job/1 x')

    const { url, init } = call(fetchMock)
    expect(url).toMatch(/\/api\/v1\/knowledge\/crawl\/job%2F1%20x$/)
    expect(init.method).toBe('DELETE')
  })

  it('surfaces the typed 501 the edge answers while job control is off', async () => {
    stubFetch({ error: { code: 'job_control_unavailable', message: 'Job control is not enabled.' } }, 501)
    await expect(pauseIngestionRun('job_1')).rejects.toMatchObject({ status: 501, code: 'job_control_unavailable' })
  })
})

describe('ingestions client capabilities (WS15.L5)', () => {
  it('reports jobControl only when the gateway says exactly true', async () => {
    stubFetch({ data: { trigger: true, kinds: ['crawl'], jobControl: true } })
    expect((await getScheduleCapabilities()).jobControl).toBe(true)

    stubFetch({ data: { trigger: true, kinds: ['crawl'], jobControl: 'yes' } })
    expect((await getScheduleCapabilities()).jobControl).toBe(false)

    stubFetch({ data: { trigger: true, kinds: ['crawl'] } })
    expect((await getScheduleCapabilities()).jobControl).toBe(false)
  })

  it('reports runs only when the gateway says exactly true', async () => {
    stubFetch({ data: { trigger: true, kinds: ['crawl'], runs: true } })
    expect((await getScheduleCapabilities()).runs).toBe(true)

    stubFetch({ data: { trigger: true, kinds: ['crawl'], runs: 'yes' } })
    expect((await getScheduleCapabilities()).runs).toBe(false)

    stubFetch({ data: { trigger: true, kinds: ['crawl'] } })
    expect((await getScheduleCapabilities()).runs).toBe(false)
  })

  it('has job control off in the all-off default and when the read fails', async () => {
    expect(NO_SCHEDULE_CAPABILITIES.jobControl).toBe(false)
    stubFetch({ error: { code: 'upstream_unavailable', message: 'down' } }, 502)
    expect(await getScheduleCapabilities()).toEqual({ ...NO_SCHEDULE_CAPABILITIES, kinds: [] })
  })
})

describe('ingestions client schedules (WS15.L5)', () => {
  it('sends the new create fields and never an activate flag', async () => {
    const fetchMock = stubFetch({ data: { id: 'sched_1' } })
    await createIngestionSchedule({
      name: 'Docs',
      kind: 'crawl',
      targetUrl: 'https://example.com',
      cron: '0 7 * * *',
      maxPages: 40,
      overlapPolicy: 'skip',
    })

    const { url, init } = call(fetchMock)
    expect(url).toMatch(/\/api\/ingestions\/schedules$/)
    const sent = JSON.parse(String(init.body)) as Record<string, unknown>
    expect(sent).toMatchObject({ kind: 'crawl', maxPages: 40, overlapPolicy: 'skip' })
    expect(sent).not.toHaveProperty('activate')
  })

  it('sends batch urls instead of a target', async () => {
    const fetchMock = stubFetch({ data: { id: 'sched_2' } })
    await createIngestionSchedule({ name: 'Batch', kind: 'batch', urls: ['https://a.example/x'], scheduleAt: '2026-12-01T07:00:00Z' })

    const sent = JSON.parse(String(call(fetchMock).init.body)) as Record<string, unknown>
    expect(sent.urls).toEqual(['https://a.example/x'])
    expect(sent).not.toHaveProperty('targetUrl')
    expect(sent).not.toHaveProperty('activate')
  })

  it('posts a schedule state change with its backfill window only when given', async () => {
    const plain = stubFetch()
    await runIngestionScheduleAction('trigger_schedule', 'sched_1')
    expect(JSON.parse(String(call(plain).init.body))).toEqual({ action: 'trigger_schedule', scheduleId: 'sched_1' })

    const windowed = stubFetch()
    await runIngestionScheduleAction('backfill_schedule', 'sched_1', undefined, {
      startAt: '2026-10-01T00:00:00Z',
      endAt: '2026-10-02T00:00:00Z',
    })
    expect(JSON.parse(String(call(windowed).init.body))).toEqual({
      action: 'backfill_schedule',
      scheduleId: 'sched_1',
      startAt: '2026-10-01T00:00:00Z',
      endAt: '2026-10-02T00:00:00Z',
    })
  })
})
