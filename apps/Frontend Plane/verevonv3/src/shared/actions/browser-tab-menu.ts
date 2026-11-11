import { z } from 'zod'

// The Browser tab context menu (WS9 tab strip, behind VITE_BROWSER_TAB_MENU=true).
// This is a presentation contract. It names the four menu actions and decides which
// items the menu offers and which of them can be chosen. It performs nothing itself:
// the chrome hands each chosen action to the handler that already owns the operation
// (select, new tab, close), and the gateway and Quarry keep the authority they had.
// It is standalone, with no action-registry row.

// The gateway checks the wire shape of a tab id; here only an empty or oversized id is refused.
const tabIdSchema = z.string().min(1).max(128)

// Only http and https pages can be opened outside Verevon, the same rule the chrome applies to a tab's url.
const outsideUrlSchema = z.string().min(1).max(2048).refine((value) => {
  try {
    const { protocol } = new URL(value)
    return protocol === 'http:' || protocol === 'https:'
  } catch {
    return false
  }
}, 'url must use http or https')

export const browserTabMenuActionSchema = z.discriminatedUnion('type', [
  z.object({ type: z.literal('browser.tab.select'), tabId: tabIdSchema }),
  z.object({ type: z.literal('browser.tab.new') }),
  z.object({ type: z.literal('browser.tab.close'), tabId: tabIdSchema }),
  z.object({ type: z.literal('browser.tab.open_outside'), url: outsideUrlSchema }),
])
export type BrowserTabMenuAction = z.infer<typeof browserTabMenuActionSchema>
export type BrowserTabMenuOutsideAction = Extract<BrowserTabMenuAction, { type: 'browser.tab.open_outside' }>

/** The outside-link action when the action is one, otherwise null. Narrows the url for the link's href. */
export function browserTabMenuOutsideAction(action: BrowserTabMenuAction): BrowserTabMenuOutsideAction | null {
  return action.type === 'browser.tab.open_outside' ? action : null
}

export type BrowserTabMenuItem = {
  /** What choosing the item does. The chrome parses it through browserTabMenuActionSchema before it acts. */
  action: BrowserTabMenuAction
  /** The label the menu shows (Norwegian, like the rest of the chrome). */
  label: string
  /** A disabled item stays in the menu so its shape does not shift, and it cannot be chosen. */
  disabled: boolean
}

export type BrowserTabMenuInput = {
  tab: { tabId: string; active: boolean }
  /** The tabs in the session. Close is offered only when there is more than one. */
  tabCount: number
  /** The browser is busy with another action. */
  busy: boolean
  /** Controls are unavailable right now (not a live session, no session id, or an action in flight). */
  controlsDisabled: boolean
  /** The chrome can close a tab. Without a close handler, Close is not offered. */
  hasCloseHandler: boolean
  /** The tab's page as an http(s) url, or null when it cannot be opened outside Verevon. */
  outsideUrl: string | null
}

/**
 * The menu items for one tab, in display order: Vis fane, Ny fane, Lukk fane (only with a close
 * handler and more than one tab) and Åpne utenfor Verevon (only for an http(s) page).
 */
export function browserTabMenuItems(input: BrowserTabMenuInput): BrowserTabMenuItem[] {
  const { tab, tabCount, busy, controlsDisabled, hasCloseHandler, outsideUrl } = input
  const items: BrowserTabMenuItem[] = [
    { action: { type: 'browser.tab.select', tabId: tab.tabId }, label: 'Vis fane', disabled: tab.active || busy },
    { action: { type: 'browser.tab.new' }, label: 'Ny fane', disabled: controlsDisabled },
  ]
  if (hasCloseHandler && tabCount > 1) {
    items.push({ action: { type: 'browser.tab.close', tabId: tab.tabId }, label: 'Lukk fane', disabled: controlsDisabled })
  }
  // The url is parsed here too, so a link with an unsafe href is never offered, whatever the caller passes.
  const outside = outsideUrl === null
    ? null
    : browserTabMenuActionSchema.safeParse({ type: 'browser.tab.open_outside', url: outsideUrl })
  if (outside?.success) {
    items.push({ action: outside.data, label: 'Åpne utenfor Verevon', disabled: false })
  }
  return items
}
