// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  ApprovalRequests,
  BrowserSiteConsentCard,
  browserSiteConsentChoices,
  browserSiteConsentLabel,
} from '@/features/chat/components/ChatMessages'
import type { ApprovalCause } from '@/shared/approvals/approval-copy'
import type { Approval } from '@/shared/api/orchestration-client'
import { I18nProvider } from '@/shared/i18n'
import { localeStorageKey } from '@/shared/i18n/locales'
import { clearSession, markSessionOnboardingComplete, setSessionUser } from '@/shared/session/session-store'

afterEach(() => {
  cleanup()
  clearSession()
  vi.unstubAllGlobals()
  vi.unstubAllEnvs()
  try {
    window.localStorage.clear()
  } catch {
    // ignore
  }
})

describe('browserSiteConsentChoices', () => {
  it('offers only what the admin ceiling allows for the host', () => {
    expect(browserSiteConsentChoices('read', false)).toEqual(['allow_this_run', 'remember_read', 'deny'])
    expect(browserSiteConsentChoices('act', false)).toEqual([
      'allow_this_run',
      'remember_read',
      'remember_act',
      'deny',
    ])
  })

  it('never offers a remembered choice under zero data retention', () => {
    expect(browserSiteConsentChoices('act', true)).toEqual([])
    expect(browserSiteConsentChoices('read', true)).toEqual([])
  })
})

describe('browserSiteConsentLabel', () => {
  it('words every choice in Norwegian and in English', () => {
    const nb = (noText: string) => noText
    const en = (_noText: string, enText: string) => enText
    expect(browserSiteConsentLabel('remember_act', { tr: nb })).toBe('Husk: lesing og handling')
    expect(browserSiteConsentLabel('remember_act', { tr: en })).toBe('Remember: reading and acting')
    expect(browserSiteConsentLabel('deny', { tr: en })).toBe('Do not allow')
  })
})

describe('BrowserSiteConsentCard', () => {
  it('names the host and reports the choice the member makes', () => {
    const onDecide = vi.fn()
    render(() => (
      <BrowserSiteConsentCard host="shop.example.com" maxLevel="read" zdr={false} onDecide={onDecide} />
    ))
    expect(screen.getByText(/shop\.example\.com/)).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Husk: bare lesing' }))
    flush()
    expect(onDecide).toHaveBeenCalledWith('remember_read')
  })

  it('does not offer a remembered act when the ceiling is read', () => {
    render(() => (
      <BrowserSiteConsentCard host="shop.example.com" maxLevel="read" zdr={false} onDecide={vi.fn()} />
    ))
    expect(screen.queryByRole('button', { name: 'Husk: lesing og handling' })).toBeNull()
  })

  it('renders nothing under zero data retention', () => {
    render(() => (
      <BrowserSiteConsentCard host="shop.example.com" maxLevel="act" zdr={true} onDecide={vi.fn()} />
    ))
    expect(screen.queryByRole('group', { name: 'Nettside for agenten' })).toBeNull()
  })
})

/** A site-consent pause as the approval record carries it: the host and the level the site policy allows. */
function sitePause(cause: Partial<ApprovalCause> = {}): Approval {
  return {
    id: 'approval-consent-1',
    kind: 'APPROVAL_KIND_TOOL_CALL',
    cause: {
      code: 'browser_site_consent',
      scopeOptions: ['once'],
      stepUpRequired: false,
      host: 'shop.example.com',
      maxLevel: 'read',
      ...cause,
    },
  }
}

function setLocale(locale: 'no' | 'en') {
  window.localStorage.setItem(localeStorageKey, locale)
}

/** Renders the approvals the way the transcript and the composer tray do, and returns the decision spy. */
function showApprovals(approvals: Approval[]) {
  const onDecide = vi.fn()
  render(() => (
    <I18nProvider>
      <ApprovalRequests approvals={approvals} onDecide={onDecide} />
    </I18nProvider>
  ))
  return onDecide
}

/** Signs a member into organisation `org_1`, the organisation a remembered choice is saved under. */
function signIn() {
  setSessionUser({ id: 'user_1', email: 'user@example.com', name: 'User', emailVerified: true })
  flush()
  markSessionOnboardingComplete({ id: 'org_1', name: 'Acme AS', role: 'member' })
  flush()
}

