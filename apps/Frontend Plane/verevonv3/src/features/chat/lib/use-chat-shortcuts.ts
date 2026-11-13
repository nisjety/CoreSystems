import { createEffect } from 'solid-js'
import {
  readActiveChatThreadId,
  readChatThreadHistory,
  selectChatThread,
} from '@/features/chat/lib/chat-thread-history'
import { isChatBrowserDockEnabled } from '@/features/browser/dock/browser-dock-state'

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
 *   - Escape, Escape -> stop the streaming answer (F-t3-077). Two bare Escape
 *     presses within 600 ms, behind VITE_CHAT_DOUBLE_ESCAPE_STOP; see
 *     createDoubleEscape. The second press takes the place of the blur.
 *   - Cmd/Ctrl+Alt+B -> show or hide the browser dock beside a live run, and
 *     Cmd/Ctrl+Alt+L -> cycle its layout (WS9.G). Matched by the physical key
 *     (`event.code`), because Option changes `event.key` on macOS, and refused
 *     for AltGr, which Windows reports as Ctrl+Alt. Acted on only while
 *     VITE_CHAT_BROWSER_DOCK is on and the page passes the actions.
 */

const CHAT_COMPOSER_SELECTOR = '.verevon-dashboard-textarea'

/**
 * The chat page's key bindings, one row each. The command palette and the shortcut sheet take their
 * hotkey strings from here (chat-commands.ts), and matchChatShortcut matches these same combos, so a
 * change to a row reaches the sheet with it. `stopRun` is the double-Escape stop, which createDoubleEscape
 * matches instead of matchChatShortcut.
 */
export const CHAT_SHORTCUTS = {
  newChat: 'mod+shift+o',
  focusComposer: 'mod+i',
  threadPrev: 'mod+shift+arrowup',
  threadNext: 'mod+shift+arrowdown',
  browserDockToggle: 'mod+alt+b',
  browserDockCycle: 'mod+alt+l',
  stopRun: 'escape escape',
} as const

export type ChatShortcutEventLike = {
  altKey: boolean
  ctrlKey: boolean
  /** The physical key (`KeyB`), when the event carries one. */
  code?: string
  key: string
  metaKey: boolean
  shiftKey: boolean
  /** Reports AltGr on Windows, which arrives as Ctrl+Alt. */
  getModifierState?: (key: string) => boolean
}

export type ChatShortcutTargetLike = {
  classList?: { contains: (name: string) => boolean }
  /** Finds the nearest ancestor (or the target itself) that matches a selector. */
  closest?: (selector: string) => unknown
  isContentEditable?: boolean
  tagName?: string
}

export type ChatShortcutDirection = 'down' | 'up'

export type ChatShortcutAction =
  | { type: 'cycle-browser-dock' }
  | { type: 'dismiss' }
  | { type: 'focus-composer' }
  | { type: 'new-chat' }
  | { type: 'select-thread'; direction: ChatShortcutDirection }
  | { type: 'stop-run' }
  | { type: 'toggle-browser-dock' }

/**
 * The physical key of the event, read from `code` when the event carries one. `key` is the fallback
 * for an event without a code (a synthetic one): Option changes `key` on macOS, not `code`.
 */
