// @vitest-environment jsdom

/**
 * The full draft of a paused mail send on its approval card (EP-10-S1). Every recipient, the full
 * subject and the full body are shown as plain text; a drafted tag is never rendered as markup.
 */

import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { createSignal, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { ApiError } from '@/shared/api/http'
import type { Approval, ApprovalPreview } from '@/shared/api/orchestration-client'
import { I18nProvider } from '@/shared/i18n'
import { localeStorageKey } from '@/shared/i18n/locales'
import {
  WritePreview,
  WritePreviewDisclosure,
  isMailSendApproval,
  mailEditDraft,
  mailEditFieldsOf,
  previewIsEditable,
  recipientLooksValid,
  type MailEditDraft,
  type WritePreviewState,
} from './WritePreview'

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

describe('the edit contract on the card (EP-10-S6)', () => {
  const EDITABLE: ApprovalPreview = { ...MAIL, to: ['a@example.com'], canEdit: true }

  it('is editable only when the gateway says so, for a normalised mail send with editable fields', () => {
    expect(previewIsEditable(EDITABLE)).toBe(true)
    expect(previewIsEditable({ ...EDITABLE, canEdit: undefined })).toBe(false)
    expect(previewIsEditable({ ...EDITABLE, canEdit: false })).toBe(false)
    expect(previewIsEditable({ ...EDITABLE, rawOnly: true })).toBe(false)
    expect(previewIsEditable({ ...EDITABLE, kind: 'other' })).toBe(false)
    expect(previewIsEditable({ ...EDITABLE, editableFields: [] })).toBe(false)
  })

  it('checks a recipient as one mailbox', () => {
    for (const ok of ['a@example.com', 'Ola Nordmann <ola@example.no>', '  b@example.org ']) {
      expect(recipientLooksValid(ok)).toBe(true)
    }
    for (const bad of ['', 'not-an-address', 'a@b', 'a@example.com, b@example.com', 'x <a@example.com', 'a@exa mple.com']) {
      expect(recipientLooksValid(bad)).toBe(false)
    }
    expect(recipientLooksValid(`${'a'.repeat(320)}@example.com`)).toBe(false)
  })

  it('patches only the changed, editable fields, and an unchanged form is no edit', () => {
    const fields = mailEditFieldsOf(EDITABLE)
    expect(mailEditDraft(EDITABLE, fields)).toBeNull()

    expect(mailEditDraft(EDITABLE, { ...fields, subject: 'New' })).toEqual({
      edit: { patch: { subject: 'New' }, baseFingerprint: EDITABLE.actionFingerprint },
      problem: null,
    })
    // A field the server does not offer for editing never enters the patch.
    expect(mailEditDraft({ ...EDITABLE, editableFields: ['to'] }, { ...fields, subject: 'New' })).toBeNull()
  })

  it('names the first limit an edit breaks, in the server order', () => {
    const fields = mailEditFieldsOf(EDITABLE)
    expect(mailEditDraft(EDITABLE, { ...fields, to: ['nope'] })?.problem).toBe('invalid_recipient')
    expect(mailEditDraft(EDITABLE, { ...fields, to: [] })?.problem).toBe('invalid_recipient')
    expect(
      mailEditDraft(EDITABLE, { ...fields, to: Array.from({ length: 51 }, (_, i) => `r${i}@example.com`) })?.problem,
    ).toBe('too_many_recipients')
    expect(mailEditDraft(EDITABLE, { ...fields, subject: 'a\nBcc: x@example.com' })?.problem).toBe('invalid_subject')
    expect(mailEditDraft(EDITABLE, { ...fields, bodyText: 'x'.repeat(48 * 1024 + 1) })?.problem).toBe('edit_too_large')
    expect(mailEditDraft(EDITABLE, { ...fields, to: ['nope'], subject: 'a\nb' })?.problem).toBe('invalid_recipient')
  })
})

describe('WritePreviewDisclosure edit mode (EP-10-S6)', () => {
  const EDITABLE: ApprovalPreview = { ...MAIL, to: ['a@example.com'], canEdit: true }

  function showEditable(options: {
    load: (approvalId: string) => Promise<ApprovalPreview | null>
    editable?: boolean
    conflict?: () => number
  }) {
    const drafts: (MailEditDraft | null)[] = []
    render(() => (
      <I18nProvider>
        <WritePreviewDisclosure
          approvalId="appr_1"
          load={options.load}
          editable={options.editable ?? true}
          onDraftChange={(draft) => drafts.push(draft)}
          conflict={options.conflict?.() ?? 0}
        />
      </I18nProvider>
    ))
    fireEvent.click(screen.getByRole('button', { name: 'Show the full email' }))
    flush()
    return drafts
  }

  const lastDraft = (drafts: (MailEditDraft | null)[]) => drafts[drafts.length - 1] ?? null

  const startEditing = async () => {
    await waitFor(() => expect(screen.getByRole('button', { name: 'Edit before sending' })).toBeTruthy())
    fireEvent.click(screen.getByRole('button', { name: 'Edit before sending' }))
    flush()
  }

  it('offers no edit without can_edit or without a card that can send edits', async () => {
    showEditable({ load: vi.fn().mockResolvedValue(MAIL) })
    await waitFor(() => expect(screen.getByText('Quarterly status')).toBeTruthy())
    expect(screen.queryByRole('button', { name: 'Edit before sending' })).toBeNull()
    cleanup()

    showEditable({ load: vi.fn().mockResolvedValue(EDITABLE), editable: false })
    await waitFor(() => expect(screen.getByText('Quarterly status')).toBeTruthy())
    expect(screen.queryByRole('button', { name: 'Edit before sending' })).toBeNull()
  })

  it('reports the edit as the person types and nothing for an unchanged form', async () => {
    const drafts = showEditable({ load: vi.fn().mockResolvedValue(EDITABLE) })
    await startEditing()
    expect(lastDraft(drafts)).toBeNull()

    fireEvent.input(screen.getByLabelText(/Subject/), { target: { value: 'Revised status' } })
    flush()
    expect(lastDraft(drafts)?.edit).toEqual({
      patch: { subject: 'Revised status' },
      baseFingerprint: EDITABLE.actionFingerprint,
    })
    expect(screen.getByText('14 / 998', { exact: false })).toBeTruthy()

    fireEvent.input(screen.getByLabelText(/Subject/), { target: { value: EDITABLE.subject } })
    flush()
    expect(lastDraft(drafts)).toBeNull()
  })

  it('adds and removes recipient chips from the keyboard and flags an invalid one', async () => {
    const drafts = showEditable({ load: vi.fn().mockResolvedValue(EDITABLE) })
    await startEditing()

    const add = screen.getByLabelText('Add a recipient')
    fireEvent.input(add, { target: { value: 'not-an-address' } })
    flush()
    fireEvent.keyDown(add, { key: 'Enter' })
    flush()
    expect(lastDraft(drafts)?.problem).toBe('invalid_recipient')
    expect(screen.getByRole('alert').textContent).toContain('not a valid email address')

    fireEvent.click(screen.getByRole('button', { name: 'Remove not-an-address' }))
    flush()
    expect(lastDraft(drafts)).toBeNull()

    fireEvent.input(add, { target: { value: 'b@example.com' } })
    flush()
    fireEvent.keyDown(add, { key: 'Enter' })
    flush()
    expect(lastDraft(drafts)?.edit.patch).toEqual({ to: ['a@example.com', 'b@example.com'] })
    expect(lastDraft(drafts)?.problem).toBeNull()
  })

  it('says the formatting goes only once the body of a formatted mail changes', async () => {
    showEditable({ load: vi.fn().mockResolvedValue({ ...EDITABLE, hasHtml: true }) })
    await startEditing()
    expect(screen.queryByText(/Formatting will be removed/)).toBeNull()

    fireEvent.input(screen.getByLabelText(/Subject/), { target: { value: 'Only the subject' } })
    flush()
    expect(screen.queryByText(/Formatting will be removed/)).toBeNull()

    fireEvent.input(screen.getByLabelText(/Message/), { target: { value: 'A new body' } })
    flush()
    expect(screen.getByText(/Formatting will be removed/)).toBeTruthy()
  })

  it('discards the edits back to the draft', async () => {
    const drafts = showEditable({ load: vi.fn().mockResolvedValue(EDITABLE) })
    await startEditing()
    fireEvent.input(screen.getByLabelText(/Subject/), { target: { value: 'Changed' } })
    flush()

    fireEvent.click(screen.getByRole('button', { name: 'Discard edits' }))
    flush()
    expect(lastDraft(drafts)).toBeNull()
    expect(screen.getByText('Quarterly status')).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Edit before sending' })).toBeTruthy()
  })

  it("refetches the draft on a conflict and keeps the person's text as their version", async () => {
    const changed: ApprovalPreview = { ...EDITABLE, subject: 'Server subject', actionFingerprint: 'e'.repeat(64) }
    const load = vi.fn().mockResolvedValueOnce(EDITABLE).mockResolvedValueOnce(changed)
    const [conflict, setConflict] = createSignal(0)
    const drafts = showEditable({ load, conflict })
    await startEditing()
    fireEvent.input(screen.getByLabelText(/Subject/), { target: { value: 'My own subject' } })
    flush()

    setConflict(1)
    flush()
    await waitFor(() => expect(load).toHaveBeenCalledTimes(2))
    await waitFor(() => expect(screen.getByText('Your version')).toBeTruthy())
    expect(screen.getByText('My own subject')).toBeTruthy()
    await waitFor(() =>
      expect((screen.getByLabelText(/Subject/) as HTMLInputElement).value).toBe('Server subject'),
    )
    // The form starts again from the new draft, so nothing is pending until the person edits it.
    expect(lastDraft(drafts)).toBeNull()
  })
})
