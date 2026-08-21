// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { OrgDeletionBanner } from '@/features/core/components/OrgDeletionBanner'
import type { DeletionStatus } from '@/shared/api/org-deletion-client'
import { clearSession, markSessionOnboardingComplete, setSessionUser } from '@/shared/session/session-store'

function jsonResponse(data: unknown, status = 200): Response {
  return new Response(JSON.stringify(data), {
    headers: { 'Content-Type': 'application/json' },
    status,
  })
}

function futureDeadline(days: number): string {
  return new Date(Date.now() + days * 24 * 60 * 60 * 1000).toISOString()
}

beforeEach(() => {
  // jsdom does not implement the Blob-download path used by "Export my data";
  // stub only the two URL methods it touches so the click handler can run.
  vi.stubGlobal('URL', {
    ...URL,
    createObjectURL: vi.fn(() => 'blob:mock'),
    revokeObjectURL: vi.fn(),
  })
})

afterEach(() => {
  cleanup()
  clearSession()
  vi.unstubAllGlobals()
})

describe('OrgDeletionBanner', () => {
  it('renders nothing when the status is not pending', () => {
    render(() => (
      <OrgDeletionBanner status={{ pending: false, org_name: 'Acme AS' }} onRefetch={vi.fn()} />
    ))

    expect(screen.queryByRole('alert')).toBeNull()
  })

  it('renders nothing while the status has not resolved yet', () => {
    render(() => <OrgDeletionBanner status={null} onRefetch={vi.fn()} />)

    expect(screen.queryByRole('alert')).toBeNull()
  })

  it('shows the org name and days remaining, with no cancel control for a plain member', () => {
    setSessionUser({ id: 'user_1', email: 'member@example.com', name: 'Member', emailVerified: true })
    flush()
    markSessionOnboardingComplete({ id: 'org_1', name: 'Acme AS', role: 'member' })
    flush()
    const status: DeletionStatus = {
      pending: true,
      deadline: futureDeadline(7),
      org_name: 'Acme AS',
      member_status: { user_id: 'user_1' },
    }

    render(() => <OrgDeletionBanner status={status} onRefetch={vi.fn()} />)

    expect(screen.getByText(/Acme AS/)).toBeTruthy()
    expect(screen.getByText(/7 dager/)).toBeTruthy()
    expect(screen.queryByRole('button', { name: 'Avbryt sletting' })).toBeNull()
    expect(screen.getByRole('button', { name: 'Eksporter mine data' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Bekreft' })).toBeTruthy()
  })

  it('shows the owner-only cancel-deletion control and restores the org on click', async () => {
    setSessionUser({ id: 'owner_1', email: 'owner@example.com', name: 'Owner', emailVerified: true })
    flush()
    markSessionOnboardingComplete({ id: 'org_1', name: 'Acme AS', role: 'owner' })
    flush()
    const fetchMock = vi.fn(async () => jsonResponse({ ok: true }))
    vi.stubGlobal('fetch', fetchMock)
    const onRefetch = vi.fn()
    const status: DeletionStatus = { pending: true, deadline: futureDeadline(3), org_name: 'Acme AS' }

    render(() => <OrgDeletionBanner status={status} onRefetch={onRefetch} />)
    fireEvent.click(screen.getByRole('button', { name: 'Avbryt sletting' }))

    await waitFor(() => expect(fetchMock).toHaveBeenCalled())
    const [path, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit]
    expect(path).toBe('/api/v1/orgs/org_1/gdpr/restore')
    expect(init.method).toBe('POST')
    await waitFor(() => expect(onRefetch).toHaveBeenCalled())
  })

  it('reconciles a false-failure 502 by checking whether the deletion was actually cancelled', async () => {
    setSessionUser({ id: 'owner_1', email: 'owner@example.com', name: 'Owner', emailVerified: true })
    flush()
    markSessionOnboardingComplete({ id: 'org_1', name: 'Acme AS', role: 'owner' })
    flush()
    const fetchMock = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input)
      if (path === '/api/v1/orgs/org_1/gdpr/restore') {
        // The restore reaches the backend and is durably recorded, but the
        // response itself is lost to a transient gateway error.
        return jsonResponse({ message: 'Bad Gateway' }, 502)
      }
      if (path === '/api/v1/orgs/org_1/gdpr/deletion/status') {
        return jsonResponse({ pending: false, org_name: 'Acme AS' } satisfies DeletionStatus)
      }
      return jsonResponse({ ok: true })
    })
    vi.stubGlobal('fetch', fetchMock)
    const onRefetch = vi.fn()
    const status: DeletionStatus = { pending: true, deadline: futureDeadline(3), org_name: 'Acme AS' }

    render(() => <OrgDeletionBanner status={status} onRefetch={onRefetch} />)
    fireEvent.click(screen.getByRole('button', { name: 'Avbryt sletting' }))

    await waitFor(() => expect(onRefetch).toHaveBeenCalled())
    expect(screen.queryByText(/unexpected error|bad gateway/i)).toBeNull()
  })

  it('shows a real failure when cancelling deletion genuinely did not go through', async () => {
    setSessionUser({ id: 'owner_1', email: 'owner@example.com', name: 'Owner', emailVerified: true })
    flush()
    markSessionOnboardingComplete({ id: 'org_1', name: 'Acme AS', role: 'owner' })
    flush()
    const fetchMock = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input)
      if (path === '/api/v1/orgs/org_1/gdpr/restore') {
        return jsonResponse({ message: 'Bad Gateway' }, 502)
      }
      if (path === '/api/v1/orgs/org_1/gdpr/deletion/status') {
        // Still pending — the restore genuinely didn't land.
        return jsonResponse({ pending: true, deadline: futureDeadline(3), org_name: 'Acme AS' } satisfies DeletionStatus)
      }
      return jsonResponse({ ok: true })
    })
    vi.stubGlobal('fetch', fetchMock)
    const onRefetch = vi.fn()
    const status: DeletionStatus = { pending: true, deadline: futureDeadline(3), org_name: 'Acme AS' }

    render(() => <OrgDeletionBanner status={status} onRefetch={onRefetch} />)
    fireEvent.click(screen.getByRole('button', { name: 'Avbryt sletting' }))

    await waitFor(() => expect(screen.getByText(/bad gateway/i)).toBeTruthy())
    expect(onRefetch).not.toHaveBeenCalled()
  })

  it('downloads the DSAR export then marks it received', async () => {
    setSessionUser({ id: 'user_1', email: 'member@example.com', name: 'Member', emailVerified: true })
    flush()
    markSessionOnboardingComplete({ id: 'org_1', name: 'Acme AS', role: 'member' })
    flush()
    const fetchMock = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input)
      if (path === '/api/v1/privacy/export') {
        return jsonResponse({
          subject: 'user',
          subject_id: 'user_1',
          generated_at: '2026-07-20T00:00:00Z',
          profile: {},
          org_memberships: [],
          api_keys: [],
          notes: [],
        })
      }
      return jsonResponse({ ok: true })
    })
    vi.stubGlobal('fetch', fetchMock)
    const onRefetch = vi.fn()
    const status: DeletionStatus = { pending: true, deadline: futureDeadline(10), org_name: 'Acme AS' }

    render(() => <OrgDeletionBanner status={status} onRefetch={onRefetch} />)
    fireEvent.click(screen.getByRole('button', { name: 'Eksporter mine data' }))

    await waitFor(() => {
      expect(fetchMock.mock.calls.some((call) => String(call[0]) === '/api/v1/privacy/export')).toBe(true)
    })
    await waitFor(() => {
      const call = fetchMock.mock.calls.find(
        (candidate) => String(candidate[0]) === '/api/v1/orgs/org_1/gdpr/deletion/mark-exported',
      )
      expect((call as unknown as [string, RequestInit] | undefined)?.[1]).toMatchObject({ method: 'POST' })
    })
    await waitFor(() => expect(onRefetch).toHaveBeenCalled())
  })

  it('reconciles a false-failure 502 by checking whether the export checkpoint was actually recorded', async () => {
    setSessionUser({ id: 'user_1', email: 'member@example.com', name: 'Member', emailVerified: true })
    flush()
    markSessionOnboardingComplete({ id: 'org_1', name: 'Acme AS', role: 'member' })
    flush()
    const fetchMock = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input)
      if (path === '/api/v1/privacy/export') {
        return jsonResponse({
          subject: 'user', subject_id: 'user_1', generated_at: '2026-07-20T00:00:00Z',
          profile: {}, org_memberships: [], api_keys: [], notes: [],
        })
      }
      if (path === '/api/v1/orgs/org_1/gdpr/deletion/mark-exported') {
        // The checkpoint reaches the backend and is durably recorded, but the
        // response itself is lost to a transient gateway error.
        return jsonResponse({ message: 'Bad Gateway' }, 502)
      }
      if (path === '/api/v1/orgs/org_1/gdpr/deletion/status') {
        return jsonResponse({
          pending: true, deadline: futureDeadline(10), org_name: 'Acme AS',
          member_status: { user_id: 'user_1', exported_at: '2026-08-07T00:00:00Z' },
        } satisfies DeletionStatus)
      }
      return jsonResponse({ ok: true })
    })
    vi.stubGlobal('fetch', fetchMock)
    const onRefetch = vi.fn()
    const status: DeletionStatus = { pending: true, deadline: futureDeadline(10), org_name: 'Acme AS' }

    render(() => <OrgDeletionBanner status={status} onRefetch={onRefetch} />)
    fireEvent.click(screen.getByRole('button', { name: 'Eksporter mine data' }))

    await waitFor(() => expect(onRefetch).toHaveBeenCalled())
    expect(screen.queryByText(/unexpected error|bad gateway/i)).toBeNull()
  })

  it('shows a real failure when the export checkpoint genuinely was not recorded', async () => {
    setSessionUser({ id: 'user_1', email: 'member@example.com', name: 'Member', emailVerified: true })
    flush()
    markSessionOnboardingComplete({ id: 'org_1', name: 'Acme AS', role: 'member' })
    flush()
    const fetchMock = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input)
      if (path === '/api/v1/privacy/export') {
        return jsonResponse({
          subject: 'user', subject_id: 'user_1', generated_at: '2026-07-20T00:00:00Z',
          profile: {}, org_memberships: [], api_keys: [], notes: [],
        })
      }
      if (path === '/api/v1/orgs/org_1/gdpr/deletion/mark-exported') {
        return jsonResponse({ message: 'Bad Gateway' }, 502)
      }
      if (path === '/api/v1/orgs/org_1/gdpr/deletion/status') {
        // No exported_at — the checkpoint genuinely didn't land.
        return jsonResponse({
          pending: true, deadline: futureDeadline(10), org_name: 'Acme AS',
          member_status: { user_id: 'user_1' },
        } satisfies DeletionStatus)
      }
      return jsonResponse({ ok: true })
    })
    vi.stubGlobal('fetch', fetchMock)
    const onRefetch = vi.fn()
    const status: DeletionStatus = { pending: true, deadline: futureDeadline(10), org_name: 'Acme AS' }

    render(() => <OrgDeletionBanner status={status} onRefetch={onRefetch} />)
    fireEvent.click(screen.getByRole('button', { name: 'Eksporter mine data' }))

    await waitFor(() => expect(screen.getByText(/bad gateway/i)).toBeTruthy())
    expect(onRefetch).not.toHaveBeenCalled()
  })

  it('acknowledges the pending-deletion notice for the calling member', async () => {
    setSessionUser({ id: 'user_1', email: 'member@example.com', name: 'Member', emailVerified: true })
    flush()
    markSessionOnboardingComplete({ id: 'org_1', name: 'Acme AS', role: 'member' })
    flush()
    const fetchMock = vi.fn(async () => jsonResponse({ ok: true }))
    vi.stubGlobal('fetch', fetchMock)
    const onRefetch = vi.fn()
    const status: DeletionStatus = { pending: true, deadline: futureDeadline(2), org_name: 'Acme AS' }

    render(() => <OrgDeletionBanner status={status} onRefetch={onRefetch} />)
    fireEvent.click(screen.getByRole('button', { name: 'Bekreft' }))

    await waitFor(() => expect(fetchMock).toHaveBeenCalled())
    const [path, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit]
    expect(path).toBe('/api/v1/orgs/org_1/gdpr/deletion/acknowledge')
    expect(init.method).toBe('POST')
    await waitFor(() => expect(onRefetch).toHaveBeenCalled())
  })

  it('reconciles a false-failure 502 by checking whether the acknowledgement was actually recorded', async () => {
    setSessionUser({ id: 'user_1', email: 'member@example.com', name: 'Member', emailVerified: true })
    flush()
    markSessionOnboardingComplete({ id: 'org_1', name: 'Acme AS', role: 'member' })
    flush()
    const fetchMock = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input)
      if (path === '/api/v1/orgs/org_1/gdpr/deletion/acknowledge') {
        return jsonResponse({ message: 'Bad Gateway' }, 502)
      }
      if (path === '/api/v1/orgs/org_1/gdpr/deletion/status') {
        return jsonResponse({
          pending: true, deadline: futureDeadline(2), org_name: 'Acme AS',
          member_status: { user_id: 'user_1', acknowledged_at: '2026-08-07T00:00:00Z' },
        } satisfies DeletionStatus)
      }
      return jsonResponse({ ok: true })
    })
    vi.stubGlobal('fetch', fetchMock)
    const onRefetch = vi.fn()
    const status: DeletionStatus = { pending: true, deadline: futureDeadline(2), org_name: 'Acme AS' }

    render(() => <OrgDeletionBanner status={status} onRefetch={onRefetch} />)
    fireEvent.click(screen.getByRole('button', { name: 'Bekreft' }))

    await waitFor(() => expect(onRefetch).toHaveBeenCalled())
    expect(screen.queryByText(/unexpected error|bad gateway/i)).toBeNull()
  })

  it('shows a real failure when acknowledging genuinely did not go through', async () => {
    setSessionUser({ id: 'user_1', email: 'member@example.com', name: 'Member', emailVerified: true })
    flush()
    markSessionOnboardingComplete({ id: 'org_1', name: 'Acme AS', role: 'member' })
    flush()
    const fetchMock = vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input)
      if (path === '/api/v1/orgs/org_1/gdpr/deletion/acknowledge') {
        return jsonResponse({ message: 'Bad Gateway' }, 502)
      }
      if (path === '/api/v1/orgs/org_1/gdpr/deletion/status') {
        // No acknowledged_at — the acknowledgement genuinely didn't land.
        return jsonResponse({
          pending: true, deadline: futureDeadline(2), org_name: 'Acme AS',
          member_status: { user_id: 'user_1' },
        } satisfies DeletionStatus)
      }
      return jsonResponse({ ok: true })
    })
    vi.stubGlobal('fetch', fetchMock)
    const onRefetch = vi.fn()
    const status: DeletionStatus = { pending: true, deadline: futureDeadline(2), org_name: 'Acme AS' }

    render(() => <OrgDeletionBanner status={status} onRefetch={onRefetch} />)
    fireEvent.click(screen.getByRole('button', { name: 'Bekreft' }))

    await waitFor(() => expect(screen.getByText(/bad gateway/i)).toBeTruthy())
    expect(onRefetch).not.toHaveBeenCalled()
  })

  it('shows the already-exported / already-acknowledged checkpoint state', () => {
    setSessionUser({ id: 'user_1', email: 'member@example.com', name: 'Member', emailVerified: true })
    flush()
    markSessionOnboardingComplete({ id: 'org_1', name: 'Acme AS', role: 'member' })
    flush()
    const status: DeletionStatus = {
      pending: true,
      deadline: futureDeadline(5),
      org_name: 'Acme AS',
      member_status: { user_id: 'user_1', exported_at: '2026-07-20T00:00:00Z', acknowledged_at: '2026-07-20T00:00:00Z' },
    }

    render(() => <OrgDeletionBanner status={status} onRefetch={vi.fn()} />)

    const exportButton = screen.getByRole('button', { name: /data eksportert/i }) as HTMLButtonElement
    const ackButton = screen.getByRole('button', { name: /bekreftet/i }) as HTMLButtonElement
    expect(ackButton.disabled).toBe(true)
    expect(exportButton).toBeTruthy()
  })
})
