// @vitest-environment jsdom

import { createRouter, memoryHistory } from '@solidjs/router'
import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { FeedbackWidget } from '@/features/core/components/FeedbackWidget'
import { I18nProvider } from '@/shared/i18n'
import { clearSession, markSessionOnboardingComplete, setSessionUser } from '@/shared/session/session-store'

function renderWidget(path = '/inbox?view=mine') {
  const TestRouter = createRouter({
    routes: [{ path: '*all', component: FeedbackWidget }],
    history: memoryHistory(path),
    explicitLinks: true,
  })

  return render(() => (
    <TestRouter>{(props) => <I18nProvider>{props.children}</I18nProvider>}</TestRouter>
  ))
}

afterEach(() => {
  cleanup()
  clearSession()
  vi.unstubAllGlobals()
})

describe('FeedbackWidget', () => {
  it('does not render the trigger when there is no active organization', () => {
    renderWidget()

    expect(screen.queryByRole('button', { name: 'Tilbakemelding' })).toBeNull()
  })

  it('does not cover the persistent Verevon composer on the Support route', () => {
    setSessionUser({ id: 'user-1', email: 'ada@example.com', name: 'Ada', emailVerified: true })
    flush()
    markSessionOnboardingComplete({ id: 'org-1', name: 'Verevon', role: 'member' })
    flush()

    renderWidget('/support?view=all')

    expect(screen.queryByRole('button', { name: 'Tilbakemelding' })).toBeNull()
  })

  it('submits a one-line note to the feedback endpoint and shows a confirmation', async () => {
    setSessionUser({ id: 'user-1', email: 'ada@example.com', name: 'Ada', emailVerified: true })
    flush()
    markSessionOnboardingComplete({ id: 'org-1', name: 'Verevon', role: 'member' })
    flush()
    const fetchMock = vi.fn(async () => new Response(JSON.stringify({ data: { id: 'conversation-feedback-1' } }), {
      headers: { 'Content-Type': 'application/json' },
      status: 201,
    }))
    vi.stubGlobal('fetch', fetchMock)

    renderWidget('/inbox?view=mine')

    fireEvent.click(screen.getByRole('button', { name: 'Tilbakemelding' }))
    flush()
    const textarea = screen.getByRole('textbox', { name: 'Tilbakemeldingsnotat' })
    fireEvent.input(textarea, { target: { value: 'The knowledge tab spinner never resolves.' } })
    flush()
    fireEvent.click(screen.getByRole('button', { name: 'Send' }))

    await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(1))
    const [url, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit]
    expect(url).toBe('/api/v1/inbox/feedback')
    expect(new Headers(init.headers).get('x-verevon-org-id')).toBe('org-1')
    expect(JSON.parse(String(init.body))).toMatchObject({
      body_text: 'The knowledge tab spinner never resolves.',
      from_name: 'Ada',
      from_email: 'ada@example.com',
      page_url: '/inbox?view=mine',
    })

    expect(await screen.findByText('Takk! Tilbakemeldingen er sendt til teamet.')).toBeTruthy()
  })

  it('disables sending until the note has non-whitespace content', () => {
    setSessionUser({ id: 'user-1', email: 'ada@example.com', name: 'Ada', emailVerified: true })
    flush()
    markSessionOnboardingComplete({ id: 'org-1', name: 'Verevon', role: 'member' })
    flush()

    renderWidget()
    fireEvent.click(screen.getByRole('button', { name: 'Tilbakemelding' }))
    flush()

    const sendButton = screen.getByRole('button', { name: 'Send' }) as HTMLButtonElement
    expect(sendButton.disabled).toBe(true)

    fireEvent.input(screen.getByRole('textbox', { name: 'Tilbakemeldingsnotat' }), { target: { value: '   ' } })
    flush()
    expect(sendButton.disabled).toBe(true)

    fireEvent.input(screen.getByRole('textbox', { name: 'Tilbakemeldingsnotat' }), { target: { value: 'Real note' } })
    flush()
    expect(sendButton.disabled).toBe(false)
  })

  it('shows an inline error and keeps the note when the submission fails', async () => {
    setSessionUser({ id: 'user-1', email: 'ada@example.com', name: 'Ada', emailVerified: true })
    flush()
    markSessionOnboardingComplete({ id: 'org-1', name: 'Verevon', role: 'member' })
    flush()
    vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify({ error: { code: 'internal_error', message: 'nope' } }), {
      headers: { 'Content-Type': 'application/json' },
      status: 500,
    })))

    renderWidget()
    fireEvent.click(screen.getByRole('button', { name: 'Tilbakemelding' }))
    flush()
    fireEvent.input(screen.getByRole('textbox', { name: 'Tilbakemeldingsnotat' }), { target: { value: 'This will fail.' } })
    flush()
    fireEvent.click(screen.getByRole('button', { name: 'Send' }))

    expect(await screen.findByText('Kunne ikke sende tilbakemeldingen. Prøv igjen.')).toBeTruthy()
    expect((screen.getByRole('textbox', { name: 'Tilbakemeldingsnotat' }) as HTMLTextAreaElement).value).toBe('This will fail.')
  })
})