function jsonResponse(data: unknown, status = 200): Response {
  return new Response(JSON.stringify(data), {
    headers: { 'Content-Type': 'application/json' },
    status,
  })
}

type Call = { url: string; method: string; body: unknown }

/** Records each request and answers it with `answer`, or with the member's choices when `answer` says nothing. */
function stubGateway(answer: (call: Call) => Response | undefined): Call[] {
  const calls: Call[] = []
  vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const call: Call = {
      url: String(input),
      method: init?.method ?? 'GET',
      body: init?.body ? JSON.parse(String(init.body)) : undefined,
    }
    calls.push(call)
    return answer(call) ?? jsonResponse({ organization_id: 'org_1', user_id: 'user_1', revision: 3, prefs: [] })
  }))
  return calls
}

describe('the consent card a site-consent pause mounts (WS21-S6)', () => {
  it('mounts the card from the pause: its host, the approval words, and the choices its level allows', () => {
    vi.stubEnv('VITE_BROWSER_SITES', 'true')
    setLocale('en')
    showApprovals([sitePause()])

    expect(screen.getByText('The agent wants to open a site')).toBeTruthy()
    expect(
      screen.getByText('The site policy asks you before the agent opens shop.example.com. At most, it may read the site.'),
    ).toBeTruthy()
    expect(screen.getByRole('group', { name: 'Site for the agent' })).toBeTruthy()
    expect(screen.getByText('The agent wants to open shop.example.com. Allow it?')).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Allow for this run' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Remember: reading only' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Do not allow' })).toBeTruthy()
    expect(screen.queryByRole('button', { name: 'Remember: reading and acting' })).toBeNull()
    expect(screen.queryByRole('button', { name: 'Approve' })).toBeNull()
  })

  it('offers the acting choice only where the site policy allows acting', () => {
    vi.stubEnv('VITE_BROWSER_SITES', 'true')
    setLocale('en')
    showApprovals([sitePause({ maxLevel: 'act' })])
    expect(screen.getByRole('button', { name: 'Remember: reading and acting' })).toBeTruthy()
  })

  it('Allow for this run approves the pause once and saves nothing', () => {
    vi.stubEnv('VITE_BROWSER_SITES', 'true')
    setLocale('en')
    const calls = stubGateway(() => undefined)
    const onDecide = showApprovals([sitePause()])

    fireEvent.click(screen.getByRole('button', { name: 'Allow for this run' }))
    flush()

    expect(onDecide).toHaveBeenCalledWith('approval-consent-1', 'approve')
    expect(calls).toHaveLength(0)
  })

  it('Do not allow rejects the pause and saves nothing', () => {
    vi.stubEnv('VITE_BROWSER_SITES', 'true')
    setLocale('en')
    const calls = stubGateway(() => undefined)
    const onDecide = showApprovals([sitePause()])

    fireEvent.click(screen.getByRole('button', { name: 'Do not allow' }))
    flush()

    expect(onDecide).toHaveBeenCalledWith('approval-consent-1', 'reject')
    expect(calls).toHaveLength(0)
  })

  it('Remember reads the revision, saves the choice for the pause host, and only then approves', async () => {
    vi.stubEnv('VITE_BROWSER_SITES', 'true')
    setLocale('en')
    signIn()
    const calls = stubGateway((call) =>
      call.method === 'PUT'
        ? jsonResponse({ host: 'shop.example.com', level: 'read', expires_at: '', updated_at: '' })
        : undefined,
    )
    const onDecide = showApprovals([sitePause()])

    fireEvent.click(screen.getByRole('button', { name: 'Remember: reading only' }))
    flush()

    await waitFor(() => expect(onDecide).toHaveBeenCalledWith('approval-consent-1', 'approve'))
    expect(calls.map((call) => call.method)).toEqual(['GET', 'PUT'])
    expect(calls[0]?.url.endsWith('/api/v1/orgs/org_1/browser-sites/me')).toBe(true)
    expect(calls[1]?.url.endsWith('/api/v1/orgs/org_1/browser-sites/me/shop.example.com')).toBe(true)
    expect(calls[1]?.body).toEqual({ expected_revision: 3, level: 'read' })
  })

  it('Remember reading and acting saves the act level for the pause host', async () => {
    vi.stubEnv('VITE_BROWSER_SITES', 'true')
    setLocale('en')
    signIn()
    const calls = stubGateway((call) =>
      call.method === 'PUT'
        ? jsonResponse({ host: 'shop.example.com', level: 'act', expires_at: '', updated_at: '' })
        : undefined,
    )
    const onDecide = showApprovals([sitePause({ maxLevel: 'act' })])

    fireEvent.click(screen.getByRole('button', { name: 'Remember: reading and acting' }))
    flush()

    await waitFor(() => expect(onDecide).toHaveBeenCalledWith('approval-consent-1', 'approve'))
    expect(calls.find((call) => call.method === 'PUT')?.body).toEqual({ expected_revision: 3, level: 'act' })
  })

  it('a refused save keeps the pause pending, approves nothing, and says why', async () => {
    vi.stubEnv('VITE_BROWSER_SITES', 'true')
    setLocale('en')
    signIn()
    stubGateway((call) =>
      call.method === 'PUT'
        ? jsonResponse({ error: { code: 'revision_conflict', message: 'the policy changed' } }, 409)
        : undefined,
    )
    const onDecide = showApprovals([sitePause()])

    fireEvent.click(screen.getByRole('button', { name: 'Remember: reading only' }))
    flush()

    await waitFor(() => expect(screen.getByRole('alert')).toBeTruthy())
    expect(onDecide).not.toHaveBeenCalled()
    expect(screen.getByRole('button', { name: 'Allow for this run' })).toBeTruthy()
  })

  it('with no active organisation nothing is saved and the pause stays pending', async () => {
    vi.stubEnv('VITE_BROWSER_SITES', 'true')
    setLocale('en')
    const calls = stubGateway(() => undefined)
    const onDecide = showApprovals([sitePause()])

    fireEvent.click(screen.getByRole('button', { name: 'Remember: reading only' }))
    flush()

    await waitFor(() => expect(screen.getByRole('alert')).toBeTruthy())
    expect(calls).toHaveLength(0)
    expect(onDecide).not.toHaveBeenCalled()
  })

  it('with the switch off every pause keeps its generic card, and mounts and reads nothing', () => {
    setLocale('en')
    for (const value of ['', 'false', 'TRUE', '1']) {
      vi.stubEnv('VITE_BROWSER_SITES', value)
      const calls = stubGateway(() => undefined)
      showApprovals([sitePause()])
      expect(screen.queryByRole('group', { name: 'Site for the agent' }), value).toBeNull()
      expect(screen.getByRole('button', { name: 'Approve' }), value).toBeTruthy()
      expect(calls, value).toHaveLength(0)
      cleanup()
    }
  })

  it('a pause that names no level, or needs a password, keeps the generic card with the switch on', () => {
    vi.stubEnv('VITE_BROWSER_SITES', 'true')
    setLocale('en')
    showApprovals([sitePause({ maxLevel: undefined })])
    expect(screen.queryByRole('group', { name: 'Site for the agent' })).toBeNull()
    expect(screen.getByRole('button', { name: 'Approve' })).toBeTruthy()
    cleanup()

    showApprovals([sitePause({ stepUpRequired: true })])
    expect(screen.queryByRole('group', { name: 'Site for the agent' })).toBeNull()
    expect(screen.getByRole('button', { name: 'Approve' })).toBeTruthy()
  })

  it('leaves a help request on its own card with the switch on', () => {
    vi.stubEnv('VITE_BROWSER_SITES', 'true')
    setLocale('en')
    showApprovals([
      {
        id: 'approval-help-2',
        kind: 'APPROVAL_KIND_TOOL_CALL',
        cause: {
          code: 'browser_human_assist',
          scopeOptions: ['once'],
          stepUpRequired: false,
          assistKind: 'login',
          assistOrigin: 'agent',
          host: 'shop.example.com',
        },
      },
    ])
    expect(screen.getByRole('button', { name: 'Skip this site' })).toBeTruthy()
    expect(screen.queryByRole('group', { name: 'Site for the agent' })).toBeNull()
  })
})
