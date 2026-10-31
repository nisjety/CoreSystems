// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { MemorySection } from '@/features/settings/components/MemorySection'

function jsonResponse(data: unknown, status = 200): Response {
  return new Response(JSON.stringify(data), {
    headers: { 'Content-Type': 'application/json' },
    status,
  })
}

const ON = { state: 'on', reason: 'on', use_allowed: true, generation_allowed: true, hidden_rows: 0 }
const EMPTY_LIST = { memories: [], degraded: false, degradation_reason: '' }

/** Routes the section's three reads and two writes; `overrides` replaces one by `METHOD path`. */
function stubBackend(
  controls: Record<string, unknown> = ON,
  overrides: Record<string, () => Response> = {},
) {
  const fetchMock = vi.fn(async (input: unknown, init?: RequestInit) => {
    const key = `${init?.method ?? 'GET'} ${String(input)}`
    const override = overrides[key]
    if (override) return override()
    if (key === 'GET /api/v1/memory/controls') return jsonResponse(controls)
    if (key === 'GET /api/v1/memory') return jsonResponse(EMPTY_LIST)
    return jsonResponse({}, 404)
  })
  vi.stubGlobal('fetch', fetchMock)
  return fetchMock
}

function callsTo(fetchMock: ReturnType<typeof stubBackend>, method: string, path: string) {
  return fetchMock.mock.calls.filter(
    ([input, init]) => String(input) === path && ((init as RequestInit | undefined)?.method ?? 'GET') === method,
  )
}

afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
  vi.restoreAllMocks()
})

describe('MemorySection controls', () => {
  it('shows the stored state as the selected choice', async () => {
    stubBackend({ ...ON, state: 'paused', reason: 'user_paused', use_allowed: false, generation_allowed: false })

    render(() => <MemorySection />)

    const paused = await screen.findByRole('radio', { name: 'Pause' })
    expect(paused.getAttribute('aria-checked')).toBe('true')
    expect(screen.getByRole('radio', { name: 'På' }).getAttribute('aria-checked')).toBe('false')
    expect(screen.getByRole('radio', { name: 'Av' }).getAttribute('aria-checked')).toBe('false')
  })

  it('sends only the chosen state and moves the selection', async () => {
    const fetchMock = stubBackend(ON, {
      'PUT /api/v1/memory/controls': () =>
        jsonResponse({ ...ON, state: 'paused', reason: 'user_paused', use_allowed: false }),
    })

    render(() => <MemorySection />)
    fireEvent.click(await screen.findByRole('radio', { name: 'Pause' }))
    flush()

    await waitFor(() => {
      expect(screen.getByRole('radio', { name: 'Pause' }).getAttribute('aria-checked')).toBe('true')
    })
    const put = callsTo(fetchMock, 'PUT', '/api/v1/memory/controls')[0] as [string, RequestInit]
    expect(JSON.parse(String(put[1].body))).toEqual({ state: 'paused' })
  })

  it('keeps the old selection and says so when the change is refused', async () => {
    stubBackend(ON, {
      'PUT /api/v1/memory/controls': () =>
        jsonResponse({ error: { code: 'memory_controls_unavailable', message: 'refused' } }, 409),
    })

    render(() => <MemorySection />)
    fireEvent.click(await screen.findByRole('radio', { name: 'Av' }))
    flush()

    expect(await screen.findByText('Kunne ikke endre minneinnstillingen.')).toBeTruthy()
    expect(screen.getByRole('radio', { name: 'På' }).getAttribute('aria-checked')).toBe('true')
  })

  it('explains an organization that turned memory off', async () => {
    stubBackend({ ...ON, reason: 'org_disabled', use_allowed: false, generation_allowed: false })

    render(() => <MemorySection />)

    expect(await screen.findByText(/Organisasjonen din har slått av minne/)).toBeTruthy()
  })

  it('locks the selector under Zero Data Retention but still allows a reset', async () => {
    stubBackend({ ...ON, reason: 'zdr', use_allowed: false, generation_allowed: false })

    render(() => <MemorySection />)

    expect(await screen.findByText(/Zero Data Retention er aktiv for denne økten/)).toBeTruthy()
    for (const name of ['På', 'Pause', 'Av']) {
      expect(screen.getByRole('radio', { name }).hasAttribute('disabled')).toBe(true)
    }
    expect(screen.getByRole('button', { name: /^Nullstill$/ }).hasAttribute('disabled')).toBe(false)
  })

  it('tells the user how many rows an off state hides', async () => {
    stubBackend({ ...ON, state: 'off', reason: 'user_off', use_allowed: false, hidden_rows: 4 })

    render(() => <MemorySection />)

    expect(await screen.findByText('4 minner er skjult mens minnet er av. De er ikke slettet.')).toBeTruthy()
  })
})

describe('MemorySection reset', () => {
  it('needs a second click, and the first one erases nothing', async () => {
    const fetchMock = stubBackend(ON, {
      'POST /api/v1/memory/reset': () =>
        jsonResponse({ durable_deleted: 3, semantic_unconfirmed: 0, degraded: false, degradation_reason: '' }),
    })

    render(() => <MemorySection />)
    fireEvent.click(await screen.findByRole('button', { name: /^Nullstill$/ }))
    flush()

    expect(await screen.findByText(/kan ikke angres/)).toBeTruthy()
    expect(callsTo(fetchMock, 'POST', '/api/v1/memory/reset')).toHaveLength(0)

    fireEvent.click(screen.getByRole('button', { name: /Bekreft nullstilling/ }))
    flush()

    expect(await screen.findByText('Minnet er nullstilt.')).toBeTruthy()
    const post = callsTo(fetchMock, 'POST', '/api/v1/memory/reset')[0] as [string, RequestInit]
    expect(JSON.parse(String(post[1].body))).toEqual({ confirm: true })
  })

  it('can be cancelled before it erases anything', async () => {
    const fetchMock = stubBackend()

    render(() => <MemorySection />)
    fireEvent.click(await screen.findByRole('button', { name: /^Nullstill$/ }))
    flush()
    fireEvent.click(await screen.findByRole('button', { name: 'Avbryt' }))
    flush()

    expect(await screen.findByRole('button', { name: /^Nullstill$/ })).toBeTruthy()
    expect(callsTo(fetchMock, 'POST', '/api/v1/memory/reset')).toHaveLength(0)
  })

  it('says when a semantic copy could not be confirmed erased', async () => {
    stubBackend(ON, {
      'POST /api/v1/memory/reset': () =>
        jsonResponse({ durable_deleted: 3, semantic_unconfirmed: 2, degraded: true, degradation_reason: 'LETTA' }),
    })

    render(() => <MemorySection />)
    fireEvent.click(await screen.findByRole('button', { name: /^Nullstill$/ }))
    flush()
    fireEvent.click(await screen.findByRole('button', { name: /Bekreft nullstilling/ }))
    flush()

    expect(await screen.findByText(/kunne ikke bekreftes slettet/)).toBeTruthy()
  })

  it('shows an error and no success note when the reset fails', async () => {
    stubBackend(ON, {
      'POST /api/v1/memory/reset': () =>
        jsonResponse({ error: { code: 'memory_reset_failed', message: 'down' } }, 502),
    })

    render(() => <MemorySection />)
    fireEvent.click(await screen.findByRole('button', { name: /^Nullstill$/ }))
    flush()
    fireEvent.click(await screen.findByRole('button', { name: /Bekreft nullstilling/ }))
    flush()

    expect(await screen.findByText('Kunne ikke nullstille minnet.')).toBeTruthy()
    expect(screen.queryByText('Minnet er nullstilt.')).toBeNull()
  })
})
