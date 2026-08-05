import { onCleanup, onMount } from 'solid-js'
import {
  readActiveChatThreadId,
  readChatThreadHistory,
  selectChatThread,
} from '@/features/chat/lib/chat-thread-history'

/**
 * Chat keyboard shortcuts, t3.chat-flavored but re-picked against this app's
 * OWN existing global bindings (see CoreNavbar.tsx's onMount keydown handler):
 *   - Cmd/Ctrl+K and bare "/" (when not typing) already open the top search.
 *   - Escape already closes navbar panels/search (closeShellOverlays).
 *   - "/" inside the composer already triggers the slash-command menu
 *     (DashboardComposer.tsx) — never bind bare "/" here.
 *
 * Chosen combos:
 *   - Cmd/Ctrl+Shift+O -> new chat. Matches t3.chat exactly; no in-app
 *     collision (only shadows the browser's own bookmark-manager chrome,
 *     which is expected/acceptable for a web app shortcut).
 *   - Cmd/Ctrl+I -> focus the composer. DEVIATION from the more obvious
 *     Cmd/Ctrl+"/": bare "/" is already globally bound (see above) and does
 *     not check for held modifiers, so Cmd/Ctrl+"/" would fire BOTH that
 *     handler (opening search) and this one at the same time. Cmd/Ctrl+I has
 *     no existing binding anywhere in this app.
 *   - Cmd/Ctrl+Shift+ArrowUp/ArrowDown -> move sidebar thread selection.
 *     DEVIATION from plain Up/Down: CoreSidebarKnowledgePanel.tsx already
 *     binds bare ArrowUp/ArrowDown for its own roving-tabindex list, and it
 *     is mounted concurrently with this page inside the persistent
 *     CoreShell/CoreSidebar. A modifier combo removes the ambiguity.
 *   - Escape -> blur the composer if it currently has focus. Deliberately
 *     narrow: DashboardComposer and CoreNavbar already own Escape-driven
 *     menu/panel closing; this only adds the "get me out of the textarea"
 *     behavior neither of them provides.
 */

const CHAT_COMPOSER_SELECTOR = '.verevon-dashboard-textarea'

export type ChatShortcutEventLike = {
  altKey: boolean
  ctrlKey: boolean
  key: string
  metaKey: boolean
  shiftKey: boolean
}

export type ChatShortcutTargetLike = {
  classList?: { contains: (name: string) => boolean }
  isContentEditable?: boolean
  tagName?: string
}

export type ChatShortcutDirection = 'down' | 'up'

export type ChatShortcutAction =
  | { type: 'dismiss' }
  | { type: 'focus-composer' }
  | { type: 'new-chat' }
  | { type: 'select-thread'; direction: ChatShortcutDirection }

/**
 * Standard "am I typing somewhere" check, mirrored from CoreNavbar's own
 * `isTyping` guard so this hook stays consistent with the app's existing
 * convention for the same problem.
 */
export function isEditableTarget(target: ChatShortcutTargetLike | null | undefined): boolean {
  if (!target) return false
  return target.tagName === 'INPUT' || target.tagName === 'TEXTAREA' || Boolean(target.isContentEditable)
}

export function isChatComposerTarget(target: ChatShortcutTargetLike | null | undefined): boolean {
  return Boolean(target?.classList?.contains('verevon-dashboard-textarea'))
}

/**
 * Pure key-matching logic, factored out from the DOM listener so it is
 * testable without a full DOM: given an event-shaped object and a
 * target-shaped object, returns the action to perform, or null if the key
 * combo is not one of ours (or is suppressed by the typing guard).
 */
export function matchChatShortcut(
  event: ChatShortcutEventLike,
  target: ChatShortcutTargetLike | null | undefined,
): ChatShortcutAction | null {
  const mod = event.metaKey || event.ctrlKey
  const key = event.key.toLowerCase()
  const typing = isEditableTarget(target)
  // Typing IN the composer itself is not "an unrelated input" — new-chat and
  // focus-composer stay available mid-message, matching t3.chat. Typing
  // anywhere else (navbar search, a sidebar panel input, etc.) suppresses
  // them so the combo cannot hijack an unrelated field.
  const typingElsewhere = typing && !isChatComposerTarget(target)

  if (mod && event.shiftKey && key === 'o') {
    return typingElsewhere ? null : { type: 'new-chat' }
  }

  if (mod && !event.shiftKey && !event.altKey && key === 'i') {
    return typingElsewhere ? null : { type: 'focus-composer' }
  }

  if (mod && event.shiftKey && (key === 'arrowup' || key === 'arrowdown')) {
    // Also suppressed while typing in the composer itself: Cmd/Ctrl+Shift+Up
    // is a native "select to start of field" gesture in some browsers, and
    // this shortcut is only meaningful while not mid-composition anyway.
    if (typing) return null
    return { type: 'select-thread', direction: key === 'arrowup' ? 'up' : 'down' }
  }

  if (event.key === 'Escape') {
    return { type: 'dismiss' }
  }

  return null
}

/**
 * Picks the next thread id for a select-thread shortcut, given the sidebar's
 * own ordering (readChatThreadHistory returns newest-first) and the
 * currently active thread id. Exported for unit testing.
 *
 * With no active thread ("new chat" view), Down lands on the most recent
 * thread (top of the sidebar) and Up lands on the oldest (bottom) — either
 * arrow gives a sensible starting point rather than doing nothing.
 */
export function nextThreadIdForDirection(
  threads: readonly { threadId: string }[],
  activeThreadId: string | null,
  direction: ChatShortcutDirection,
): string | null {
  if (threads.length === 0) return null

  const currentIndex = activeThreadId ? threads.findIndex((thread) => thread.threadId === activeThreadId) : -1
  const delta = direction === 'down' ? 1 : -1
  const nextIndex = currentIndex === -1
    ? (direction === 'down' ? 0 : threads.length - 1)
    : Math.min(Math.max(currentIndex + delta, 0), threads.length - 1)

  const next = threads[nextIndex]
  return next && next.threadId !== activeThreadId ? next.threadId : null
}

export interface UseChatShortcutsOptions {
  /**
   * Starts a new chat. Passed in by the caller rather than imported directly
   * from use-chat-controller.ts, whose `startNewChat` already carries the
   * thread-reset logic (clearActiveChatThreadId + resetChatState) — this hook
   * only decides *when* to call it.
   */
  startNewChat: () => void
}

/**
 * Registers the chat page's keyboard shortcuts as a single document-level
 * keydown listener, cleaned up on unmount.
 */
export function useChatShortcuts(options: UseChatShortcutsOptions): void {
  onMount(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null
      const action = matchChatShortcut(event, target)
      if (!action) return

      switch (action.type) {
        case 'new-chat': {
          event.preventDefault()
          options.startNewChat()
          break
        }
        case 'focus-composer': {
          event.preventDefault()
          document.querySelector<HTMLTextAreaElement>(CHAT_COMPOSER_SELECTOR)?.focus()
          break
        }
        case 'select-thread': {
          const nextId = nextThreadIdForDirection(
            readChatThreadHistory(),
            readActiveChatThreadId(),
            action.direction,
          )
          if (nextId) {
            event.preventDefault()
            selectChatThread(nextId)
          }
          break
        }
        case 'dismiss': {
          const active = document.activeElement as HTMLElement | null
          if (active && isChatComposerTarget(active)) active.blur()
          break
        }
      }
    }

    document.addEventListener('keydown', handleKeyDown)
    onCleanup(() => document.removeEventListener('keydown', handleKeyDown))
  })
}
