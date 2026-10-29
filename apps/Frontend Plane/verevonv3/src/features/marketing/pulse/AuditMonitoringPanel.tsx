import { For, Show, createEffect, createSignal, createUniqueId, onCleanup } from 'solid-js'
import { localeDateTime, useI18n } from '@/shared/i18n'
import { configureAuditMonitoring, loadAuditMonitoring, type AuditMonitoring, type MonitoringCadence, type MonitoringReason, type MonitoringStatus } from '@/shared/api/marketing/monitoring-client'
import type { DiscoveryContext } from '@/shared/api/marketing/discovery-client'
import './audit-monitoring-panel.css'

type Props = { websiteId: string; site: string; revision: number; canConfigure?: boolean }

/** Reading this panel never enables monitoring or starts capture. */
export function AuditMonitoringPanel(props: Props) {
  const i18n = useI18n(), id = createUniqueId()
  const [snapshot, setSnapshot] = createSignal<AuditMonitoring | null>(null)
  const [cadence, setCadence] = createSignal<MonitoringCadence>('daily')
  const [loading, setLoading] = createSignal(true), [saving, setSaving] = createSignal(false)
  const [error, setError] = createSignal(''), [notice, setNotice] = createSignal('')
  const [refreshKey, setRefreshKey] = createSignal(0)
  let generation = 0, writeController: AbortController | undefined
  const context = (): DiscoveryContext => ({ websiteId: props.websiteId, site: props.site, websiteRevision: props.revision })
  const canConfigure = () => props.canConfigure ?? true
  const disabled = () => loading() || saving() || !snapshot()
  const stamp = (value: string) => value ? `${new Intl.DateTimeFormat(localeDateTime(i18n.locale()), { dateStyle: 'medium', timeStyle: 'short', timeZone: 'UTC' }).format(new Date(value))} UTC` : '—'
  const statusLabel = (value: MonitoringStatus) => ({ not_configured: i18n.tr('Ikke konfigurert', 'Not configured'), active: i18n.tr('Aktiv', 'Active'), paused: i18n.tr('Satt på pause', 'Paused'), context_changed: i18n.tr('Nettstedet er endret', 'Website context changed'), blocked: i18n.tr('Blokkert', 'Blocked') })[value]
  const outcomeLabel = (value: AuditMonitoring['lastOutcome']) => ({ none: i18n.tr('Ingen kjøring ennå', 'No run yet'), running: i18n.tr('Pågår', 'Running'), complete: i18n.tr('Fullført', 'Complete'), failed: i18n.tr('Mislyktes', 'Failed') })[value]
  const reasonLabel = (value: MonitoringReason) => ({ not_configured: i18n.tr('Ikke konfigurert', 'Not configured'), context_changed: i18n.tr('Nettstedet er endret', 'Website context changed'), capacity_deferred: i18n.tr('Utsatt på grunn av kapasitet', 'Deferred for capacity'), interrupted: i18n.tr('Avbrutt kjøring', 'Interrupted run'), source_unavailable: i18n.tr('Kilden er utilgjengelig', 'Source unavailable'), quota_limited: i18n.tr('Kvotegrense nådd', 'Quota limited'), timeout: i18n.tr('Tidsavbrudd', 'Timed out'), access_denied: i18n.tr('Tilgang avvist', 'Access denied'), policy_blocked: i18n.tr('Blokkert av kildepolicy', 'Blocked by source policy'), invalid_source: i18n.tr('Ugyldige kildebevis', 'Invalid source evidence'), unsupported_content: i18n.tr('Innholdet støttes ikke', 'Unsupported content'), storage_unavailable: i18n.tr('Lagring er utilgjengelig', 'Storage unavailable') })[value]

  createEffect(() => {
    const selected = context(), version = ++generation
    refreshKey()
    const controller = new AbortController()
    writeController?.abort()
    setSnapshot(null); setLoading(true); setSaving(false); setError(''); setNotice('')
    void loadAuditMonitoring(selected, controller.signal).then((value) => {
      if (controller.signal.aborted || version !== generation) return
      setSnapshot(value); setCadence(value.cadence)
    }).catch(() => {
      if (!controller.signal.aborted && version === generation) setError(i18n.tr('Overvåkingsstatus kunne ikke lastes. Oppdater før du endrer tidsplanen.', 'Monitoring status could not be loaded. Refresh before changing the schedule.'))
    }).finally(() => { if (!controller.signal.aborted && version === generation) setLoading(false) })
    onCleanup(() => { controller.abort(); writeController?.abort() })
  })

  async function configure(enabled: boolean) {
    const current = snapshot()
    if (!current || disabled() || !canConfigure()) return
    const version = generation, selected = context(), controller = new AbortController()
    writeController = controller; setSaving(true); setError(''); setNotice('')
    try {
      const value = await configureAuditMonitoring({ ...selected, scheduleRevision: current.revision, enabled, cadence: enabled ? cadence() : current.cadence }, controller.signal)
      if (!controller.signal.aborted && version === generation) {
        setSnapshot(value); setCadence(value.cadence)
        setNotice(i18n.tr('Tidsplanen er lagret. Ingen umiddelbar innhenting startes.', 'Schedule saved. No immediate capture is started.'))
      }
    } catch (cause) {
      if (!controller.signal.aborted && version === generation) setError(cause instanceof Error && cause.message === 'monitoring_conflict'
        ? i18n.tr('Nettstedet eller tidsplanen er endret. Oppdater før du lagrer på nytt.', 'The website or schedule changed. Refresh before saving again.')
        : i18n.tr('Lagringen kunne ikke bekreftes. Oppdater status før du prøver igjen.', 'The save result could not be confirmed. Refresh status before retrying.'))
    } finally { if (!controller.signal.aborted && version === generation) setSaving(false) }
  }

  return <section class="audit-monitoring-panel" aria-labelledby={`${id}-title`} data-testid="audit-monitoring-panel">
    <header><div><span>Marketing Core</span><h2 id={`${id}-title`}>{i18n.tr('Planlagt overvåking', 'Scheduled monitoring')}</h2><p>{i18n.tr('Begrensede nettstedsrevisjoner på en lagret tidsplan.', 'Bounded website audits on a saved schedule.')}</p></div><button type="button" disabled={loading() || saving()} onClick={() => setRefreshKey((value) => value + 1)}>{i18n.tr('Oppdater overvåkingsstatus', 'Refresh monitoring status')}</button></header>
    <Show when={loading()}><p role="status">{i18n.tr('Laster tidsplan…', 'Loading schedule…')}</p></Show>
    <Show when={error()}><p role="alert">{error()}</p></Show><Show when={notice()}><p role="status">{notice()}</p></Show>
    <Show when={snapshot()}>{(value) => <>
      <dl><div><dt>{i18n.tr('Status', 'Status')}</dt><dd>{statusLabel(value().status)}</dd></div><div><dt>{i18n.tr('Neste kjøring · UTC', 'Next run · UTC')}</dt><dd>{stamp(value().nextRunAt)}</dd></div><div><dt>{i18n.tr('Siste resultat', 'Last outcome')}</dt><dd>{outcomeLabel(value().lastOutcome)}</dd></div><div><dt>{i18n.tr('Påfølgende feil', 'Consecutive failures')}</dt><dd>{value().failureCount}</dd></div><div><dt>{i18n.tr('Oppdatert · UTC', 'Updated · UTC')}</dt><dd>{stamp(value().updatedAt)}</dd></div></dl>
      <Show when={value().reason}>{(reason) => <p>{reasonLabel(reason())}</p>}</Show>
      <Show when={value().lastAuditId}><p>{i18n.tr('Siste revisjon', 'Latest audit')}: <code>{value().lastAuditId}</code></p></Show>
      <Show when={value().history.length > 0}><details><summary>{i18n.tr('Planlagte kjøringer', 'Scheduled runs')}</summary><For each={value().history}>{(receipt) => <article class="audit-monitoring-panel__receipt"><strong>{outcomeLabel(receipt.status)}</strong><code>{receipt.auditId}</code><p>{i18n.tr('Planlagt', 'Scheduled')}: {stamp(receipt.scheduledFor)} · {i18n.tr('Startet', 'Started')}: {stamp(receipt.startedAt)} · {i18n.tr('Fullført', 'Completed')}: {stamp(receipt.completedAt)}</p><p>{i18n.tr('Forsøk', 'Attempt')} {receipt.attempt} · {receipt.checkedPages} {i18n.tr('sider kontrollert', 'pages checked')} · {receipt.findingCount} {i18n.tr('funn', 'findings')}</p><Show when={receipt.reason}>{(reason) => <p>{reasonLabel(reason())}</p>}</Show></article>}</For></details></Show>
    </>}</Show>
    <Show when={canConfigure()} fallback={<p>{i18n.tr('Kun lesetilgang. En arbeidsområdemedlem kan endre tidsplanen.', 'Read-only access. A workspace member can configure the schedule.')}</p>}>
      <form onSubmit={(event) => { event.preventDefault(); void configure(true) }}><label for={`${id}-cadence`}>{i18n.tr('Intervall', 'Cadence')}<select id={`${id}-cadence`} value={cadence()} disabled={disabled()} onChange={(event) => setCadence(event.currentTarget.value as MonitoringCadence)}><option value="daily">{i18n.tr('Daglig · hver 24. time', 'Daily · every 24 hours')}</option><option value="weekly">{i18n.tr('Ukentlig · hver 7. dag', 'Weekly · every 7 days')}</option></select></label><button type="submit" disabled={disabled()}>{saving() ? i18n.tr('Lagrer…', 'Saving…') : snapshot()?.enabled ? i18n.tr('Lagre tidsplan', 'Save schedule') : i18n.tr('Aktiver overvåking', 'Enable monitoring')}</button><Show when={snapshot()?.enabled}><button type="button" disabled={disabled()} onClick={() => void configure(false)}>{i18n.tr('Sett overvåking på pause', 'Pause monitoring')}</button></Show></form>
    </Show>
    <footer>{i18n.tr('Første aktivering eller gjenopptakelse planlegger neste kjøring om 24 timer eller 7 dager i forløpt UTC-tid, ikke lokal kalender eller sommertid. Ingen umiddelbar innhenting. Serveren kjører tidsplanen uavhengig av denne siden og oppretter eller fullfører ikke arbeidsoppgaver automatisk.', 'First enable or resume schedules the next run after 24 hours or 7 days of elapsed UTC time, not local calendar or daylight-saving time. No immediate capture. The server runs independently of this page and does not create or close work tasks automatically.')}</footer>
  </section>
}
