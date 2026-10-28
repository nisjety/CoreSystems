// @vitest-environment jsdom

/**
 * A high-risk approval is granted only after the person re-confirms their password (build plan
 * WS13 S5). Rejecting never asks for one, and an approval whose cause asks for no step-up is
 * granted straight away.
 */

import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { ApiError } from '@/shared/api/http'
import type { Approval } from '@/shared/api/orchestration-client'
import { I18nProvider } from '@/shared/i18n'
import { localeStorageKey } from '@/shared/i18n/locales'
import { ApprovalRequests } from './ChatMessages'

const { mockConfirmStepUp } = vi.hoisted(() => ({ mockConfirmStepUp: vi.fn() }))
vi.mock('@/shared/approvals/step-up', () => ({ confirmStepUp: mockConfirmStepUp }))

const OWNER_EFFECT: Approval = {
  id: 'approval-1',
  kind: 'APPROVAL_KIND_DESTRUCTIVE',
  cause: {
    code: 'owner_effect_reserved',
    tool: 'tickets.create',
    scopeOptions: ['once'],
    stepUpRequired: true,
  },
}

const HOOK_ASK: Approval = {
  id: 'approval-2',
  kind: 'APPROVAL_KIND_DESTRUCTIVE',
  cause: { code: 'hook_ask', tool: 'read_file', scopeOptions: ['once'], stepUpRequired: false },
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

function setLocale(locale: 'no' | 'en') {
  window.localStorage.setItem(localeStorageKey, locale)
}

/** Solid applies a handler's updates on flush, so the DOM is read only after one. */
const act = (fire: () => void) => {
  fire()
  flush()
}

function passwordField(): HTMLInputElement {
  const field = document.querySelector<HTMLInputElement>('input[type="password"]')
  if (!field) throw new Error('the password confirmation is not on the page')
  return field
}

function confirmationForm(): HTMLFormElement {
  const form = document.querySelector<HTMLFormElement>('form')
  if (!form) throw new Error('the password confirmation form is not on the page')
  return form
}

function hasPasswordField(): boolean {
  return document.querySelector('input[type="password"]') !== null
}

beforeEach(() => {
  setLocale('en')
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

describe('approving a high-risk effect', () => {
  it('asks for the password before it grants anything', () => {
    const onDecide = show([OWNER_EFFECT])

    act(() => fireEvent.click(screen.getByRole('button', { name: 'Approve' })))

    expect(hasPasswordField()).toBe(true)
    expect(onDecide).not.toHaveBeenCalled()
    expect(mockConfirmStepUp).not.toHaveBeenCalled()
  })

  it('confirms the password once, then grants the approval', async () => {
    mockConfirmStepUp.mockResolvedValue({
      method: 'password',
      stepUpAt: '2026-10-28T12:00:00.000Z',
      expiresAt: '2026-10-28T12:02:00.000Z',
      expiresInSeconds: 120,
    })
    const onDecide = show([OWNER_EFFECT])
    act(() => fireEvent.click(screen.getByRole('button', { name: 'Approve' })))
    act(() => fireEvent.input(passwordField(), { target: { value: 'correct horse' } }))

    act(() => fireEvent.submit(confirmationForm()))

    await waitFor(() => expect(onDecide).toHaveBeenCalledWith('approval-1', 'approve'))
    expect(mockConfirmStepUp).toHaveBeenCalledTimes(1)
    expect(mockConfirmStepUp).toHaveBeenCalledWith('correct horse')
    expect(onDecide).toHaveBeenCalledTimes(1)
  })

  it('holds the grant back and says so when the password is wrong', async () => {
    mockConfirmStepUp.mockRejectedValue(
      new ApiError('The password is not correct.', 401, 'INVALID_PASSWORD'),
    )
    const onDecide = show([OWNER_EFFECT])
    act(() => fireEvent.click(screen.getByRole('button', { name: 'Approve' })))
    act(() => fireEvent.input(passwordField(), { target: { value: 'wrong' } }))

    act(() => fireEvent.submit(confirmationForm()))

    await waitFor(() => {
      flush()
      expect(screen.getByRole('alert').textContent).toBe('That password is not correct.')
    })
    expect(onDecide).not.toHaveBeenCalled()
  })

  it('cancelling the prompt grants nothing and drops what was typed', () => {
    const onDecide = show([OWNER_EFFECT])
    act(() => fireEvent.click(screen.getByRole('button', { name: 'Approve' })))
    act(() => fireEvent.input(passwordField(), { target: { value: 'typed but not sent' } }))

    act(() => fireEvent.click(screen.getByRole('button', { name: 'Cancel' })))

    expect(hasPasswordField()).toBe(false)
    expect(onDecide).not.toHaveBeenCalled()
    expect(mockConfirmStepUp).not.toHaveBeenCalled()
  })
})

describe('the approvals that need no confirmation', () => {
  it('grants a call whose cause asks for no step-up straight away', () => {
    const onDecide = show([HOOK_ASK])

    act(() => fireEvent.click(screen.getByRole('button', { name: 'Approve' })))

    expect(onDecide).toHaveBeenCalledWith('approval-2', 'approve')
    expect(hasPasswordField()).toBe(false)
  })

  it('rejects a step-up approval without asking for a password', () => {
    const onDecide = show([OWNER_EFFECT])

    act(() => fireEvent.click(screen.getByRole('button', { name: 'Reject' })))

    expect(onDecide).toHaveBeenCalledWith('approval-1', 'reject')
    expect(hasPasswordField()).toBe(false)
    expect(mockConfirmStepUp).not.toHaveBeenCalled()
  })
})
