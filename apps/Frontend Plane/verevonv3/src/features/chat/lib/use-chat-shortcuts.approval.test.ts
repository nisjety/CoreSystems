import { describe, expect, it } from 'vitest'
import {
  matchApprovalShortcut,
  type ApprovalShortcutEventLike,
  type ChatShortcutTargetLike,
} from '@/features/chat/lib/use-chat-shortcuts'

function event(overrides: Partial<ApprovalShortcutEventLike> & { key: string }): ApprovalShortcutEventLike {
  return { altKey: false, ctrlKey: false, metaKey: false, shiftKey: false, ...overrides }
}

const cardTarget: ChatShortcutTargetLike = {
  classList: { contains: () => false },
  isContentEditable: false,
  tagName: 'SECTION',
}

const redirectTarget: ChatShortcutTargetLike = {
  classList: { contains: (name: string) => name === 'verevon-browser-approval__redirect' },
  isContentEditable: false,
  tagName: 'TEXTAREA',
}

const otherInput: ChatShortcutTargetLike = {
  classList: { contains: () => false },
  isContentEditable: false,
  tagName: 'INPUT',
}

const noText = { redirectText: '' }
const withText = { redirectText: 'open the invoice tab' }

describe('matchApprovalShortcut', () => {
  it('approves on Ctrl+Enter and Cmd+Enter when no redirect text is typed', () => {
    expect(matchApprovalShortcut(event({ key: 'Enter', ctrlKey: true }), cardTarget, noText)).toEqual({ type: 'approve' })
    expect(matchApprovalShortcut(event({ key: 'Enter', metaKey: true }), cardTarget, noText)).toEqual({ type: 'approve' })
  })

  it('refuses to approve while redirect text is typed, so the text is not discarded by a stray combo', () => {
    expect(matchApprovalShortcut(event({ key: 'Enter', ctrlKey: true }), redirectTarget, withText)).toBeNull()
  })

  it('rejects on Mod+Shift+Enter when no redirect text is typed', () => {
    expect(matchApprovalShortcut(event({ key: 'Enter', ctrlKey: true, shiftKey: true }), cardTarget, noText)).toEqual({
      type: 'reject',
    })
  })

  it('submits the redirect on Mod+Shift+Enter and on a plain Enter in the redirect box when text is present', () => {
    expect(matchApprovalShortcut(event({ key: 'Enter', metaKey: true, shiftKey: true }), redirectTarget, withText)).toEqual({
      type: 'submit-redirect',
    })
    expect(matchApprovalShortcut(event({ key: 'Enter' }), redirectTarget, withText)).toEqual({ type: 'submit-redirect' })
  })

  it('does not submit an empty redirect on a plain Enter, and keeps Shift+Enter as a newline', () => {
    expect(matchApprovalShortcut(event({ key: 'Enter' }), redirectTarget, noText)).toBeNull()
    expect(matchApprovalShortcut(event({ key: 'Enter', shiftKey: true }), redirectTarget, withText)).toBeNull()
  })

  it('leaves a bare Enter on a focused button to the button', () => {
    expect(matchApprovalShortcut(event({ key: 'Enter' }), cardTarget, noText)).toBeNull()
  })

  it('clears the redirect on Escape only inside the redirect box', () => {
    expect(matchApprovalShortcut(event({ key: 'Escape' }), redirectTarget, withText)).toEqual({ type: 'clear-redirect' })
    expect(matchApprovalShortcut(event({ key: 'Escape' }), cardTarget, noText)).toBeNull()
  })

  it('never hijacks keys typed in another field', () => {
    expect(matchApprovalShortcut(event({ key: 'Enter', ctrlKey: true }), otherInput, noText)).toBeNull()
    expect(matchApprovalShortcut(event({ key: 'Escape' }), otherInput, noText)).toBeNull()
  })

  it('ignores IME composition and Alt combos', () => {
    expect(matchApprovalShortcut(event({ key: 'Enter', ctrlKey: true, isComposing: true }), cardTarget, noText)).toBeNull()
    expect(matchApprovalShortcut(event({ key: 'Enter', ctrlKey: true, altKey: true }), cardTarget, noText)).toBeNull()
  })

  it('ignores unrelated keys', () => {
    expect(matchApprovalShortcut(event({ key: 'a', ctrlKey: true }), cardTarget, noText)).toBeNull()
  })
})
