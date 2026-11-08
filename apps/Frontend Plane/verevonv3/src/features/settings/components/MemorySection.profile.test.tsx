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
const LIST = {
  memories: [
    { memory_id: 'm-1', topic: 'USER', content: 'Prefers tables', updated_at: '', provenance: 'stated' },
    { memory_id: 'm-2', topic: 'USER', content: 'Works at Aquatiq', updated_at: '', provenance: 'stated' },
  ],
  degraded: false,
  degradation_reason: '',
}
const BUILT = {
  profile: {
    content: '- Prefers tables\n- Works at Aquatiq',
    source_memory_ids: ['m-1', 'm-2'],
    origin: 'built',
    revision: 4,
    updated_at: '2026-11-03T10:00:00Z',
    sources_deleted_since_edit: 0,
  },
  skip_reason: '',
}

/** Routes the section's reads; `overrides` replaces one answer by `METHOD path`. */
function stubBackend(profile: () => Response, overrides: Record<string, () => Response> = {}) {
  const fetchMock = vi.fn(async (input: unknown, init?: RequestInit) => {
    const key = `${init?.method ?? 'GET'} ${String(input)}`
    const override = overrides[key]
    if (override) return override()
    if (key === 'GET /api/v1/memory/controls') return jsonResponse(ON)
    if (key === 'GET /api/v1/memory') return jsonResponse(LIST)
    if (key === 'GET /api/v1/memory/profile') return profile()
    return jsonResponse({}, 404)
  })
  vi.stubGlobal('fetch', fetchMock)
  return fetchMock
}

function bodyOf(fetchMock: ReturnType<typeof stubBackend>, method: string, path: string): unknown {
  const call = fetchMock.mock.calls.find(
    ([input, init]) => String(input) === path && ((init as RequestInit | undefined)?.method ?? 'GET') === method,
  ) as [string, RequestInit] | undefined
  return call ? JSON.parse(String(call[1].body)) : undefined
}

afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
  vi.restoreAllMocks()
})

describe('MemorySection profile card', () => {
  it('is not shown, and says nothing, while the deployment has the profile switched off', async () => {
    stubBackend(() =>
      jsonResponse(
        { error: { code: 'profile_not_enabled', message: 'profile_not_enabled', details: { reason: '' } } },
        409,
      ),
    )

    render(() => <MemorySection />)

    expect(await screen.findByText('Prefers tables')).toBeTruthy()
    expect(screen.queryByText('Profil')).toBeNull()
    expect(screen.queryByText('Kunne ikke laste profilen.')).toBeNull()
  })

  it('shows the built text with each line linked to its memory', async () => {
    stubBackend(() => jsonResponse(BUILT))

    render(() => <MemorySection />)

    expect(await screen.findByText('Profil')).toBeTruthy()
    const text = screen.getByLabelText('Profiltekst') as HTMLTextAreaElement
    expect(text.value).toBe('- Prefers tables\n- Works at Aquatiq')
    expect(screen.getByText('Bygd fra 2 minner:')).toBeTruthy()
    expect(screen.getByRole('link', { name: '- Prefers tables' }).getAttribute('href')).toBe('#memory-m-1')
    expect(screen.getByRole('link', { name: '- Works at Aquatiq' }).getAttribute('href')).toBe('#memory-m-2')
  })

  it('saves the edit against the revision it was made on', async () => {
    const fetchMock = stubBackend(() => jsonResponse(BUILT), {
      'PUT /api/v1/memory/profile': () =>
        jsonResponse({
          profile: { ...BUILT.profile, content: '- Prefers tables, always', origin: 'edited', revision: 5 },
          skip_reason: '',
        }),
    })

    render(() => <MemorySection />)
    const text = (await screen.findByLabelText('Profiltekst')) as HTMLTextAreaElement
    const save = screen.getByRole('button', { name: 'Lagre profil' })
    expect(save.hasAttribute('disabled')).toBe(true)

    fireEvent.input(text, { target: { value: '- Prefers tables, always' } })
    flush()
    fireEvent.click(screen.getByRole('button', { name: 'Lagre profil' }))
    flush()

    await waitFor(() => {
      expect(bodyOf(fetchMock, 'PUT', '/api/v1/memory/profile')).toEqual({
        content: '- Prefers tables, always',
        expectedRevision: 4,
      })
    })
  })

  it('rebuilds on request with force, replacing an edit', async () => {
    const fetchMock = stubBackend(() => jsonResponse(BUILT), {
      'POST /api/v1/memory/profile/rebuild': () => jsonResponse(BUILT),
    })

    render(() => <MemorySection />)
    fireEvent.click(await screen.findByRole('button', { name: 'Bygg på nytt fra minnene mine' }))
    flush()

    await waitFor(() => {
      expect(bodyOf(fetchMock, 'POST', '/api/v1/memory/profile/rebuild')).toEqual({ force: true })
    })
  })

  it('keeps a paused profile editable but offers no rebuild', async () => {
    stubBackend(() => jsonResponse({ ...BUILT, skip_reason: 'user_paused' }))

    render(() => <MemorySection />)

    expect(await screen.findByText(/Profilen brukes ikke mens minnet står på pause/)).toBeTruthy()
    expect((screen.getByLabelText('Profiltekst') as HTMLTextAreaElement).hasAttribute('disabled')).toBe(false)
    expect(
      screen.getByRole('button', { name: 'Bygg på nytt fra minnene mine' }).hasAttribute('disabled'),
    ).toBe(true)
  })

  it('says how many sources of an edited profile were deleted', async () => {
    stubBackend(() =>
      jsonResponse({
        profile: { ...BUILT.profile, origin: 'edited', sources_deleted_since_edit: 1 },
        skip_reason: '',
      }),
    )

    render(() => <MemorySection />)

    expect(await screen.findByText(/^1 kilde\(r\) er slettet siden du redigerte/)).toBeTruthy()
    // An edited text has no line-to-memory pairing to show.
    expect(screen.queryByText(/Bygd fra/)).toBeNull()
  })

  it('shows no text and no editor under Zero Data Retention', async () => {
    stubBackend(() => jsonResponse({ profile: null, skip_reason: 'zdr' }))

    render(() => <MemorySection />)

    expect(await screen.findByText(/Profilen vises ikke, brukes ikke og kan ikke endres herfra/)).toBeTruthy()
    expect(screen.queryByLabelText('Profiltekst')).toBeNull()
    expect(screen.queryByRole('button', { name: 'Lagre profil' })).toBeNull()
  })

  it('takes a deleted memory out of the card by reading the profile again', async () => {
    let profileReads = 0
    stubBackend(
      () => {
        profileReads += 1
        return jsonResponse(BUILT)
      },
      { 'DELETE /api/v1/memory/m-2': () => jsonResponse({ deleted: true, degraded: false, degradation_reason: '' }) },
    )

    render(() => <MemorySection />)
    fireEvent.click(await screen.findByRole('button', { name: 'Slett minne: Works at Aquatiq' }))
    flush()
    fireEvent.click(screen.getByRole('button', { name: 'Bekreft sletting av minne: Works at Aquatiq' }))
    flush()

    await waitFor(() => {
      expect(profileReads).toBe(2)
    })
  })
})
