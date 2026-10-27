/* @refresh reload */
import { render } from '@solidjs/web'
import App from '@/app/App'
import { installChunkRecovery } from '@/app/chunk-recovery'
import { RegionBoundary } from '@/shared/ui/resilience/RegionBoundary'
import 'katex/dist/katex.min.css'
import '@/styles/global.css'

const root = document.getElementById('root')

// Before render: a chunk that fails to load during the first route must be caught too.
installChunkRecovery()

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
