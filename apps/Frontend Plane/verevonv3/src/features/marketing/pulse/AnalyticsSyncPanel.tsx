import { For, Show, createEffect, createSignal, createUniqueId, onCleanup, onMount } from 'solid-js'
import { localeDateTime, useI18n } from '@/shared/i18n'
import { loadAnalyticsSync, requestAnalyticsSync, setAnalyticsSyncEnabled, type AnalyticsSyncContext, type AnalyticsSyncReason, type AnalyticsSyncState } from '@/shared/api/marketing/analytics-sync-client'
import type { PulseQuarter } from '@/shared/api/marketing/pulse-client'
import './analytics-sync-panel.css'

type Props = { websiteId: string; site: string; revision: number; quarter: PulseQuarter; canManageIntegrations?: boolean; onSettled?: () => void }
const active = (value: AnalyticsSyncState | null) => value?.status === 'queued' || value?.status === 'running'

/** GET reads metadata only. Collection and scheduling require an explicit admin action. */
export function AnalyticsSyncPanel(props: Props) {
  const i18n = useI18n(), id = createUniqueId()
  const [snapshot, setSnapshot] = createSignal<AnalyticsSyncState | null>(null)
  const [loading, setLoading] = createSignal(true), [saving, setSaving] = createSignal(false)
  const [error, setError] = createSignal(''), [pollExpired, setPollExpired] = createSignal(false)
  const [refresh, setRefresh] = createSignal(0)
  const [now, setNow] = createSignal(Date.now())
  onMount(() => { const clock = setInterval(() => setNow(Date.now()), 30000); onCleanup(() => clearInterval(clock)) })
  const stale = (lastSuccessAt?: string) => !!lastSuccessAt && now() >= Date.parse(lastSuccessAt) + 6 * 3600000
  const reconnect = (reason?: AnalyticsSyncReason) => !!reason && ['authorization_required', 'requires_consent', 'not_connected', 'not_found', 'needs_selection', 'not_configured'].includes(reason)
  const allowed = () => props.canManageIntegrations ?? false
  const context = (): AnalyticsSyncContext => ({ websiteId: props.websiteId, site: props.site, websiteRevision: props.revision, quarter: props.quarter })
  let generation = 0, polls = 0, timer: ReturnType<typeof setTimeout> | undefined, controller: AbortController | undefined
  let activeJobs: ReadonlySet<string> = new Set(), previousContext = ''
  const stamp = (value?: string) => value ? `${new Intl.DateTimeFormat(localeDateTime(i18n.locale()), { dateStyle: 'medium', timeStyle: 'short', timeZone: 'UTC' }).format(new Date(value))} UTC` : '—'
  const label = (value: AnalyticsSyncState['status']) => ({ idle: i18n.tr('Ikke hentet', 'Not collected'), queued: i18n.tr('I kø', 'Queued'), running: i18n.tr('Henter data', 'Collecting data'), complete: i18n.tr('Alle kilder svarte', 'All sources responded'), partial: i18n.tr('Delvis tilgjengelig', 'Partially available'), failed: i18n.tr('Ingen kilder tilgjengelige', 'No sources available') })[value]
  const reason = (value: AnalyticsSyncReason) => ({ not_configured: i18n.tr('Ikke konfigurert', 'Not configured'), not_connected: i18n.tr('Ikke tilkoblet', 'Not connected'), requires_consent: i18n.tr('Trenger samtykke', 'Consent required'), needs_selection: i18n.tr('Velg prosjekt', 'Project selection required'), not_found: i18n.tr('Ingen passende tilgang funnet', 'No matching access found'), source_unavailable: i18n.tr('Kilden er utilgjengelig', 'Source unavailable'), quota_limited: i18n.tr('Kvotegrense nådd', 'Quota limited'), authorization_required: i18n.tr('Tilgang må fornyes', 'Authorization needs attention'), property_unverified: i18n.tr('Nettstedet er ikke verifisert', 'Website not verified'), pending_verification: i18n.tr('Venter på verifisering', 'Pending verification'), no_data: i18n.tr('Ingen rapportdata', 'No report data'), timeout: i18n.tr('Tidsavbrudd', 'Timed out'), interrupted: i18n.tr('Avbrutt', 'Interrupted'), context_changed: i18n.tr('Nettstedet er endret', 'Website context changed') })[value]

  function accept(value: AnalyticsSyncState) {
    const previous = snapshot()
    setSnapshot(value)
    // A scheduled job can finish between metadata polls. Refresh reports once
    // for its new receipt, while an initial saved receipt remains a pure read.
    if (!active(value) && previous?.jobId && previous.jobId !== value.jobId && !active(previous)) props.onSettled?.()
    if (!value.jobId) return
    if (active(value)) activeJobs = new Set([...activeJobs, value.jobId])
    else if (activeJobs.has(value.jobId)) {
      activeJobs = new Set([...activeJobs].filter((job) => job !== value.jobId))
      props.onSettled?.()
    }
  }
  function schedule(selected: AnalyticsSyncContext, version: number, signal: AbortSignal, value: AnalyticsSyncState) {
    if (!active(value)) {
      timer = setTimeout(() => { void read(selected, version, signal) }, 60000)
      return
    }
    if (polls >= 90) { setPollExpired(true); return }
    timer = setTimeout(() => { polls++; void read(selected, version, signal) }, 2000)
  }
  async function read(selected: AnalyticsSyncContext, version: number, signal: AbortSignal) {
    try {
      const value = await loadAnalyticsSync(selected, signal)
      if (signal.aborted || generation !== version) return
      accept(value); setError(''); schedule(selected, version, signal, value)
    } catch {
      if (!signal.aborted && generation === version) { setSnapshot(null); setError(i18n.tr('Innhentingsstatus kunne ikke lastes. Oppdater status før du prøver igjen.', 'Collection status could not be loaded. Refresh status before retrying.')) }
    } finally { if (!signal.aborted && generation === version) setLoading(false) }
  }
  createEffect(() => {
    const selected = context(), key = `${selected.websiteId}|${selected.site}|${selected.websiteRevision}|${selected.quarter}|${allowed()}`, version = ++generation
    refresh()
    if (key !== previousContext) { activeJobs = new Set(); previousContext = key }
    controller?.abort(); clearTimeout(timer); polls = 0
    const current = new AbortController(); controller = current
    setSnapshot(null); setLoading(true); setSaving(false); setError(''); setPollExpired(false)
    void read(selected, version, current.signal)
    onCleanup(() => { current.abort(); clearTimeout(timer) })
  })
  async function change(enabled?: boolean) {
    if (!allowed() || loading() || saving() || error() || !snapshot() || (enabled === undefined && active(snapshot()))) return
    const current = controller, selected = context(), version = generation
    if (!current) return
    setSaving(true); setError(''); setPollExpired(false); polls = 0; clearTimeout(timer)
    try {
      const value = enabled === undefined ? await requestAnalyticsSync(selected, current.signal) : await setAnalyticsSyncEnabled(selected, enabled, current.signal)
      if (current.signal.aborted || generation !== version || !allowed()) return
      accept(value); schedule(selected, version, current.signal, value)
    } catch (cause) {
      if (!current.signal.aborted && generation === version) setError(cause instanceof Error && cause.message === 'analytics_sync_conflict' ? i18n.tr('Nettstedet er endret. Oppdater status.', 'Website changed. Refresh status.') : i18n.tr('Endringen kunne ikke bekreftes. Oppdater status før du prøver igjen.', 'The change could not be confirmed. Refresh status before retrying.'))
    } finally { if (!current.signal.aborted && generation === version) setSaving(false) }
  }

  return <section class="analytics-sync" aria-labelledby={`${id}-title`} data-testid="analytics-sync-panel" aria-busy={loading() || saving()}>
    <header><div><span>{i18n.tr('Lagrede analysedata', 'Saved analytics')}</span><h2 id={`${id}-title`}>{i18n.tr('Innhenting og bakgrunnsoppdatering', 'Collection and background sync')}</h2><p>{i18n.tr('Search Console og GA4 følger valgt periode.', 'Search Console and GA4 follow the selected period.')} {props.quarter} · {i18n.tr('Clarity har sitt eget rullerende 72-timersvindu.', 'Clarity keeps its own rolling 72-hour window.')}</p></div><button type="button" disabled={loading() || saving()} onClick={() => setRefresh((value) => value + 1)}>{i18n.tr('Oppdater innhentingsstatus', 'Refresh collection status')}</button></header>
    <Show when={loading()}><p role="status">{i18n.tr('Laster innhentingsstatus…', 'Loading collection status…')}</p></Show>
    <Show when={error()}><p role="alert">{error()}</p></Show>
    <Show when={snapshot()}>{(value) => <><p role="status"><strong>{label(value().status)}</strong> · {value().enabled ? i18n.tr('Bakgrunnsoppdatering aktiv', 'Background sync enabled') : i18n.tr('Bakgrunnsoppdatering pauset', 'Background sync paused')}</p><ul><For each={value().steps}>{(step) => <li><strong>{{ gsc: 'Google Search Console', ga4: 'Google Analytics 4', clarity: 'Microsoft Clarity' }[step.key]}</strong><span>{step.status === 'connected' ? i18n.tr('Rapport hentet', 'Report collected') : step.status === 'unavailable' ? reason(step.reason!) : step.status === 'queued' ? (value().status === 'idle' ? i18n.tr('Ikke startet', 'Not started') : i18n.tr('I kø', 'Queued')) : i18n.tr('Henter', 'Collecting')}</span><small>{i18n.tr('Sist vellykket', 'Last successful sync')}: {stamp(step.lastSuccessAt)}</small><Show when={stale(step.lastSuccessAt)}><span class="analytics-sync__stale">{i18n.tr('Utdaterte data · ny innhenting trengs', 'Stale data · collection needed')}</span></Show><small>{i18n.tr('Siste forsøk', 'Last attempt')}: {stamp(step.observedAt)}</small><Show when={allowed() && reconnect(step.reason)}><a href="/settings/integrations">{i18n.tr('Administrer tilkobling', 'Manage connection')} ↗</a></Show></li>}</For></ul><Show when={value().nextSyncAt}><p>{i18n.tr('Neste forsøk', 'Next attempt')}: {stamp(value().nextSyncAt)}</p></Show><Show when={value().jobId}><details><summary>{i18n.tr('Innhentingskvittering', 'Collection receipt')}</summary><code>{value().jobId}</code><p>{i18n.tr('Fullført', 'Completed')}: {stamp(value().completedAt)} · {i18n.tr('Neste innhenting', 'Next collection')}: {stamp(value().nextSyncAt)} · {i18n.tr('Snapshot utløper', 'Snapshot expires')}: {stamp(value().expiresAt)}</p></details></Show></>}</Show>
    <Show when={pollExpired()}><p role="status">{i18n.tr('Automatisk statusoppdatering stoppet etter tre minutter. Serverjobben kan fortsatt kjøre. Oppdater status manuelt.', 'Automatic status refresh stopped after three minutes. The server job may still be running. Refresh status manually.')}</p></Show>
    <Show when={allowed()} fallback={<p>{i18n.tr('En administrator kan konfigurere innhenting. Du har lesetilgang.', 'An administrator can configure collection. You have read-only access.')}</p>}><div class="analytics-sync__controls"><button type="button" disabled={loading() || saving() || !!error() || !snapshot() || active(snapshot())} onClick={() => void change()}>{i18n.tr('Hent nå og aktiver bakgrunnsoppdatering', 'Collect now and enable background sync')}</button><Show when={snapshot()?.status !== 'idle' && snapshot()}><button type="button" disabled={loading() || saving() || !!error()} onClick={() => void change(!snapshot()!.enabled)}>{snapshot()?.enabled ? i18n.tr('Pause bakgrunnsoppdatering', 'Pause background sync') : i18n.tr('Gjenoppta bakgrunnsoppdatering', 'Resume background sync')}</button></Show></div></Show>
    <footer>{i18n.tr('Bakgrunnsinnhenting skjer minst hver sjette time; eldre kildedata oppdateres tidligere. Lagrede rapporter kan brukes i opptil seks timer, med tilgangskontroll ved lesing. Tilbaketrukket tilgang som serveren kjenner til, skjuler data med en gang; endringer hos leverandøren kan først oppdages ved neste kontroll. Pausing sletter ikke lagrede rapporter eller opphever kontotilgang. Rapportlesing starter ikke innhenting; egne sammenligningsanalyser kan fortsatt hente data direkte.', 'Background collection runs at least every six hours; older source data is refreshed sooner. Saved snapshots may be used for up to six hours, with access checks on reads. Revocation known to the server hides data immediately; provider-side changes may only be detected at the next check. Pausing does not delete saved snapshots or revoke account access. Reading dashboard reports does not start collection; separate comparison analyses may still fetch data directly.')}</footer>
  </section>
}
