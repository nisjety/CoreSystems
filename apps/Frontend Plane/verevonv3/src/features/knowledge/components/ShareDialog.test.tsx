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
