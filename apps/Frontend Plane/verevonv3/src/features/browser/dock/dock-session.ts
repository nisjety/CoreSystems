import { onCleanup } from 'solid-js'
import {
  createBrowserSessionController,
  type BrowserSessionController,
} from '@/features/browser/session/use-browser-session-controller'
import { useLocalRetentionAllowed } from '@/features/dashboard/home/local-retention'
import { isChatBrowserDockEnabled } from './browser-dock-state'

// The interactive browser session of one chat page (WS9.H). The dock pane drives it through the same
// controller the Knowledge composer will use (WS9.F), so a session opened here is the person's own human
// session. The agent never uses it, and the pane never sends page content to the model.
//
// One controller lives for as long as the chat page does. Hiding or showing the dock does not touch it,
// so an open page stays open across dock toggles. The page closes it when the chat page unmounts.
//
// Zero retention comes from two places: the chat thread on screen is temporary, or the workspace's
// retention verdict denies local copies. Either makes every session this page opens ZDR. A ZDR session
// never carries a profile: the controller enforces that, and this page passes no profile at all.

export type DockBrowserSessionOptions = {
  /** The active workspace. A null or empty value means no session can start. */
  orgId: () => string | null | undefined
  /** Whether the chat thread on screen is temporary (zero retention). */
  threadTemporary: () => boolean
  /** The caller's copy translator (Norwegian and English). */
  tr?: (noText: string, enText: string) => string
  /** Overrides the build flag. Tests use it; the page leaves it out. */
  enabled?: boolean
}

export type DockBrowserSession = {
  /** The page's controller. Null while the dock is off in this build, so nothing is created. */
  controller: BrowserSessionController | null
  /** Zero retention for this page right now. Read at each open and by the omnibox. */
  zdr: () => boolean
}

/**
 * The dock's browser session for the chat page that calls it. Call it once, in the page's own setup.
 * The controller is created here and closed when the page unmounts.
 */
export function useDockBrowserSession(options: DockBrowserSessionOptions): DockBrowserSession {
  const enabled = options.enabled ?? isChatBrowserDockEnabled()
  // The retention listener is only attached when the dock can run at all.
  const retentionAllowed = enabled ? useLocalRetentionAllowed() : () => true
  const zdr = () => options.threadTemporary() || !retentionAllowed()
  const controller = enabled
    ? createBrowserSessionController({
        orgId: options.orgId,
        zdr,
        ...(options.tr ? { tr: options.tr } : {}),
      })
    : null
  onCleanup(() => {
    if (!controller) return
    // Closes whatever is open, then detaches the page-hide close. Deferred, because stopping a session
    // writes signals and the page is being torn down. Both calls are safe to repeat.
    queueMicrotask(() => {
      void controller.close().finally(() => controller.dispose())
    })
  })
  return { controller, zdr }
}
