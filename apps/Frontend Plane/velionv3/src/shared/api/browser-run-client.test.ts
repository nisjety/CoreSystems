import { afterEach, describe, expect, it, vi } from 'vitest'
import { controlBrowserAiRun, startBrowserAiRun } from './browser-run-client'

function jsonResponse(data: unknown, status = 200): Response {
  return new Response(JSON.stringify({ data }), {
    status,
    headers: { 'Content-Type': 'application/json' },
  })
}

describe('startBrowserAiRun', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('posts to the session ai-runs route with camelCase params and org header', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      jsonResponse({ runId: 'run_1', threadId: 'thread_1', planId: 'plan_1' }),
    )
    vi.stubGlobal('fetch', fetchMock)

    const result = await startBrowserAiRun('org_1', 'session_1', {
      goal: 'find the cheapest flight',
      allowedDomains: ['norwegian.com'],
      maxSteps: 10,
      maxRuntimeSeconds: 90,
    })

    const [path, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(path).toContain('/api/v1/browser/sessions/session_1/ai-runs')
    expect(init.method).toBe('POST')
    expect((init.headers as Headers).get('x-velion-org-id')).toBe('org_1')
    expect(JSON.parse(init.body as string)).toEqual({
      goal: 'find the cheapest flight',
      allowedDomains: ['norwegian.com'],
      maxSteps: 10,
      maxRuntimeS: 90,
      stopCriteria: undefined,
      requireApproval: undefined,
      maxCostUsd: undefined,
    })
    expect(result).toEqual({ runId: 'run_1', threadId: 'thread_1', planId: 'plan_1' })
  })
})

describe('controlBrowserAiRun', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('posts the requested action to the run control route', async () => {
    const fetchMock = vi.fn().mockResolvedValue(jsonResponse({ status: 'paused' }))
    vi.stubGlobal('fetch', fetchMock)

    await controlBrowserAiRun('org_1', 'run_1', 'pause')

    const [path, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(path).toContain('/api/v1/browser/runs/run_1/control')
    expect(init.method).toBe('POST')
    expect(JSON.parse(init.body as string)).toEqual({ action: 'pause' })
  })
})
