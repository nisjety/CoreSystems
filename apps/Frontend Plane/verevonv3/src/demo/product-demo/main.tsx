// The guard is the first import on purpose. It installs the fixed clock, in-memory storage
// and the network policy before any other module, including the app, is evaluated.
import './guard'
import '@/styles/global.css'
import 'katex/dist/katex.min.css'
import './fonts.css'
import '@/demo/home.css'
import './demo.css'
import { render } from '@solidjs/web'
import { buildHomeWorld } from '@/demo/fixtures/fjordform-home'
import { installDemoLive } from '@/demo/live'
import { DEMO_NOW_ISO } from '@/demo/live/shared-types'
import { announceReady, startBridge } from './bridge'
import { startDemoLocalisation } from './localise'

startBridge()
void boot()

async function boot(): Promise<void> {
  const root = document.getElementById('root')
  if (!root) throw new Error('product-demo: the #root element is missing')

  // The same fictional instant the guard's clock starts from.
  const now = Date.parse(DEMO_NOW_ISO)
  const world = buildHomeWorld(now)
  installDemoLive({ now, routes: world.routes, threadSeed: world.threadSeed })
  for (const [key, value] of Object.entries(world.storageSeed)) {
    window.localStorage.setItem(key, value)
  }
  document.documentElement.lang = 'nb'
  // The scope of the demo-only CSS in src/demo/home.css (hides the feedback widget, Språk, Logg ut, Utseende).
  document.documentElement.classList.add('verevon-demo')

  // Imported only now: no app module reads storage or the network before it is seeded.
  const { default: DemoApp, DemoChrome } = await import('./DemoApp')
  render(() => <DemoApp />, root)
  const chrome = document.createElement('div')
  chrome.className = 'demo-chrome'
  document.body.append(chrome)
  render(() => <DemoChrome />, chrome)
  startDemoLocalisation(root)
  announceReady()
}
