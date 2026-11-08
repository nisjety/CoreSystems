// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// Control the honesty gate per test, same convention as ownership-gate.dod.test.tsx.
// vi.mock is hoisted above the component import, so ShareDialog sees this
// mock's isGateOpen() rather than the real server-fetched singleton.
let gateOpen = true
vi.mock('@/shared/context/ownership-gate', () => ({
  isGateOpen: () => gateOpen,
  enforcementMode: () => (gateOpen ? 'strict' : 'off'),
  ownershipStatus: () => undefined,
}))

import { ShareDialog } from '@/features/knowledge/components/ShareDialog'
import { I18nProvider } from '@/shared/i18n'
import { ERROR_COPY } from '@/shared/i18n/errors'
import { localeStorageKey } from '@/shared/i18n/locales'

function jsonResponse(data: unknown, status = 200): Response {
  return new Response(JSON.stringify(data), {
    headers: { 'Content-Type': 'application/json' },
    status,
  })
}

const oneGrant = {
  grants: [
    { grant_id: 'grant_1', subject_id: 'user_2', role: 'viewer', granted_by: 'user_1', granted_at: '2026-08-01T00:00:00.000Z' },
  ],
}

beforeEach(() => {
  gateOpen = true
})

afterEach(() => {
  cleanup()
  try {
    window.localStorage.removeItem(localeStorageKey)
  } catch {
    // storage unavailable: nothing to clean
  }
  vi.unstubAllGlobals()
})

describe('ShareDialog', () => {
  it('lists the current shares and removes one on the happy path', async () => {
    let removed = false
    const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input)
      if (url === '/api/v1/documents/doc-1/shares' && (!init || init.method === undefined)) {
        return jsonResponse(removed ? { grants: [] } : oneGrant)
      }
      if (url === '/api/v1/documents/doc-1/shares/user_2' && init?.method === 'DELETE') {
        removed = true
        return jsonResponse({ ok: true })
      }
      return jsonResponse({ grants: [] })
    })
    vi.stubGlobal('fetch', fetchMock)

    render(() => <ShareDialog docId="doc-1" onClose={() => undefined} />)

    expect(await screen.findByText('user_2')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Remove user_2' }))

    await waitFor(() => expect(screen.queryByText('user_2')).toBeNull())
    expect(screen.queryByRole('alert')).toBeNull()
  })

  it('reconciles a false-failure 502 by checking whether the share was actually removed', async () => {
    let revoked = false
    const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input)
      if (url === '/api/v1/documents/doc-1/shares' && (!init || init.method === undefined)) {
        return jsonResponse(revoked ? { grants: [] } : oneGrant)
      }
      if (url === '/api/v1/documents/doc-1/shares/user_2' && init?.method === 'DELETE') {
        // The revoke reaches the backend and is durably recorded, but the
        // response itself is lost to a transient gateway error.
        revoked = true
        return jsonResponse({ message: 'Bad Gateway' }, 502)
      }
      return jsonResponse({ grants: [] })
    })
    vi.stubGlobal('fetch', fetchMock)

    render(() => <ShareDialog docId="doc-1" onClose={() => undefined} />)

    expect(await screen.findByText('user_2')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Remove user_2' }))

    await waitFor(() => expect(screen.queryByText('user_2')).toBeNull())
    expect(screen.queryByRole('alert')).toBeNull()
  })

  it('shows a real failure when a revoke request errors and the share genuinely is still shared', async () => {
    const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input)
      if (url === '/api/v1/documents/doc-1/shares' && (!init || init.method === undefined)) {
        return jsonResponse(oneGrant)
      }
      if (url === '/api/v1/documents/doc-1/shares/user_2' && init?.method === 'DELETE') {
        return jsonResponse({ message: 'Bad Gateway' }, 502)
      }
      return jsonResponse({ grants: [] })
    })
    vi.stubGlobal('fetch', fetchMock)

    render(() => <ShareDialog docId="doc-1" onClose={() => undefined} />)

    expect(await screen.findByText('user_2')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Remove user_2' }))

    expect((await screen.findByRole('alert')).textContent).toMatch(/delingen kunne ikke fjernes/i)
    expect(screen.getByText('user_2')).toBeTruthy()
  })
})

describe('ShareDialog (WS14-S4 rows and refusals)', () => {
  it('shows no Remove button on a room row, and one on every person row', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () =>
        jsonResponse({
          grants: [
            { grant_id: 'g1', subject_type: 'user', subject_id: 'user_2', role: 'view', granted_by: 'user_1', granted_at: '2026-11-01T00:00:00.000Z' },
            { grant_id: 'g2', subject_type: 'space', subject_id: 'space:org-1:room-7', role: 'view', granted_by: 'user_1', granted_at: '2026-11-01T00:00:00.000Z' },
            { grant_id: 'g3', subject_id: 'user_3', role: 'view', granted_by: 'user_1', granted_at: '2026-11-01T00:00:00.000Z' },
          ],
        }),
      ),
    )

    render(() => <ShareDialog docId="doc-1" onClose={() => undefined} />)

    expect(await screen.findByText('space:org-1:room-7')).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Remove user_2' })).toBeTruthy()
    // A row without subject_type is a person (older Control responses).
    expect(screen.getByRole('button', { name: 'Remove user_3' })).toBeTruthy()
    expect(screen.queryByRole('button', { name: 'Remove space:org-1:room-7' })).toBeNull()
    expect(screen.getByText('Room')).toBeTruthy()
    expect(screen.getAllByRole('button', { name: /^Remove / })).toHaveLength(2)
  })

  const refusals: Array<[string, number]> = [
    ['role_not_supported', 400],
    ['self_share', 409],
    ['share_limit_reached', 409],
    ['sharing_unavailable', 503],
    ['shares_disabled', 404],
  ]

  for (const locale of ['no', 'en'] as const) {
    for (const [code, status] of refusals) {
      it(`renders the ${locale} copy for the ${code} refusal`, async () => {
        window.localStorage.setItem(localeStorageKey, locale)
        vi.stubGlobal(
          'fetch',
          vi.fn(async (_input: RequestInfo | URL, init?: RequestInit) => {
            if (init?.method === 'POST') {
              return jsonResponse({ error: { code, message: 'RAW SERVER TEXT' } }, status)
            }
            return jsonResponse({ grants: [] })
          }),
        )

        render(() => (
          <I18nProvider>
            <ShareDialog docId="doc-1" onClose={() => undefined} />
          </I18nProvider>
        ))

        const input = (await screen.findByPlaceholderText('user-id or name@company.com')) as HTMLInputElement
        fireEvent.input(input, { target: { value: 'user_9' } })
        // Solid 2 flushes the signal on a microtask: wait for the button to enable.
        const share = screen.getByRole('button', { name: 'Share' }) as HTMLButtonElement
        await waitFor(() => expect(share.disabled).toBe(false))
        fireEvent.click(share)

        const alert = await screen.findByRole('alert')
        expect(alert.textContent).toBe(ERROR_COPY[code]?.[locale])
        expect(alert.textContent).not.toContain('RAW SERVER TEXT')
      })
    }
  }

  it('has copy in both locales for every refusal the share routes can return', () => {
    for (const [code] of refusals) {
      expect(ERROR_COPY[code]?.no, code).toBeTruthy()
      expect(ERROR_COPY[code]?.en, code).toBeTruthy()
      expect(ERROR_COPY[code]?.no, code).not.toBe(ERROR_COPY[code]?.en)
    }
  })
})
