// @vitest-environment jsdom
import { createRoot, flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'

import {
  matchChatShortcut,
  useChatShortcuts,
  type ChatShortcutEventLike,
  type ChatShortcutTargetLike,
  type UseChatShortcutsOptions,
} from '@/features/chat/lib/use-chat-shortcuts'

function makeEvent(overrides: Partial<ChatShortcutEventLike> & { key: string }): ChatShortcutEventLike {
  return { altKey: false, ctrlKey: false, metaKey: false, shiftKey: false, ...overrides }
}

const page: ChatShortcutTargetLike = { classList: { contains: () => false }, isContentEditable: false, tagName: 'DIV' }
const input: ChatShortcutTargetLike = { classList: { contains: () => false }, isContentEditable: false, tagName: 'INPUT' }
const composer: ChatShortcutTargetLike = {
  classList: { contains: (name) => name === 'verevon-dashboard-textarea' },
  isContentEditable: false,
  tagName: 'TEXTAREA',
}

describe('browser dock chords (WS9.G)', () => {
  it('Ctrl+Alt+B and Cmd+Option+B toggle the dock, matched by the physical key', () => {
    expect(matchChatShortcut(makeEvent({ key: 'b', code: 'KeyB', ctrlKey: true, altKey: true }), page))
      .toEqual({ type: 'toggle-browser-dock' })
    // On macOS Option changes the character, so Option+B reports another key with the same physical code.
    expect(matchChatShortcut(makeEvent({ key: '∫', code: 'KeyB', metaKey: true, altKey: true }), page))
      .toEqual({ type: 'toggle-browser-dock' })
  })

  it('Mod+Alt+L cycles the dock layout', () => {
    expect(matchChatShortcut(makeEvent({ key: 'l', code: 'KeyL', ctrlKey: true, altKey: true }), page))
      .toEqual({ type: 'cycle-browser-dock' })
  })

  it('falls back to the key for an event that carries no physical code', () => {
    expect(matchChatShortcut(makeEvent({ key: 'b', ctrlKey: true, altKey: true }), page))
      .toEqual({ type: 'toggle-browser-dock' })
  })

  it('needs the Mod key, Alt, and no Shift', () => {
    expect(matchChatShortcut(makeEvent({ key: 'b', code: 'KeyB', altKey: true }), page)).toBeNull()
    expect(matchChatShortcut(makeEvent({ key: 'b', code: 'KeyB', ctrlKey: true }), page)).toBeNull()
    expect(matchChatShortcut(makeEvent({ key: 'B', code: 'KeyB', ctrlKey: true, altKey: true, shiftKey: true }), page)).toBeNull()
  })

  it('refuses AltGr, which Windows reports as Ctrl+Alt', () => {
    const altGr = { getModifierState: (name: string) => name === 'AltGraph' }
    expect(matchChatShortcut(makeEvent({ key: 'b', code: 'KeyB', ctrlKey: true, altKey: true, ...altGr }), page)).toBeNull()
  })

  it('is suppressed while typing in another field, and allowed in the composer', () => {
    expect(matchChatShortcut(makeEvent({ key: 'b', code: 'KeyB', ctrlKey: true, altKey: true }), input)).toBeNull()
    expect(matchChatShortcut(makeEvent({ key: 'b', code: 'KeyB', ctrlKey: true, altKey: true }), composer))
      .toEqual({ type: 'toggle-browser-dock' })
  })

  it('leaves the existing chords and Escape as they were', () => {
    expect(matchChatShortcut(makeEvent({ key: 'i', ctrlKey: true }), page)).toEqual({ type: 'focus-composer' })
    expect(matchChatShortcut(makeEvent({ key: 'i', ctrlKey: true, altKey: true }), page)).toBeNull()
    expect(matchChatShortcut(makeEvent({ key: 'O', ctrlKey: true, shiftKey: true }), page)).toEqual({ type: 'new-chat' })
    expect(matchChatShortcut(makeEvent({ key: 'Escape' }), page)).toEqual({ type: 'dismiss' })
    expect(matchChatShortcut(makeEvent({ key: 'b', ctrlKey: true }), page)).toBeNull()
  })
})

describe('useChatShortcuts browser dock actions (WS9.G)', () => {
  const disposers: Array<() => void> = []

  afterEach(() => {
    for (const dispose of disposers.splice(0)) dispose()
    vi.unstubAllEnvs()
  })

  function mount(options: UseChatShortcutsOptions) {
    disposers.push(createRoot((dispose) => {
      useChatShortcuts(options)
      return dispose
    }))
    flush()
  }

  function press(init: KeyboardEventInit): KeyboardEvent {
    const event = new KeyboardEvent('keydown', { bubbles: true, cancelable: true, ...init })
    document.dispatchEvent(event)
    return event
  }

  it('does nothing while the flag is off, so the page keeps its keys', () => {
    vi.unstubAllEnvs()
    const toggle = vi.fn()
    mount({ startNewChat: vi.fn(), toggleBrowserDock: toggle })
    const event = press({ key: 'b', code: 'KeyB', ctrlKey: true, altKey: true })
    expect(toggle).not.toHaveBeenCalled()
    expect(event.defaultPrevented).toBe(false)
  })

  it('runs the toggle and the cycle while the flag is on, and prevents the default', () => {
    vi.stubEnv('VITE_CHAT_BROWSER_DOCK', '1')
    const toggle = vi.fn()
    const cycle = vi.fn()
    mount({ startNewChat: vi.fn(), toggleBrowserDock: toggle, cycleBrowserDock: cycle })
    const toggled = press({ key: 'b', code: 'KeyB', ctrlKey: true, altKey: true })
    const cycled = press({ key: 'l', code: 'KeyL', ctrlKey: true, altKey: true })
    expect(toggle).toHaveBeenCalledTimes(1)
    expect(cycle).toHaveBeenCalledTimes(1)
    expect(toggled.defaultPrevented).toBe(true)
    expect(cycled.defaultPrevented).toBe(true)
  })

  it('leaves the chord alone when the page does not wire the action', () => {
    vi.stubEnv('VITE_CHAT_BROWSER_DOCK', '1')
    mount({ startNewChat: vi.fn() })
    expect(press({ key: 'b', code: 'KeyB', ctrlKey: true, altKey: true }).defaultPrevented).toBe(false)
  })
})
