// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
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
    markSessionOnboardingComplete({ id: 'org_1', name: 'Acme AS', role: 'admin' })

    render(() => <OrgDeletionDangerZone />)

    expect(screen.queryByRole('button', { name: /delete organization/i })).toBeNull()
  })

  it('is hidden when there is no active organization', () => {
    render(() => <OrgDeletionDangerZone />)

    expect(screen.queryByRole('button', { name: /delete organization/i })).toBeNull()
  })

  it('shows the control for the org owner and keeps the confirm button disabled until the name matches', () => {
    setSessionUser({ id: 'owner_1', email: 'owner@example.com', name: 'Owner', emailVerified: true })
    markSessionOnboardingComplete({ id: 'org_1', name: 'Acme AS', role: 'owner' })

    render(() => <OrgDeletionDangerZone />)
    fireEvent.click(screen.getByRole('button', { name: 'Delete organization…' }))

    const confirmButton = screen.getByRole('button', { name: 'Delete organization' }) as HTMLButtonElement
    expect(confirmButton.disabled).toBe(true)

    const input = screen.getByRole('textbox')
    fireEvent.input(input, { target: { value: 'Wrong Name' } })
    expect(confirmButton.disabled).toBe(true)

    fireEvent.input(input, { target: { value: 'Acme AS' } })
    expect(confirmButton.disabled).toBe(false)
  })

  it('soft-deletes with confirm:true and the exact org name, then shows the scheduled message', async () => {
    setSessionUser({ id: 'owner_1', email: 'owner@example.com', name: 'Owner', emailVerified: true })
    markSessionOnboardingComplete({ id: 'org_1', name: 'Acme AS', role: 'owner' })
    const fetchMock = vi.fn(async () => jsonResponse({ organization_id: 'org_1' }))
    vi.stubGlobal('fetch', fetchMock)

    render(() => <OrgDeletionDangerZone />)
    fireEvent.click(screen.getByRole('button', { name: 'Delete organization…' }))
    fireEvent.input(screen.getByRole('textbox'), { target: { value: 'Acme AS' } })
    fireEvent.click(screen.getByRole('button', { name: 'Delete organization' }))

    await waitFor(() => expect(fetchMock).toHaveBeenCalled())
    const [path, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit]
    expect(path).toBe('/api/v1/orgs/org_1/gdpr/soft-delete')
    expect(init.method).toBe('DELETE')
    expect(JSON.parse(String(init.body))).toEqual({ confirm: true, org_name: 'Acme AS' })
    expect(await screen.findByText(/deletion scheduled/i)).toBeTruthy()
    expect(screen.queryByRole('button', { name: 'Delete organization' })).toBeNull()
  })

  it('surfaces an org-name mismatch error from the backend instead of silently failing', async () => {
    setSessionUser({ id: 'owner_1', email: 'owner@example.com', name: 'Owner', emailVerified: true })
    markSessionOnboardingComplete({ id: 'org_1', name: 'Acme AS', role: 'owner' })
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
    fireEvent.input(screen.getByRole('textbox'), { target: { value: 'Acme AS' } })
    fireEvent.click(screen.getByRole('button', { name: 'Delete organization' }))

    expect(await screen.findByText(/organization name confirmation does not match/i)).toBeTruthy()
  })

  it('cancels the confirm form and clears the typed name', () => {
    setSessionUser({ id: 'owner_1', email: 'owner@example.com', name: 'Owner', emailVerified: true })
    markSessionOnboardingComplete({ id: 'org_1', name: 'Acme AS', role: 'owner' })

    render(() => <OrgDeletionDangerZone />)
    fireEvent.click(screen.getByRole('button', { name: 'Delete organization…' }))
    fireEvent.input(screen.getByRole('textbox'), { target: { value: 'Acme AS' } })
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }))

    expect(screen.queryByRole('textbox')).toBeNull()
    expect(screen.getByRole('button', { name: 'Delete organization…' })).toBeTruthy()
  })
})
