// @vitest-environment jsdom

/**
 * "Send with my edits" on a paused mail send's approval card (EP-10-S6). Edit mode exists only when
 * the gateway marks the draft `can_edit` and the card can send an edited approval; an edited
 * approval is ONCE only, the WS13-S5 step-up runs first, an unchanged form is a plain approval and
 * a 409 approval_changed refetches the draft while keeping the person's text.
 */

import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi, type Mock } from 'vitest'
import { ApiError } from '@/shared/api/http'
import type { Approval, ApprovalEdit, ApprovalPreview } from '@/shared/api/orchestration-client'
import { I18nProvider } from '@/shared/i18n'
import { localeStorageKey } from '@/shared/i18n/locales'
import { ApprovalRequests } from './ChatMessages'

const { mockGetApprovalPreview, mockConfirmStepUp } = vi.hoisted(() => ({
  mockGetApprovalPreview: vi.fn(),
  mockConfirmStepUp: vi.fn(),
}))
vi.mock('@/shared/api/orchestration-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/orchestration-client')>()),
  getApprovalPreview: mockGetApprovalPreview,
}))
vi.mock('@/shared/approvals/step-up', () => ({ confirmStepUp: mockConfirmStepUp }))

const MAIL_SEND: Approval = {
  id: 'approval-mail',
  kind: 'APPROVAL_KIND_TOOL_CALL',
  cause: {
    code: 'provider_write',
    tool: 'execute_provider_action',
    scopeOptions: ['once', 'thread', 'run'],
    stepUpRequired: false,
  },
  detail: "send an email to a@example.com with the subject 'Status'; it opens 'Hei'",
}

const PREVIEW: ApprovalPreview = {
  kind: 'mail_send',
  provider: 'google',
  operation: 'gmail.send',
  connectionId: 'conn-1',
  to: ['a@example.com'],
  subject: 'Status',
  bodyText: 'Hei, the whole body',
  hasHtml: false,
  isReply: false,
  rawOnly: false,
  editableFields: ['to', 'subject', 'body_text'],
  actionFingerprint: 'f'.repeat(64),
  revision: 0,
  canEdit: true,
}

type DecideEdited = (approvalId: string, edit: ApprovalEdit) => Promise<void>

/** Renders the card. `null` for `onDecideEdited` is a card that cannot send an edited approval. */
function show(
  approvals: Approval[],
  onDecideEdited: Mock<DecideEdited> | null = vi.fn<DecideEdited>().mockResolvedValue(undefined),
) {
  const onDecide = vi.fn()
  render(() => (
    <I18nProvider>
      <ApprovalRequests approvals={approvals} onDecide={onDecide} onDecideEdited={onDecideEdited ?? undefined} />
    </I18nProvider>
  ))
  return { onDecide, onDecideEdited: onDecideEdited ?? vi.fn<DecideEdited>() }
}

/** Solid applies a handler's updates on flush, so the DOM is read only after one. */
const act = (fire: () => void) => {
  fire()
  flush()
}

const button = (name: string | RegExp) => screen.getByRole('button', { name })
const queryButton = (name: string | RegExp) => screen.queryByRole('button', { name })

async function openEditor() {
  act(() => fireEvent.click(button('Show the full email')))
  await waitFor(() => expect(button('Edit before sending')).toBeTruthy())
  act(() => fireEvent.click(button('Edit before sending')))
}

const editSubject = (value: string) =>
  act(() => fireEvent.input(screen.getByLabelText(/Subject/), { target: { value } }))

