// @vitest-environment jsdom

import { createRouter, memoryHistory } from '@solidjs/router'
import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'
import VerevonIngestionsPage from '@/features/ingestions/components/VerevonIngestionsPage'
import { jobControlState, RunJobControls } from '@/features/ingestions/components/RunJobControls'
import type { RunItem } from '@/shared/api/ingestions-client'

// WS15.L5: pause, resume and cancel on the job row, hidden unless the installation
// reports the jobControl capability.

function run(overrides: Partial<RunItem> = {}): RunItem {
  return {
    id: 'job_1',
    kind: 'crawl',
    status: 'running',
    createdAt: '2026-11-01T10:00:00Z',
    target: 'https://example.com/docs',
    progress: { completed: 3, total: 10 },
    stats: {},
    ...overrides,
  }
}

function jsonResponse(body: unknown, status = 200) {
  return new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } })
}

type Seen = { method: string; url: string }

function stubApi(options: { jobControl: boolean; runs?: RunItem[]; controlStatus?: number } = { jobControl: true }) {
  const seen: Seen[] = []
  const runs = options.runs ?? [run()]
  vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input)
    const method = init?.method ?? 'GET'
    seen.push({ method, url })
    if (url.includes('/api/v1/knowledge/crawl/')) {
      if (options.controlStatus && options.controlStatus >= 400) {
        return jsonResponse({ error: { code: 'job_control_unavailable', message: 'Job control is not enabled.' } }, options.controlStatus)
      }
      return jsonResponse({ data: { id: 'job_1', kind: 'crawl', status: 'paused', desiredState: 'paused' } })
    }
    if (url.endsWith('/api/ingestions/schedules/capabilities')) {
      return jsonResponse({ data: { trigger: false, backfill: false, overlap: false, kinds: ['crawl'], jobControl: options.jobControl } })
    }
    if (url.endsWith('/api/ingestions/runs')) return jsonResponse({ data: runs })
    if (url.endsWith('/api/ingestions/schedules')) return jsonResponse({ data: [] })
    if (url.endsWith('/api/ingestions/sources')) return jsonResponse({ data: { integrations: [], quarrySources: [] } })
    if (url.endsWith('/api/ingestions/profiles')) return jsonResponse({ data: { profiles: [] } })
    return jsonResponse({ error: { code: 'not_found', message: `Unhandled ${url}` } }, 404)
  }))
  return seen
}

function controlCalls(seen: Seen[]) {
  return seen.filter((entry) => entry.url.includes('/api/v1/knowledge/crawl/'))
}

afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
})

describe('jobControlState', () => {
  it('offers pause on a live crawl, resume on a paused one and nothing otherwise', () => {
    expect(jobControlState(run({ status: 'running' }))).toBe('running')
    expect(jobControlState(run({ status: 'queued' }))).toBe('running')
    expect(jobControlState(run({ status: 'paused' }))).toBe('paused')
    expect(jobControlState(run({ status: 'running', desiredState: 'paused' }))).toBe('paused')
    expect(jobControlState(run({ status: 'running', desiredState: 'cancelled' }))).toBe('none')
    expect(jobControlState(run({ status: 'completed' }))).toBe('none')
    expect(jobControlState(run({ status: 'failed' }))).toBe('none')
    expect(jobControlState(run({ kind: 'batch', status: 'running' }))).toBe('none')
  })
})

