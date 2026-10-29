import { Navigate, Route, Router } from '@solidjs/router'
import { render } from 'solid-js/web'
import { I18nProvider } from '@/shared/i18n'
import { StudioDemoShell } from './StudioDemoShell'
import '@/styles/global.css'
import './studio-demo.css'

const root = document.getElementById('root')

if (!root) {
  throw new Error('Studio demo root element is missing')
}

render(() => (
  <I18nProvider>
    <Router root={(props) => <>{props.children}</>}>
      <Route path="/" component={() => <Navigate href="/studio/canvas" />} />
      <Route path="/*all" component={StudioDemoShell} />
    </Router>
  </I18nProvider>
), root)
