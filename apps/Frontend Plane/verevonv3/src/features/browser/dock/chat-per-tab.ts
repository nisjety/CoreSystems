import type { BrowserDockChatPerTab } from './browser-dock-state'

// Chat per browser tab (WS9.H). When the browser's active tab changes, the chat either stays where it
// is (`keep`, the default) or moves to the chat that belongs to the tab it switched to (`new`).
//
// A tab's chat is the thread that was on screen while that tab was active. The map that holds it lives
// in memory for one chat page only. It is never written to storage, so a reload starts every browser tab
// with no remembered chat. It holds browser tab ids and chat thread ids: never a URL, a title or page text.

export type ChatPerTabRule = BrowserDockChatPerTab

/** The rule a person gets until they choose another. Under it the chat never moves on its own. */
export const CHAT_PER_TAB_DEFAULT: ChatPerTabRule = 'keep'

/** Browser tab id to chat thread id, for one chat page. Memory only. */
export type ChatThreadByTab = {
  /** The thread remembered for a browser tab, or null when the tab has none. */
  threadFor: (tabKey: string) => string | null
  /** Remembers the thread on screen for a browser tab. An empty tab id or thread id is ignored. */
  remember: (tabKey: string, threadId: string | null | undefined) => void
  /** Forgets one browser tab, for example when it was closed. */
  forget: (tabKey: string) => void
  /** Forgets every browser tab. */
  clear: () => void
  /** How many browser tabs have a remembered thread. */
  size: () => number
}

/** A new, empty map for one chat page. Nothing is read from or written to storage. */
export function createChatThreadByTab(): ChatThreadByTab {
  const threads = new Map<string, string>()
  return {
    threadFor: (tabKey) => threads.get(tabKey) ?? null,
    remember: (tabKey, threadId) => {
      if (!tabKey || !threadId) return
      threads.set(tabKey, threadId)
    },
    forget: (tabKey) => {
      threads.delete(tabKey)
    },
    clear: () => {
      threads.clear()
    },
    size: () => threads.size,
  }
}

/** What a browser tab switch does to the chat. */
export type ChatTabSwitchPlan =
  | { kind: 'stay' }
  | { kind: 'new-thread' }
  | { kind: 'select-thread'; threadId: string }

/**
 * Whether a switch from one browser tab to another moves the chat under `rule`. Only `new` moves it, and
 * only between two known, different tabs. The first tab of a session has no previous tab, so it never moves
 * the chat.
 */
export function shouldStartNewThread(
  previousTab: string | null | undefined,
  nextTab: string | null | undefined,
  rule: ChatPerTabRule,
): boolean {
  if (rule !== 'new') return false
  if (!previousTab || !nextTab) return false
  return previousTab !== nextTab
}

/**
 * The chat's move for a browser tab switch. Under `keep` the chat stays where it is. Under `new` a switch
 * goes to the tab's own remembered thread when it has one, and starts a new thread when it has none.
 */
export function planChatForTabSwitch(input: {
  previousTab: string | null | undefined
  nextTab: string | null | undefined
  rule: ChatPerTabRule
  threadFor: (tabKey: string) => string | null
}): ChatTabSwitchPlan {
  const { previousTab, nextTab, rule, threadFor } = input
  if (!nextTab || !shouldStartNewThread(previousTab, nextTab, rule)) return { kind: 'stay' }
  const remembered = threadFor(nextTab)
  return remembered ? { kind: 'select-thread', threadId: remembered } : { kind: 'new-thread' }
}
