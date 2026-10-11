/* @refresh reload */
import { render } from '@solidjs/web'
import App from '@/app/App'
import 'katex/dist/katex.min.css'
import '@/styles/global.css'

const root = document.getElementById('root')

const boot = () => render(() => <App />, root!)

// Product-scene runtime (plans/product-scenes/CONTRACT.md). Compiled in only
// when VITE_SCENE_MODE=1; a normal build contains none of src/demo.
if (import.meta.env.VITE_SCENE_MODE === '1') {
  void import('@/demo/install').then((runtime) => {
    runtime.installSceneRuntime()
    boot()
  })
} else {
  boot()
}
