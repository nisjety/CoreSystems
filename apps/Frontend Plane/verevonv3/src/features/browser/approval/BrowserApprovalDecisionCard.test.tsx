// @vitest-environment jsdom

import { fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { describe, expect, it, vi } from 'vitest'
import { BrowserApprovalDecisionCard } from './BrowserApprovalDecisionCard'
import type { ApprovalCardModel } from './approval-model'

// No I18nProvider wraps these renders, so useI18n() resolves the Norwegian
// argument of tr(), the same convention the other chat and browser tests use.
const model: ApprovalCardModel = {
  approvalId: 'appr_9',
  source: 'browser',
  reason: 'Pays for the order',
  actionType: 'click',
  target: 'shop.example.com/checkout · #pay',
  riskCategory: 'checkout',
}

describe('BrowserApprovalDecisionCard', () => {
  it('approves when the Approve button is clicked, with an empty reason', async () => {
    const decide = vi.fn().mockResolvedValue(undefined)
    const onDecided = vi.fn()
    render(() => <BrowserApprovalDecisionCard approval={model} decide={decide} onDecided={onDecided} />)

    fireEvent.click(screen.getByRole('button', { name: /godkjenn/i }))

    await waitFor(() => expect(onDecided).toHaveBeenCalledTimes(1))
    expect(decide).toHaveBeenCalledWith({ approvalId: 'appr_9', decision: 'approve', reason: '' })
  })

  it('approves on Ctrl+Enter inside the card', async () => {
    const decide = vi.fn().mockResolvedValue(undefined)
    render(() => <BrowserApprovalDecisionCard approval={model} decide={decide} />)

    fireEvent.keyDown(screen.getByRole('group'), { key: 'Enter', ctrlKey: true })

    await waitFor(() => expect(decide).toHaveBeenCalledWith({ approvalId: 'appr_9', decision: 'approve', reason: '' }))
  })

  it('rejects on Ctrl+Shift+Enter inside the card', async () => {
    const decide = vi.fn().mockResolvedValue(undefined)
    render(() => <BrowserApprovalDecisionCard approval={model} decide={decide} />)

    fireEvent.keyDown(screen.getByRole('group'), { key: 'Enter', ctrlKey: true, shiftKey: true })

    await waitFor(() => expect(decide).toHaveBeenCalledWith({ approvalId: 'appr_9', decision: 'reject', reason: '' }))
  })

  it('keeps the redirect box and its send button hidden unless allowRedirect is set', () => {
    render(() => <BrowserApprovalDecisionCard approval={model} decide={vi.fn()} />)

    expect(screen.queryByRole('textbox')).toBeNull()
    expect(screen.queryByRole('button', { name: /send beskjed/i })).toBeNull()
  })

  it('sends typed redirect text as a reject reason, trimmed', async () => {
    const decide = vi.fn().mockResolvedValue(undefined)
    render(() => <BrowserApprovalDecisionCard approval={model} allowRedirect decide={decide} />)

    fireEvent.input(screen.getByRole('textbox'), { target: { value: '  use the saved card  ' } })
    fireEvent.click(screen.getByRole('button', { name: /send beskjed/i }))

    await waitFor(() =>
      expect(decide).toHaveBeenCalledWith({ approvalId: 'appr_9', decision: 'reject', reason: 'use the saved card' }),
    )
  })

  it('does not send when the redirect box is empty, and keeps Ctrl+Enter from approving over typed text', () => {
    const decide = vi.fn().mockResolvedValue(undefined)
    render(() => <BrowserApprovalDecisionCard approval={model} allowRedirect decide={decide} />)

    fireEvent.input(screen.getByRole('textbox'), { target: { value: 'keep me' } })
    fireEvent.keyDown(screen.getByRole('textbox'), { key: 'Enter', ctrlKey: true })

    expect(decide).not.toHaveBeenCalled()
  })

  it('keeps the card pending and shows an alert when the decision fails', async () => {
    const decide = vi.fn().mockRejectedValue(new Error('gateway down'))
    const onDecided = vi.fn()
    render(() => <BrowserApprovalDecisionCard approval={model} decide={decide} onDecided={onDecided} />)

    fireEvent.click(screen.getByRole('button', { name: /avslå/i }))

    await waitFor(() => expect(screen.getByRole('alert')).toBeTruthy())
    expect(onDecided).not.toHaveBeenCalled()
    expect(screen.getByRole('button', { name: /godkjenn/i })).toBeTruthy()
  })
})
