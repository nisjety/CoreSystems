// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { flush, type ComponentProps } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { IngestionsRunsView } from '@/features/ingestions/components/IngestionsRunsView'
import { defaultRunFilter } from '@/features/ingestions/lib/runs'
import type { RunItem } from '@/shared/api/ingestions-client'

// E4.1-5: "Ingest selected" on a run's fetched pages, behind VITE_INGEST_SELECTED.

const run: RunItem = {
  id: 'job_1',
  runId: 'run_1',
  kind: 'crawl',
  status: 'completed',
  createdAt: '2026-11-01T10:00:00Z',
  startedAt: '2026-11-01T10:00:00Z',
  completedAt: '2026-11-01T10:01:00Z',
  target: 'https://docs.example.com',
  progress: { completed: 3, total: 3 },
  stats: {},
}

function resultItem(seq: number, url: string) {
  return {
    seq,
    url,
    status: 200,
    contentType: 'text/html',
    title: null,
    fetchedAt: '2026-11-01T10:00:30Z',
    wordCount: 120,
    driver: 'static',
    markdownArtifactId: null,
    sourceArtifactId: null,
    bytes: 2048,
  }
}

const firstPage = {
  items: [
    resultItem(1, 'https://docs.example.com/a'),
    resultItem(2, 'https://docs.example.com/b'),
    resultItem(3, 'https://docs.example.com/a'),
  ],
  nextSeq: 3,
  scanCapped: false,
}

const secondPage = {
  items: [resultItem(4, 'https://docs.example.com/c')],
  nextSeq: null,
  scanCapped: false,
}

const zdrEnvelope = {
  data: null,
  meta: { request_id: 'req_1' },
  error: {
    code: 'FORBIDDEN',
    message: 'ZDR active: durable batch jobs are not available under zero data retention',
    details: { zdr_refusal: { code: 'zdr_durable_mutation_forbidden', action: 'batch' } },
  },
}

function jsonResponse(body: unknown, status = 200) {
  return new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } })
}

type Call = { url: string; method: string; body: string }

/** Answers the results and job reads the page makes; `answer` decides the batch create. */
function stubApi(answer: (method: string, url: URL) => Response | null = () => null) {
  const calls: Call[] = []
  vi.stubGlobal(
    'fetch',
    vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input)
      const method = init?.method ?? 'GET'
      calls.push({ url, method, body: typeof init?.body === 'string' ? init.body : '' })
      const parsed = new URL(url, 'http://gateway.test')
      if (method === 'GET' && parsed.pathname.endsWith('/api/ingestions/jobs/job_1/results')) {
        return jsonResponse({ data: parsed.searchParams.has('afterSeq') ? secondPage : firstPage })
      }
      const handled = answer(method, parsed)
      if (handled) return handled
      return jsonResponse({ error: { code: 'not_found', message: `Unhandled ${url}` } }, 404)
    }),
  )
  return calls
}

function renderRuns(overrides: Partial<ComponentProps<typeof IngestionsRunsView>> = {}) {
  const props = {
    runs: [run],
    filter: defaultRunFilter,
    onFilterChange: vi.fn(),
    selectedRunId: 'job_1',
    onSelectRun: vi.fn(),
    evidence: null,
    evidenceLoading: false,
    onRerun: vi.fn(),
    onControl: vi.fn(async () => undefined),
    controlError: null,
    onIngestStarted: vi.fn(),
    ...overrides,
  }
  render(() => <IngestionsRunsView {...props} />)
  return props
}

async function openPages() {
  fireEvent.click(screen.getByRole('button', { name: /utdata/i }))
  flush()
  await screen.findAllByRole('button', { name: 'https://docs.example.com/b' })
}

function postedBatches(calls: Call[]) {
  return calls.filter((call) => call.method === 'POST' && call.url.endsWith('/api/ingestions/runs'))
}

afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
  vi.unstubAllEnvs()
})