function isPhysicalKey(event: ChatShortcutEventLike, code: string, key: string): boolean {
  return event.code ? event.code === code : event.key.toLowerCase() === key
}

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
 * The combos are the rows of CHAT_SHORTCUTS.
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

  // The browser-dock chords read the physical key (see isPhysicalKey). AltGr is refused: on Windows it
  // arrives as Ctrl+Alt and types characters on several keyboard layouts.
  if (mod && event.altKey && !event.shiftKey && !event.getModifierState?.('AltGraph')) {
    if (isPhysicalKey(event, 'KeyB', 'b')) {
      return typingElsewhere ? null : { type: 'toggle-browser-dock' }
    }
    if (isPhysicalKey(event, 'KeyL', 'l')) {
      return typingElsewhere ? null : { type: 'cycle-browser-dock' }
    }
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
 * Keyboard-first approval card (E3.2-b, features/browser/approval). Only keys
 * that arrive inside the card reach this matcher: the card listens on its own
 * root element, so these combos never hijack the composer or the page.
 *
 *   - Cmd/Ctrl+Enter            -> approve (refused while redirect text is typed,
 *                                  so a stray combo cannot discard the text).
 *   - Cmd/Ctrl+Shift+Enter      -> reject; with redirect text it submits the
 *                                  redirect (a reject that carries the text).
 *   - Enter in the redirect box -> submit the redirect, when it has text.
 *                                  Shift+Enter keeps its newline.
 *   - Escape in the redirect box -> clear the redirect text.
 *
 * A bare Enter on a focused button keeps the button's own activation.
 * IME composition (isComposing) is always ignored.
 */

export const APPROVAL_REDIRECT_SELECTOR = '.verevon-browser-approval__redirect'

export type ApprovalShortcutEventLike = ChatShortcutEventLike & {
  isComposing?: boolean
}

export type ApprovalShortcutAction =
  | { type: 'approve' }
  | { type: 'reject' }
  | { type: 'submit-redirect' }
  | { type: 'clear-redirect' }

export type ApprovalShortcutContext = {
  /** The redirect text currently typed in the card ('' when none). */
  redirectText: string
}

export function isApprovalRedirectTarget(target: ChatShortcutTargetLike | null | undefined): boolean {
  return Boolean(target?.classList?.contains('verevon-browser-approval__redirect'))
}

export function matchApprovalShortcut(
  event: ApprovalShortcutEventLike,
  target: ChatShortcutTargetLike | null | undefined,
  context: ApprovalShortcutContext,
): ApprovalShortcutAction | null {
  if (event.isComposing || event.altKey) return null
  const inRedirect = isApprovalRedirectTarget(target)
  const hasText = context.redirectText.trim().length > 0
  const mod = event.metaKey || event.ctrlKey

  if (event.key === 'Escape') {
    return inRedirect ? { type: 'clear-redirect' } : null
  }

  // Any other editable (a field outside the redirect box) keeps its keys.
  if (isEditableTarget(target) && !inRedirect) return null

  if (event.key !== 'Enter') return null

  if (mod && event.shiftKey) {
    return hasText ? { type: 'submit-redirect' } : { type: 'reject' }
  }
  if (mod) {
    return hasText ? null : { type: 'approve' }
  }
  if (event.shiftKey) return null
  return inRedirect && hasText ? { type: 'submit-redirect' } : null
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

/**
 * Moves focus into the chat composer. The keydown and the command palette both run this, so the two
 * cannot disagree about what the shortcut does.
 */
export function focusChatComposer(): void {
  document.querySelector<HTMLTextAreaElement>(CHAT_COMPOSER_SELECTOR)?.focus()
}

/**
 * Blurs the composer when it has focus, which is what a bare Escape does on this page. The single-press
 * dismiss and a double-Escape that stops nothing both run it.
 */
function blurChatComposer(): void {
  const active = document.activeElement as HTMLElement | null
  if (active && isChatComposerTarget(active)) active.blur()
}

/**
 * Selects the sidebar thread one step up or down from the active one. Returns the id it selected, or
 * null when there is none (the keydown then leaves the key to the browser). Shared by the keydown and
 * the command palette.
 */
export function selectAdjacentChatThread(direction: ChatShortcutDirection): string | null {
  const nextId = nextThreadIdForDirection(readChatThreadHistory(), readActiveChatThreadId(), direction)
  if (nextId) selectChatThread(nextId)
  return nextId
}

/**
 * Whether the double-Escape stop is on for this build (F-t3-077). `VITE_CHAT_DOUBLE_ESCAPE_STOP` counts only as
 * the literal "true", so an unset variable is off and Escape keeps its single-press blur.
 */
export const chatDoubleEscapeStopEnabled = (): boolean => import.meta.env.VITE_CHAT_DOUBLE_ESCAPE_STOP === 'true'

export type DoubleEscapeEventLike = ChatShortcutEventLike & {
  /** True when an open surface (an autocomplete, a popover) already took the press. */
  defaultPrevented?: boolean
  isComposing?: boolean
  /** True for an auto-repeat of a held key: holding Escape is one press, not two. */
  repeat?: boolean
}

/** Feeds one keydown to the double-Escape stop. Returns stop-run on the second press, and null otherwise. */
export type DoubleEscape = (
  event: DoubleEscapeEventLike,
  target: ChatShortcutTargetLike | null | undefined,
  now?: number,
) => ChatShortcutAction | null

/**
 * An Escape that another surface owns: an open menu or dialog closes on it, and a field other than the
 * composer keeps it (a search box clears on it). Such a press neither counts toward a stop nor starts one.
 */
function isEscapeOwnedElsewhere(target: ChatShortcutTargetLike | null | undefined): boolean {
  if (isEditableTarget(target) && !isChatComposerTarget(target)) return true
  return Boolean(target?.closest?.('[role="dialog"], [role="menu"]'))
}

/**
 * The double-Escape stop (F-t3-077). A stateful companion to matchChatShortcut: create one per page and give
 * it every keydown. The second bare Escape within `windowMs` of the first returns stop-run. Any other key
 * starts the count over, and so does an Escape that is modified, already taken by an open surface, or owned
 * elsewhere. A held key's repeats and the Escape of an IME composition are ignored. Off unless
 * VITE_CHAT_DOUBLE_ESCAPE_STOP is "true".
 */
export function createDoubleEscape(windowMs = 600): DoubleEscape {
  let firstPressAt: number | null = null
  return (event, target, now = Date.now()) => {
    if (!chatDoubleEscapeStopEnabled()) return null
    if (event.key !== 'Escape') {
      firstPressAt = null
      return null
    }
    if (event.repeat || event.isComposing) return null
    const modified = event.altKey || event.ctrlKey || event.metaKey || event.shiftKey
    if (modified || event.defaultPrevented || isEscapeOwnedElsewhere(target)) {
      firstPressAt = null
      return null
    }
    if (firstPressAt === null || now - firstPressAt > windowMs) {
      firstPressAt = now
      return null
    }
    firstPressAt = null
    return { type: 'stop-run' }
  }
}

export interface UseChatShortcutsOptions {
  /**
   * Starts a new chat. Passed in by the caller rather than imported directly
   * from use-chat-controller.ts, whose `startNewChat` already carries the
   * thread-reset logic (clearActiveChatThreadId + resetChatState) — this hook
   * only decides *when* to call it.
   */
  startNewChat: () => void
  /**
   * Shows or hides the browser dock beside a live run (WS9.G). Acted on only while
   * VITE_CHAT_BROWSER_DOCK is on, so a page that does not pass it keeps today's keys.
   */
  toggleBrowserDock?: () => void
  /** Moves the browser dock to its next layout (WS9.G). The same flag rule applies. */
  cycleBrowserDock?: () => void
  /**
   * Stops the streaming answer on a double Escape (F-t3-077). Acted on only while VITE_CHAT_DOUBLE_ESCAPE_STOP is
   * "true". The page decides whether there is a run to stop, and returns true only when it stopped one. Otherwise
   * the second Escape is an ordinary Escape.
   */
  stopRun?: () => boolean
}

/**
 * Registers the chat page's keyboard shortcuts as a single document-level
 * keydown listener, cleaned up on unmount.
 */
export function useChatShortcuts(options: UseChatShortcutsOptions): void {
  createEffect(
    () => undefined,
    () => {
      const doubleEscape = createDoubleEscape()
      const handleKeyDown = (event: KeyboardEvent) => {
        const target = event.target as HTMLElement | null
        // The double-Escape stop sees every key, so any other key starts its count over. On the second press it
        // takes the Escape; its stop-run case blurs the composer when that stopped nothing.
        const action = doubleEscape(event, target) ?? matchChatShortcut(event, target)
        if (!action) return

        switch (action.type) {
          case 'new-chat': {
            event.preventDefault()
            options.startNewChat()
            break
          }
          case 'focus-composer': {
            event.preventDefault()
            focusChatComposer()
            break
          }
          case 'select-thread': {
            // Left to the browser when there is no thread to move to (Shift+Arrow keeps its native selection).
            if (selectAdjacentChatThread(action.direction)) event.preventDefault()
            break
          }
          case 'stop-run': {
            if (options.stopRun?.()) break
            // Nothing was stopped, so this Escape is an ordinary one and blurs the composer as the first press did.
            blurChatComposer()
            break
          }
          case 'toggle-browser-dock': {
            const toggle = options.toggleBrowserDock
            if (!toggle || !isChatBrowserDockEnabled()) break
            event.preventDefault()
            toggle()
            break
          }
          case 'cycle-browser-dock': {
            const cycle = options.cycleBrowserDock
            if (!cycle || !isChatBrowserDockEnabled()) break
            event.preventDefault()
            cycle()
            break
          }
          case 'dismiss': {
            blurChatComposer()
            break
          }
        }
      }

      document.addEventListener('keydown', handleKeyDown)
      return () => document.removeEventListener('keydown', handleKeyDown)
    },
  )
}
