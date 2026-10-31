import { createRouter, memoryHistory, useNavigate, useParams } from '@solidjs/router'
import { createEffect, createSignal, lazy, Show } from 'solid-js'
import type { JSX } from '@solidjs/web'
import { AppShell } from '@/app/shell/AppShell'
import { demoAllows } from '@/demo/allowlist'
import { getSession } from '@/shared/session/session-store'
import { internalPath, isUnofferedPath } from './guard-policy'

// The routes the demo offers, as in src/app/App.tsx: the dashboard and the chat, with its threads.
// Every other route ends at UnavailablePage. Nothing here touches the sign-in, onboarding or settings routes.
const DashboardPage = lazy(() => import('@/features/dashboard/components/DashboardPage'))
const ChatPage = lazy(() => import('@/features/chat/components/ChatPage'))

const [notice, setNotice] = createSignal('')
let noticeTimer: number | undefined

function showNotice(text: string): void {
  setNotice(text)
  window.clearTimeout(noticeTimer)
  noticeTimer = window.setTimeout(() => setNotice(''), 2600)
}

function SessionFallback() {
  return (
    <Show
      when={getSession().status !== 'unauthenticated'}
      fallback={
        <section class="demo-page" aria-labelledby="demo-start-title">
          <h1 id="demo-start-title">Demoen kunne ikke starte</h1>
          <p>Last siden på nytt for å prøve igjen.</p>
          <button type="button" class="demo-button" onClick={() => window.location.reload()}>
            Last på nytt
          </button>
        </section>
      }
    >
      <div class="session-loading" role="status" aria-live="polite" aria-busy="true">
        <span class="session-loading__spinner" aria-hidden="true" />
        <span class="session-loading__label">Laster …</span>
      </div>
    </Show>
  )
}

/**
 * The product's session gate (RequireAuth in src/app/App.tsx). The demo has no sign-in
 * page, so an unauthenticated session shows a notice instead of redirecting to /login.
 */
function DemoSession(props: { children?: JSX.Element }) {
  return (
    <Show
      when={getSession().status === 'authenticated' && getSession().onboardingStatus === 'COMPLETED'}
      fallback={<SessionFallback />}
    >
      {props.children}
    </Show>
  )
}

function UnavailablePage() {
  const navigate = useNavigate()
  return (
    <section class="demo-page" aria-labelledby="demo-unavailable-title">
      <h1 id="demo-unavailable-title">Ikke tilgjengelig i demoen</h1>
      <p>Denne delen av Verevon er ikke med i demoen. Gå til Hjem for å prøve oversikten og chatten.</p>
      <button type="button" class="demo-button" onClick={() => navigate('/dashboard')}>
        Tilbake til Hjem
      </button>
    </section>
  )
}

/**
 * A chat route for one thread. `/chat/:threadId` also matches the product's utility pages (findings,
 * reports, routines), which the demo does not offer: they show the unavailable page.
 */
function ChatThreadRoute() {
  const params = useParams<{ threadId?: string }>()
  return (
    <Show when={!isUnofferedPath(`/chat/${params.threadId ?? ''}`)} fallback={<UnavailablePage />}>
      <ChatPage />
    </Show>
  )
}

/**
 * The demo's link policy, in the capture phase so it runs before the router. A link that
 * leaves the demo, or a route the demo does not offer, is refused with a notice. A link the
 * demo offers is routed in memory: a plain anchor would otherwise load the real site inside
 * the frame. Anchors marked `link` are claimed by the router itself. Call it inside the
 * router's render prop, so every route, including the unavailable page, is covered.
 */
function useDemoLinks(): void {
  const navigate = useNavigate()
  // Solid 2 effects split into a tracked compute and an effect that returns its cleanup (as in AppShell).
  createEffect(
    () => undefined,
    () => {
      const onClick = (event: MouseEvent) => {
        if (event.button !== 0 || !(event.target instanceof Element)) return
        const anchor = event.target.closest('a[href]')
        if (!anchor) return
        const href = anchor.getAttribute('href') ?? ''
        if (href.startsWith('#')) return
        const path = internalPath(href, document.baseURI)
        if (path === null || !demoAllows(path) || isUnofferedPath(path)) {
          event.preventDefault()
          showNotice('Denne lenken er ikke med i demoen.')
          return
        }
        if (anchor.hasAttribute('link')) return
        event.preventDefault()
        navigate(path)
      }
      window.addEventListener('click', onClick, true)
      return () => window.removeEventListener('click', onClick, true)
    },
  )
}

/** The persistent label and the notice line. main.tsx mounts it beside the app, outside the product's layout. */
export function DemoChrome() {
  return (
    <>
      <div class="demo-ribbon">Demo · fiktive data</div>
      <div class="demo-notice" role="status" aria-live="polite" data-visible={notice() ? 'true' : 'false'}>
        {notice()}
      </div>
    </>
  )
}

const Router = createRouter({
  routes: [
    {
      path: '/',
      component: (props: { children?: JSX.Element }) => <DemoSession>{props.children}</DemoSession>,
      children: [
        { path: '/', component: DashboardPage },
        { path: '/dashboard', component: DashboardPage },
        { path: '/chat', component: ChatPage },
        { path: '/chat/:threadId', component: ChatThreadRoute },
      ],
    },
    { path: '*404', component: UnavailablePage },
  ],
  history: memoryHistory('/dashboard'),
  explicitLinks: true,
})

/** One root element, as in src/app/App.tsx. The chrome is mounted separately by main.tsx. */
export default function DemoApp() {
  return (
    <Router>
      {(props) => {
        useDemoLinks()
        return <AppShell {...props} />
      }}
    </Router>
  )
}
