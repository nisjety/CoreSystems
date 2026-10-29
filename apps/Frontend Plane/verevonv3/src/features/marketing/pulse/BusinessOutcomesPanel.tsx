import { Show, createEffect, createSignal, createUniqueId, onCleanup } from 'solid-js'
import { localeDateTime, useI18n } from '@/shared/i18n'
import { loadOutcomes, type OutcomeSnapshot } from '@/shared/api/marketing/outcomes-client'
import type { PulseQuarter } from '@/shared/api/marketing/pulse-client'
import './business-outcomes.css'

export function BusinessOutcomesPanel(props: { websiteId: string; site: string; revision: number; quarter: PulseQuarter }) {
  const i18n = useI18n(), id = createUniqueId()
  const [environment, setEnvironment] = createSignal<'development' | 'production'>('production')
  const [snapshot, setSnapshot] = createSignal<OutcomeSnapshot | null>(null), [failed, setFailed] = createSignal(false), [loading, setLoading] = createSignal(true), [refresh, setRefresh] = createSignal(0)
  const stamp = (v: string) => new Intl.DateTimeFormat(localeDateTime(i18n.locale()), { dateStyle: 'medium', timeStyle: 'short', timeZone: 'Europe/Oslo' }).format(new Date(v))
  createEffect(() => {
    const selected = { websiteId: props.websiteId, site: props.site, websiteRevision: props.revision, quarter: props.quarter, environment: environment() }
    refresh(); const controller = new AbortController(); onCleanup(() => controller.abort())
    setSnapshot(null); setFailed(false); setLoading(true)
    void loadOutcomes(selected, controller.signal).then((value) => { if (!controller.signal.aborted) setSnapshot(value) }).catch(() => { if (!controller.signal.aborted) setFailed(true) }).finally(() => { if (!controller.signal.aborted) setLoading(false) })
  })
  return <section class="business-outcomes" aria-labelledby={`${id}-title`} aria-busy={loading()}>
    <header><div><span>Leads Core</span><h2 id={`${id}-title`}>{i18n.tr('Fra henvendelse til lead', 'From inquiry to lead')}</h2><p>{i18n.tr('Bekreftede innsendinger fra nettstedet. Kontaktopplysninger lagres bare i Leads Core.', 'Confirmed website submissions. Contact details stay in Leads Core.')}</p></div><div class="business-outcomes__controls"><label>{i18n.tr('Miljø', 'Environment')}<select value={environment()} onChange={(e) => setEnvironment(e.currentTarget.value as 'production' | 'development')}><option value="production">{i18n.tr('Produksjon', 'Production')}</option><option value="development">{i18n.tr('Utvikling', 'Development')}</option></select></label><button type="button" disabled={loading()} onClick={() => setRefresh((n) => n + 1)}>{i18n.tr('Oppdater utfall', 'Refresh outcomes')}</button></div></header>
    <Show when={environment() === 'development'}><p class="business-outcomes__notice">{i18n.tr('Kun lokal utvikling · disse innsendingene teller ikke som produksjonskonverteringer.', 'Local development only · these submissions do not count as production conversions.')}</p></Show>
    <Show when={loading()}><p role="status">{i18n.tr('Laster bekreftede utfall…', 'Loading confirmed outcomes…')}</p></Show>
    <Show when={failed()}><p role="alert">{i18n.tr('Utfall kunne ikke lastes. Prøv igjen.', 'Outcomes could not be loaded. Try again.')}</p></Show>
    <Show when={snapshot()?.status === 'unavailable'}><p role="status">{snapshot()?.reason === 'not_connected' ? i18n.tr('Nettstedets skjemamottak er ikke koblet til for dette miljøet.', 'Website form delivery is not connected for this environment.') : i18n.tr('Leads Core er midlertidig utilgjengelig.', 'Leads Core is temporarily unavailable.')}</p></Show>
    <div class="business-outcomes__cards"><article><span>{i18n.tr('Bekreftede henvendelser', 'Confirmed inquiries')}</span><strong>{snapshot()?.metrics?.inquiries ?? '—'}</strong></article><article><span>{i18n.tr('Mottatt i Leads Core', 'Received in Leads Core')}</span><strong>{snapshot()?.metrics?.leads ?? '—'}</strong><small>{i18n.tr('Samme innsending · ett lead per kvittering', 'Same submission · one lead per receipt')}</small></article></div>
    <Show when={snapshot()?.asOf}><footer>{snapshot()?.period.startDate} – {snapshot()?.period.endDate} · Europe/Oslo · {i18n.tr('Kontrollert', 'Checked')}: {stamp(snapshot()!.asOf!)}<Show when={snapshot()?.lastReceivedAt}> · {i18n.tr('Sist mottatt', 'Last received')}: {stamp(snapshot()!.lastReceivedAt!)}</Show></footer></Show>
  </section>
}
