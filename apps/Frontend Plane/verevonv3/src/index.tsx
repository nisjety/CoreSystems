/* @refresh reload */
import { render } from '@solidjs/web'
import App from '@/app/App'
import 'katex/dist/katex.min.css'
import { shouldEnableSceneMode } from '@/demo/scene-mode'
import '@/styles/external-fonts.css'
import '@/styles/global.css'

const root = document.getElementById('root')

const boot = () => render(() => <App />, root!)

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
