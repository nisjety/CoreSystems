import { untrack } from 'solid-js'
import { createRouter, useNavigate } from '@solidjs/router'
import { createEffect } from 'solid-js'
import { render } from '@solidjs/web'
import { I18nProvider } from '@/shared/i18n'
import { StudioDemoShell } from './StudioDemoShell'
import '@/styles/global.css'
import './studio-demo.css'

const root = document.getElementById('root')

if (!root) {
  throw new Error('Studio demo root element is missing')
}

const Router = createRouter({ routes: [
  { path: '/', component: () => { const navigate = useNavigate(); createEffect(() => undefined, () => untrack(() => navigate('/studio/canvas', { replace: true }))); return null } },
  { path: '/*all', component: StudioDemoShell },
] })
render(() => <I18nProvider><Router /></I18nProvider>, root)
