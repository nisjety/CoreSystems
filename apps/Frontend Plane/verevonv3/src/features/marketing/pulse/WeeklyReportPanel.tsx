import { For, Show, createEffect, createSignal, createUniqueId, onCleanup } from 'solid-js'
import { localeDateTime, useI18n } from '@/shared/i18n'
import { loadWeeklyReport, type WeeklyAuditReport } from '@/shared/api/marketing/weekly-report-client'
import { changeKinds } from '@/shared/api/marketing/audit-history-boundary'
import { currentUTCWeek, isUTCWeekStart, shiftUTCWeek } from '@/shared/api/marketing/report-week'
import { insightChangeLabel } from './audit-insight-labels'
import './audit-history-panels.css'

type Props = { websiteId: string; site: string; revision: number; refreshKey?: number }
export function WeeklyReportPanel(props: Props) {
  const i18n = useI18n(), id = createUniqueId()
  const [week, setWeek] = createSignal(currentUTCWeek()), [snapshot, setSnapshot] = createSignal<WeeklyAuditReport | null>(null)
  const [loading, setLoading] = createSignal(true), [error, setError] = createSignal(''), [selectionError, setSelectionError] = createSignal(''), [refresh, setRefresh] = createSignal(0)
  const stamp = (value: string) => value ? `${new Intl.DateTimeFormat(localeDateTime(i18n.locale()), { dateStyle: 'medium', timeStyle: 'short', timeZone: 'UTC' }).format(new Date(value))} UTC` : '—'
  function selectWeek(value: string) {
    if (!isUTCWeekStart(value) || value > currentUTCWeek()) { setSelectionError(i18n.tr('Velg en mandag i UTC, ikke en fremtidig uke.', 'Choose a Monday in UTC, not a future week.')); return false }
    setSelectionError(''); setWeek(value); return true
  }
  createEffect(() => {
    const context = { websiteId: props.websiteId, site: props.site, websiteRevision: props.revision }, selected = week()
    void props.refreshKey
    refresh()
    const controller = new AbortController()
    setSnapshot(null); setLoading(true); setError(''); setSelectionError('')
    void loadWeeklyReport(context, selected, controller.signal).then((value) => { if (!controller.signal.aborted) setSnapshot(value) })
      .catch(() => { if (!controller.signal.aborted) setError(i18n.tr('Ukentlig rapport kunne ikke lastes. Oppdater for å prøve igjen.', 'Weekly report could not be loaded. Refresh to retry.')) })
      .finally(() => { if (!controller.signal.aborted) setLoading(false) })
    onCleanup(() => controller.abort())
  })
  return <section class="audit-history-panel" aria-labelledby={`${id}-title`} data-testid="weekly-report-panel">
    <header><div><span>Marketing Core · UTC</span><h2 id={`${id}-title`}>{i18n.tr('Ukentlig revisjonsrapport', 'Weekly audit report')}</h2><p>{i18n.tr('Lagrede observasjoner fra mandag til neste mandag, sluttidspunkt ikke inkludert.', 'Recorded observations from Monday to the next Monday, end exclusive.')}</p></div><button type="button" disabled={loading()} onClick={() => setRefresh((n) => n + 1)}>{i18n.tr('Oppdater ukesrapport', 'Refresh weekly report')}</button></header>
    <div class="audit-history-panel__navigation"><button type="button" disabled={loading()} onClick={() => selectWeek(shiftUTCWeek(week(), -1))}>{i18n.tr('Forrige uke', 'Previous week')}</button><label for={`${id}-week`}>{i18n.tr('Uke fra mandag · UTC', 'Week starting Monday · UTC')}<input id={`${id}-week`} type="date" value={week()} max={currentUTCWeek()} disabled={loading()} onChange={(event) => { if (!selectWeek(event.currentTarget.value)) event.currentTarget.value = week() }} /></label><button type="button" disabled={loading() || week() >= currentUTCWeek()} onClick={() => selectWeek(shiftUTCWeek(week(), 1))}>{i18n.tr('Neste uke', 'Next week')}</button><button type="button" disabled={loading() || week() === currentUTCWeek()} onClick={() => selectWeek(currentUTCWeek())}>{i18n.tr('Denne uken', 'Current week')}</button></div>
    <Show when={loading()}><p role="status">{i18n.tr('Laster ukesrapport…', 'Loading weekly report…')}</p></Show><Show when={error() || selectionError()}><p role="alert">{selectionError() || error()}</p></Show>
    <Show when={snapshot()}>{(value) => <>
      <p>{value().status === 'open' ? i18n.tr('Åpen uke · foreløpig', 'Open week · provisional') : i18n.tr('Avsluttet UTC-uke', 'Closed UTC week')} · {value().weekStart} – {value().endExclusive} {i18n.tr('(slutt eksklusiv)', '(end exclusive)')}</p>
      <Show when={!value().observed}><p class="audit-history-panel__warning">{i18n.tr('Ingen registrerte revisjonsobservasjoner i denne uken. Dette betyr ikke at nettstedet er feilfritt.', 'No recorded audit observations for this week. This does not mean the website is healthy.')}</p></Show>
      <Show when={value().historyLimited}><p class="audit-history-panel__warning">{i18n.tr('Hull i registrering eller oppbevaring begrenser rapporten.', 'Recording or retention gaps limit this report.')}</p></Show><Show when={value().truncated}><p class="audit-history-panel__warning">{i18n.tr('Visningen er avkortet. Summene kan dekke flere observasjoner enn kvitteringene nedenfor.', 'Display is truncated. Totals can cover more observations than the receipts below.')}</p></Show>
      <dl><div><dt>{i18n.tr('Revisjoner', 'Audits')}</dt><dd>{value().audits.total}</dd></div><div><dt>{i18n.tr('Fullførte / mislykkede revisjoner', 'Complete / failed audits')}</dt><dd>{value().audits.complete} / {value().audits.failed}</dd></div><div><dt>{i18n.tr('Kontrollerte / mislykkede sidehentinger', 'Checked / failed page captures')}</dt><dd>{value().pages.checked} / {value().pages.failed}</dd></div><div><dt>{i18n.tr('Ikke beståtte kontrollsignaler', 'Failed check observations')}</dt><dd>{value().failedChecks}</dd></div></dl>
      <p>{i18n.tr('Innhentingsobservasjoner, ikke unike sider. Gjentatte kontroller telles hver gang.', 'Capture observations, not unique pages. Repeated checks count each time.')}</p>
      <p>{i18n.tr('Funnobservasjoner per revisjon, ikke unike funn. Uendrede og uavklarte forhold kan telles igjen ved hver kontroll.', 'Per-audit finding observations, not unique findings. Unchanged and unknown conditions can count again in each audit.')}</p>
      <dl><For each={changeKinds}>{(kind) => <div><dt>{insightChangeLabel(kind, i18n.tr)}</dt><dd>{value().changes[kind]}</dd></div>}</For></dl>
      <Show when={value().work} fallback={<p>{i18n.tr('Gjeldende arbeidskø er utilgjengelig.', 'Current work queue is unavailable.')}</p>}>{(work) => <div><h3>{i18n.tr('Gjeldende arbeidskø per', 'Current work queue as of')} {stamp(value().tasksAsOf)}</h3><p>{i18n.tr('Dette er dagens status, ikke et øyeblikksbilde ved ukeslutt.', 'This is current status, not a week-end snapshot.')}</p><dl><div><dt>{i18n.tr('Åpne / pågående', 'Open / in progress')}</dt><dd>{work().open} / {work().inProgress}</dd></div><div><dt>{i18n.tr('Fullførte / avviste', 'Done / dismissed')}</dt><dd>{work().done} / {work().dismissed}</dd></div></dl></div>}</Show>
      <Show when={value().receipts.length > 0}><details><summary>{i18n.tr('Revisjonskvitteringer', 'Audit receipts')}</summary><For each={value().receipts}>{(receipt) => <article><code>{receipt.auditId}</code><p>{receipt.status === 'complete' ? i18n.tr('Fullført', 'Complete') : i18n.tr('Mislyktes', 'Failed')} · {stamp(receipt.completedAt)}</p><p>{receipt.checkedPages} {i18n.tr('kontrollerte sider', 'checked pages')} · {receipt.failedPages} {i18n.tr('mislykkede sidehentinger', 'failed page captures')} · {receipt.findingCount} {i18n.tr('funn', 'findings')}</p><Show when={receipt.reason}><code>{receipt.reason}</code></Show></article>}</For></details></Show>
      <Show when={value().availableWeeks.length > 0}><details><summary>{i18n.tr('Tilgjengelige registrerte uker', 'Available recorded weeks')}</summary><div class="audit-history-panel__navigation"><For each={value().availableWeeks}>{(available) => <button type="button" disabled={loading() || available === week()} onClick={() => selectWeek(available)}>{available}</button>}</For></div></details></Show>
      <p>{i18n.tr('Registrering startet', 'Recording since')}: {stamp(value().trackingSince)} · {i18n.tr('Lest', 'Read at')}: {stamp(value().asOf)}</p>
    </>}</Show>
    <footer>{i18n.tr('Rapporten er delvis og bygger bare på faktisk lagrede revisjonsobservasjoner. Å lese, oppdatere eller velge uke starter ingen nye innhentinger, sender ingen e-post og endrer ingen arbeidsoppgaver.', 'This partial report uses only retained audit observations. Reading, refreshing or selecting a week starts no captures, sends no email and changes no work tasks.')}</footer>
  </section>
}
