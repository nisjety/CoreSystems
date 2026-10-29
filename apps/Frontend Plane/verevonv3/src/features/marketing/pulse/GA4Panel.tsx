import { For, Show, createEffect, createUniqueId } from 'solid-js'
import { localeDateTime, useI18n } from '@/shared/i18n'
import { loadGA4, type GA4Metrics, type GA4Snapshot } from '../../../shared/api/marketing/ga4-client.ts'
import type { PulseQuarter } from '../../../shared/api/marketing/pulse-client.ts'
import type { AnalyticsPanelState } from '../../../shared/api/marketing/analytics-boundary.ts'
import { createAnalyticsPanelState } from './analytics-panel-state.ts'
import { formatPulseDate } from '../../../shared/api/marketing/pulse-timeline.ts'
import './analytics-panels.css'
import { ReportDisclosure } from './ReportDisclosure'
import './pulse-dashboard.css'

type Props = { site: string; websiteId: string; revision: number; quarter: PulseQuarter; refreshKey?: number; onState?: (state: AnalyticsPanelState) => void; onManageWebsite?: () => void; onSnapshot?: (snapshot: GA4Snapshot | null) => void }
export function GA4Panel(props: Props) {
  const i18n = useI18n()
  const id = createUniqueId()
  const state = createAnalyticsPanelState(
    () => { void props.refreshKey; return { site: props.site, websiteId: props.websiteId, revision: props.revision, quarter: props.quarter } },
    (context, signal) => loadGA4(context.quarter!, context.site, context.websiteId, signal),
    (value) => props.onState?.(value),
  )
  const connected = () => state.snapshot()?.status === 'connected'
  createEffect(() => props.onSnapshot?.(state.loading() || state.failed() ? null : state.snapshot()))
  const totals = () => connected() ? state.snapshot()?.totals : null
  const number = (value: number | null | undefined, decimals = 0) => value == null ? '—' : new Intl.NumberFormat(localeDateTime(i18n.locale()), { maximumFractionDigits: decimals, notation: value > Number.MAX_SAFE_INTEGER ? 'scientific' : 'standard' }).format(value)
  const date = (value: string) => formatPulseDate(value, localeDateTime(i18n.locale()), true)
  const period = () => {
    const range = state.snapshot()?.period
    return range ? `${date(range.startDate)} – ${date(range.endDate)}` : '—'
  }
  const reportingHost = () => {
    const snapshot = state.snapshot()
    return snapshot?.status === 'connected' ? new URL(snapshot.reportingSite ?? snapshot.site).hostname : undefined
  }
  const cards = (): { key: keyof GA4Metrics; label: string; decimals?: number; unit?: string }[] => [
    { key: 'users', label: i18n.tr('Brukere', 'Users') }, { key: 'sessions', label: i18n.tr('Økter', 'Sessions') },
    { key: 'pageViews', label: i18n.tr('Sidevisninger', 'Page views') }, { key: 'engagedSessions', label: i18n.tr('Engasjerte økter', 'Engaged sessions') },
    { key: 'keyEvents', label: i18n.tr('Viktige hendelser', 'Key events'), decimals: 2 },
    { key: 'engagementSeconds', label: i18n.tr('Engasjementstid', 'Engagement time'), decimals: 2, unit: 's' },
  ]
  const message = () => {
    if (state.loading()) return i18n.tr('Laster Analytics…', 'Loading Analytics…')
    if (state.failed()) return i18n.tr('Analytics kunne ikke lastes. Andre Synlighet-paneler påvirkes ikke.', 'Analytics could not be loaded. Other Visibility panels are unaffected.')
    switch (state.snapshot()?.reason) {
      case 'snapshot_pending': case 'snapshot_expired': return i18n.tr('Lagret Analytics-rapport mangler eller er utløpt. Bruk innhentingspanelet for å hente en ny rapport.', 'The saved Analytics snapshot is missing or expired. Use the collection panel to collect a new report.')
      case 'authorization_required': return i18n.tr('Analytics-tilgangen må kontrolleres i Integration Core før lagrede data kan vises.', 'Analytics authorization needs attention in Integration Core before saved data can be shown.')
      case 'requires_consent': return i18n.tr('Google-kontoen må godkjenne lesetilgang til Analytics.', 'The Google account needs to approve read-only Analytics access.')
      case 'needs_selection': return i18n.tr('Flere Analytics-koblinger passer. En entydig egenskap og nettstrøm må velges før rapporter kan vises.', 'Several Analytics mappings match. A unique property and web stream must be selected before reports can be shown.')
      case 'not_found': return i18n.tr('Ingen tilgjengelig Analytics-nettstrøm samsvarer med dette nettstedet.', 'No accessible Analytics web stream matches this website.')
      case 'not_connected': case 'not_configured': return i18n.tr('Analytics er ikke koblet til for dette nettstedet. Administrer nettstedets integrasjoner for å fortsette.', 'Analytics is not connected for this website. Manage its integrations to continue.')
      case 'source_unavailable': return i18n.tr('Analytics-kilden er midlertidig utilgjengelig.', 'The Analytics source is temporarily unavailable.')
      default: return i18n.tr('Ingen rapportdata for denne perioden.', 'No report data for this period.')
    }
  }
  const quality = () => state.snapshot()?.quality
  const hasQualityNotice = () => quality()?.thresholded || quality()?.sampled || quality()?.dataLoss
  return <section class="analytics-panel pulse-dashboard pulse-dashboard--ga4" aria-labelledby={`${id}-title`} data-testid="ga4-panel" aria-busy={state.loading()}>
    <header class="analytics-panel__header"><div><span class="analytics-panel__eyebrow">Google Analytics 4</span><h2 id={`${id}-title`}>{i18n.tr('Besøksaktivitet', 'Website activity')}</h2><Show when={reportingHost()}>{(host) => <p data-testid="ga4-reporting-host">{i18n.tr('GA4-rapporteringsvert', 'GA4 reporting host')}: {host()}</p>}</Show><p data-testid="ga4-period">{period()} · {props.quarter}<Show when={state.snapshot()?.timeZone}>{(zone) => <> · {zone()}</>}</Show></p></div><div class="analytics-panel__controls"><span class="analytics-panel__readonly">{i18n.tr('Kun lesing', 'Read only')}</span><button type="button" disabled={state.loading()} onClick={state.refresh}>{i18n.tr('Oppdater', 'Refresh')}</button></div></header>
    <Show when={state.loading() || state.failed() || !connected() || totals() === null}><p role={state.failed() ? 'alert' : 'status'} class="analytics-panel__status">{message()}<Show when={props.onManageWebsite && !state.loading() && !connected() && state.snapshot()?.reason !== 'snapshot_pending' && state.snapshot()?.reason !== 'snapshot_expired'}><button type="button" onClick={() => props.onManageWebsite?.()}>{i18n.tr('Administrer nettsted', 'Manage website')}</button></Show></p></Show>
    <div class="analytics-panel__cards analytics-panel__cards--six"><For each={cards()}>{(card) => <article><span>{card.label}</span><strong data-testid={`ga4-${card.key}`}>{number(totals()?.[card.key], card.decimals)}<Show when={totals() && card.unit}><small> {card.unit}</small></Show></strong></article>}</For></div>
    <Show when={connected() && hasQualityNotice()}><aside role="note" class="analytics-panel__quality"><strong>{i18n.tr('Datakvalitet', 'Data quality')}</strong><Show when={quality()?.thresholded}><span>{i18n.tr('Google bruker personvernterskler.', 'Google applies privacy thresholds.')}</span></Show><Show when={quality()?.sampled}><span>{i18n.tr('Rapporten er basert på et utvalg.', 'The report is sampled.')}</span></Show><Show when={quality()?.dataLoss}><span>{i18n.tr('Google rapporterer datatap i denne rapporten.', 'Google reports data loss in this report.')}</span></Show></aside></Show>
    <Show when={connected() && state.snapshot()!.days.length > 0}><ReportDisclosure source="Google Analytics 4" title={i18n.tr('Daglig aktivitet', 'Daily activity')}><div class="analytics-panel__table-wrap"><table data-testid="ga4-days"><caption>{i18n.tr('Daglig aktivitet', 'Daily activity')}</caption><thead><tr><th>{i18n.tr('Dato', 'Date')}</th><th>{i18n.tr('Brukere', 'Users')}</th><th>{i18n.tr('Økter', 'Sessions')}</th><th>{i18n.tr('Sidevisninger', 'Page views')}</th><th>{i18n.tr('Viktige hendelser', 'Key events')}</th></tr></thead><tbody><For each={state.snapshot()!.days}>{(day) => <tr><td>{date(day.date)}</td><td>{number(day.users)}</td><td>{number(day.sessions)}</td><td>{number(day.pageViews)}</td><td>{number(day.keyEvents, 2)}</td></tr>}</For></tbody></table></div></ReportDisclosure></Show>
    <footer>{i18n.tr('Periodens brukertall kommer fra Google og er ikke summert fra daglige brukere. Manglende dager fylles ikke med null.', 'Period-wide users come from Google and are not added from daily users. Missing days are not filled with zero.')}<Show when={state.snapshot()?.updatedAt}>{(updated) => <span>{i18n.tr('Hentet', 'Fetched')} {new Intl.DateTimeFormat(localeDateTime(i18n.locale()), { dateStyle: 'medium', timeStyle: 'short' }).format(new Date(updated()))}</span>}</Show></footer>
  </section>
}
