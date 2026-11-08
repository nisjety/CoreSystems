// @vitest-environment jsdom

/**
 * Thumbs and reason chips on a durable run's result. Off by default. With the switch
 * on, a rating goes by run id, a reason is offered only once a thumbs-down is stored,
 * and a refused rating (Zero Data Retention) is said, not shown as saved.
 */

import { fireEvent, render, waitFor, within } from '@solidjs/testing-library'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { I18nProvider } from '@/shared/i18n'
import { localeStorageKey } from '@/shared/i18n/locales'
import { ResultFeedback, resultFeedbackEnabled } from './ResultFeedback'

function jsonResponse(data: unknown, status = 200): Response {
  return new Response(JSON.stringify(data), {
    headers: { 'Content-Type': 'application/json' },
    status,
  })
}

/** The bodies sent to the rating route, in order. */
function sentRatings(fetchMock: { mock: { calls: unknown[][] } }): Array<Record<string, unknown>> {
  return fetchMock.mock.calls
    .filter((call) => call[0] === '/api/v1/chat/feedback')
    .map((call) => JSON.parse(String((call[1] as RequestInit).body)))
}

afterEach(() => {
  vi.unstubAllGlobals()
  vi.unstubAllEnvs()
})

describe('ResultFeedback', () => {
  it('renders nothing while the switch is off, and sends nothing', () => {
    const fetchMock = vi.fn(async (_path: string, _init?: RequestInit) => jsonResponse({ accepted: true }))
    vi.stubGlobal('fetch', fetchMock)

    const { container, unmount } = render(() => <ResultFeedback runId="run_1" />)

    expect(resultFeedbackEnabled()).toBe(false)
    expect(container.querySelector('.verevon-result-feedback')).toBeNull()
    expect(fetchMock).not.toHaveBeenCalled()
    unmount()
  })

  it('stores a thumbs-up for the run, shows it pressed, and offers no reason', async () => {
    vi.stubEnv('VITE_AGENT_RESULT_FEEDBACK', 'true')
    const fetchMock = vi.fn(async (_path: string, _init?: RequestInit) => jsonResponse({ accepted: true }))
    vi.stubGlobal('fetch', fetchMock)
    const { getByRole, queryByRole, unmount } = render(() => <ResultFeedback runId="run_1" />)

    fireEvent.click(getByRole('button', { name: 'Godt svar' }))

    await waitFor(() => expect(getByRole('button', { name: 'Godt svar' }).getAttribute('aria-pressed')).toBe('true'))
    expect(sentRatings(fetchMock)).toEqual([{ run_id: 'run_1', rating: 'positive' }])
    expect(queryByRole('group', { name: 'Hva var galt?' })).toBeNull()
    unmount()
  })

  it('offers the reasons only after a thumbs-down is stored, then sends the chosen reason as the note', async () => {
    vi.stubEnv('VITE_AGENT_RESULT_FEEDBACK', 'true')
    const fetchMock = vi.fn(async (_path: string, _init?: RequestInit) => jsonResponse({ accepted: true }))
    vi.stubGlobal('fetch', fetchMock)
    const { getByRole, findByRole, findByText, unmount } = render(() => <ResultFeedback runId="run_1" />)

    fireEvent.click(getByRole('button', { name: 'Dårlig svar' }))
    const reasons = await findByRole('group', { name: 'Hva var galt?' })
    fireEvent.click(within(reasons).getByRole('button', { name: 'Unøyaktig' }))

    expect(await findByText(/Takk — merket som/)).toBeTruthy()
    expect(sentRatings(fetchMock)).toEqual([
      { run_id: 'run_1', rating: 'negative' },
      { run_id: 'run_1', rating: 'negative', note: 'Unøyaktig' },
    ])
    expect(getByRole('button', { name: 'Dårlig svar' }).getAttribute('aria-pressed')).toBe('true')
    unmount()
  })

  it('shows the zero-retention refusal, offers no reason and sends no note, because nothing was stored', async () => {
    vi.stubEnv('VITE_AGENT_RESULT_FEEDBACK', 'true')
    const fetchMock = vi.fn(async (_path: string, _init?: RequestInit) =>
      jsonResponse({ error: { code: 'zdr_durable_mutation_forbidden', message: 'refused' } }, 412),
    )
    vi.stubGlobal('fetch', fetchMock)
    const { getByRole, findByRole, queryByRole, unmount } = render(() => <ResultFeedback runId="run_1" />)

    fireEvent.click(getByRole('button', { name: 'Dårlig svar' }))

    expect((await findByRole('alert')).textContent).toBe('Vurderinger lagres ikke når Zero Data Retention er aktivt.')
    expect(queryByRole('group', { name: 'Hva var galt?' })).toBeNull()
    expect(getByRole('button', { name: 'Dårlig svar' }).getAttribute('aria-pressed')).toBe('false')
    expect(sentRatings(fetchMock)).toEqual([{ run_id: 'run_1', rating: 'negative' }])
    unmount()
  })

  it('says a rating that failed for another reason, and leaves the thumb unpressed', async () => {
    vi.stubEnv('VITE_AGENT_RESULT_FEEDBACK', 'true')
    vi.stubGlobal('fetch', vi.fn(async (_path: string, _init?: RequestInit) =>
      jsonResponse({ error: { code: 'bad_gateway', message: 'down' } }, 502),
    ))
    const { getByRole, findByRole, unmount } = render(() => <ResultFeedback runId="run_1" />)

    fireEvent.click(getByRole('button', { name: 'Godt svar' }))

    expect((await findByRole('alert')).textContent).toBe('Kunne ikke lagre vurderingen. Prøv igjen.')
    expect(getByRole('button', { name: 'Godt svar' }).getAttribute('aria-pressed')).toBe('false')
    unmount()
  })

  it('offers no reason while the thumbs-down is still in flight', async () => {
    vi.stubEnv('VITE_AGENT_RESULT_FEEDBACK', 'true')
    let release: () => void = () => {}
    const fetchMock = vi.fn((_path: string, _init?: RequestInit) => new Promise<Response>((resolve) => {
      release = () => resolve(jsonResponse({ accepted: true }))
    }))
    vi.stubGlobal('fetch', fetchMock)
    const { getByRole, queryByRole, findByRole, unmount } = render(() => <ResultFeedback runId="run_1" />)

    fireEvent.click(getByRole('button', { name: 'Dårlig svar' }))
    await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(1))
    expect(queryByRole('group', { name: 'Hva var galt?' })).toBeNull()

    release()
    expect(await findByRole('group', { name: 'Hva var galt?' })).toBeTruthy()
    unmount()
  })

  it('sends one rating while one is in flight, however often the thumb is pressed', async () => {
    vi.stubEnv('VITE_AGENT_RESULT_FEEDBACK', 'true')
    let release: () => void = () => {}
    const fetchMock = vi.fn((_path: string, _init?: RequestInit) => new Promise<Response>((resolve) => {
      release = () => resolve(jsonResponse({ accepted: true }))
    }))
    vi.stubGlobal('fetch', fetchMock)
    const { getByRole, unmount } = render(() => <ResultFeedback runId="run_1" />)

    fireEvent.click(getByRole('button', { name: 'Godt svar' }))
    await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(1))
    fireEvent.click(getByRole('button', { name: 'Godt svar' }))
    expect(fetchMock).toHaveBeenCalledTimes(1)

    release()
    await waitFor(() => expect(getByRole('button', { name: 'Godt svar' }).getAttribute('aria-pressed')).toBe('true'))
    unmount()
  })
})