describe('RunJobControls', () => {
  it('pauses a running crawl without a confirmation and tells the page to reload', async () => {
    const seen = stubApi()
    const onChanged = vi.fn()
    render(() => <RunJobControls run={run()} onChanged={onChanged} />)

    fireEvent.click(screen.getByRole('button', { name: /^pause$/i }))
    await waitFor(() => expect(onChanged).toHaveBeenCalledTimes(1))
    expect(controlCalls(seen)).toEqual([{ method: 'POST', url: expect.stringMatching(/crawl\/job_1\/pause$/) }])
  })

  it('resumes a paused crawl', async () => {
    const seen = stubApi()
    const onChanged = vi.fn()
    render(() => <RunJobControls run={run({ status: 'paused' })} onChanged={onChanged} />)

    expect(screen.queryByRole('button', { name: /^pause$/i })).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: /gjenoppta|resume/i }))
    await waitFor(() => expect(onChanged).toHaveBeenCalledTimes(1))
    expect(controlCalls(seen)).toEqual([{ method: 'POST', url: expect.stringMatching(/crawl\/job_1\/resume$/) }])
  })

  it('does not cancel until the second, explicit confirmation', async () => {
    const seen = stubApi()
    const onChanged = vi.fn()
    render(() => <RunJobControls run={run()} onChanged={onChanged} />)

    fireEvent.click(screen.getByRole('button', { name: /^avbryt$|^cancel$/i }))
    expect(controlCalls(seen)).toEqual([])
    await waitFor(() => expect(screen.getByText(/kan ikke gjenopptas|cannot be resumed/i)).toBeTruthy())

    fireEvent.click(screen.getByRole('button', { name: /behold|keep running/i }))
    await waitFor(() => expect(screen.getByRole('button', { name: /^pause$/i })).toBeTruthy())
    expect(controlCalls(seen)).toEqual([])

    fireEvent.click(screen.getByRole('button', { name: /^avbryt$|^cancel$/i }))
    await waitFor(() => expect(screen.getByRole('button', { name: /ja, avbryt|yes, cancel/i })).toBeTruthy())
    fireEvent.click(screen.getByRole('button', { name: /ja, avbryt|yes, cancel/i }))
    await waitFor(() => expect(onChanged).toHaveBeenCalledTimes(1))
    expect(controlCalls(seen)).toEqual([{ method: 'DELETE', url: expect.stringMatching(/crawl\/job_1$/) }])
  })

  it('draws nothing for a finished or non-crawl job', () => {
    stubApi()
    const done = render(() => <RunJobControls run={run({ status: 'completed' })} onChanged={vi.fn()} />)
    expect(done.container.querySelector('button')).toBeNull()
    cleanup()
    const batch = render(() => <RunJobControls run={run({ kind: 'batch' })} onChanged={vi.fn()} />)
    expect(batch.container.querySelector('button')).toBeNull()
  })

  it('shows the plain not-enabled message when the edge still answers 501', async () => {
    stubApi({ jobControl: true, controlStatus: 501 })
    const onChanged = vi.fn()
    render(() => <RunJobControls run={run()} onChanged={onChanged} />)

    fireEvent.click(screen.getByRole('button', { name: /^pause$/i }))
    await waitFor(() => expect(screen.getByRole('alert').textContent).toMatch(/ikke aktivert|not enabled/i))
    expect(onChanged).not.toHaveBeenCalled()
  })
})

describe('Ingestions page job controls (capability gate)', () => {
  function renderPage() {
    const TestRouter = createRouter({
      routes: [{ path: '/ingestions', component: VerevonIngestionsPage }],
      history: memoryHistory('/ingestions'),
      explicitLinks: true,
    })
    return render(() => <TestRouter>{(props) => <>{props.children}</>}</TestRouter>)
  }

  it('hides every job control when the installation does not report jobControl', async () => {
    const seen = stubApi({ jobControl: false })
    renderPage()

    await waitFor(() => expect(screen.getAllByText('https://example.com/docs').length).toBeGreaterThan(0))
    // The capability read must have landed (and rendered) before "absent" means anything.
    await waitFor(() => expect(seen.some((entry) => entry.url.endsWith('/schedules/capabilities'))).toBe(true))
    await new Promise((resolve) => setTimeout(resolve, 50))
    expect(screen.queryByRole('button', { name: /^pause$/i })).toBeNull()
    expect(screen.queryByRole('columnheader', { name: /handlinger|actions/i })).toBeNull()
  })

  it('shows pause and cancel on the crawl row when jobControl is reported', async () => {
    stubApi({ jobControl: true })
    renderPage()

    await waitFor(() => expect(screen.getByRole('button', { name: /^pause$/i })).toBeTruthy())
    expect(screen.getByRole('button', { name: /^avbryt$|^cancel$/i })).toBeTruthy()
    expect(screen.getByRole('columnheader', { name: /handlinger|actions/i })).toBeTruthy()
  })

  it('reloads the job list after a pause and stays on the runs view', async () => {
    const seen = stubApi({ jobControl: true })
    renderPage()

    await waitFor(() => expect(screen.getByRole('button', { name: /^pause$/i })).toBeTruthy())
    const runListReads = () => seen.filter((entry) => entry.url.endsWith('/api/ingestions/runs')).length
    const before = runListReads()
    fireEvent.click(screen.getByRole('button', { name: /^pause$/i }))
    await waitFor(() => expect(runListReads()).toBeGreaterThan(before))
    // Clicking a control must not select the row, which would switch to the evidence view.
    expect(screen.getByRole('columnheader', { name: /handlinger|actions/i })).toBeTruthy()
  })
})
