// @vitest-environment jsdom

/**
 * EP-1 S2: the card a help request shows. The agent met a sign-in, CAPTCHA, consent or firewall wall
 * and asks the person to clear it. The card names the wall and the host from the approval record,
 * offers Take over (the live panel's control call, with the epoch the projection shows) only when the
 * turn's browser can be taken, and always offers Skip this site, which declines the request.
 */

import { cleanup, fireEvent, render, screen } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { Approval } from '@/shared/api/orchestration-client'
import type { ChatRunBrowserControl } from '@/features/chat/lib/chat-run-watch'
import { I18nProvider } from '@/shared/i18n'
import { localeStorageKey } from '@/shared/i18n/locales'
import { ApprovalRequests } from './ChatMessages'

function setLocale(locale: 'no' | 'en') {
  window.localStorage.setItem(localeStorageKey, locale)
}

function helpRequest(assistKind: string, host = 'example.com'): Approval {
  return {
    id: 'approval-help-1',
    kind: 'APPROVAL_KIND_TOOL_CALL',
    cause: {
      code: 'browser_human_assist',
      tool: 'browser.observe',
      scopeOptions: ['once'],
      stepUpRequired: false,
      assistKind,
      assistOrigin: 'agent',
      host,
    },
  }
}

const bound: ChatRunBrowserControl = { sessionId: 'qrun_1', controlMode: 'agent_control', controlEpoch: 3 }

function show(
  approvals: Approval[],
  options: {
    onDecide?: (approvalId: string, decision: string) => void
    control?: ChatRunBrowserControl
    onTakeOver?: (sessionId: string, expectedEpoch: number | null) => void
  } = {},
) {
  render(() => (
    <I18nProvider>
      <ApprovalRequests
        approvals={approvals}
        onDecide={options.onDecide ?? (() => {})}
        browserControl={options.control}
        onBrowserTakeOver={options.onTakeOver}
      />
    </I18nProvider>
  ))
}

afterEach(() => {
  cleanup()
  try {
    window.localStorage.clear()
  } catch {
    // ignore
  }
})

describe('the help request card', () => {
  it('renders Take over and Skip this site with the wall and the host from the record', () => {
    setLocale('en')
    show([helpRequest('login')], { control: bound, onTakeOver: vi.fn() })

    expect(screen.getByText('The site asks you to sign in')).toBeTruthy()
    expect(screen.getByText(/sign in on example\.com/)).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Take over' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Skip this site' })).toBeTruthy()
    // A help request is not an ordinary approval: it offers no Approve or Reject.
    expect(screen.queryByRole('button', { name: 'Approve' })).toBeNull()
    expect(screen.queryByRole('button', { name: 'Reject' })).toBeNull()
  })

  it('Take over calls the control action with the projection epoch', () => {
    setLocale('en')
    const onTakeOver = vi.fn()
    const onDecide = vi.fn()
    show([helpRequest('captcha')], { control: bound, onTakeOver, onDecide })

    fireEvent.click(screen.getByRole('button', { name: 'Take over' }))
    flush()

    expect(onTakeOver).toHaveBeenCalledWith('qrun_1', 3)
    expect(onDecide).not.toHaveBeenCalled()
  })

  it('Skip this site declines the request', () => {
    setLocale('en')
    const onDecide = vi.fn()
    show([helpRequest('consent')], { control: bound, onTakeOver: vi.fn(), onDecide })

    fireEvent.click(screen.getByRole('button', { name: 'Skip this site' }))
    flush()

    expect(onDecide).toHaveBeenCalledWith('approval-help-1', 'reject')
  })

  it('offers only Skip this site while the browser cannot be taken, and says so', () => {
    setLocale('en')
    show([helpRequest('waf')], { control: bound })

    expect(screen.queryByRole('button', { name: 'Take over' })).toBeNull()
    expect(screen.getByRole('button', { name: 'Skip this site' })).toBeTruthy()
    expect(screen.getByText('You cannot take over this browser yet. Skip this site and the agent uses another source.')).toBeTruthy()
  })

  it('offers no Take over without a bound browser, even with the control action', () => {
    setLocale('en')
    show([helpRequest('login')], { onTakeOver: vi.fn() })

    expect(screen.queryByRole('button', { name: 'Take over' })).toBeNull()
  })

  it('words the card in Norwegian', () => {
    setLocale('no')
    show([helpRequest('login')], { control: bound, onTakeOver: vi.fn() })

    expect(screen.getByText('Nettstedet ber deg logge inn')).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Ta over' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Hopp over dette nettstedet' })).toBeTruthy()
  })

  it('keeps the takeover pause on the ordinary approval card', () => {
    setLocale('en')
    show([
      {
        id: 'approval-takeover-1',
        kind: 'APPROVAL_KIND_TOOL_CALL',
        cause: {
          code: 'browser_human_assist',
          scopeOptions: ['once'],
          stepUpRequired: false,
          assistKind: 'user_takeover',
          assistOrigin: 'person',
        },
      },
    ], { control: bound, onTakeOver: vi.fn() })

    expect(screen.getByText('The browser is with you')).toBeTruthy()
    expect(screen.queryByRole('button', { name: 'Skip this site' })).toBeNull()
  })
})
