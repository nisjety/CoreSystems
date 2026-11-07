// @vitest-environment jsdom

/**
 * "Show the full email" on the approval card (EP-10-S1). It appears only on a paused mail send and
 * only with `VITE_APPROVAL_WRITE_PREVIEW` the literal `true`; with the switch off the card is as
 * it was. Opening it reads the draft once.
 */

import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { Approval, ApprovalPreview } from '@/shared/api/orchestration-client'
import { I18nProvider } from '@/shared/i18n'
import { localeStorageKey } from '@/shared/i18n/locales'
import { ApprovalRequests } from './ChatMessages'

const { mockGetApprovalPreview } = vi.hoisted(() => ({ mockGetApprovalPreview: vi.fn() }))
vi.mock('@/shared/api/orchestration-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/orchestration-client')>()),
  getApprovalPreview: mockGetApprovalPreview,
}))

const MAIL_SEND: Approval = {
  id: 'approval-mail',
  kind: 'APPROVAL_KIND_TOOL_CALL',
  cause: { code: 'provider_write', tool: 'execute_provider_action', scopeOptions: ['once'], stepUpRequired: false },
  detail: "send an email to a@example.com with the subject 'Status'; it opens 'Hei'",
}

const SLACK_POST: Approval = {
  id: 'approval-slack',
  kind: 'APPROVAL_KIND_TOOL_CALL',
  cause: { code: 'provider_write', tool: 'execute_provider_action', scopeOptions: ['once'], stepUpRequired: false },
}

const OWNER_EFFECT: Approval = {
  id: 'approval-owner',
  kind: 'APPROVAL_KIND_DESTRUCTIVE',
  cause: { code: 'owner_effect_reserved', tool: 'tickets.create', scopeOptions: ['once'], stepUpRequired: false },
}

const PREVIEW: ApprovalPreview = {
  kind: 'mail_send',
  provider: 'google',
  operation: 'gmail.send',
  connectionId: 'conn-1',
  to: ['a@example.com'],
  subject: 'Status',
  bodyText: 'Hei, the whole body and not just its opening',
  hasHtml: false,
  isReply: false,
  rawOnly: false,
  editableFields: ['to', 'subject', 'body_text'],
  actionFingerprint: 'f'.repeat(64),
  revision: 0,
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

const disclosures = () => screen.queryAllByRole('button', { name: 'Show the full email' })

beforeEach(() => {
  window.localStorage.setItem(localeStorageKey, 'en')
  mockGetApprovalPreview.mockReset()
})

afterEach(() => {
  cleanup()
  vi.unstubAllEnvs()
  try {
    window.localStorage.clear()
  } catch {
    // ignore
  }
})

describe('the full-email disclosure on the approval card', () => {
  it('is absent with the switch off, so the card is unchanged', () => {
    for (const value of [undefined, '1', 'TRUE ']) {
      if (value !== undefined) vi.stubEnv('VITE_APPROVAL_WRITE_PREVIEW', value)
      show([MAIL_SEND])
      expect(disclosures()).toHaveLength(0)
      cleanup()
      vi.unstubAllEnvs()
    }
    expect(mockGetApprovalPreview).not.toHaveBeenCalled()
  })

  it('appears only on a mail send', () => {
    vi.stubEnv('VITE_APPROVAL_WRITE_PREVIEW', 'true')
    show([MAIL_SEND, SLACK_POST, OWNER_EFFECT])
    expect(disclosures()).toHaveLength(1)
  })

  it('reads the draft once, on open, and leaves the decision buttons as they were', async () => {
    vi.stubEnv('VITE_APPROVAL_WRITE_PREVIEW', 'true')
    mockGetApprovalPreview.mockResolvedValue(PREVIEW)
    const onDecide = show([MAIL_SEND])
    expect(mockGetApprovalPreview).not.toHaveBeenCalled()

    fireEvent.click(disclosures()[0]!)
    flush()
    await waitFor(() => expect(screen.getByText(PREVIEW.bodyText)).toBeTruthy())
    expect(mockGetApprovalPreview).toHaveBeenCalledTimes(1)
    expect(mockGetApprovalPreview.mock.calls[0]?.[0]).toBe('approval-mail')

    fireEvent.click(screen.getByRole('button', { name: 'Approve' }))
    flush()
    expect(onDecide).toHaveBeenCalledWith('approval-mail', 'approve')
  })
})
