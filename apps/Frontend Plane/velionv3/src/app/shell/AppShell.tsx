import { useLocation } from '@solidjs/router'
import { onMount, Show, type JSX } from 'solid-js'
import { QueryProvider } from '@/app/providers/QueryProvider'
import { CoreShell } from '@/features/core/components/CoreShell'
import { I18nProvider } from '@/shared/i18n'
import { loadSession } from '@/shared/session/session-store'

export function AppShell(props: { children?: JSX.Element }) {
  const location = useLocation()
  const isStandaloneSurface = () =>
    location.pathname.startsWith('/onboarding') ||
    location.pathname.startsWith('/login') ||
    location.pathname.startsWith('/auth')

  // Load the session once for the whole app; route guards read the store.
  onMount(() => {
    void loadSession()
  })

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
