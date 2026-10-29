import { For, Show, createEffect, createUniqueId } from 'solid-js'
import { localeDateTime, useI18n } from '@/shared/i18n'
import { loadClarity, type ClarityMetrics, type ClarityUXMetrics, type ClaritySnapshot } from '../../../shared/api/marketing/clarity-client.ts'
import { createAnalyticsPanelState } from './analytics-panel-state.ts'
import type { AnalyticsPanelState } from '../../../shared/api/marketing/analytics-boundary.ts'
import './analytics-panels.css'
import { ReportDisclosure } from './ReportDisclosure'
import './pulse-dashboard.css'

type Props = { site: string; websiteId: string; revision: number; refreshKey?: number; onState?: (state: AnalyticsPanelState) => void; onManageWebsite?: () => void; onSnapshot?: (snapshot: ClaritySnapshot | null) => void }
/** Clarity exports one recent project-wide window; it must not follow the GA4 quarter. */
export function ClarityPanel(props: Props) {
  const i18n = useI18n()
  const id = createUniqueId()
  const state = createAnalyticsPanelState(
    () => { void props.refreshKey; return { site: props.site, websiteId: props.websiteId, revision: props.revision } },
    (context, signal) => loadClarity(context.site, context.websiteId, signal),
    (value) => props.onState?.(value),
  )
  const connected = () => state.snapshot()?.status === 'connected'
  createEffect(() => props.onSnapshot?.(state.loading() || state.failed() ? null : state.snapshot()))
  const metrics = () => connected() ? state.snapshot()?.metrics : null
  const number = (value: number | null | undefined, decimals = 0) => value == null ? '—' : new Intl.NumberFormat(localeDateTime(i18n.locale()), { maximumFractionDigits: decimals }).format(value)
  const cards = (): { key: Exclude<keyof ClarityMetrics, 'ux'>; label: string; decimals?: number }[] => [
    { key: 'sessions', label: i18n.tr('Økter', 'Sessions') }, { key: 'users', label: i18n.tr('Brukere', 'Users') },
    { key: 'botSessions', label: i18n.tr('Bot-økter', 'Bot sessions') }, { key: 'pagesPerSession', label: i18n.tr('Sider per økt', 'Pages per session'), decimals: 2 },
  ]
  const uxRows = (): { key: keyof ClarityUXMetrics; label: string; unit: '%' | 's' }[] => [
    { key: 'averageScrollDepth', label: i18n.tr('Gjennomsnittlig rulledybde', 'Average scroll depth'), unit: '%' },
    { key: 'activeSeconds', label: i18n.tr('Aktiv tid per økt', 'Active time per session'), unit: 's' },
    { key: 'totalSeconds', label: i18n.tr('Total tid per økt', 'Total time per session'), unit: 's' },
    { key: 'deadClickPercent', label: i18n.tr('Økter med døde klikk', 'Sessions with dead clicks'), unit: '%' },
    { key: 'rageClickPercent', label: i18n.tr('Økter med frustrasjonsklikk', 'Sessions with rage clicks'), unit: '%' },
    { key: 'quickBackPercent', label: i18n.tr('Økter med rask retur', 'Sessions with quick backs'), unit: '%' },
    { key: 'excessiveScrollPercent', label: i18n.tr('Økter med overdreven rulling', 'Sessions with excessive scrolling'), unit: '%' },
    { key: 'scriptErrorPercent', label: i18n.tr('Økter med JavaScript-feil', 'Sessions with JavaScript errors'), unit: '%' },
    { key: 'errorClickPercent', label: i18n.tr('Økter med feilklikk', 'Sessions with error clicks'), unit: '%' },
  ]
  const projectLink = (view: 'impressions' | 'heatmaps') => `https://clarity.microsoft.com/projects/view/${state.snapshot()?.projectId}/${view}?date=Last%203%20days`
  const message = () => {
    if (state.loading()) return i18n.tr('Laster Clarity…', 'Loading Clarity…')
    if (state.failed()) return i18n.tr('Clarity kunne ikke lastes. Andre Synlighet-paneler påvirkes ikke.', 'Clarity could not be loaded. Other Visibility panels are unaffected.')
    switch (state.snapshot()?.reason) {
      case 'snapshot_pending': case 'snapshot_expired': return i18n.tr('Lagret Clarity-rapport mangler eller er utløpt. Bruk innhentingspanelet for å hente en ny eksport.', 'The saved Clarity snapshot is missing or expired. Use the collection panel to collect a new export.')
      case 'quota_limited': return i18n.tr('Clarity-eksportkvoten er nådd. Prøv innhenting senere; rapportpanelet leser bare lagrede data.', 'The Clarity export quota has been reached. Retry collection later; this report panel only reads saved data.')
      case 'authorization_required': return i18n.tr('Clarity-tilgangen må kontrolleres i Integration Core.', 'Clarity authorization needs attention in Integration Core.')
      case 'not_connected': case 'pending_verification': return i18n.tr('Clarity-prosjektet er ikke verifisert og tilkoblet for dette nettstedet.', 'The Clarity project is not verified and connected for this website.')
      case 'not_configured': return i18n.tr('Clarity-eksport er ikke konfigurert i Integration Core.', 'Clarity export is not configured in Integration Core.')
      case 'no_data': return i18n.tr('Ingen eksporterte rapportdata for dette tidsvinduet.', 'No exported report data for this window.')
      default: return i18n.tr('Clarity-kilden er midlertidig utilgjengelig.', 'The Clarity source is temporarily unavailable.')
    }
  }
  return <section class="analytics-panel pulse-dashboard pulse-dashboard--clarity" aria-labelledby={`${id}-title`} data-testid="clarity-panel" aria-busy={state.loading()}>
    <header class="analytics-panel__header"><div><span class="analytics-panel__eyebrow">Microsoft Clarity</span><h2 id={`${id}-title`}>{i18n.tr('Brukerinnsikt', 'Visitor insights')}</h2><p data-testid="clarity-window">{i18n.tr('Siste 72 timer', 'Last 72 hours')} · UTC</p></div><div class="analytics-panel__controls"><span class="analytics-panel__readonly">{i18n.tr('Kun lesing', 'Read only')}</span><button type="button" disabled={state.loading()} onClick={state.refresh}>{i18n.tr('Oppdater', 'Refresh')}</button></div></header>
    <p class="analytics-panel__scope">{i18n.tr('Eksportert trafikk for hele det tilkoblede Clarity-prosjektet, ikke bare valgt nettadresse.', 'Exported traffic for the whole connected Clarity project, not just the selected URL.')}<Show when={state.snapshot()?.projectId}>{(project) => <span class="analytics-panel__project">{i18n.tr('Prosjekt', 'Project')} <code>{project()}</code></span>}</Show></p>
    <Show when={state.loading() || state.failed() || !connected()}><p role={state.failed() ? 'alert' : 'status'} class="analytics-panel__status">{message()}<Show when={props.onManageWebsite && !state.loading() && !connected() && state.snapshot()?.reason !== 'snapshot_pending' && state.snapshot()?.reason !== 'snapshot_expired'}><button type="button" onClick={() => props.onManageWebsite?.()}>{i18n.tr('Administrer nettsted', 'Manage website')}</button></Show></p></Show>
    <div class="analytics-panel__cards"><For each={cards()}>{(card) => <article><span>{card.label}</span><strong data-testid={`clarity-${card.key}`}>{number(metrics()?.[card.key], card.decimals)}</strong></article>}</For></div>
    <Show when={connected()}>
      <ReportDisclosure source="Microsoft Clarity" title={i18n.tr('Atferd, friksjon og opptak', 'Behavior, friction and recordings')}>
      <div class="analytics-panel__ux">
        <h3>{i18n.tr('Atferd og friksjon', 'Behavior and friction')}</h3>
        <p>{i18n.tr('Observerte prosjektgjennomsnitt og andel økter. Manglende eksportverdier betyr ikke at et problem er fraværende.', 'Observed project averages and percentages of sessions. Missing export values do not mean a problem is absent.')}</p>
        <dl><For each={uxRows()}>{(row) => <div><dt>{row.label}</dt><dd data-testid={`clarity-${row.key}`}>{number(metrics()?.ux?.[row.key], 2)}<Show when={metrics()?.ux?.[row.key] != null}><span> {row.unit}</span></Show></dd></div>}</For></dl>
      </div>
      <Show when={state.snapshot()?.projectId}>
        <nav class="analytics-panel__explore" aria-label={i18n.tr('Utforsk i Microsoft Clarity', 'Explore in Microsoft Clarity')}>
          <a href={projectLink('impressions')} target="_blank" rel="noopener noreferrer">{i18n.tr('Se øktopptak i Clarity', 'View recordings in Clarity')} ↗</a>
          <a href={projectLink('heatmaps')} target="_blank" rel="noopener noreferrer">{i18n.tr('Se varmekart i Clarity', 'View heatmaps in Clarity')} ↗</a>
        </nav>
        <p class="analytics-panel__scope">{i18n.tr('Åpnes i Microsoft Clarity og krever tilgang til prosjektet. Opptak og varmekart er ikke bygget inn i Synlighet; eksport-API-et leverer aggregater.', 'Opens in Microsoft Clarity and requires access to the project. Recordings and heatmaps are not embedded in Visibility; the export API supplies aggregates.')}</p>
      </Show>
      </ReportDisclosure>
    </Show>
    <Show when={state.snapshot()?.window.observedAt}>{(observed) => <p class="analytics-panel__timestamp">{i18n.tr('Eksport observert', 'Export observed')} {new Intl.DateTimeFormat(localeDateTime(i18n.locale()), { dateStyle: 'medium', timeStyle: 'short', timeZone: 'UTC' }).format(new Date(observed()))} UTC</p>}</Show>
    <footer>{i18n.tr('Dette tidsvinduet følger ikke valgt kvartal. Integration Core mellomlagrer eksporten på grunn av lav leverandørkvote. Manglende målinger vises som —, ikke null.', 'This window does not follow the selected quarter. Integration Core caches exports because of the low provider quota. Missing metrics appear as —, not zero.')}</footer>
  </section>
}
