// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { MailTaskAddressSection } from '@/features/settings/components/MailTaskAddressSection'

function jsonResponse(data: unknown, status = 200): Response {
  return new Response(JSON.stringify(data), {
    headers: { 'Content-Type': 'application/json' },
    status,
  })
}

function errorResponse(status: number, code: string): Response {
  return new Response(JSON.stringify({ error: { code, message: 'nope' } }), {
    headers: { 'Content-Type': 'application/json' },
    status,
  })
}

afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
  vi.restoreAllMocks()
})

describe('MailTaskAddressSection', () => {
  it('renders Norwegian labels by default and loads the address', async () => {
    const fetchMock = vi.fn(async () => jsonResponse({ address: 'abc123def456@tasks.verevon.ai' }))
    vi.stubGlobal('fetch', fetchMock)

    render(() => <MailTaskAddressSection />)

    expect(await screen.findByText('E-post til oppgave')).toBeTruthy()
    expect(screen.getByText(/Bare e-post fra din egen verifiserte adresse godtas/)).toBeTruthy()
    expect(await screen.findByText('abc123def456@tasks.verevon.ai')).toBeTruthy()

    const [path, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit]
    expect(path).toBe('/api/v1/users/me/mail-task-address')
    expect(init.method).toBe('POST')
  })

  it('copies the address to the clipboard and shows confirmation', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => jsonResponse({ address: 'abc123def456@tasks.verevon.ai' })))
    const writeText = vi.fn(async () => undefined)
    vi.stubGlobal('navigator', { ...navigator, clipboard: { writeText } })

    render(() => <MailTaskAddressSection />)
    await screen.findByText('abc123def456@tasks.verevon.ai')

    fireEvent.click(screen.getByRole('button', { name: /Kopier/ }))

    await waitFor(() => expect(writeText).toHaveBeenCalledWith('abc123def456@tasks.verevon.ai'))
    expect(await screen.findByText('Kopiert')).toBeTruthy()
  })

  it('regenerates: DELETE then GET, and shows the new address', async () => {
    let callCount = 0
    const fetchMock = vi.fn(async (_input: unknown, init?: RequestInit) => {
      callCount += 1
      if (callCount === 1) {
        expect(init?.method).toBe('POST')
        return jsonResponse({ address: 'old-alias@tasks.verevon.ai' })
      }
      if (callCount === 2) {
        expect(init?.method).toBe('DELETE')
        return jsonResponse({})
      }
      expect(init?.method).toBe('GET')
      return jsonResponse({ address: 'new-alias@tasks.verevon.ai' })
    })
    vi.stubGlobal('fetch', fetchMock)
    vi.spyOn(window, 'confirm').mockReturnValue(true)

    render(() => <MailTaskAddressSection />)
    await screen.findByText('old-alias@tasks.verevon.ai')

    fireEvent.click(screen.getByRole('button', { name: /Generer på nytt/ }))

    expect(await screen.findByText('new-alias@tasks.verevon.ai')).toBeTruthy()
    expect(screen.queryByText('old-alias@tasks.verevon.ai')).toBeNull()
    expect(callCount).toBe(3)
  })

  it('does not regenerate when the confirmation dialog is declined', async () => {
    const fetchMock = vi.fn(async () => jsonResponse({ address: 'abc123def456@tasks.verevon.ai' }))
    vi.stubGlobal('fetch', fetchMock)
    vi.spyOn(window, 'confirm').mockReturnValue(false)

    render(() => <MailTaskAddressSection />)
    await screen.findByText('abc123def456@tasks.verevon.ai')

    fireEvent.click(screen.getByRole('button', { name: /Generer på nytt/ }))

    // Only the initial load call — the confirm-declined regenerate never
    // issues a DELETE.
    await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(1))
  })

  it('shows the "not available yet" state for a 503 without a generic error', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => errorResponse(503, 'mail_task_not_configured')))

    render(() => <MailTaskAddressSection />)

    expect(
      await screen.findByText('E-post til oppgave er ikke tilgjengelig ennå for denne organisasjonen.'),
    ).toBeTruthy()
    expect(screen.queryByText('Kunne ikke laste adressen.')).toBeNull()
  })
})
