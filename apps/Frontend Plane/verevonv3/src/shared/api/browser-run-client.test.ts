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
    // The REAL wire response is snake_case — model-gateway's `browser_run.rs`
    // constructs `json!({ "run_id", "thread_id", "plan_id" })` directly and
    // the Verevon gateway forwards it verbatim (confirmed live: a prior
    // version of this test mocked a camelCase response here, which let a
    // real snake_case/camelCase mismatch bug ship silently — every AI-loop
    // run through the SPA started successfully server-side but the client
    // never resolved a `runId` to stream events for, so it looked stuck
    // forever with no visible error. Regression-tested end-to-end by
    // `tests/e2e/browser-workspace-hitl.spec.ts`).
    const fetchMock = vi.fn().mockResolvedValue(
      jsonResponse({ run_id: 'run_1', thread_id: 'thread_1', plan_id: 'plan_1' }),
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
    expect((init.headers as Headers).get('x-verevon-org-id')).toBe('org_1')
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
