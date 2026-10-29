import { describe, expect, it } from 'vitest'
import {
  isChatComposerTarget,
  isEditableTarget,
  matchChatShortcut,
  nextThreadIdForDirection,
  type ChatShortcutEventLike,
  type ChatShortcutTargetLike,
} from '@/features/chat/lib/use-chat-shortcuts'

function makeEvent(overrides: Partial<ChatShortcutEventLike> & { key: string }): ChatShortcutEventLike {
  return {
    altKey: false,
    ctrlKey: false,
    metaKey: false,
    shiftKey: false,
    ...overrides,
  }
}

function makeTarget(overrides: Partial<ChatShortcutTargetLike> = {}): ChatShortcutTargetLike {
  return {
    classList: { contains: () => false },
    isContentEditable: false,
    tagName: 'DIV',
    ...overrides,
  }
}

const composerTarget: ChatShortcutTargetLike = {
  classList: { contains: (name) => name === 'verevon-dashboard-textarea' },
  isContentEditable: false,
  tagName: 'TEXTAREA',
}

describe('isEditableTarget', () => {
  it('treats input, textarea and contentEditable elements as typing', () => {
    expect(isEditableTarget(makeTarget({ tagName: 'INPUT' }))).toBe(true)
    expect(isEditableTarget(makeTarget({ tagName: 'TEXTAREA' }))).toBe(true)
    expect(isEditableTarget(makeTarget({ tagName: 'DIV', isContentEditable: true }))).toBe(true)
  })

  it('treats a plain div/button and null/undefined as not typing', () => {
    expect(isEditableTarget(makeTarget({ tagName: 'DIV' }))).toBe(false)
    expect(isEditableTarget(makeTarget({ tagName: 'BUTTON' }))).toBe(false)
    expect(isEditableTarget(null)).toBe(false)
    expect(isEditableTarget(undefined)).toBe(false)
  })
})

describe('isChatComposerTarget', () => {
  it('recognizes the composer textarea by its class', () => {
    expect(isChatComposerTarget(composerTarget)).toBe(true)
  })

  it('rejects any other element', () => {
    expect(isChatComposerTarget(makeTarget({ tagName: 'TEXTAREA' }))).toBe(false)
    expect(isChatComposerTarget(null)).toBe(false)
  })
})