describe("Ingest selected on a run's pages", () => {
  it('shows no checkboxes and no ingest control while the switch is off', async () => {
    stubApi()
    renderRuns()
    await openPages()

    expect(screen.queryByRole('checkbox', { name: /^velg /i })).toBeNull()
    expect(screen.queryByRole('group', { name: /hent på nytt og lagre/i })).toBeNull()
  })

  it('counts the picks first, sends nothing in dry run, then ingests the picked URLs as one batch', async () => {
    vi.stubEnv('VITE_INGEST_SELECTED', 'true')
    const calls = stubApi((method, url) => {
      if (method === 'POST' && url.pathname.endsWith('/api/ingestions/runs')) {
        return jsonResponse({ data: { run: { id: 'job_b9', kind: 'batch', status: 'queued', createdAt: '2026-11-08T10:00:00Z', target: '2 URLs' } } }, 201)
      }
      return null
    })
    const props = renderRuns()
    await openPages()

    const [firstA, duplicateA] = screen.getAllByRole('checkbox', { name: 'Velg https://docs.example.com/a' }) as HTMLElement[]
    fireEvent.click(firstA as HTMLElement)
    fireEvent.click(duplicateA as HTMLElement)
    fireEvent.click(screen.getByRole('checkbox', { name: 'Velg https://docs.example.com/b' }))
    flush()

    expect(screen.getByText('3 valgt · 2 unike URL-er · 1 hoppes over (samme URL)')).toBeTruthy()
    const dryRun = screen.getByRole('checkbox', { name: /tørrkjøring/i }) as HTMLInputElement
    expect(dryRun.checked).toBe(true)

    fireEvent.click(screen.getByRole('button', { name: 'Tell valgte' }))
    flush()
    expect(screen.getByRole('status').textContent).toBe(
      'Tørrkjøring: 2 URL-er ville blitt hentet på nytt og lagret. Ingenting er sendt.',
    )
    expect(postedBatches(calls)).toEqual([])

    fireEvent.click(dryRun)
    flush()
    fireEvent.click(screen.getByRole('button', { name: 'Hent på nytt og lagre' }))

    await waitFor(() => expect(postedBatches(calls)).toHaveLength(1))
    const [batch] = postedBatches(calls)
    expect(JSON.parse(batch?.body ?? '')).toEqual({
      kind: 'batch',
      urls: ['https://docs.example.com/a', 'https://docs.example.com/b'],
      ingest: true,
    })
    expect(await screen.findByText('Batch startet: 2 URL-er, jobb job_b9.')).toBeTruthy()
    expect(props.onIngestStarted).toHaveBeenCalledTimes(1)
    expect(screen.getByText('Velg sider for å hente dem på nytt og lagre dem.')).toBeTruthy()
    expect((screen.getByRole('checkbox', { name: /tørrkjøring/i }) as HTMLInputElement).checked).toBe(true)
  })

  it('shows a batch refused under ZDR as the typed refusal and keeps the picks', async () => {
    vi.stubEnv('VITE_INGEST_SELECTED', 'true')
    const onIngestStarted = vi.fn()
    stubApi((method, url) =>
      method === 'POST' && url.pathname.endsWith('/api/ingestions/runs') ? jsonResponse(zdrEnvelope, 403) : null,
    )
    renderRuns({ onIngestStarted })
    await openPages()

    fireEvent.click(screen.getByRole('checkbox', { name: 'Velg https://docs.example.com/b' }))
    fireEvent.click(screen.getByRole('checkbox', { name: /tørrkjøring/i }))
    flush()
    fireEvent.click(screen.getByRole('button', { name: 'Hent på nytt og lagre' }))

    expect(await screen.findByRole('alert')).toBeTruthy()
    expect(screen.getByRole('alert').textContent).toBe(
      'Legitimasjon med Zero Data Retention kan ikke opprette, endre eller slette varig data.',
    )
    expect(onIngestStarted).not.toHaveBeenCalled()
    expect(screen.getByText('1 valgt · 1 unike URL-er')).toBeTruthy()
  })

  it('clears the picks when the page moves on, so a pick is for the page on screen only', async () => {
    vi.stubEnv('VITE_INGEST_SELECTED', 'true')
    stubApi()
    renderRuns()
    await openPages()

    fireEvent.click(screen.getByRole('checkbox', { name: 'Velg https://docs.example.com/b' }))
    flush()
    expect(screen.getByText('1 valgt · 1 unike URL-er')).toBeTruthy()

    fireEvent.click(screen.getByRole('button', { name: 'Neste' }))
    await screen.findByRole('button', { name: 'https://docs.example.com/c' })
    flush()

    expect(screen.queryByText(/valgt ·/)).toBeNull()
    expect((screen.getByRole('checkbox', { name: 'Velg https://docs.example.com/c' }) as HTMLInputElement).checked).toBe(false)
  })
})
