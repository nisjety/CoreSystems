// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { OrgDeletionDangerZone } from '@/features/settings/components/OrgDeletionDangerZone'
import { clearSession, markSessionOnboardingComplete, setSessionUser } from '@/shared/session/session-store'

function jsonResponse(data: unknown, status = 200): Response {
  return new Response(JSON.stringify(data), {
    headers: { 'Content-Type': 'application/json' },
    status,
  })
}

afterEach(() => {
  cleanup()
  clearSession()
  vi.unstubAllGlobals()
})

describe('OrgDeletionDangerZone', () => {
  it('is hidden entirely for a non-owner (admin or member)', () => {
    setSessionUser({ id: 'user_1', email: 'admin@example.com', name: 'Admin', emailVerified: true })
    flush()
    markSessionOnboardingComplete({ id: 'org_1', name: 'Acme AS', role: 'admin' })
    flush()

    render(() => <OrgDeletionDangerZone />)

    expect(screen.queryByRole('button', { name: /delete organization/i })).toBeNull()
  })

  it('is hidden when there is no active organization', () => {
    render(() => <OrgDeletionDangerZone />)

    expect(screen.queryByRole('button', { name: /delete organization/i })).toBeNull()
  })

  it('shows the control for the org owner and keeps the confirm button disabled until the name matches', () => {
    setSessionUser({ id: 'owner_1', email: 'owner@example.com', name: 'Owner', emailVerified: true })
    flush()
    markSessionOnboardingComplete({ id: 'org_1', name: 'Acme AS', role: 'owner' })
    flush()

    render(() => <OrgDeletionDangerZone />)
    fireEvent.click(screen.getByRole('button', { name: 'Delete organization…' }))
    flush()

    const confirmButton = screen.getByRole('button', { name: 'Delete organization' }) as HTMLButtonElement
    expect(confirmButton.disabled).toBe(true)

    const input = screen.getByRole('textbox')
    fireEvent.input(input, { target: { value: 'Wrong Name' } })
    flush()
    expect(confirmButton.disabled).toBe(true)

    fireEvent.input(input, { target: { value: 'Acme AS' } })
    flush()
    expect(confirmButton.disabled).toBe(false)
  })

  it('soft-deletes with confirm:true and the exact org name, then shows the scheduled message', async () => {
    setSessionUser({ id: 'owner_1', email: 'owner@example.com', name: 'Owner', emailVerified: true })
    flush()
    markSessionOnboardingComplete({ id: 'org_1', name: 'Acme AS', role: 'owner' })
    flush()
    const fetchMock = vi.fn(async () => jsonResponse({ organization_id: 'org_1' }))
    vi.stubGlobal('fetch', fetchMock)

    render(() => <OrgDeletionDangerZone />)
    fireEvent.click(screen.getByRole('button', { name: 'Delete organization…' }))
    flush()
    fireEvent.input(screen.getByRole('textbox'), { target: { value: 'Acme AS' } })
    flush()
    fireEvent.click(screen.getByRole('button', { name: 'Delete organization' }))
    flush()

    await waitFor(() => expect(fetchMock).toHaveBeenCalled())
    const [path, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit]
    expect(path).toBe('/api/v1/orgs/org_1/gdpr/soft-delete')
    expect(init.method).toBe('DELETE')
    expect(JSON.parse(String(init.body))).toEqual({ confirm: true, org_name: 'Acme AS' })
    expect(await screen.findByText(/deletion scheduled/i)).toBeTruthy()
    expect(screen.queryByRole('button', { name: 'Delete organization' })).toBeNull()
  })

  it('reconciles a false-failure 502 by checking whether the org was actually scheduled for deletion', async () => {
    setSessionUser({ id: 'owner_1', email: 'owner@example.com', name: 'Owner', emailVerified: true })
    flush()
    markSessionOnboardingComplete({ id: 'org_1', name: 'Acme AS', role: 'owner' })
    flush()
    const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (path === '/api/v1/orgs/org_1/gdpr/soft-delete' && init?.method === 'DELETE') {
        // The soft-delete reaches the backend and is durably recorded, but
        // the response itself is lost to a transient gateway error.
        return jsonResponse({ message: 'Bad Gateway' }, 502)
      }
      if (path === '/api/v1/orgs/org_1/gdpr/deletion/status') {
        return jsonResponse({ pending: true, deadline: '2026-09-06T00:00:00Z', org_name: 'Acme AS' })
      }
      return jsonResponse({ ok: true })
    })
    vi.stubGlobal('fetch', fetchMock)

    render(() => <OrgDeletionDangerZone />)
    fireEvent.click(screen.getByRole('button', { name: 'Delete organization…' }))
    flush()
    fireEvent.input(screen.getByRole('textbox'), { target: { value: 'Acme AS' } })
    flush()
    fireEvent.click(screen.getByRole('button', { name: 'Delete organization' }))
    flush()

    expect(await screen.findByText(/deletion scheduled/i)).toBeTruthy()
    expect(screen.queryByText(/bad gateway/i)).toBeNull()
  })

  it('shows a real failure when the soft-delete genuinely did not go through', async () => {
    setSessionUser({ id: 'owner_1', email: 'owner@example.com', name: 'Owner', emailVerified: true })
    flush()
    markSessionOnboardingComplete({ id: 'org_1', name: 'Acme AS', role: 'owner' })
    flush()
    const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (path === '/api/v1/orgs/org_1/gdpr/soft-delete' && init?.method === 'DELETE') {
        return jsonResponse({ message: 'Bad Gateway' }, 502)
      }
      if (path === '/api/v1/orgs/org_1/gdpr/deletion/status') {
        // Still not pending — the soft-delete genuinely didn't land.
        return jsonResponse({ pending: false, org_name: 'Acme AS' })
      }
      return jsonResponse({ ok: true })
    })
    vi.stubGlobal('fetch', fetchMock)

    render(() => <OrgDeletionDangerZone />)
    fireEvent.click(screen.getByRole('button', { name: 'Delete organization…' }))
    flush()
    fireEvent.input(screen.getByRole('textbox'), { target: { value: 'Acme AS' } })
    flush()
    fireEvent.click(screen.getByRole('button', { name: 'Delete organization' }))
    flush()

    expect(await screen.findByText(/bad gateway/i)).toBeTruthy()
    expect(screen.queryByText(/deletion scheduled/i)).toBeNull()
  })

  it('surfaces an org-name mismatch error from the backend instead of silently failing', async () => {
    setSessionUser({ id: 'owner_1', email: 'owner@example.com', name: 'Owner', emailVerified: true })
    flush()
    markSessionOnboardingComplete({ id: 'org_1', name: 'Acme AS', role: 'owner' })
    flush()
    vi.stubGlobal(
      'fetch',
      vi.fn(async () =>
        jsonResponse(
          { error: 'organization name confirmation does not match; type the exact organization name to confirm' },
          400,
        ),
      ),
    )

    render(() => <OrgDeletionDangerZone />)
    fireEvent.click(screen.getByRole('button', { name: 'Delete organization…' }))
    flush()
    fireEvent.input(screen.getByRole('textbox'), { target: { value: 'Acme AS' } })
    flush()
    fireEvent.click(screen.getByRole('button', { name: 'Delete organization' }))
    flush()

    expect(await screen.findByText(/organization name confirmation does not match/i)).toBeTruthy()
  })

  it('cancels the confirm form and clears the typed name', () => {
    setSessionUser({ id: 'owner_1', email: 'owner@example.com', name: 'Owner', emailVerified: true })
    flush()
    markSessionOnboardingComplete({ id: 'org_1', name: 'Acme AS', role: 'owner' })
    flush()

    render(() => <OrgDeletionDangerZone />)
    fireEvent.click(screen.getByRole('button', { name: 'Delete organization…' }))
    flush()
    fireEvent.input(screen.getByRole('textbox'), { target: { value: 'Acme AS' } })
    flush()
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }))
    flush()

    expect(screen.queryByRole('textbox')).toBeNull()
    expect(screen.getByRole('button', { name: 'Delete organization…' })).toBeTruthy()
  })
})
