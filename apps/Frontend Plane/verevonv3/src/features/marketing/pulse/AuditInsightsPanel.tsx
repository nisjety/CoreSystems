import { For, Show, createEffect, createSignal, createUniqueId, onCleanup } from 'solid-js'
import { localeDateTime, useI18n } from '@/shared/i18n'
import { loadAuditInsights, type AuditInsights } from '@/shared/api/marketing/audit-insights-client'
import { changeKinds } from '@/shared/api/marketing/audit-history-boundary'
import { auditRuleLabel } from './audit-work-labels'
import { insightAlertLabel, insightChangeLabel } from './audit-insight-labels'
import './audit-history-panels.css'

type Props = { websiteId: string; site: string; revision: number; refreshKey?: number }
export function AuditInsightsPanel(props: Props) {
  const i18n = useI18n(), id = createUniqueId()
  const [snapshot, setSnapshot] = createSignal<AuditInsights | null>(null)
  const [loading, setLoading] = createSignal(true), [error, setError] = createSignal(''), [refresh, setRefresh] = createSignal(0)
  const stamp = (value: string) => value ? `${new Intl.DateTimeFormat(localeDateTime(i18n.locale()), { dateStyle: 'medium', timeStyle: 'short', timeZone: 'UTC' }).format(new Date(value))} UTC` : '—'
  createEffect(() => {
    const selected = { websiteId: props.websiteId, site: props.site, websiteRevision: props.revision }
    void props.refreshKey
    refresh()
    const controller = new AbortController()
    setSnapshot(null); setLoading(true); setError('')
    void loadAuditInsights(selected, controller.signal).then((value) => { if (!controller.signal.aborted) setSnapshot(value) })
      .catch(() => { if (!controller.signal.aborted) setError(i18n.tr('Revisjonsinnsikt kunne ikke lastes. Oppdater for å prøve igjen.', 'Audit insights could not be loaded. Refresh to retry.')) })
      .finally(() => { if (!controller.signal.aborted) setLoading(false) })
    onCleanup(() => controller.abort())
  })
  return <section class="audit-history-panel" aria-labelledby={`${id}-title`} data-testid="audit-insights-panel">
    <header><div><span>Marketing Core · {i18n.tr('Kun lesing', 'Read only')}</span><h2 id={`${id}-title`}>{i18n.tr('Endringer og varsler', 'Audit changes and alerts')}</h2><p>{i18n.tr('Sammenlign dokumenterte signaler, ikke et oppdiktet helsetall.', 'Compare recorded signals, not an invented health score.')}</p></div><button type="button" disabled={loading()} onClick={() => setRefresh((n) => n + 1)}>{i18n.tr('Oppdater revisjonsinnsikt', 'Refresh audit insights')}</button></header>
    <Show when={loading()}><p role="status">{i18n.tr('Laster innsikt…', 'Loading insights…')}</p></Show><Show when={error()}><p role="alert">{error()}</p></Show>
    <Show when={snapshot()}>{(value) => <>
      <Show when={!value().comparisonAvailable}><p class="audit-history-panel__warning">{i18n.tr('Ingen tidligere sammenlignbar grunnlinje. Nye funn er et utgangspunkt, ikke dokumentert forverring.', 'No prior comparable baseline. New findings establish a starting point, not proven deterioration.')}</p></Show>
      <Show when={value().historyLimited}><p class="audit-history-panel__warning">{i18n.tr('Hull i registrering eller oppbevaring begrenser hva historikken kan bevise.', 'Recording or retention gaps limit what this history can prove.')}</p></Show>
      <Show when={value().truncated}><p class="audit-history-panel__warning">{i18n.tr('Visningen er avkortet. Fravær av et funn er ikke bevis for at det er løst.', 'Display is truncated. An absent finding is not evidence of resolution.')}</p></Show>
      <p>{i18n.tr('Tilstander for registrerte funn, ikke alle kontrollregler. Null uavklarte funn betyr ikke full dekning.', 'Tracked finding states, not all checked rules. Zero unknown findings does not mean complete coverage.')}</p>
      <dl data-testid="insight-counts"><For each={changeKinds}>{(kind) => <div><dt>{insightChangeLabel(kind, i18n.tr)}</dt><dd>{value().summary[kind]}</dd></div>}</For></dl>
      <Show when={value().alerts.length > 0}><div class="audit-history-panel__alerts" aria-label={i18n.tr('Varsler i Studio', 'In-app alerts')}><For each={value().alerts}>{(alert) => <article><strong>{insightAlertLabel(alert.kind, i18n.tr)}</strong><Show when={alert.ruleId !== '' ? alert.ruleId : undefined}>{(rule) => <p>{auditRuleLabel(rule(), i18n.tr)}</p>}</Show><Show when={alert.pageUrl}><p>{alert.pageUrl}</p></Show><small>{stamp(alert.observedAt)} · {alert.auditId}</small><Show when={alert.reason}><code>{alert.reason}</code></Show></article>}</For></div></Show>
      <Show when={value().items.length > 0} fallback={<p>{i18n.tr('Ingen registrerte forhold i det tilgjengelige bevisutvalget.', 'No recorded conditions in the retained evidence sample.')}</p>}><div class="audit-history-panel__items"><For each={value().items}>{(item) => <article><div class="audit-history-panel__item-title"><strong>{auditRuleLabel(item.ruleId, i18n.tr)}</strong><span>{insightChangeLabel(item.status, i18n.tr)}</span></div><a href={item.pageUrl} target="_blank" rel="noopener noreferrer">{item.pageUrl}</a><p>{i18n.tr('Sist observert', 'Last observed')}: {stamp(item.observedAt)}</p><Show when={item.status === 'unknown'}><p class="audit-history-panel__warning">{i18n.tr('Bevis kan være eldre enn siste revisjon. Uavklart betyr ikke løst.', 'Proof may predate the latest audit. Unknown does not mean resolved.')}</p></Show><details><summary>{i18n.tr('Kildebevis', 'Source proof')}</summary><code>{item.proofAuditId}</code><code>{item.runId}</code><code>{item.fingerprint}</code><p>{i18n.tr('Kontrollsignal', 'Check signal')}: {item.checkStatus}</p><For each={item.relatedUrls}>{(url) => <a href={url} target="_blank" rel="noopener noreferrer">{url}</a>}</For></details></article>}</For></div></Show>
      <p>{i18n.tr('Registrering startet', 'Recording since')}: {stamp(value().trackingSince)} · {i18n.tr('Lest', 'Read at')}: {stamp(value().asOf)}</p>
    </>}</Show>
    <footer>{i18n.tr('Dette er begrensede observasjoner, ikke full dekning av nettstedet. Et bestått signal gjelder bare faktisk kontrollerte sider. Varsler vises bare i Studio; ingen e-post sendes, og arbeidsoppgaver opprettes eller fullføres ikke automatisk.', 'These are bounded observations, not whole-site coverage. A passing signal applies only to pages actually checked. Alerts stay in Studio; no email is sent and no work tasks are created or closed automatically.')}</footer>
  </section>
}
