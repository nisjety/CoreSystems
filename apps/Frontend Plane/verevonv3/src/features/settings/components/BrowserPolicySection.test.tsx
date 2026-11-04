// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  BrowserPolicyRevisionConflict,
  BrowserPolicySection,
  buildBrowserPolicyDocument,
  canonicalBrowserHost,
  canonicalBrowserHostList,
  type BrowserPolicyDocument,
  type BrowserPolicyLoad,
  type BrowserPolicyPort,
  type BrowserPolicyProposal,
  type BrowserPolicyProposalResult,
  type BrowserPolicyView,
} from '@/features/settings/components/BrowserPolicySection'
import { clearSession, markSessionOnboardingComplete, setSessionUser } from '@/shared/session/session-store'

// Settings > Browser (E3.2-g2). The org policy API is a fake port here: the section only draws the
// document it is given and sends the change it is asked to, so each test can say what was sent.

const ALL_BASELINE: Record<string, 'allowed' | 'disabled'> = {
  'browser.act.input': 'allowed',
  'browser.act.interact': 'allowed',
  'browser.act.upload': 'allowed',
  'browser.act.download': 'allowed',
  'browser.profile.persistent': 'allowed',
  'search.paid_provider': 'allowed',
}

function storedView(overrides: Partial<BrowserPolicyView> = {}): BrowserPolicyView {
  return {
    revision: 3,
    canManage: true,
    document: {
      caps: { 'browser.act.input': 'disabled', 'browser.recording': 'disabled' },
      robots_max: 'strict',
      proxy_max: 'none',
      domains: { allow: ['example.com'], deny: ['ads.example.net'] },
    },
    effective: {},
    entitlements: { 'web.robots_override': false, 'web.proxy_pool': false, 'web.paid_search': true },
    originConsents: [],
    ...overrides,
  }
}

function loaded(view: BrowserPolicyView): BrowserPolicyLoad {
  return { available: true, ...view }
}

function fakePort(
  load: () => Promise<BrowserPolicyLoad>,
  propose: (orgId: string, proposal: BrowserPolicyProposal) => Promise<BrowserPolicyProposalResult> = async () => ({
    status: 'waiting_approval',
    runId: 'run_1',
  }),
) {
  const loadCalls = vi.fn(load)
  const proposeCalls = vi.fn(propose)
  const port: BrowserPolicyPort = { load: loadCalls, propose: proposeCalls }
  return { port, loadCalls, proposeCalls }
}

function signIn(role: 'owner' | 'admin' | 'member') {
  setSessionUser({ id: 'user_1', email: 'user@example.com', name: 'User', emailVerified: true })
  flush()
  markSessionOnboardingComplete({ id: 'org_1', name: 'Acme AS', role })
  flush()
}

// Solid batches writes until a flush, so each interaction settles before the next.
const click = (element: HTMLElement) => {
  fireEvent.click(element)
  flush()
}

const typeInto = (element: HTMLElement, value: string) => {
  fireEvent.input(element, { target: { value } })
  flush()
}

const textarea = (id: string) => document.getElementById(id) as HTMLTextAreaElement

const saveButton = () => screen.getByRole('button', { name: 'Send til godkjenning' }) as HTMLButtonElement

function isDisabled(element: HTMLElement): boolean {
  return (
    element.hasAttribute('disabled') ||
    element.getAttribute('aria-disabled') === 'true' ||
    element.getAttribute('data-disabled') !== null
  )
}

async function settle() {
  await new Promise((resolve) => setTimeout(resolve, 0))
  flush()
}

afterEach(() => {
  cleanup()
  clearSession()
})

describe('canonicalBrowserHost', () => {
  it('lower-cases and trims a host name', () => {
    expect(canonicalBrowserHost('  Example.COM ')).toEqual({ ok: true, host: 'example.com' })
  })

  it('names the reason a value is not a host name', () => {
    const cases: Array<[string, string]> = [
      ['https://example.com', 'host_only'],
      ['example.com:8443', 'host_only'],
      ['*.example.com', 'wildcard'],
      ['192.168.0.1', 'ip_literal'],
      ['[::1]', 'ip_literal'],
      ['example..com', 'invalid'],
      ['-bad.example.com', 'invalid'],
      ['example.com.', 'invalid'],
      ['exa_mple.com', 'invalid'],
    ]
    for (const [input, problem] of cases) {
      expect(canonicalBrowserHost(input)).toEqual({ ok: false, problem })
    }
  })
})

