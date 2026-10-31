// @vitest-environment jsdom

/**
 * The approval card says why the pause was raised, in the reader's language.
 *
 * It used to headline the raw `APPROVAL_KIND_*` token the wire carries. Now the headline and the
 * sentence under it come from the cause code the server states (`shared/approvals/approval-copy`),
 * an approval with no cause says that something waits and nothing more, and the token never
 * reaches the page.
 */

import { cleanup, render, screen } from '@solidjs/testing-library'
import { afterEach, describe, expect, it } from 'vitest'
import type { Approval } from '@/shared/api/orchestration-client'
import { I18nProvider } from '@/shared/i18n'
import { localeStorageKey } from '@/shared/i18n/locales'
import { ApprovalRequests } from './ChatMessages'

function setLocale(locale: 'no' | 'en') {
  window.localStorage.setItem(localeStorageKey, locale)
}

function show(approvals: Approval[]) {
  render(() => (
    <I18nProvider>
      <ApprovalRequests approvals={approvals} onDecide={() => {}} />
    </I18nProvider>
  ))
}

const PROVIDER_WRITE: Approval = {
  id: 'approval-1',
  kind: 'APPROVAL_KIND_DESTRUCTIVE',
  cause: { code: 'provider_write', tool: 'execute_provider_action', scopeOptions: ['once'], stepUpRequired: false },
}

afterEach(() => {
  cleanup()
  try {
    window.localStorage.clear()
  } catch {
    // ignore
  }
})

describe('the approval card', () => {
  it('headlines the cause and gives its reason in English', () => {
    setLocale('en')
    show([PROVIDER_WRITE])

    expect(screen.getByText('A write to a connected service')).toBeTruthy()
    expect(screen.getByText('The agent wants to change something in a connected service. It may be hard to undo.')).toBeTruthy()
    expect(document.body.textContent).not.toContain('APPROVAL_KIND')
  })

  it('headlines the cause and gives its reason in Norwegian', () => {
    setLocale('no')
    show([PROVIDER_WRITE])

    expect(screen.getByText('Skriving til en tilkoblet tjeneste')).toBeTruthy()
    expect(screen.getByText(/^Agenten vil endre noe hos en tilkoblet tjeneste\./)).toBeTruthy()
    expect(document.body.textContent).not.toContain('APPROVAL_KIND')
  })

  it('names the risk of a browser gate', () => {
    setLocale('en')
    show([
      {
        id: 'approval-2',
        kind: 'APPROVAL_KIND_TOOL_CALL',
        cause: { code: 'browser_action_risk', riskCategory: 'login', scopeOptions: ['once'], stepUpRequired: false },
      },
    ])

    expect(screen.getByText('Signing in')).toBeTruthy()
  })

  it('says that something waits, and never the kind, when the approval states no cause', () => {
    setLocale('en')
    show([{ id: 'approval-3', kind: 'APPROVAL_KIND_TOOL_CALL' }])

    expect(screen.getByText('The agent is waiting for approval before the next step')).toBeTruthy()
    expect(document.body.textContent).not.toContain('APPROVAL_KIND')
    expect(document.querySelector('.verevon-chat-approval__detail')).toBeNull()
  })

  it('shows a detail the server wrote when there is no cause', () => {
    setLocale('en')
    show([{ id: 'approval-4', detail: 'Delete 3 files' }])

    expect(screen.getByText('Delete 3 files')).toBeTruthy()
  })
})
