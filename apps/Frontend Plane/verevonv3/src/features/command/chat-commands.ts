/**
 * The chat page's shortcuts as registered commands (F-t3-077). The shortcut sheet lists every row and the
 * command palette lists the action rows. The keys stay in use-chat-shortcuts: every command here is
 * `handledByPage`, so a press runs once, from that listener, and the palette runs the same callbacks
 * through the same helpers.
 *
 * ChatPage registers these on mount, behind `commandPaletteEnabled()`, and calls the disposer on
 * cleanup. Presentation only: nothing here grants anything.
 */

import { isChatBrowserDockEnabled } from '@/features/browser/dock/browser-dock-state'
import {
  CHAT_SHORTCUTS,
  focusChatComposer,
  selectAdjacentChatThread,
  type UseChatShortcutsOptions,
} from '@/features/chat/lib/use-chat-shortcuts'
import { registerCommands } from './command-registry'

/** What the chat commands run: the callbacks ChatPage passes to useChatShortcuts, unchanged. */
export type ChatCommandActions = UseChatShortcutsOptions

/**
 * Registers the chat commands and returns the disposer that removes exactly these. `when` holds only
 * while this registration is live, so a descriptor that outlives the page never shows.
 */
export function registerChatCommands(actions: ChatCommandActions): () => void {
  let active = true
  const when = () => active
  const dispose = registerCommands([
    {
      id: 'chat.new',
      labelNo: 'Ny samtale',
      labelEn: 'New chat',
      group: 'action',
      hotkey: CHAT_SHORTCUTS.newChat,
      kind: 'ui',
      handledByPage: true,
      when,
      run: () => actions.startNewChat(),
    },
    {
      id: 'chat.focus_composer',
      labelNo: 'Hopp til meldingsfeltet',
      labelEn: 'Go to the message field',
      group: 'action',
      hotkey: CHAT_SHORTCUTS.focusComposer,
      kind: 'ui',
      handledByPage: true,
      when,
      run: () => {
        // The palette hands focus back to its opener as it closes, so the composer takes it a frame later.
        requestAnimationFrame(() => focusChatComposer())
      },
    },
    {
      id: 'chat.thread_prev',
      labelNo: 'Forrige samtale',
      labelEn: 'Previous conversation',
      group: 'navigate',
      hotkey: CHAT_SHORTCUTS.threadPrev,
      kind: 'ui',
      handledByPage: true,
      when,
      run: () => {
        selectAdjacentChatThread('up')
      },
    },
    {
      id: 'chat.thread_next',
      labelNo: 'Neste samtale',
      labelEn: 'Next conversation',
      group: 'navigate',
      hotkey: CHAT_SHORTCUTS.threadNext,
      kind: 'ui',
      handledByPage: true,
      when,
      run: () => {
        selectAdjacentChatThread('down')
      },
    },
    {
      id: 'chat.browser_dock_toggle',
      labelNo: 'Vis eller skjul nettleseren',
      labelEn: 'Show or hide the browser',
      group: 'action',
      hotkey: CHAT_SHORTCUTS.browserDockToggle,
      kind: 'ui',
      handledByPage: true,
      when: () => active && isChatBrowserDockEnabled(),
      run: () => actions.toggleBrowserDock?.(),
    },
    {
      id: 'chat.browser_dock_cycle',
      labelNo: 'Endre plassering av nettleseren',
      labelEn: 'Change the browser position',
      group: 'action',
      hotkey: CHAT_SHORTCUTS.browserDockCycle,
      kind: 'ui',
      handledByPage: true,
      when: () => active && isChatBrowserDockEnabled(),
      run: () => actions.cycleBrowserDock?.(),
    },
  ])
  return () => {
    active = false
    dispose()
  }
}
