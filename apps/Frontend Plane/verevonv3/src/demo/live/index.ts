/**
 * `installDemoLive`: the interactive demo's runtime (stage 04 of the product
 * loop). Call it after `installFixedClock` and before the app renders.
 *
 * It installs the in-page fetch interceptor (src/demo/install.ts) with the live
 * hook. The engine answers the chat, thread, run and approval endpoints in
 * memory, the home world's routes answer the dashboard, and the shared fixtures
 * answer the rest. Nothing leaves the page: every `/api/*` request is answered
 * here, and the only requests are for the bundle's own assets.
 */
import './demo.css'
import { installSceneRuntime, setLiveHook } from '../install'
import type { DemoLiveHandle, DemoLiveOptions } from './shared-types'
import { createBrowserScheduler, createStreamChannelFactory } from './channel'
import { createResponder } from './responder'
import { createLiveRoutes } from './routes'
import { scenarioById, scenarios, seededTranscripts } from './scenarios'
import { createThreadStore } from './thread-store'
import { installLiveSuggestions } from './suggestions-live'

const DEMO_CLASS = 'verevon-demo-live'
/** Keys the product writes for the conversations it shows. Cleared by `reset()`, as on a fresh visit. */
const PRODUCT_LOCAL_KEYS = ['verevon.chat.threadId', 'verevon.chat.threadHistory.v1', 'verevon.chat.threadTranscripts.v1']
const PRODUCT_SESSION_KEYS = ['verevon.chat.pendingLaunch']

export function installDemoLive(options: DemoLiveOptions): DemoLiveHandle {
  installSceneRuntime()
  const scheduler = createBrowserScheduler()
  const startedAt = scheduler.now()
  const wallNow = () => options.now + (scheduler.now() - startedAt)
  const isoNow = () => new Date(wallNow()).toISOString()

  const store = createThreadStore(options.threadSeed, seededTranscripts(options.threadSeed))
  const engine = createResponder({
    store,
    scenarios,
    scheduler,
    openChannel: createStreamChannelFactory(scheduler),
    wallNow,
  })
  const routes = [...createLiveRoutes({ store, engine, isoNow }), ...options.routes]
  setLiveHook({ routes })
  document.documentElement.classList.add(DEMO_CLASS)
  const suggestions = installLiveSuggestions({ scenarios })

  return {
    dispose() {
      setLiveHook(null)
      suggestions.dispose()
      engine.dispose()
      document.documentElement.classList.remove(DEMO_CLASS)
    },
    reset() {
      engine.reset()
      clearProductStorage()
    },
    startScenario(id) {
      const scenario = scenarioById(id)
      if (scenario) suggestions.start(scenario)
    },
  }
}

/** Forgets the product's own copies of the conversations. Storage may be unavailable (an opaque origin has none). */
function clearProductStorage(): void {
  try {
    forget(window.localStorage, PRODUCT_LOCAL_KEYS)
  } catch {
    // No local storage in this context: there is nothing to forget.
  }
  try {
    forget(window.sessionStorage, PRODUCT_SESSION_KEYS)
  } catch {
    // No session storage in this context: there is nothing to forget.
  }
}

function forget(storage: Storage, keys: readonly string[]): void {
  for (const key of keys) {
    try {
      storage.removeItem(key)
    } catch {
      // A blocked key stays; the thread list no longer lists it either way.
    }
  }
}

export type { DemoLiveHandle, DemoLiveOptions } from './shared-types'
