// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { OrgWebAccessSection } from '@/features/customize/OrgWebAccessSection'
import { I18nProvider } from '@/shared/i18n'
import { clearSession, markSessionOnboardingComplete, setSessionUser } from '@/shared/session/session-store'

function jsonResponse(data: unknown, status = 200): Response {
  return new Response(JSON.stringify(data), {
    headers: { 'Content-Type': 'application/json' },
    status,
  })
}

function signIn(role: 'admin' | 'member') {
  setSessionUser({ id: 'user_1', email: 'user@example.com', name: 'User', emailVerified: true })
  flush()
  markSessionOnboardingComplete({ id: 'org_1', name: 'Acme AS', role })
  flush()
}

const STORED = {
  org_id: 'org_1',
  revision: 4,
  digest: 'd4',
  settings: { mode_cap: 'read_only', 'web.mode': 'live', 'web.paid_providers': 'on' },
}

type Call = { method: string; path: string; body: unknown }

/** Answers the policy route: a GET reads the stored document, a PUT is answered by `put`. Every call is recorded. */
function stubPolicy(stored: unknown = STORED, put: () => Response = () => jsonResponse({ ...STORED, revision: 5 })): Call[] {
  const calls: Call[] = []
  vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = new URL(String(input), 'http://localhost')
    const method = init?.method ?? 'GET'
    calls.push({ method, path: url.pathname, body: init?.body ? JSON.parse(String(init.body)) : undefined })
    return method === 'PUT' ? put() : jsonResponse(stored)
  }))
  return calls
}

beforeEach(() => {
  window.localStorage.setItem('verevon.locale', 'en')
})

afterEach(() => {
  cleanup()
  clearSession()
  vi.unstubAllGlobals()
})

describe('OrgWebAccessSection', () => {
  it('an admin turns web access off and saves the whole document at the revision it read', async () => {
    signIn('admin')
    const calls = stubPolicy()

    render(() => <I18nProvider><OrgWebAccessSection /></I18nProvider>)
    const mode = (await screen.findByRole('combobox', { name: /Web access for agents/ })) as HTMLSelectElement
    expect(mode.value).toBe('live')
    fireEvent.change(mode, { target: { value: 'off' } })
    flush()
    fireEvent.click(screen.getByRole('button', { name: 'Save web access' }))
    flush()

    await waitFor(() => expect(calls.some((call) => call.method === 'PUT')).toBe(true))
    const put = calls.find((call) => call.method === 'PUT')
    expect(put?.path).toBe('/api/v1/org/agent-policy')
    // The server takes an omitted key as its default, so the settings this section does not change go back as stored.
    expect(put?.body).toEqual({
      revision: 4,
      settings: { mode_cap: 'read_only', 'web.mode': 'off', 'web.paid_providers': 'on' },
    })
  })

  it('a member sees the switches locked, with no save control', async () => {
    signIn('member')
    stubPolicy()

    render(() => <I18nProvider><OrgWebAccessSection /></I18nProvider>)
    const paid = (await screen.findByRole('combobox', { name: /Paid search services/ })) as HTMLSelectElement

    expect(paid.disabled).toBe(true)
    expect(screen.queryByRole('button', { name: 'Save web access' })).toBeNull()
  })

  it('reads a web mode the server would not enforce as off, never as live', async () => {
    signIn('admin')
    stubPolicy({ ...STORED, settings: { 'web.mode': 'cached_only' } })

    render(() => <I18nProvider><OrgWebAccessSection /></I18nProvider>)
    const mode = (await screen.findByRole('combobox', { name: /Web access for agents/ })) as HTMLSelectElement

    expect(mode.value).toBe('off')
  })
})
