// @vitest-environment jsdom

/**
 * W2-WS13-S7-spa-decide: "Always allow this tool". The control appears only where the server offers it
 * (rule_offered === true AND a thread scope in scope_options), and a press decides with ('thread', promote).
 * It rides the password step-up exactly as the thread scope does. The SPA never offers what the server did not.
 */

import { cleanup, fireEvent, render, screen } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { normalizeApprovalCause, type Approval } from '@/shared/api/orchestration-client'
import { I18nProvider } from '@/shared/i18n'
import { localeStorageKey } from '@/shared/i18n/locales'
import { ApprovalRequests } from './ChatMessages'

const { mockConfirmStepUp } = vi.hoisted(() => ({ mockConfirmStepUp: vi.fn() }))
vi.mock('@/shared/approvals/step-up', () => ({ confirmStepUp: mockConfirmStepUp }))

function approval(cause: Partial<NonNullable<Approval['cause']>>): Approval {
  return {
    id: 'approval-mcp',
    kind: 'APPROVAL_KIND_DESTRUCTIVE',
    cause: {
      code: 'capability_high_risk',
      tool: 'mcp__notes__create_note',
      scopeOptions: ['once', 'thread'],
      stepUpRequired: false,
      ruleOffered: true,
      ...cause,
    } as NonNullable<Approval['cause']>,
  }
}

function show(approvals: Approval[]) {
  const onDecide = vi.fn()
  render(() => (
    <I18nProvider>
      <ApprovalRequests approvals={approvals} onDecide={onDecide} />
    </I18nProvider>
  ))
  return onDecide
}

const act = (fire: () => void) => {
  fire()
  flush()
}

const alwaysButton = () => screen.queryByRole('button', { name: 'Always allow this tool' })

beforeEach(() => {
  window.localStorage.setItem(localeStorageKey, 'en')
  mockConfirmStepUp.mockReset()
})

afterEach(() => {
  cleanup()
  try {
    window.localStorage.clear()
  } catch {
    // ignore
  }
})

describe('normalizeApprovalCause ruleOffered', () => {
  it('offers Always only for an explicit true', () => {
    expect(normalizeApprovalCause({ code: 'capability_high_risk', rule_offered: true })?.ruleOffered).toBe(true)
    for (const value of [undefined, false, 'true', 1, null]) {
      expect(normalizeApprovalCause({ code: 'capability_high_risk', rule_offered: value })?.ruleOffered).toBeUndefined()
    }
  })
})

describe('the Always control', () => {
  it('decides for the thread with promote, and says what the rule lifts', () => {
    const onDecide = show([approval({})])

    act(() => fireEvent.click(alwaysButton()!))

    expect(onDecide).toHaveBeenCalledTimes(1)
    expect(onDecide).toHaveBeenCalledWith('approval-mcp', 'approve', 'thread', true)
    expect(document.body.textContent).toContain('Your permission mode can still ask.')
  })

  it('leaves the other decisions without promote', () => {
    const onDecide = show([approval({})])
    act(() => fireEvent.click(screen.getByRole('button', { name: 'Approve for this thread' })))
    act(() => fireEvent.click(screen.getByRole('button', { name: 'Approve' })))
    expect(onDecide.mock.calls).toEqual([
      ['approval-mcp', 'approve', 'thread'],
      ['approval-mcp', 'approve'],
    ])
  })

  it.each([
    ['the rule is not offered', { ruleOffered: undefined }],
    ['the rule is offered as false', { ruleOffered: false }],
    ['the thread scope is not offered', { scopeOptions: ['once', 'run'] }],
    ['only once is offered', { scopeOptions: ['once'] }],
  ])('is not shown when %s', (_label, cause) => {
    show([approval(cause)])
    expect(alwaysButton()).toBeNull()
    expect(document.body.textContent).not.toContain('A rule is proposed in settings')
  })

  it('is shown in Norwegian with the same meaning', () => {
    window.localStorage.setItem(localeStorageKey, 'no')
    const onDecide = show([approval({})])
    act(() => fireEvent.click(screen.getByRole('button', { name: 'Godkjenn alltid for dette verktøyet' })))
    expect(onDecide).toHaveBeenCalledWith('approval-mcp', 'approve', 'thread', true)
  })

  it('carries promote through the password step-up', async () => {
    mockConfirmStepUp.mockResolvedValue({
      method: 'password',
      stepUpAt: '2026-11-12T12:00:00.000Z',
      expiresAt: '2026-11-12T12:02:00.000Z',
      expiresInSeconds: 120,
    })
    const onDecide = show([approval({ stepUpRequired: true })])

    act(() => fireEvent.click(alwaysButton()!))
    expect(onDecide).not.toHaveBeenCalled()
    const field = document.querySelector<HTMLInputElement>('input[type="password"]')!
    act(() => fireEvent.input(field, { target: { value: 'correct horse' } }))
    act(() => fireEvent.submit(document.querySelector('form')!))

    await vi.waitFor(() => expect(onDecide).toHaveBeenCalledTimes(1))
    expect(onDecide).toHaveBeenCalledWith('approval-mcp', 'approve', 'thread', true)
  })

  it('drops promote when the step-up is cancelled and a plain approve follows', async () => {
    mockConfirmStepUp.mockResolvedValue({
      method: 'password',
      stepUpAt: '2026-11-12T12:00:00.000Z',
      expiresAt: '2026-11-12T12:02:00.000Z',
      expiresInSeconds: 120,
    })
    const onDecide = show([approval({ stepUpRequired: true })])

    act(() => fireEvent.click(alwaysButton()!))
    act(() => fireEvent.click(screen.getByRole('button', { name: /Cancel|Avbryt/ })))
    act(() => fireEvent.click(screen.getByRole('button', { name: 'Approve' })))
    const field = document.querySelector<HTMLInputElement>('input[type="password"]')!
    act(() => fireEvent.input(field, { target: { value: 'correct horse' } }))
    act(() => fireEvent.submit(document.querySelector('form')!))

    await vi.waitFor(() => expect(onDecide).toHaveBeenCalledTimes(1))
    expect(onDecide).toHaveBeenCalledWith('approval-mcp', 'approve')
  })
})
