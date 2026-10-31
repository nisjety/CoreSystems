import { setWebsitePreferenceScope } from '@/shared/api/marketing/website-client'
import { useLocation } from '@solidjs/router'
import { VisibilityToolsPage } from './VisibilityToolsPage'
import { visibilityToolViews } from '@/shared/actions/visibility-tools'
import { MarketingPulse as Visibility } from '@/features/marketing/pulse/MarketingPulse'
import { createMemo, Show } from 'solid-js'
import { useI18n } from '@/shared/i18n'
import { getSession } from '@/shared/session/session-store'

/** Authenticated shell entry point for the shared Visibility Core workspace. */
export default function VisibilityPage() {
  const session = getSession()
  const location = useLocation()
  const view = () => visibilityToolViews.find(v => location.pathname === `/engagement/visibility/${v}`)
  const i18n = useI18n()
  const workspaceIdentity = createMemo(() => (
    session.status === 'authenticated' && session.user?.id && session.activeOrg?.id
      ? JSON.stringify([session.user.id, session.activeOrg.id])
      : null
  ))
  // A platform administrator is not automatically an administrator of this org.
  const canManageIntegrations = () => ['owner', 'admin'].includes(session.activeOrg?.role.trim().toLowerCase() ?? '')
  const canConfigureMonitoring = () => ['owner', 'admin', 'member'].includes(session.activeOrg?.role.trim().toLowerCase() ?? '')

  return (
    <Show
      when={workspaceIdentity()}
      keyed
      fallback={<p role="status">{i18n.tr('Velg et arbeidsområde for å åpne Synlighet.', 'Select a workspace to open Visibility.')}</p>}
    >
      {(identity) => {setWebsitePreferenceScope(identity);return Boolean(identity) && <Show when={view()} fallback={<Visibility canManageIntegrations={canManageIntegrations()} canConfigureMonitoring={canConfigureMonitoring()} />}>{value => <VisibilityToolsPage view={value()} canWrite={canConfigureMonitoring()}/>}</Show>}}
    </Show>
  )
}
