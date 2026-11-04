// @vitest-environment jsdom

/**
 * The approval card offers a standing scope only where the server's offer lists it (build plan WS13
 * S6). A standing scope is its own explicit control and is never pre-selected: the plain approval
 * stays the one decision, and it carries no scope at all.
 */

import { cleanup, fireEvent, render, screen } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { Approval, ApprovalDecision, ApprovalScope } from '@/shared/api/orchestration-client'
import { I18nProvider } from '@/shared/i18n'
import { localeStorageKey } from '@/shared/i18n/locales'
import { ApprovalRequests } from './ChatMessages'

type DecideSpy = (approvalId: string, decision: ApprovalDecision, scope?: ApprovalScope) => void

const READ: Approval = {
  id: 'approval-read',
  kind: 'APPROVAL_KIND_DESTRUCTIVE',
  cause: {
    code: 'consent_to_disclose',
    tool: 'knowledge_search',
    scopeOptions: ['once', 'thread', 'run'],
    stepUpRequired: false,
  },
}

const PROVIDER_WRITE: Approval = {
  id: 'approval-write',
  kind: 'APPROVAL_KIND_DESTRUCTIVE',
  cause: {
    code: 'provider_write',
    tool: 'execute_provider_action',
    scopeOptions: ['once'],
    stepUpRequired: false,
  },
}

function setLocale(locale: 'no' | 'en') {
  window.localStorage.setItem(localeStorageKey, locale)
}

function show(approvals: Approval[], onDecide: DecideSpy) {
  render(() => (
    <I18nProvider>
      <ApprovalRequests approvals={approvals} onDecide={onDecide} />
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

describe('the approval card standing scopes', () => {
  it('offers a standing scope only where the server offers it', () => {
    setLocale('en')
    show([READ, PROVIDER_WRITE], vi.fn())

    expect(screen.getAllByRole('button', { name: 'Approve for this thread' })).toHaveLength(1)
    expect(screen.getAllByRole('button', { name: 'Approve for this run' })).toHaveLength(1)
    expect(screen.getAllByRole('button', { name: 'Approve' })).toHaveLength(2)
  })

  it('a standing scope is an explicit click and carries its scope', () => {
    setLocale('en')
    const onDecide = vi.fn()
    show([READ], onDecide)

    fireEvent.click(screen.getByRole('button', { name: 'Approve for this thread' }))
    expect(onDecide).toHaveBeenCalledTimes(1)
    expect(onDecide).toHaveBeenCalledWith('approval-read', 'approve', 'thread')
  })

  it('the run scope is offered in the same way', () => {
    setLocale('en')
    const onDecide = vi.fn()
    show([READ], onDecide)

    fireEvent.click(screen.getByRole('button', { name: 'Approve for this run' }))
    expect(onDecide).toHaveBeenCalledWith('approval-read', 'approve', 'run')
  })

  it('the one decision stays the default and carries no scope', () => {
    setLocale('en')
    const onDecide = vi.fn()
    show([READ], onDecide)

    fireEvent.click(screen.getByRole('button', { name: 'Approve' }))
    expect(onDecide).toHaveBeenCalledWith('approval-read', 'approve')
  })

  it('the Norwegian labels name the same two scopes', () => {
    setLocale('no')
    const onDecide = vi.fn()
    show([READ], onDecide)

    expect(screen.getByRole('button', { name: 'Godkjenn for denne tråden' })).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Godkjenn for denne kjøringen' }))
    expect(onDecide).toHaveBeenCalledWith('approval-read', 'approve', 'run')
  })
})