describe('canonicalBrowserHostList', () => {
  it('sorts and de-duplicates the hosts, and reports the tokens that are not hosts', () => {
    expect(canonicalBrowserHostList('b.example.org\nExample.com, example.com\n')).toEqual({
      hosts: ['b.example.org', 'example.com'],
      problems: [],
    })
    expect(canonicalBrowserHostList('ok.example.com https://x.example.com')).toEqual({
      hosts: ['ok.example.com'],
      problems: [{ token: 'https://x.example.com', problem: 'host_only' }],
    })
  })
})

describe('buildBrowserPolicyDocument', () => {
  it('keeps the ceilings and the keys it does not manage, and drops a switch that is back at its baseline', () => {
    const stored: BrowserPolicyDocument = {
      caps: { 'browser.act.input': 'disabled', 'browser.recording': 'disabled' },
      robots_max: 'strict',
      proxy_max: 'none',
      domains: { allow: ['example.com'], deny: [] },
    }
    expect(buildBrowserPolicyDocument(stored, { caps: ALL_BASELINE, allow: ['example.com'], deny: [] })).toEqual({
      caps: { 'browser.recording': 'disabled' },
      robots_max: 'strict',
      proxy_max: 'none',
      domains: { allow: ['example.com'], deny: [] },
    })
  })

  it('omits empty containers, as org-core normalises them', () => {
    expect(buildBrowserPolicyDocument(null, { caps: ALL_BASELINE, allow: [], deny: [] })).toEqual({})
  })
})

