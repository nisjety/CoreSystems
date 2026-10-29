import { For, Show, createEffect, createSignal, createUniqueId, onCleanup } from 'solid-js'
import { localeDateTime, useI18n } from '@/shared/i18n'
import { loadWebVitals, syncWebVitals, type VitalsEnvironment, type WebVitalsReport } from '@/shared/api/marketing/web-vitals-client'
import type { DiscoveryContext } from '@/shared/api/marketing/discovery-client'
import './web-vitals-panel.css'
import './pulse-dashboard.css'

type Props = { websiteId: string; site: string; revision: number; canManageIntegrations?: boolean }
/** Read-only aggregate rendering never starts tracking or imports samples automatically. */
export function WebVitalsPanel(props: Props) {
  const i18n = useI18n(), id = createUniqueId()
  const [environment, setEnvironment] = createSignal<VitalsEnvironment>('production')
  const [report, setReport] = createSignal<WebVitalsReport | null>(null), [loading, setLoading] = createSignal(true)
  const [saving, setSaving] = createSignal(false), [error, setError] = createSignal(''), [refresh, setRefresh] = createSignal(0)
  const context = (): DiscoveryContext => ({ websiteId: props.websiteId, site: props.site, websiteRevision: props.revision })
  const allowed = () => props.canManageIntegrations ?? false
  let generation = 0, controller: AbortController | undefined
  const number = (value: number | null, digits = 0) => value === null ? '—' : new Intl.NumberFormat(localeDateTime(i18n.locale()), { maximumFractionDigits: digits }).format(value)
  const stamp = (value: string) => `${new Intl.DateTimeFormat(localeDateTime(i18n.locale()), { dateStyle: 'medium', timeStyle: 'short', timeZone: 'UTC' }).format(new Date(value))} UTC`
  async function read(selected: DiscoveryContext, selectedEnvironment: VitalsEnvironment, current: AbortController, version: number, sync = false) {
    try {
      const value = await (sync ? syncWebVitals : loadWebVitals)(selected, selectedEnvironment, current.signal)
      if (current.signal.aborted || version !== generation || sync && !allowed()) return
      setReport(value); setError('')
    } catch {
      if (!current.signal.aborted && version === generation) { setReport(null); setError(i18n.tr('Nettlesermålinger kunne ikke lastes. Prøv å oppdatere.', 'Browser measurements could not be loaded. Try refreshing.')) }
    } finally { if (!current.signal.aborted && version === generation) { setLoading(false); setSaving(false) } }
  }
  createEffect(() => {
    const selected = context(), selectedEnvironment = environment(), version = ++generation
    refresh(); allowed(); controller?.abort()
    const current = new AbortController(); controller = current
    setReport(null); setLoading(true); setSaving(false); setError('')
    void read(selected, selectedEnvironment, current, version)
    onCleanup(() => current.abort())
  })
  const sync = () => {
    if (!allowed() || loading() || saving() || !controller) return
    setSaving(true); setError('')
    void read(context(), environment(), controller, generation, true)
  }
  return <section class="web-vitals-panel pulse-dashboard" aria-labelledby={`${id}-title`} aria-busy={loading() || saving()} data-testid="web-vitals-panel">
    <header><div><span>{i18n.tr('Førsteparts nettlesermålinger', 'First-party browser measurements')}</span><h2 id={`${id}-title`}>Web Vitals</h2><p>{i18n.tr('Samtykkebaserte nettlesermålinger, ikke CrUX eller Lighthouse. Siste syv dager etter mottakstid, UTC; uavhengig av valgt kvartal.', 'Consented browser measurements, not CrUX or Lighthouse. Last seven days by receipt time, UTC; independent of the selected quarter.')}</p></div><div class="web-vitals-panel__controls"><label for={`${id}-environment`}>{i18n.tr('Målemiljø', 'Measurement environment')}<select id={`${id}-environment`} value={environment()} onChange={(event) => setEnvironment(event.currentTarget.value as VitalsEnvironment)}><option value="production">{i18n.tr('Produksjon', 'Production')}</option><option value="development">{i18n.tr('Utvikling · localhost', 'Development · localhost')}</option></select></label><button type="button" disabled={loading() || saving()} onClick={() => setRefresh((value) => value + 1)}>{i18n.tr('Oppdater nettlesermålinger', 'Refresh browser measurements')}</button><Show when={allowed()}><button type="button" disabled={loading() || saving()} onClick={sync}>{i18n.tr('Hent fra nettstedet', 'Import from website')}</button></Show></div></header>
    <Show when={environment() === 'development'}><p class="web-vitals-panel__notice">{i18n.tr('Utvikling · localhost-tester, ikke produksjonstrafikk. Disse målingene blandes aldri med produksjon.', 'Development · localhost tests, not production traffic. These measurements are never mixed with production.')}</p></Show>
    <Show when={loading()}><p role="status">{i18n.tr('Laster nettlesermålinger…', 'Loading browser measurements…')}</p></Show><Show when={saving()}><p role="status">{i18n.tr('Henter målinger fra nettstedet…', 'Importing website measurements…')}</p></Show><Show when={error()}><p role="alert">{error()}</p></Show>
    <Show when={report()}>{(value) => <>
      <p role="status">{{ not_configured: i18n.tr('Nettstedets serverbaserte måleeksport er ikke konfigurert.', 'The website’s server-side measurement export is not configured.'), awaiting_samples: i18n.tr('Venter på samtykkebaserte nettlesermålinger', 'Waiting for consented browser measurements'), connected: i18n.tr('Nettlesermålinger tilgjengelige', 'Browser measurements available') }[value().status]}</p>
      <Show when={value().reportingOrigin}><p>{i18n.tr('Måleopphav', 'Reporting origin')}: <code>{value().reportingOrigin}</code></p></Show>
      <p>{value().window.startDate} – {value().window.endDate} · UTC · {number(value().sampleCount)} {i18n.tr('målinger', 'metric measurements')}</p>
      <div class="web-vitals-panel__groups"><For each={value().groups}>{(group) => <article><h3>{group.viewport === 'narrow' ? i18n.tr('Smalt visningsvindu', 'Narrow viewport') : i18n.tr('Bredt visningsvindu', 'Wide viewport')}</h3><dl><For each={['lcp', 'inp', 'cls'] as const}>{(key) => <div><dt>{key.toUpperCase()} · p75</dt><dd data-testid={`vitals-${group.viewport}-${key}`}>{number(group.metrics[key].p75, key === 'cls' ? 4 : 0)}<Show when={group.metrics[key].p75 !== null && key !== 'cls'}><small> ms</small></Show></dd><p>{number(group.metrics[key].count)} {i18n.tr('målinger', 'measurements')}</p><Show when={group.metrics[key].count > 0 && group.metrics[key].count < 20}><p class="web-vitals-panel__notice">{i18n.tr('Begrenset utvalg: færre enn 20 målinger', 'Limited sample: fewer than 20 measurements')}</p></Show></div>}</For></dl></article>}</For></div>
      <p>{i18n.tr('Sist lest', 'As of')}: {stamp(value().asOf)}</p>
    </>}</Show>
    <footer>{i18n.tr('Antall er metriske målinger, ikke besøkende eller sidevisninger. Gruppene følger visningsvinduets bredde, ikke fysisk enhet. Manglende målinger vises som —; ingen samlet bestått-vurdering beregnes. Lesing starter ikke sporing. Nettstedet krever statistikk-samtykke før innhenting.', 'Counts are metric measurements, not visitors or pageviews. Groups follow viewport width, not physical device. Missing measurements appear as —; no overall passing assessment is calculated. Reading does not start tracking. Website collection requires Statistics consent.')}</footer>
  </section>
}
