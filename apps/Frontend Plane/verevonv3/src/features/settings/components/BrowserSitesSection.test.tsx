// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  BrowserSitesSection,
  decisionLabel,
  levelLabel,
  sourceLabel,
} from '@/features/settings/components/BrowserSitesSection'
import { clearSession, markSessionOnboardingComplete, setSessionUser } from '@/shared/session/session-store'

function jsonResponse(data: unknown, status = 200): Response {
  return new Response(JSON.stringify(data), {
    headers: { 'Content-Type': 'application/json' },
    status,
  })
}

function signIn(role: 'owner' | 'admin' | 'member') {
  setSessionUser({ id: 'user_1', email: 'user@example.com', name: 'User', emailVerified: true })
  flush()
  markSessionOnboardingComplete({ id: 'org_1', name: 'Acme AS', role })
  flush()
}

// Solid batches writes until a flush, so each interaction is settled before the next.
const click = (element: HTMLElement) => { fireEvent.click(element); flush() }
const change = (element: HTMLElement, value: string) => { fireEvent.change(element, { target: { value } }); flush() }
const type = (element: HTMLElement, value: string) => { fireEvent.input(element, { target: { value } }); flush() }

const CHOICES = {
  organization_id: 'org_1',
  user_id: 'user_1',
  revision: 3,
  prefs: [
    {
      host: 'shop.example.com',
      level: 'read',
      expires_at: '2026-12-01T00:00:00Z',
      updated_at: '2026-11-01T00:00:00Z',
    },
  ],
}

type Call = { url: string; method: string; body: unknown }

/** Answers each request with `answer`, or with the stored choices when `answer` says nothing. */
function stubGateway(answer: (call: Call) => Response | undefined): Call[] {
  const calls: Call[] = []
  vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const call: Call = {
      url: String(input),
      method: init?.method ?? 'GET',
      body: init?.body ? JSON.parse(String(init.body)) : undefined,
    }
    calls.push(call)
    return answer(call) ?? jsonResponse(CHOICES)
  }))
  return calls
}

afterEach(() => {
  cleanup()
  clearSession()
  vi.unstubAllGlobals()
  vi.unstubAllEnvs()
})

describe('browser site labels', () => {
  it('names each level, decision and source in Norwegian by default', () => {
    const tr = (no: string) => no
    expect(levelLabel('read', tr)).toBe('Bare lesing')
    expect(decisionLabel('ask', tr)).toBe('Du blir spurt før første bruk')
    expect(sourceLabel('org_block', tr)).toBe('Blokkert av administrator')
  })
})

describe('BrowserSitesSection', () => {
  it('is absent, and reads nothing, unless the build switches browser sites on', () => {
    const calls = stubGateway(() => undefined)
    signIn('member')
    render(() => <BrowserSitesSection />)
    flush()
    expect(screen.queryByTestId('browser-sites-section')).toBeNull()
    expect(calls).toHaveLength(0)
  })

  it('lists the caller’s own choices and forgets one through the gateway', async () => {
    vi.stubEnv('VITE_BROWSER_SITES', 'true')
    const calls = stubGateway((call) =>
      call.method === 'DELETE' ? new Response(null, { status: 204 }) : undefined,
    )
    signIn('member')
    render(() => <BrowserSitesSection />)
    await waitFor(() => expect(screen.getByText('shop.example.com')).toBeTruthy())
    click(screen.getByRole('button', { name: /Glem/ }))
    await waitFor(() =>
      expect(
        calls.some(
          (call) =>
            call.method === 'DELETE' &&
            call.url.endsWith('/api/v1/orgs/org_1/browser-sites/me/shop.example.com'),
        ),
      ).toBe(true),
    )
  })

  it('sets a new choice against the revision it read, and sends the host only in the path', async () => {
    vi.stubEnv('VITE_BROWSER_SITES', 'true')
    const calls = stubGateway((call) =>
      call.method === 'PUT'
        ? jsonResponse({ host: 'news.example.org', level: 'act', expires_at: '', updated_at: '' })
        : undefined,
    )
    signIn('member')
    render(() => <BrowserSitesSection />)
    await waitFor(() => expect(screen.getByText('shop.example.com')).toBeTruthy())
    type(screen.getByLabelText('Legg til nettside'), 'News.example.org')
    change(screen.getByLabelText('Valg'), 'act')
    click(screen.getByRole('button', { name: /Lagre valg/ }))
    await waitFor(() => expect(calls.some((call) => call.method === 'PUT')).toBe(true))
    const put = calls.find((call) => call.method === 'PUT')
    expect(put?.url.endsWith('/api/v1/orgs/org_1/browser-sites/me/news.example.org')).toBe(true)
    expect(put?.body).toEqual({ expected_revision: 3, level: 'act' })
  })

  it('explains why a site is not open, in plain words', async () => {
    vi.stubEnv('VITE_BROWSER_SITES', 'true')
    stubGateway((call) =>
      call.url.includes('/explain')
        ? jsonResponse({
            organization_id: 'org_1',
            user_id: 'user_1',
            revision: 3,
            mode: 'open',
            decision: { host: 'bank.example', effective: 'deny', cap: 'none', source: 'org_block' },
            org_rule: 'block',
            user_level: 'none',
            user_expires_at: null,
          })
        : undefined,
    )
    signIn('member')
    render(() => <BrowserSitesSection />)
    await waitFor(() => expect(screen.getByText('shop.example.com')).toBeTruthy())
    type(screen.getByLabelText('Hvorfor åpnes eller åpnes ikke en side?'), 'bank.example')
    click(screen.getByRole('button', { name: /Forklar/ }))
    await waitFor(() =>
      expect(screen.getByTestId('browser-sites-explanation').textContent).toContain(
        'Nettstedet er ikke tillatt',
      ),
    )
    expect(screen.getByTestId('browser-sites-explanation').textContent).toContain(
      'Blokkert av administrator',
    )
  })
})
