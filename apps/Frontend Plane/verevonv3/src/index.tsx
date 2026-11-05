/* @refresh reload */
import { render } from '@solidjs/web'
import App from '@/app/App'
import { installChunkRecovery } from '@/app/chunk-recovery'
import { startVersionCheck } from '@/shared/build/version-check'
import { RegionBoundary } from '@/shared/ui/resilience/RegionBoundary'
import 'katex/dist/katex.min.css'
import { shouldEnableSceneMode } from '@/demo/scene-mode'
import '@/styles/external-fonts.css'
import '@/styles/global.css'

const root = document.getElementById('root')

// Before render: a chunk that fails to load during the first route must be caught too.
installChunkRecovery()

const boot = () => {
  render(
    () => (
      <RegionBoundary region="root">
        <App />
      </RegionBoundary>
    ),
    root!,
  )

  // The boot watchdog (WS07-S2) waits for this flag; it means the first render ran.
  ;(window as unknown as { __verevonBoot?: boolean }).__verevonBoot = true
  document.getElementById('verevon-boot-failure')?.remove()

  // Stale-tab check (WS07-S2): compares /version.json with this build; shows the banner, never reloads.
  startVersionCheck()
}

// Product-scene runtime (plans/product-scenes/CONTRACT.md). Compiled in only
// when VITE_SCENE_MODE=1 on the development server and a /scene/<id> path; a
// normal build contains none of src/demo.
if (import.meta.env.DEV && shouldEnableSceneMode(import.meta.env.VITE_SCENE_MODE, true, window.location.pathname)) {
  void import('@/demo/install').then((runtime) => {
    runtime.installSceneRuntime()
    boot()
  })
} else {
  boot()
}
