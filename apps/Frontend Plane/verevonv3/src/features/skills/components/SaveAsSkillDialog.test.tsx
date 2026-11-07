// @vitest-environment jsdom

// EP-11 S1: Save as skill. The dialog starts from the answer, never shows on a temporary chat, and saves a
// private skill (scope user) only when the person presses save.

import { fireEvent, render, waitFor } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'

vi.mock('@/shared/session/session-store', () => ({
  getSession: () => ({ activeOrg: { id: 'org-1', name: 'Test', role: 'member' } }),
}))

vi.mock('@/shared/api/skills-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/skills-client')>()
  return { ...actual, createSkill: vi.fn() }
})

import { createSkill } from '@/shared/api/skills-client'
import { ApiError } from '@/shared/api/api-error'
import { SaveAsSkillDialog, suggestSkillName } from './SaveAsSkillDialog'

const mockedCreate = vi.mocked(createSkill)
const ANSWER = '# Utrulling\n\n1. Kjør testene\n2. Bygg\n3. Rull ut'

function renderDialog(overrides: Partial<Parameters<typeof SaveAsSkillDialog>[0]> = {}) {
  const onClose = vi.fn()
  const onSaved = vi.fn()
  const utils = render(() => (
    <SaveAsSkillDialog
      threadId="thread-1"
      messageId="msg-1"
      content={ANSWER}
      onClose={onClose}
      onSaved={onSaved}
      {...overrides}
    />
  ))
  return { ...utils, onClose, onSaved }
}

afterEach(() => {
  vi.clearAllMocks()
})

describe('SaveAsSkillDialog', () => {
  it('prefills_name_and_content', () => {
    const { getByLabelText, getByRole, unmount } = renderDialog()
    expect((getByLabelText('Navn') as HTMLInputElement).value).toBe('Utrulling')
    expect((getByLabelText('Innhold') as HTMLTextAreaElement).value).toBe(ANSWER)
    // "Use in my chats" starts on.
    expect((getByRole('switch') as HTMLInputElement).checked).toBe(true)
    unmount()
  })

  it('hides_on_temporary_chat', () => {
    const { queryByRole, unmount } = renderDialog({ temporary: true })
    expect(queryByRole('dialog')).toBeNull()
    unmount()
  })

  it('sends_user_scope', async () => {
    mockedCreate.mockResolvedValue({ id: 'skill_new' })
    const { getByRole, onSaved, onClose, unmount } = renderDialog()
    fireEvent.submit(getByRole('dialog'))
    await waitFor(() => expect(mockedCreate).toHaveBeenCalledTimes(1))
    expect(mockedCreate.mock.calls[0]?.[0]).toBe('org-1')
    expect(mockedCreate.mock.calls[0]?.[1]).toEqual({
      name: 'Utrulling',
      description: undefined,
      content: ANSWER,
      enabled: true,
      scope: 'user',
    })
    await waitFor(() => expect(onSaved).toHaveBeenCalledWith('skill_new'))
    expect(onClose).toHaveBeenCalled()
    unmount()
  })

  // EP-11 S4: a lint refusal is shown next to the field it is about, and nothing closes.
  it('shows_lint_refusal_next_to_the_field', async () => {
    mockedCreate.mockRejectedValue(
      new ApiError('The skill does not pass the skill lint.', 422, 'skill_lint_failed', {
        details: { findings: [{ code: 'content_too_long', field: 'content', severity: 'error' }] },
      }),
    )
    const { getByRole, getByLabelText, onClose, unmount } = renderDialog()
    fireEvent.submit(getByRole('dialog'))
    await waitFor(() => expect(getByRole('alert').textContent).toContain('Ferdigheten ble ikke lagret'))
    const note = document.querySelector('[data-lint-code="content_too_long"]')
    expect(note?.textContent).toContain('32 000')
    // The note follows the content field, not another one.
    expect(getByLabelText('Innhold').closest('label')?.nextElementSibling).toBe(note)
    expect(onClose).not.toHaveBeenCalled()
    unmount()
  })

  // A save that went ahead with warnings says so before the dialog closes.
  it('keeps_open_with_lint_warnings_after_save', async () => {
    mockedCreate.mockResolvedValue({
      id: 'skill_new',
      warnings: [{ code: 'transcript_shape', field: 'content', severity: 'warn' }],
    } as unknown as { id: string })
    const { getByRole, getByText, onSaved, onClose, unmount } = renderDialog()
    fireEvent.submit(getByRole('dialog'))
    await waitFor(() => expect(onSaved).toHaveBeenCalledWith('skill_new'))
    expect(getByRole('status').textContent).toContain('samtalelogg')
    expect(onClose).not.toHaveBeenCalled()
    fireEvent.click(getByText('Ferdig'))
    expect(onClose).toHaveBeenCalled()
    unmount()
  })
})

describe('suggestSkillName', () => {
  it('takes the first heading, else the first line, at most 60 characters', () => {
    expect(suggestSkillName('Intro\n# Utrulling\nmer')).toBe('Utrulling')
    expect(suggestSkillName('Slik gjør du det\nmer tekst')).toBe('Slik gjør du det')
    expect(suggestSkillName('x'.repeat(80))).toHaveLength(60)
  })
})
