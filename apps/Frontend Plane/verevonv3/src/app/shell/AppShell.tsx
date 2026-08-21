import { useLocation } from '@solidjs/router'
import { createEffect, Show } from 'solid-js'
import type { JSX } from '@solidjs/web'
import { QueryProvider } from '@/app/providers/QueryProvider'
import { CoreShell } from '@/features/core/components/CoreShell'
import { SESSION_EXPIRED_EVENT } from '@/shared/api/http'
import { I18nProvider } from '@/shared/i18n'
import { clearSession, loadSession } from '@/shared/session/session-store'

export function AppShell(props: { children?: JSX.Element }) {
  const location = useLocation()
  const isStandaloneSurface = () =>
    location.pathname.startsWith('/onboarding') ||
    location.pathname.startsWith('/accept-invitation/') ||
    location.pathname.startsWith('/login') ||
    location.pathname.startsWith('/auth')

  // Load the session once for the whole app; route guards read the store.
  createEffect(
    () => undefined,
    () => {
      void loadSession()
      const handleSessionExpired = () => clearSession()
      window.addEventListener(SESSION_EXPIRED_EVENT, handleSessionExpired)
      return () => window.removeEventListener(SESSION_EXPIRED_EVENT, handleSessionExpired)
    },
  )

  return (
    <I18nProvider>
      <QueryProvider>
        <Show when={!isStandaloneSurface()} fallback={<>{props.children}</>}>
          <CoreShell>{props.children}</CoreShell>
        </Show>
      </QueryProvider>
    </I18nProvider>
  )
}