describe('ResultFeedback in English', () => {
  beforeEach(() => {
    window.localStorage.setItem(localeStorageKey, 'en')
  })

  afterEach(() => {
    window.localStorage.removeItem(localeStorageKey)
  })

  it('offers the English reasons after a thumbs-down, and names the one chosen', async () => {
    vi.stubEnv('VITE_AGENT_RESULT_FEEDBACK', 'true')
    const fetchMock = vi.fn(async (_path: string, _init?: RequestInit) => jsonResponse({ accepted: true }))
    vi.stubGlobal('fetch', fetchMock)
    const { getByRole, findByRole, findByText, unmount } = render(() => (
      <I18nProvider>
        <ResultFeedback runId="run_1" />
      </I18nProvider>
    ))

    expect(getByRole('group', { name: 'Rate this answer' })).toBeTruthy()
    fireEvent.click(getByRole('button', { name: 'Bad response' }))
    const reasons = await findByRole('group', { name: 'What was wrong?' })
    fireEvent.click(within(reasons).getByRole('button', { name: 'Inaccurate' }))

    expect(await findByText(/Thanks — flagged as/)).toBeTruthy()
    expect(sentRatings(fetchMock)).toEqual([
      { run_id: 'run_1', rating: 'negative' },
      { run_id: 'run_1', rating: 'negative', note: 'Inaccurate' },
    ])
    unmount()
  })

  it('says a refused rating in English', async () => {
    vi.stubEnv('VITE_AGENT_RESULT_FEEDBACK', 'true')
    vi.stubGlobal('fetch', vi.fn(async (_path: string, _init?: RequestInit) =>
      jsonResponse({ error: { code: 'zdr_durable_mutation_forbidden', message: 'refused' } }, 412),
    ))
    const { getByRole, findByRole, unmount } = render(() => (
      <I18nProvider>
        <ResultFeedback runId="run_1" />
      </I18nProvider>
    ))

    fireEvent.click(getByRole('button', { name: 'Bad response' }))

    expect((await findByRole('alert')).textContent).toBe('Ratings are not saved while zero data retention is on.')
    unmount()
  })
})