beforeEach(() => {
  window.localStorage.setItem(localeStorageKey, 'en')
  vi.stubEnv('VITE_APPROVAL_WRITE_PREVIEW', 'true')
  mockGetApprovalPreview.mockReset()
  mockConfirmStepUp.mockReset()
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

describe('the editable approval card', () => {
  it('offers no edit mode without can_edit', async () => {
    mockGetApprovalPreview.mockResolvedValue({ ...PREVIEW, canEdit: false })
    show([MAIL_SEND])
    act(() => fireEvent.click(button('Show the full email')))
    await waitFor(() => expect(screen.getByText(PREVIEW.bodyText)).toBeTruthy())
    expect(queryButton('Edit before sending')).toBeNull()
  })

  it('offers no edit mode on a card that cannot send an edited approval', async () => {
    mockGetApprovalPreview.mockResolvedValue(PREVIEW)
    show([MAIL_SEND], null)
    act(() => fireEvent.click(button('Show the full email')))
    await waitFor(() => expect(screen.getByText(PREVIEW.bodyText)).toBeTruthy())
    expect(queryButton('Edit before sending')).toBeNull()
  })

  it('sends a plain approval while the form equals the draft', async () => {
    mockGetApprovalPreview.mockResolvedValue(PREVIEW)
    const { onDecide, onDecideEdited } = show([MAIL_SEND])
    await openEditor()

    expect(button('Approve')).toBeTruthy()
    act(() => fireEvent.click(button('Approve')))
    expect(onDecide).toHaveBeenCalledWith('approval-mail', 'approve')
    expect(onDecideEdited).not.toHaveBeenCalled()
  })

  it('becomes Send with my edits, hides the standing scopes and sends the patch once', async () => {
    mockGetApprovalPreview.mockResolvedValue(PREVIEW)
    const { onDecide, onDecideEdited } = show([MAIL_SEND])
    expect(button('Approve for this thread')).toBeTruthy()
    await openEditor()
    editSubject('Revised status')

    expect(queryButton('Approve')).toBeNull()
    expect(queryButton('Approve for this thread')).toBeNull()
    expect(queryButton('Approve for this run')).toBeNull()

    act(() => fireEvent.click(button('Send with my edits')))
    await waitFor(() => expect(onDecideEdited).toHaveBeenCalledTimes(1))
    expect(onDecideEdited).toHaveBeenCalledWith('approval-mail', {
      patch: { subject: 'Revised status' },
      baseFingerprint: PREVIEW.actionFingerprint,
    })
    expect(onDecide).not.toHaveBeenCalled()
  })

  it('blocks the send while a recipient is not a valid address', async () => {
    mockGetApprovalPreview.mockResolvedValue(PREVIEW)
    const { onDecideEdited } = show([MAIL_SEND])
    await openEditor()
    const add = screen.getByLabelText('Add a recipient')
    act(() => fireEvent.input(add, { target: { value: 'not-an-address' } }))
    act(() => fireEvent.keyDown(add, { key: 'Enter' }))

    const send = button('Send with my edits') as HTMLButtonElement
    expect(send.disabled).toBe(true)
    act(() => fireEvent.click(send))
    expect(onDecideEdited).not.toHaveBeenCalled()
  })

  it('asks for the step-up password first, then sends the edited approval', async () => {
    mockGetApprovalPreview.mockResolvedValue(PREVIEW)
    mockConfirmStepUp.mockResolvedValue(undefined)
    const stepUp: Approval = { ...MAIL_SEND, cause: { ...MAIL_SEND.cause!, stepUpRequired: true } }
    const { onDecideEdited } = show([stepUp])
    await openEditor()
    editSubject('Revised status')

    act(() => fireEvent.click(button('Send with my edits')))
    expect(onDecideEdited).not.toHaveBeenCalled()
    const password = document.querySelector<HTMLInputElement>('input[type="password"]')
    expect(password).not.toBeNull()
    act(() => fireEvent.input(password!, { target: { value: 'pw' } }))
    expect(button('Confirm and send with my edits')).toBeTruthy()
    act(() => fireEvent.submit(document.querySelector('form.verevon-chat-approval__confirm')!))

    await waitFor(() => expect(onDecideEdited).toHaveBeenCalledTimes(1))
    expect(mockConfirmStepUp).toHaveBeenCalledWith('pw')
    expect(onDecideEdited.mock.calls[0]?.[1]).toMatchObject({ patch: { subject: 'Revised status' } })
  })

  it("on a 409 approval_changed refetches the draft and keeps the person's text", async () => {
    const changed: ApprovalPreview = { ...PREVIEW, subject: 'Server subject', actionFingerprint: 'e'.repeat(64) }
    mockGetApprovalPreview.mockResolvedValueOnce(PREVIEW).mockResolvedValueOnce(changed)
    const refused = vi
      .fn<DecideEdited>()
      .mockRejectedValue(new ApiError('changed', 409, 'approval_changed'))
    const { onDecide } = show([MAIL_SEND], refused)
    await openEditor()
    editSubject('My own subject')

    act(() => fireEvent.click(button('Send with my edits')))
    await waitFor(() => expect(mockGetApprovalPreview).toHaveBeenCalledTimes(2))
    await waitFor(() => expect(screen.getByText('Your version')).toBeTruthy())
    expect(screen.getByText('My own subject')).toBeTruthy()
    expect(screen.getByRole('alert').textContent).toContain('The draft changed since you opened it')
    await waitFor(() =>
      expect((screen.getByLabelText(/Subject/) as HTMLInputElement).value).toBe('Server subject'),
    )
    // Nothing is pending against the new draft until the person edits it again.
    expect(button('Approve')).toBeTruthy()
    expect(onDecide).not.toHaveBeenCalled()
  })

  it('words any other refusal without the raw server message', async () => {
    mockGetApprovalPreview.mockResolvedValue(PREVIEW)
    const refused = vi
      .fn<DecideEdited>()
      .mockRejectedValue(new ApiError('internal sentence', 403, 'approval_edit_disabled'))
    show([MAIL_SEND], refused)
    await openEditor()
    editSubject('Revised')

    act(() => fireEvent.click(button('Send with my edits')))
    await waitFor(() => expect(screen.getByRole('alert')).toBeTruthy())
    expect(screen.getByRole('alert').textContent).not.toContain('internal sentence')
  })

  it('is usable from the keyboard alone at a narrow width', async () => {
    window.innerWidth = 375
    window.dispatchEvent(new Event('resize'))
    mockGetApprovalPreview.mockResolvedValue(PREVIEW)
    const { onDecideEdited } = show([MAIL_SEND])
    await openEditor()

    // Every control of the editor is a native, focusable element in reading order.
    const add = screen.getByLabelText('Add a recipient')
    add.focus()
    expect(document.activeElement).toBe(add)
    act(() => fireEvent.input(add, { target: { value: 'b@example.com' } }))
    act(() => fireEvent.keyDown(add, { key: ',' }))
    expect(screen.getByRole('button', { name: 'Remove b@example.com' })).toBeTruthy()

    const send = button('Send with my edits')
    send.focus()
    act(() => fireEvent.click(send))
    await waitFor(() => expect(onDecideEdited).toHaveBeenCalledTimes(1))
    expect(onDecideEdited).toHaveBeenCalledWith('approval-mail', {
      patch: { to: ['a@example.com', 'b@example.com'] },
      baseFingerprint: PREVIEW.actionFingerprint,
    })
  })
})
