import { BarChart3, Gauge, Users } from 'lucide-solid'
import { For, Show, createMemo } from 'solid-js'
import { localeDateTime, useI18n } from '@/shared/i18n'
import type { GA4Snapshot } from '@/shared/api/marketing/ga4-client'
import type { ClaritySnapshot } from '@/shared/api/marketing/clarity-client'
import type { PageSpeedSnapshot } from '@/shared/api/marketing/pagespeed-client'
import { formatPulseDate, pulsePeriodDates } from '@/shared/api/marketing/pulse-timeline'
import { pulseServiceNames, type PulseServiceKey } from './ConnectedServices'
import './provider-overview.css'

type Metric = { label: string; value: number | null | undefined; unit?: string; decimals?: number }
export function ProviderOverview(props: { provider: PulseServiceKey; ga4: GA4Snapshot | null; clarity: ClaritySnapshot | null; pageSpeed: PageSpeedSnapshot | null }) {
  const i18n = useI18n()
  const number = (value: number | null | undefined, unit = '', decimals = 0) => value == null ? '—' : `${new Intl.NumberFormat(localeDateTime(i18n.locale()), { maximumFractionDigits: decimals }).format(value)}${unit}`
  const ga4 = () => props.ga4?.status === 'connected' ? props.ga4 : null
  const clarity = () => props.clarity?.status === 'connected' ? props.clarity : null
  const speed = () => props.pageSpeed?.status === 'complete' ? props.pageSpeed.report : null
  const title = () => props.provider === 'ga4' ? i18n.tr('Økter', 'Sessions') : props.provider === 'clarity' ? i18n.tr('Økter · siste 72 timer', 'Sessions · last 72 hours') : props.provider === 'website' ? i18n.tr('Ytelse · Lighthouse', 'Performance · Lighthouse') : pulseServiceNames[props.provider]
  const primary = () => props.provider === 'ga4' ? ga4()?.totals?.sessions : props.provider === 'clarity' ? clarity()?.metrics?.sessions : props.provider === 'website' ? speed()?.performance : undefined
  const cards = (): Metric[] => props.provider === 'ga4' ? [
    { label: i18n.tr('Brukere', 'Users'), value: ga4()?.totals?.users }, { label: i18n.tr('Økter', 'Sessions'), value: ga4()?.totals?.sessions }, { label: i18n.tr('Viktige hendelser', 'Key events'), value: ga4()?.totals?.keyEvents, decimals: 2 },
  ] : props.provider === 'clarity' ? [
    { label: i18n.tr('Økter', 'Sessions'), value: clarity()?.metrics?.sessions }, { label: i18n.tr('Brukere', 'Users'), value: clarity()?.metrics?.users }, { label: i18n.tr('Sider per økt', 'Pages per session'), value: clarity()?.metrics?.pagesPerSession, decimals: 2 },
  ] : props.provider === 'website' ? [
    { label: 'LCP · lab', value: speed()?.lcpMs, unit: ' ms' }, { label: 'CLS · lab', value: speed()?.cls, decimals: 3 }, { label: 'TBT · lab', value: speed()?.tbtMs, unit: ' ms' },
  ] : [ { label: i18n.tr('Rapportdata', 'Report data'), value: undefined }, { label: i18n.tr('Målinger', 'Measurements'), value: undefined }, { label: i18n.tr('Resultater', 'Results'), value: undefined } ]
  const bars = createMemo(() => {
    const report = ga4()
    if (props.provider !== 'ga4' || !report) return []
    const days = new Map(report.days.map(day => [day.date, day.sessions]))
    const maximum = Math.max(1, ...report.days.map(day => day.sessions))
    return pulsePeriodDates(report.period).map(date => ({ date, value: days.get(date), height: days.has(date) ? Math.max(1, days.get(date)! / maximum * 100) : 0 }))
  })
  const period = () => {
    const report = ga4()
    if (props.provider === 'ga4' && report) return `${formatPulseDate(report.period.startDate, localeDateTime(i18n.locale()), true)} – ${formatPulseDate(report.period.endDate, localeDateTime(i18n.locale()), true)} · ${report.timeZone ?? ''}`
    if (props.provider === 'clarity') return i18n.tr('Prosjektaggregat · siste 72 timer · UTC', 'Project aggregate · last 72 hours · UTC')
    if (props.provider === 'website') return speed()?.fetchTime ? new Intl.DateTimeFormat(localeDateTime(i18n.locale()), { dateStyle: 'medium', timeStyle: 'short' }).format(new Date(speed()!.fetchTime)) : i18n.tr('Ingen lagret labmåling', 'No saved lab measurement')
    return i18n.tr('Se rapportens eget tidsvindu nedenfor', 'See the report’s own time window below')
  }
  const measurements = (): Metric[] => props.provider === 'clarity' ? [
    { label: i18n.tr('Gjennomsnittlig rulledybde', 'Average scroll depth'), value: clarity()?.metrics?.ux?.averageScrollDepth, unit: '%' },
    { label: i18n.tr('Økter med døde klikk', 'Sessions with dead clicks'), value: clarity()?.metrics?.ux?.deadClickPercent, unit: '%' },
    { label: i18n.tr('Økter med frustrasjonsklikk', 'Sessions with rage clicks'), value: clarity()?.metrics?.ux?.rageClickPercent, unit: '%' },
  ] : props.provider === 'website' ? [
    { label: i18n.tr('Ytelse', 'Performance'), value: speed()?.performance, unit: '/100' }, { label: i18n.tr('Tilgjengelighet', 'Accessibility'), value: speed()?.accessibility, unit: '/100' }, { label: i18n.tr('Beste praksis', 'Best practices'), value: speed()?.bestPractices, unit: '/100' }, { label: 'SEO', value: speed()?.seo, unit: '/100' },
  ] : []
  return <div class="provider-overview" data-testid="provider-overview" data-provider={props.provider}>
    <section class="marketing-pulse__workspace" aria-label={title()}>
      <div class="marketing-pulse__signal-surface">
        <div class="marketing-pulse__signal-heading"><div><strong data-testid="pulse-primary-value">{number(primary())}</strong><p>{title()}</p><p class="marketing-pulse__date-range">{period()}</p></div><span class="marketing-pulse__state-pill">{pulseServiceNames[props.provider]}</span></div>
        <Show when={props.provider === 'website' && props.pageSpeed?.cache}><p class="provider-overview__note" role="status">{props.pageSpeed?.cache?.refreshStatus === 'running' || props.pageSpeed?.cache?.refreshStatus === 'queued' ? i18n.tr('Oppdateres i bakgrunnen · siste vellykkede måling vises', 'Refreshing in the background · showing the last successful measurement') : props.pageSpeed?.cache?.refreshStatus === 'failed' ? i18n.tr('Oppdateringen mislyktes · siste vellykkede måling vises', 'Refresh failed · showing the last successful measurement') : props.pageSpeed?.cache?.stale ? i18n.tr('Lagret rapport er eldre enn ett døgn', 'The saved report is more than a day old') : i18n.tr('Lagret rapport · automatisk oppdatering hvert døgn', 'Saved report · automatic daily refresh')}</p></Show>
        <Show when={props.provider === 'ga4'} fallback={<div class="provider-overview__measurements"><For each={measurements()}>{item => <div><div><span>{item.label}</span><strong>{number(item.value, item.unit, props.provider === 'clarity' ? 2 : 0)}</strong></div><div class="provider-overview__meter"><Show when={item.value != null}><span style={{ width: `${Math.min(100, item.value!)}%` }}/></Show></div></div>}</For><Show when={measurements().length === 0}><p>{i18n.tr('Åpne leverandørens rapport for tilgjengelige målinger.', 'Open the provider’s report for available measurements.')}</p></Show></div>}>
          <div class="provider-overview__chart" role="img" aria-label={i18n.tr('Daglige økter fra Google Analytics. Tomme dager er ukjente.', 'Daily sessions from Google Analytics. Missing days are unknown.')}>
            <Show when={bars().some(bar => bar.value !== undefined)} fallback={<div class="marketing-pulse__chart-empty"><BarChart3 size={18}/><span>{i18n.tr('Ingen tilgjengelige dagsdata fra Analytics.', 'No daily Analytics data available.')}</span></div>}>
              <div class="provider-overview__bars" style={{ 'grid-template-columns': `repeat(${bars().length}, minmax(0, 1fr))` }}><For each={bars()}>{bar => <span data-date={bar.date} data-measured={bar.value !== undefined ? '' : undefined} title={`${bar.date} · ${number(bar.value)}`} style={{ height: `${bar.height}%` }}/>}</For></div>
            </Show>
          </div>
          <Show when={ga4()}>{report => <div class="provider-overview__dates"><span>{formatPulseDate(report().period.startDate, localeDateTime(i18n.locale()), true)}</span><span>{formatPulseDate(report().period.endDate, localeDateTime(i18n.locale()), true)}</span></div>}</Show>
        </Show>
        <p class="provider-overview__note">{props.provider === 'ga4' ? i18n.tr('Periodetotal fra Google. Manglende dager fylles ikke med null.', 'Period total from Google. Missing days are not filled with zero.') : props.provider === 'clarity' ? i18n.tr('Clarity leverer prosjektaggregater, ikke en daglig tidsserie. Følger ikke kvartalsfilteret.', 'Clarity supplies project aggregates, not a daily time series. Independent of the quarter filter.') : props.provider === 'website' ? i18n.tr('Lighthouse-labmåling. Ikke CrUX-feltdata. Følger enheten valgt i rapporten nedenfor.', 'Lighthouse lab measurement. Not CrUX field data. Follows the device selected in the report below.') : i18n.tr('Målinger følger leverandørens eget rapportvindu.', 'Measurements follow the provider’s own reporting window.')}</p>
      </div>
    </section>
    <section class="marketing-pulse__metrics" aria-label={i18n.tr('Nøkkeltall', 'Key metrics')}><For each={cards()}>{(card, index) => <article class="marketing-pulse__metric-card"><div><Show when={index() === 0} fallback={index() === 1 ? <BarChart3 size={16}/> : <Gauge size={16}/>}><Users size={16}/></Show><span>{card.label}</span></div><strong data-testid="provider-metric-value">{number(card.value, card.unit, card.decimals)}</strong><p>{pulseServiceNames[props.provider]}</p></article>}</For></section>
  </div>
}
