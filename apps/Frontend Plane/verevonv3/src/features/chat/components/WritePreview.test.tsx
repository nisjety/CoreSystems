// @vitest-environment jsdom

/**
 * The full draft of a paused mail send on its approval card (EP-10-S1). Every recipient, the full
 * subject and the full body are shown as plain text; a drafted tag is never rendered as markup.
 */

import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { ApiError } from '@/shared/api/http'
import type { Approval, ApprovalPreview } from '@/shared/api/orchestration-client'
import { I18nProvider } from '@/shared/i18n'
import { localeStorageKey } from '@/shared/i18n/locales'
import { WritePreview, WritePreviewDisclosure, isMailSendApproval, type WritePreviewState } from './WritePreview'

const MAIL: ApprovalPreview = {
  kind: 'mail_send',
  provider: 'google',
  operation: 'gmail.send',
  connectionId: 'conn-1',
  to: Array.from({ length: 12 }, (_, i) => `r${i}@example.com`),
  subject: 'Quarterly status',
  bodyText: 'Hei,\n\n<script>alert(1)</script> <b>bold</b>\nHilsen',
  hasHtml: false,
  isReply: false,
  rawOnly: false,
  editableFields: ['to', 'subject', 'body_text'],
  actionFingerprint: 'f'.repeat(64),
  revision: 0,
}

function showState(state: WritePreviewState) {
  return render(() => (
    <I18nProvider>
      <WritePreview state={state} />
    </I18nProvider>
  ))
}

beforeEach(() => {
  window.localStorage.setItem(localeStorageKey, 'en')
})

afterEach(() => {
  cleanup()
  try {
    window.localStorage.clear()
  } catch {
    // ignore
  }
})

describe('WritePreview', () => {
  it('shows every recipient, the full subject and the full body', () => {
    showState({ status: 'ready', preview: MAIL })

    for (const recipient of MAIL.to) expect(screen.getByText(recipient)).toBeTruthy()
    expect(screen.getByText('Quarterly status')).toBeTruthy()
    const body = document.querySelector('pre.verevon-chat-approval__preview-body')
    expect(body?.textContent).toBe(MAIL.bodyText)
  })

  it('renders the body as plain text and never injects markup', () => {
    showState({ status: 'ready', preview: MAIL })

    expect(document.querySelector('script')).toBeNull()
    expect(document.querySelector('.verevon-chat-approval__preview b')).toBeNull()
    expect(document.body.textContent).toContain('<script>alert(1)</script>')
  })

  it('notes a reply and a formatted mail without showing the formatted copy', () => {
    showState({ status: 'ready', preview: { ...MAIL, isReply: true, hasHtml: true } })

    expect(screen.getByText('Reply in an existing thread.')).toBeTruthy()
    expect(screen.getByText('Formatted mail: the formatting is not shown here.')).toBeTruthy()
  })

  it('has a loading, an error and a not-available state', () => {
    showState({ status: 'loading' })
    expect(screen.getByText('Loading the email …')).toBeTruthy()
    cleanup()

    showState({ status: 'error', message: 'The email could not be loaded.' })
    expect(screen.getByRole('alert').textContent).toBe('The email could not be loaded.')
    cleanup()

    showState({ status: 'unavailable' })
    expect(screen.getByText('Preview not available.')).toBeTruthy()
    cleanup()

    // Another kind of call carries no values, so there is nothing to show beyond the card.
    showState({ status: 'ready', preview: { ...MAIL, kind: 'other', to: [], subject: '', bodyText: '' } })
    expect(screen.getByText('Preview not available.')).toBeTruthy()
    cleanup()

    showState({ status: 'ready', preview: { ...MAIL, rawOnly: true, to: [], subject: '', bodyText: '' } })
    expect(screen.getByText('This email is already encoded and cannot be shown here.')).toBeTruthy()
  })
})

describe('WritePreviewDisclosure', () => {
  function showDisclosure(load: (approvalId: string) => Promise<ApprovalPreview | null>) {
    render(() => (
      <I18nProvider>
        <WritePreviewDisclosure approvalId="appr_1" load={load} />
      </I18nProvider>
    ))
    return screen.getByRole('button', { name: 'Show the full email' })
  }

  it('fetches the draft once, on the first open, and keeps it across a close', async () => {
    const load = vi.fn().mockResolvedValue(MAIL)
    const toggle = showDisclosure(load)
    expect(load).not.toHaveBeenCalled()

    fireEvent.click(toggle)
    flush()
    await waitFor(() => expect(screen.getByText('Quarterly status')).toBeTruthy())
    expect(load).toHaveBeenCalledWith('appr_1')
    expect(toggle.getAttribute('aria-expanded')).toBe('true')

    fireEvent.click(toggle)
    flush()
    expect(screen.queryByText('Quarterly status')).toBeNull()
    fireEvent.click(toggle)
    flush()
    await waitFor(() => expect(screen.getByText('Quarterly status')).toBeTruthy())
    expect(load).toHaveBeenCalledTimes(1)
  })

  it('says not available when the server answers with no preview', async () => {
    const toggle = showDisclosure(vi.fn().mockResolvedValue(null))
    fireEvent.click(toggle)
    flush()
    await waitFor(() => expect(screen.getByText('Preview not available.')).toBeTruthy())
  })

  it('words a refusal through the error copy, never its raw message', async () => {
    const refused = new ApiError('internal sentence that must not show', 412, 'zdr_durable_mutation_forbidden')
    const toggle = showDisclosure(vi.fn().mockRejectedValue(refused))
    fireEvent.click(toggle)
    flush()
    await waitFor(() => expect(screen.getByRole('alert')).toBeTruthy())
    expect(screen.getByRole('alert').textContent).not.toContain('internal sentence that must not show')
  })
})

describe('isMailSendApproval', () => {
  const base: Approval = {
    id: 'appr_1',
    cause: { code: 'provider_write', tool: 'execute_provider_action', scopeOptions: ['once'], stepUpRequired: false },
    detail: "send an email to a@example.com with the subject 'S'",
  }

  it('is true only for a provider action whose reason is a mail send', () => {
    expect(isMailSendApproval(base)).toBe(true)
    expect(isMailSendApproval({ ...base, detail: 'post a message' })).toBe(false)
    expect(isMailSendApproval({ ...base, detail: undefined })).toBe(false)
    const otherTool: Approval = {
      ...base,
      cause: { code: 'provider_write', tool: 'delete_records', scopeOptions: ['once'], stepUpRequired: false },
    }
    expect(isMailSendApproval(otherTool)).toBe(false)
    expect(isMailSendApproval({ ...base, cause: undefined })).toBe(false)
  })
})
