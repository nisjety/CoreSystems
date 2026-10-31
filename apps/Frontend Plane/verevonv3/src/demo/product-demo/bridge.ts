/**
 * Messages across the iframe boundary (protocol v1, src/demo/live/shared-types.ts).
 * The demo sends only `ready`, `escape` and `error`, which carry no visitor data, and it
 * obeys only its parent window. The target origin is '*' because an opaque-origin
 * document cannot name the page that embeds it. The host verifies event.source itself.
 */
import type { DemoToSiteMessage, SiteToDemoMessage } from '@/demo/live/shared-types'

const SOURCE = 'verevon-demo'

type DemoState = 'active' | 'inactive' | 'paused'

export type BridgeOptions = {
  /** Reloads the demo document. A reset is a fresh visit, so nothing survives it. Defaults to a page reload. */
  reload?: () => void
}

function isFramed(): boolean {
  return window.parent !== window
}

function postToSite(message: DemoToSiteMessage): void {
  if (!isFramed()) return
  window.parent.postMessage(message, '*')
}

function setDemoState(state: DemoState): void {
  document.documentElement.dataset.demoState = state
  // Framed on the site, the demo is inert (no pointer, no focus) unless the visitor has activated it
  // and the stage is showing it.
  document.documentElement.inert = isFramed() && state !== 'active'
}

/** What the host has asked for. `resume` only lets an activated demo run again; it never activates one. */
const host = { activated: false }

function receiveSiteMessage(event: MessageEvent, reload: () => void): void {
  if (!isFramed() || event.source !== window.parent) return
  const data = event.data as Partial<SiteToDemoMessage> | null
  if (!data || data.source !== SOURCE || data.v !== 1) return
  switch (data.type) {
    case 'activate':
      host.activated = true
      setDemoState('active')
      return
    case 'resume':
      setDemoState(host.activated ? 'active' : 'inactive')
      return
    case 'deactivate':
      host.activated = false
      setDemoState('inactive')
      return
    case 'pause':
      setDemoState(host.activated ? 'paused' : 'inactive')
      return
    case 'reset':
      reload()
      return
    default:
      return
  }
}

/**
 * Starts listening for the host and reporting errors. Call before the app boots. Returns a function
 * that stops listening (the unit tests call it between cases).
 */
export function startBridge(options: BridgeOptions = {}): () => void {
  const reload = options.reload ?? (() => window.location.reload())
  const onMessage = (event: MessageEvent): void => receiveSiteMessage(event, reload)
  const onKeydown = (event: KeyboardEvent): void => {
    // Escape that a component already used (closing a menu or dialog) must not also release the stage.
    if (event.key === 'Escape' && !event.defaultPrevented) postToSite({ source: SOURCE, v: 1, type: 'escape' })
  }
  // The host learns the kind of failure only. The message and stack stay inside the demo.
  const onError = (): void => postToSite({ source: SOURCE, v: 1, type: 'error', message: 'uncaught error' })
  const onRejection = (): void => postToSite({ source: SOURCE, v: 1, type: 'error', message: 'unhandled rejection' })
  host.activated = false
  setDemoState(isFramed() ? 'inactive' : 'active')
  window.addEventListener('message', onMessage)
  window.addEventListener('keydown', onKeydown)
  window.addEventListener('error', onError)
  window.addEventListener('unhandledrejection', onRejection)
  return () => {
    window.removeEventListener('message', onMessage)
    window.removeEventListener('keydown', onKeydown)
    window.removeEventListener('error', onError)
    window.removeEventListener('unhandledrejection', onRejection)
  }
}

/** Tells the host the demo is on screen and ready to receive messages. */
export function announceReady(): void {
  postToSite({ source: SOURCE, v: 1, type: 'ready' })
}
