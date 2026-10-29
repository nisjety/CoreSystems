/* @refresh reload */
import { render } from 'solid-js/web'
import App from '@/app/App'
import { shouldEnableSceneMode } from '@/demo/scene-mode'
import '@/styles/external-fonts.css'
import '@/styles/global.css'

const root = document.getElementById('root')

const boot = () => render(() => <App />, root!)

// Install fictional API fixtures before rendering the scene-only development route.
if (import.meta.env.DEV && shouldEnableSceneMode(import.meta.env.VITE_SCENE_MODE, true, window.location.pathname)) {
  void import('@/demo/install').then((runtime) => {
    runtime.installSceneRuntime()
    boot()
  })
} else {
  boot()
}