describe('matchChatShortcut', () => {
  it('matches Cmd+Shift+O and Ctrl+Shift+O for new-chat', () => {
    expect(matchChatShortcut(makeEvent({ key: 'o', metaKey: true, shiftKey: true }), makeTarget()))
      .toEqual({ type: 'new-chat' })
    expect(matchChatShortcut(makeEvent({ key: 'O', ctrlKey: true, shiftKey: true }), makeTarget()))
      .toEqual({ type: 'new-chat' })
  })

  it('does not match Shift+O without a modifier, or mod+O without Shift', () => {
    expect(matchChatShortcut(makeEvent({ key: 'o', shiftKey: true }), makeTarget())).toBeNull()
    expect(matchChatShortcut(makeEvent({ key: 'o', metaKey: true }), makeTarget())).toBeNull()
  })

  it('suppresses new-chat while typing in an unrelated field, but allows it in the composer', () => {
    const event = makeEvent({ key: 'o', metaKey: true, shiftKey: true })
    expect(matchChatShortcut(event, makeTarget({ tagName: 'INPUT' }))).toBeNull()
    expect(matchChatShortcut(event, composerTarget)).toEqual({ type: 'new-chat' })
  })

  it('matches Cmd/Ctrl+I for focus-composer and never bare "/"', () => {
    expect(matchChatShortcut(makeEvent({ key: 'i', metaKey: true }), makeTarget()))
      .toEqual({ type: 'focus-composer' })
    expect(matchChatShortcut(makeEvent({ key: 'i', ctrlKey: true }), makeTarget()))
      .toEqual({ type: 'focus-composer' })
    // Bare "/" (with or without a modifier) must never be claimed here: it is
    // already the composer's slash-command trigger and CoreNavbar's search
    // shortcut. This hook must not shadow it.
    expect(matchChatShortcut(makeEvent({ key: '/' }), makeTarget())).toBeNull()
    expect(matchChatShortcut(makeEvent({ key: '/', metaKey: true }), makeTarget())).toBeNull()
  })

  it('suppresses Cmd/Ctrl+I while typing in an unrelated field, but allows it in the composer', () => {
    const event = makeEvent({ key: 'i', metaKey: true })
    expect(matchChatShortcut(event, makeTarget({ tagName: 'INPUT' }))).toBeNull()
    expect(matchChatShortcut(event, composerTarget)).toEqual({ type: 'focus-composer' })
  })

  it('does not match mod+Shift+I (avoids colliding with a future/other Shift+I binding)', () => {
    expect(matchChatShortcut(makeEvent({ key: 'i', metaKey: true, shiftKey: true }), makeTarget())).toBeNull()
  })

  it('matches Cmd/Ctrl+Shift+ArrowUp/ArrowDown for select-thread', () => {
    expect(matchChatShortcut(makeEvent({ key: 'ArrowUp', metaKey: true, shiftKey: true }), makeTarget()))
      .toEqual({ type: 'select-thread', direction: 'up' })
    expect(matchChatShortcut(makeEvent({ key: 'ArrowDown', ctrlKey: true, shiftKey: true }), makeTarget()))
      .toEqual({ type: 'select-thread', direction: 'down' })
  })

  it('suppresses select-thread while typing anywhere, including the composer', () => {
    const event = makeEvent({ key: 'ArrowDown', metaKey: true, shiftKey: true })
    expect(matchChatShortcut(event, makeTarget({ tagName: 'INPUT' }))).toBeNull()
    expect(matchChatShortcut(event, composerTarget)).toBeNull()
  })

  it('does not match bare arrow keys (left to the sidebar knowledge panel roving tabindex)', () => {
    expect(matchChatShortcut(makeEvent({ key: 'ArrowUp' }), makeTarget())).toBeNull()
    expect(matchChatShortcut(makeEvent({ key: 'ArrowDown' }), makeTarget())).toBeNull()
  })

  it('matches Escape for dismiss regardless of target', () => {
    expect(matchChatShortcut(makeEvent({ key: 'Escape' }), makeTarget())).toEqual({ type: 'dismiss' })
    expect(matchChatShortcut(makeEvent({ key: 'Escape' }), composerTarget)).toEqual({ type: 'dismiss' })
  })

  it('returns null for an unrelated key', () => {
    expect(matchChatShortcut(makeEvent({ key: 'a', metaKey: true }), makeTarget())).toBeNull()
  })
})

describe('nextThreadIdForDirection', () => {
  const threads = [{ threadId: 'a' }, { threadId: 'b' }, { threadId: 'c' }]

  it('returns null for an empty thread list', () => {
    expect(nextThreadIdForDirection([], null, 'down')).toBeNull()
    expect(nextThreadIdForDirection([], 'a', 'up')).toBeNull()
  })

  it('with no active thread, down lands on the first (most recent) and up on the last (oldest)', () => {
    expect(nextThreadIdForDirection(threads, null, 'down')).toBe('a')
    expect(nextThreadIdForDirection(threads, null, 'up')).toBe('c')
  })

  it('moves down and up through the list from the active thread', () => {
    expect(nextThreadIdForDirection(threads, 'a', 'down')).toBe('b')
    expect(nextThreadIdForDirection(threads, 'b', 'down')).toBe('c')
    expect(nextThreadIdForDirection(threads, 'c', 'up')).toBe('b')
    expect(nextThreadIdForDirection(threads, 'b', 'up')).toBe('a')
  })

  it('clamps at both ends instead of wrapping', () => {
    expect(nextThreadIdForDirection(threads, 'c', 'down')).toBeNull()
    expect(nextThreadIdForDirection(threads, 'a', 'up')).toBeNull()
  })

  it('treats an active thread id that is no longer in the list like "no active thread"', () => {
    expect(nextThreadIdForDirection(threads, 'missing', 'down')).toBe('a')
    expect(nextThreadIdForDirection(threads, 'missing', 'up')).toBe('c')
  })
})
