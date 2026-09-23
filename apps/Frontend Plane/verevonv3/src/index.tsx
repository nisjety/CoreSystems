/* @refresh reload */
import { render } from '@solidjs/web'
import App from '@/app/App'
import 'katex/dist/katex.min.css'
import '@/styles/global.css'

const root = document.getElementById('root')

render(() => <App />, root!)
