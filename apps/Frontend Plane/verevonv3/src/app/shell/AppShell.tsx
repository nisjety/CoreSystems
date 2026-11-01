import { useLocation } from '@solidjs/router'
import { createEffect, Errored, Show, type Accessor } from 'solid-js'
import type { JSX } from '@solidjs/web'
import { chunkReloadStarted, isChunkLoadError } from '@/app/chunk-recovery'
import { QueryProvider } from '@/app/providers/QueryProvider'
import { StaleBuildNotice } from '@/app/shell/StaleBuildNotice'
import { CoreShell } from '@/features/core/components/CoreShell'
import { SESSION_EXPIRED_EVENT } from '@/shared/api/http'
import { I18nProvider, translateApiError, useI18n } from '@/shared/i18n'
import { ClientContractBanner } from '@/shared/ui/resilience/ClientContractBanner'
import { NewVersionBanner } from '@/shared/ui/resilience/NewVersionBanner'
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
        {/* Outside the boundary, so a failed route cannot take the stale-build notice down with it. */}
        <Errored
          fallback={(error) => {
            // A boundary with a fallback does not log the error itself, so the console keeps it here.
            console.error(error())
            return <RoutedFailure error={error} />
          }}
        >
          <Show when={!isStandaloneSurface()} fallback={<>{props.children}</>}>
            <CoreShell>{props.children}</CoreShell>
          </Show>
        </Errored>
        <StaleBuildNotice />
        <NewVersionBanner />
        <ClientContractBanner />
      </QueryProvider>
    </I18nProvider>
  )
}

/**
 * What a failed route shows in place of its content. A chunk that a newer build
 * replaced is the stale-build notice's job, so nothing is drawn for it. Any other
 * failure gets the app's generic error copy rather than a blank page. Nothing is
 * drawn either while the automatic reload is under way, because the page is leaving.
 */
function RoutedFailure(props: { error: Accessor<unknown> }) {
  const i18n = useI18n()
  return (
    <Show when={!chunkReloadStarted() && !isChunkLoadError(props.error())}>
      <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
        {translateApiError(props.error(), i18n.tr)}
      </p>
    </Show>
  )
}
