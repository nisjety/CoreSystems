/* @refresh reload */
import { render } from '@solidjs/web'
import App from '@/app/App'
import { installChunkRecovery } from '@/app/chunk-recovery'
import 'katex/dist/katex.min.css'
import '@/styles/global.css'

const root = document.getElementById('root')

// Before render: a chunk that fails to load during the first route must be caught too.
installChunkRecovery()

render(() => <App />, root!)