describe('BrowserPolicySection', () => {
  it('stays hidden when the org policy API does not report itself available', async () => {
    signIn('admin')
    const { port, loadCalls, proposeCalls } = fakePort(async (): Promise<BrowserPolicyLoad> => ({ available: false }))
    render(() => <BrowserPolicySection port={port} />)
    await waitFor(() => expect(loadCalls).toHaveBeenCalled())
    await settle()
    expect(screen.queryByText('Nettleser')).toBeNull()
    expect(proposeCalls).not.toHaveBeenCalled()
  })

  it('stays hidden when the load fails', async () => {
    signIn('admin')
    const { port, loadCalls } = fakePort(async (): Promise<BrowserPolicyLoad> => {
      throw new Error('gateway unavailable')
    })
    render(() => <BrowserPolicySection port={port} />)
    await waitFor(() => expect(loadCalls).toHaveBeenCalled())
    await settle()
    expect(screen.queryByText('Nettleser')).toBeNull()
  })

  it('is hidden by default until the action contract is wired', async () => {
    signIn('admin')
    render(() => <BrowserPolicySection />)
    await settle()
    expect(screen.queryByText('Nettleser')).toBeNull()
  })

  it('shows the stored switches to an admin and saves against the stored revision', async () => {
    signIn('admin')
    const { port, proposeCalls } = fakePort(async () => loaded(storedView()))
    render(() => <BrowserPolicySection port={port} />)
    const input = await screen.findByRole('switch', { name: 'Skrive i nettsider' })
    expect(input.getAttribute('aria-checked')).toBe('false')
    expect(screen.queryByRole('button', { name: 'Send til godkjenning' })).toBeNull()

    click(input)
    click(saveButton())

    await waitFor(() => expect(proposeCalls).toHaveBeenCalledTimes(1))
    expect(proposeCalls.mock.calls[0]?.[1]).toEqual({
      ifMatch: 3,
      document: {
        caps: { 'browser.recording': 'disabled' },
        robots_max: 'strict',
        proxy_max: 'none',
        domains: { allow: ['example.com'], deny: ['ads.example.net'] },
      },
    })
  })

  it('refuses a pasted URL in a host list and keeps the save disabled', async () => {
    signIn('admin')
    const { port, proposeCalls } = fakePort(async () => loaded(storedView()))
    render(() => <BrowserPolicySection port={port} />)
    await screen.findByRole('switch', { name: 'Skrive i nettsider' })

    typeInto(textarea('browser-policy-allow'), 'https://example.com/path')

    expect(screen.getByText(/Skriv bare domenenavnet/)).toBeTruthy()
    expect(saveButton().disabled).toBe(true)
    click(saveButton())
    expect(proposeCalls).not.toHaveBeenCalled()
  })

  it('canonicalises the host lists before it sends them', async () => {
    signIn('admin')
    const { port, proposeCalls } = fakePort(async () => loaded(storedView()))
    render(() => <BrowserPolicySection port={port} />)
    await screen.findByRole('switch', { name: 'Skrive i nettsider' })

    typeInto(textarea('browser-policy-allow'), '  Example.COM \nexample.com\nb.example.org')
    click(saveButton())

    await waitFor(() => expect(proposeCalls).toHaveBeenCalledTimes(1))
    expect(proposeCalls.mock.calls[0]?.[1]?.document.domains).toEqual({
      allow: ['b.example.org', 'example.com'],
      deny: ['ads.example.net'],
    })
  })

  it('says a change is sent for approval and not applied', async () => {
    signIn('admin')
    const { port } = fakePort(async () => loaded(storedView()))
    render(() => <BrowserPolicySection port={port} />)
    click(await screen.findByRole('switch', { name: 'Skrive i nettsider' }))

    click(saveButton())

    expect(await screen.findByText('Sendt til godkjenning. Ingenting endres før endringen er godkjent.')).toBeTruthy()
  })

  it('reloads on a revision conflict and keeps the draft on screen', async () => {
    signIn('admin')
    const { port, loadCalls } = fakePort(
      async () => loaded(storedView()),
      async () => {
        throw new BrowserPolicyRevisionConflict()
      },
    )
    render(() => <BrowserPolicySection port={port} />)
    click(await screen.findByRole('switch', { name: 'Skrive i nettsider' }))

    click(saveButton())

    expect(await screen.findByText(/Innstillingene ble endret av noen andre/)).toBeTruthy()
    await waitFor(() => expect(loadCalls).toHaveBeenCalledTimes(2))
    expect(screen.getByRole('switch', { name: 'Skrive i nettsider' }).getAttribute('aria-checked')).toBe('true')
  })

  it('shows a member the effective view with no way to change it', async () => {
    signIn('member')
    const { port } = fakePort(async () =>
      loaded(
        storedView({
          canManage: false,
          document: null,
          effective: { caps: { 'browser.act.input': 'disabled' }, domains: { allow: ['example.com'] } },
        }),
      ),
    )
    render(() => <BrowserPolicySection port={port} />)

    expect(await screen.findByText('Bare administratorer kan endre nettleserinnstillingene.')).toBeTruthy()
    expect(screen.queryByRole('button', { name: 'Send til godkjenning' })).toBeNull()
    const switches = screen.getAllByRole('switch')
    expect(switches.length).toBeGreaterThan(0)
    expect(switches.every(isDisabled)).toBe(true)
    expect(textarea('browser-policy-allow').disabled).toBe(true)
  })

  it('keeps paid search off when the plan does not include it', async () => {
    signIn('admin')
    const { port } = fakePort(async () =>
      loaded(
        storedView({
          document: { caps: { 'search.paid_provider': 'disabled' } },
          entitlements: { 'web.paid_search': false },
        }),
      ),
    )
    render(() => <BrowserPolicySection port={port} />)

    const paid = await screen.findByRole('switch', { name: 'Betalt søk' })
    expect(isDisabled(paid)).toBe(true)
    expect(screen.getByLabelText(/Ikke med i denne planen/)).toBeTruthy()
  })

  it('lists per-origin consent read-only, with an empty state when there is none', async () => {
    signIn('admin')
    const { port } = fakePort(async () => loaded(storedView()))
    render(() => <BrowserPolicySection port={port} />)
    expect(await screen.findByTestId('browser-policy-consents-empty')).toBeTruthy()
  })

  it('renders per-origin consent rows with no controls', async () => {
    signIn('admin')
    const { port } = fakePort(async () =>
      loaded(
        storedView({
          originConsents: [
            {
              startHost: 'shop.example.com',
              hosts: ['cdn.example.com'],
              decision: 'allow',
              scope: 'conversation',
              decidedAt: '2026-11-02T10:00:00Z',
            },
          ],
        }),
      ),
    )
    render(() => <BrowserPolicySection port={port} />)
    const list = await screen.findByTestId('browser-policy-consents')
    expect(list.textContent).toContain('shop.example.com')
    expect(list.textContent).toContain('cdn.example.com')
    expect(list.querySelectorAll('input, textarea, button')).toHaveLength(0)
  })
})
