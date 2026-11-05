import { createEffect, createSignal, Show } from 'solid-js'
import { IngestionsMonitoringView } from '@/features/ingestions/components/IngestionsMonitoringView'
import { listMonitors, type ChangeMonitor } from '@/shared/api/monitoring-client'
import { translateApiError, useI18n } from '@/shared/i18n'
import '@/features/ingestions/components/ingestions.css'

/** Uses the same live monitor schedules and checks as the ingestion workspace. */
export function KnowledgeMonitoring(props: { orgId: string }) {
  const i18n = useI18n()
  const [monitors, setMonitors] = createSignal<ChangeMonitor[] | null>(null)
  const [error, setError] = createSignal<string | null>(null)
  let generation = 0
  async function refresh() {
    const current = ++generation
    setError(null)
    try {
      const result = await listMonitors()
      if (current === generation) setMonitors(result)
    } catch (reason) {
      if (current === generation) {
        setMonitors(null)
        setError(translateApiError(reason, i18n.tr, { no: 'Kunne ikke laste overvåkede sider.', en: 'Could not load monitored pages.' }))
      }
    }
  }
  createEffect(() => props.orgId, (orgId) => { if (orgId) void refresh() })
  return <>
    <Show when={error()}>{message => <p role="alert">{message()}</p>}</Show>
    <Show when={props.orgId} fallback={<p>{i18n.tr('Kobler til arbeidsområdet ...', 'Connecting to the workspace ...')}</p>}>
      <IngestionsMonitoringView orgId={props.orgId} monitors={monitors()} onMonitorsChanged={refresh} showWebhooks={false} />
    </Show>
  </>
}
