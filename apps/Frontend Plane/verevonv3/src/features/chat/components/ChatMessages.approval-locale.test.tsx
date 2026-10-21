// @vitest-environment jsdom

/**
 * The approval block and the plan-grant control follow the reader's language.
 *
 * Under `no` the wording is the Norwegian the product has always shown. Under
 * `en` every control the reader acts on, every rung name and the granted
 * sentence are English, and the rung names come from one helper so the grant
 * control and the granted sentence cannot drift apart.
 */

import { cleanup, fireEvent, render, screen } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { Approval } from '@/shared/api/orchestration-client'
import { I18nProvider } from '@/shared/i18n'
import { localeStorageKey } from '@/shared/i18n/locales'
import { ApprovalRequests, PlanApprovalControl } from './ChatMessages'

const APPROVALS: Approval[] = [{ id: 'approval-1', kind: 'tool', detail: 'Skriv til arbeidsområdet' }]

function setLocale(locale: 'no' | 'en') {
  window.localStorage.setItem(localeStorageKey, locale)
}

afterEach(() => {
  cleanup()
  try {
    window.localStorage.clear()
  } catch {
    // ignore
  }
})

describe('ApprovalRequests', () => {
  it('names its decision buttons in English under en', () => {
    setLocale('en')
    const onDecide = vi.fn()
    render(() => (
      <I18nProvider>
        <ApprovalRequests approvals={APPROVALS} onDecide={onDecide} />
      </I18nProvider>
    ))

    fireEvent.click(screen.getByRole('button', { name: 'Approve' }))
    expect(onDecide).toHaveBeenLastCalledWith('approval-1', 'approve')
    fireEvent.click(screen.getByRole('button', { name: 'Reject' }))
    expect(onDecide).toHaveBeenLastCalledWith('approval-1', 'reject')
  })

  it('keeps the Norwegian decision buttons under no', () => {
    setLocale('no')
    render(() => (
      <I18nProvider>
        <ApprovalRequests approvals={APPROVALS} onDecide={() => {}} />
      </I18nProvider>
    ))

    expect(screen.getByRole('button', { name: 'Godkjenn' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Avvis' })).toBeTruthy()
  })
})

describe('PlanApprovalControl', () => {
  it('offers the grantable rungs, the lead and the approve button in English', () => {
    setLocale('en')
    render(() => (
      <I18nProvider>
        <PlanApprovalControl pending={false} onApprove={() => {}} />
      </I18nProvider>
    ))

    expect(screen.getByRole('radio', { name: 'Write in the workspace' })).toBeTruthy()
    expect(screen.getByRole('radio', { name: 'Do anything, including outbound actions' })).toBeTruthy()
    expect(screen.queryByText(/bare undersøke/)).toBeNull()
    expect(screen.getByText(
      'This was a plan – nothing has been executed. Choose how much the agent may do, and say why.',
    )).toBeTruthy()
    expect(screen.getByText('At least 12 characters – a reason someone can assess.')).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Approve' })).toBeTruthy()
  })

  it('keeps the Norwegian wording under no', () => {
    setLocale('no')
    render(() => (
      <I18nProvider>
        <PlanApprovalControl pending={false} onApprove={() => {}} />
      </I18nProvider>
    ))

    expect(screen.getByRole('radio', { name: 'skrive i arbeidsområdet' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Godkjenn' })).toBeTruthy()
    expect(screen.getByText(/^Dette var en plan – ingenting er utført\./)).toBeTruthy()
  })

  it('says the pending approval in English', () => {
    setLocale('en')
    render(() => (
      <I18nProvider>
        <PlanApprovalControl pending onApprove={() => {}} />
      </I18nProvider>
    ))

    expect(screen.getByRole('button', { name: 'Approving…' })).toBeTruthy()
  })

  it('states a granted rung as one English sentence', () => {
    setLocale('en')
    render(() => (
      <I18nProvider>
        <PlanApprovalControl grantedRung="danger_full_access" pending={false} onApprove={() => {}} />
      </I18nProvider>
    ))

    expect(screen.getByText('Approved: the agent can now do anything, including outbound actions.')).toBeTruthy()
  })

  it('states a granted rung as one Norwegian sentence under no', () => {
    setLocale('no')
    render(() => (
      <I18nProvider>
        <PlanApprovalControl grantedRung="workspace_write" pending={false} onApprove={() => {}} />
      </I18nProvider>
    ))

    expect(screen.getByText('Godkjent: agenten kan nå skrive i arbeidsområdet.')).toBeTruthy()
  })
})
