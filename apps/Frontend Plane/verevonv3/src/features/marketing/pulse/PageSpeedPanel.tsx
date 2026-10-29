import { For, Show, createEffect, createSignal, createUniqueId, onCleanup } from 'solid-js'
import { localeDateTime, useI18n } from '@/shared/i18n'
import { loadPageSpeed, startPageSpeed, type PageSpeedSnapshot, type PageSpeedStrategy } from '../../../shared/api/marketing/pagespeed-client.ts'
import { PageSpeedDiagnostics } from './PageSpeedDiagnostics.tsx'
import { saveMarketingTechnical } from '../../../shared/api/marketing/marketing-work-client.ts'
import { loadWebsitePreparation } from '../../../shared/api/marketing/preparation-client'
import { pageSpeedCheckIds, type PageSpeedCheckId } from '../../../shared/actions/marketing-pagespeed'
import './pagespeed-panel.css'
import { ReportDisclosure } from './ReportDisclosure'
import './pulse-dashboard.css'

/** Point-in-time lab checks are independent of Search Console's quarter range. */
export function PageSpeedPanel(props: { site: string; websiteId?: string; revision?: number; refreshKey?: number; savedFindings?: readonly string[]; onTaskSaved?: () => void; onSnapshot?: (snapshot: PageSpeedSnapshot | null) => void }) {
  const i18n = useI18n()
  const id = createUniqueId()
  const [strategy, setStrategy] = createSignal<PageSpeedStrategy>('mobile')
  const [snapshot, setSnapshot] = createSignal<PageSpeedSnapshot | null>(null)
  const [loading, setLoading] = createSignal(true)
  const [failed, setFailed] = createSignal(false)
  const [pollExpired, setPollExpired] = createSignal(false)
  const [waitingPreparation, setWaitingPreparation] = createSignal(false)
  const [savingId, setSavingId] = createSignal<PageSpeedCheckId | null>(null)
  const [savedHere, setSavedHere] = createSignal<ReadonlySet<string>>(new Set())
  const [saveError, setSaveError] = createSignal('')
  let diagnostics: HTMLDetailsElement | undefined
  let saveController: AbortController | undefined
  let controller: AbortController | undefined
  let timer: ReturnType<typeof setTimeout> | undefined
  let deadline = 0
  let generation = 0

  const clearTimer = () => { if (timer !== undefined) clearTimeout(timer); timer = undefined }
  async function request(start: boolean, site: string, device: PageSpeedStrategy, version: number) {
    clearTimer()
    setLoading(true)
    setFailed(false)
    controller = new AbortController()
    const current = controller
    try {
      const value = await (start ? startPageSpeed : loadPageSpeed)(site, device, current.signal, props.websiteId)
      if (current.signal.aborted || version !== generation) return
      setSnapshot(value)
      setWaitingPreparation(false)
      if (value.status === 'idle' && props.websiteId && props.revision) {
        try {
          const preparation = await loadWebsitePreparation({ websiteId: props.websiteId, site, websiteRevision: props.revision }, current.signal)
          if (current.signal.aborted || version !== generation) return
          if (preparation.status === 'queued' || preparation.status === 'running') {
            setWaitingPreparation(true)
            if (Date.now() < deadline) timer = setTimeout(() => { void request(false, site, device, version) }, 5000)
            else setPollExpired(true)
          }
        } catch { /* An unavailable preparation receipt cannot start a provider audit. */ }
      }
      if (value.status === 'running' || value.cache?.refreshStatus === 'queued' || value.cache?.refreshStatus === 'running') {
        if (Date.now() < deadline) timer = setTimeout(() => { void request(false, site, device, version) }, 5000)
        else setPollExpired(true)
      } else if (value.cache?.nextRefreshAt) {
        // Wake an open dashboard for the next saved result; this GET never starts provider work.
        const delay = Math.min(86_400_000, Math.max(30_000, Date.parse(value.cache.nextRefreshAt) - Date.now() + 5000))
        timer = setTimeout(() => { deadline = Date.now() + 120_000; void request(false, site, device, version) }, delay)
      }
    } catch { if (!current.signal.aborted && version === generation) setFailed(true) }
    finally { if (!current.signal.aborted && version === generation) setLoading(false) }
  }
  createEffect(() => {
    const site = props.site
    void props.websiteId
    void props.revision
    void props.refreshKey
    const device = strategy()
    const version = ++generation
    setSnapshot(null); setFailed(false); setPollExpired(false); setWaitingPreparation(false)
    saveController?.abort(); setSavingId(null); setSavedHere(new Set<string>()); setSaveError('')
    deadline = Date.now() + 120_000
    void request(false, site, device, version)
    onCleanup(() => { controller?.abort(); saveController?.abort(); clearTimer() })
  })
  async function saveCheck(checkId: PageSpeedCheckId) {
    const websiteId = props.websiteId; const revision = props.revision; const report = snapshot()?.report
    if (!websiteId || !revision || !report || snapshot()?.status !== 'complete' || savingId()) return
    const site = props.site; const device = strategy(); const version = generation
    const current = new AbortController(); saveController = current
    setSavingId(checkId); setSaveError('')
    try {
      await saveMarketingTechnical({ websiteId, site, websiteRevision: revision, strategy: device, checkId, fetchTime: report.fetchTime }, current.signal)
      if (current.signal.aborted || generation !== version) return
      setSavedHere((values) => new Set(values).add(`${device}:${checkId}`)); props.onTaskSaved?.()
    } catch (error) {
      if (!current.signal.aborted && generation === version) setSaveError(error instanceof Error && error.message === 'work_conflict'
        ? i18n.tr('Målingen er endret. Oppdater status før du lagrer kontrollen på nytt.', 'The audit changed. Refresh its status before saving this check again.')
        : i18n.tr('Kontrollen kunne ikke lagres. Prøv igjen når tilkoblingen er klar.', 'Could not save this check. Retry when the connection is available.'))
    } finally { if (saveController === current) setSavingId(null) }
  }
  const savedIds = () => (snapshot()?.report?.checks ?? []).filter((check) => (props.savedFindings ?? []).includes(`${strategy()}:${check.id}`) || savedHere().has(`${strategy()}:${check.id}`)).map((check) => check.id)
  const run = () => { controller?.abort(); setPollExpired(false); deadline = Date.now() + 120_000; void request(true, props.site, strategy(), generation) }
  const refresh = () => { controller?.abort(); setPollExpired(false); deadline = Date.now() + 120_000; void request(false, props.site, strategy(), generation) }
  const number = (value: number | null | undefined, decimals = 0) => value === null || value === undefined ? '—' : new Intl.NumberFormat(localeDateTime(i18n.locale()), { maximumFractionDigits: decimals }).format(value)
  const reason = () => (snapshot()?.cache?.refreshReason ?? snapshot()?.reason) === 'quota_limited'
    ? i18n.tr('Google avviste målingen på grunn av kvote. Kontroller PageSpeed-nøkkel og kvote i Integration Core. Search Console påvirkes ikke.', 'Google rejected this audit due to quota. Check the PageSpeed key and quota in Integration Core. Search Console is unaffected.')
    : (snapshot()?.cache?.refreshReason ?? snapshot()?.reason) === 'not_configured'
      ? i18n.tr('PageSpeed er ikke konfigurert i Integration Core. Legg til prosjektets nøkkel der – ikke i nettleseren.', 'PageSpeed is not configured in Integration Core. Add the project key there, not in the browser.')
      : i18n.tr('Målingen kunne ikke fullføres. Andre Synlighet-data påvirkes ikke.', 'The audit could not complete. Other Visibility data is unaffected.')
  const measurement = () => snapshot()?.status === 'complete' ? snapshot()?.report : undefined
  createEffect(() => props.onSnapshot?.(measurement() ? snapshot() : null))
  const refreshing = () => snapshot()?.status === 'running' || snapshot()?.cache?.refreshStatus === 'queued' || snapshot()?.cache?.refreshStatus === 'running'
  const dateTime = (value?: string) => value ? new Intl.DateTimeFormat(localeDateTime(i18n.locale()), { dateStyle: 'medium', timeStyle: 'short' }).format(new Date(value)) : '—'
  const checkCounts = () => {
    const checks = measurement()?.checks ?? []
    return { passed: checks.filter(check => check.status === 'passed').length, review: checks.filter(check => ['failed', 'manual', 'error'].includes(check.status)).length, missing: pageSpeedCheckIds.length - checks.filter(check => check.status !== 'not_reported').length }
  }
  const openDiagnostics = () => { if (diagnostics) { diagnostics.open = true; diagnostics.scrollIntoView({ behavior: 'smooth', block: 'start' }) } }
  const scores = () => [
    { key: 'performance', label: i18n.tr('Ytelse', 'Performance'), value: measurement()?.performance },
    { key: 'accessibility', label: i18n.tr('Tilgjengelighet', 'Accessibility'), value: measurement()?.accessibility },
    { key: 'bestPractices', label: i18n.tr('Beste praksis', 'Best practices'), value: measurement()?.bestPractices },
    { key: 'seo', label: 'SEO', value: measurement()?.seo },
  ]
  return <section class="pagespeed-panel" aria-labelledby={`${id}-title`} data-testid="pagespeed-panel">
    <header class="pagespeed-panel__header"><div><span class="pagespeed-panel__eyebrow">Google PageSpeed Insights</span><h2 id={`${id}-title`}>{i18n.tr('Nettstedskvalitet', 'Website quality')}</h2><p>{i18n.tr('En labmåling fra nettstedets forside. Et tydelig utgangspunkt.', 'A lab audit starting from the website’s homepage. A clear starting point.')}</p></div>
      <div class="pagespeed-panel__actions"><label><span>{i18n.tr('Enhet', 'Device')}</span><select value={strategy()} onChange={(event) => setStrategy(event.currentTarget.value as PageSpeedStrategy)}><option value="mobile">{i18n.tr('Mobil', 'Mobile')}</option><option value="desktop">{i18n.tr('Datamaskin', 'Desktop')}</option></select></label><button type="button" disabled={loading() || refreshing()} onClick={run}>{i18n.tr('Kjør måling', 'Run audit')}</button></div>
    </header>
    <div aria-live="polite" class="pagespeed-panel__state">
      <Show when={measurement() && snapshot()?.cache}>
        <div class="pagespeed-panel__cache" data-stale={snapshot()?.cache?.stale} data-testid="pagespeed-cache-state">
          <div><strong>{snapshot()?.cache?.stale ? i18n.tr('Lagret rapport · eldre enn ett døgn', 'Saved report · more than a day old') : i18n.tr('Lagret rapport', 'Saved report')}</strong><span>{i18n.tr('Sist målt', 'Last measured')} {dateTime(measurement()?.fetchTime)}</span></div>
          <p>{refreshing() ? i18n.tr('Oppdateres i bakgrunnen. Siste vellykkede måling vises imens.', 'Refreshing in the background. The last successful measurement stays visible.') : snapshot()?.cache?.refreshStatus === 'failed' ? i18n.tr('Oppdateringen mislyktes. Siste vellykkede måling beholdes.', 'The refresh failed. The last successful measurement is retained.') : i18n.tr('Oppdateres automatisk hvert døgn.', 'Refreshes automatically every day.')}</p>
          <Show when={snapshot()?.cache?.nextRefreshAt && !refreshing()}><small>{snapshot()?.cache?.refreshStatus === 'failed' ? i18n.tr('Neste forsøk', 'Next attempt') : i18n.tr('Neste oppdatering', 'Next refresh')}: {dateTime(snapshot()?.cache?.nextRefreshAt)}</small></Show>
          <Show when={snapshot()?.cache?.refreshStatus === 'failed'}><small>{reason()}</small></Show>
          <Show when={refreshing() && pollExpired()}><button type="button" onClick={refresh}>{i18n.tr('Oppdater status', 'Refresh status')}</button></Show>
        </div>
      </Show>
      <Show when={failed()}><p role="alert">{i18n.tr('PageSpeed kunne ikke nås. Ingen nye målinger er tilgjengelige.', 'PageSpeed could not be reached. No new measurements are available.')} <button type="button" onClick={refresh}>{i18n.tr('Oppdater status', 'Refresh status')}</button></p></Show>
      <Show when={!failed() && !snapshot() && loading()}><p>{i18n.tr('Henter siste målestatus…', 'Loading last audit status…')}</p></Show>
      <Show when={!failed() && snapshot()?.status === 'idle'}><p role="status">{waitingPreparation() ? i18n.tr('Nettstedet klargjøres i bakgrunnen. Målingen vises automatisk når den er klar.', 'Website preparation is running in the background. The measurement appears automatically when ready.') : i18n.tr('Ingen lagret måling for denne enheten ennå. Automatisk innhenting klargjøres i bakgrunnen.', 'No saved measurement for this device yet. Automatic collection is being prepared in the background.')}<Show when={waitingPreparation() && pollExpired()}><button type="button" onClick={refresh}>{i18n.tr('Oppdater status', 'Refresh status')}</button></Show></p></Show>
      <Show when={!failed() && snapshot()?.status === 'failed'}><p role="status">{reason()}</p><Show when={snapshot()?.cache?.nextRefreshAt}><p>{i18n.tr('Neste automatiske forsøk', 'Next automatic attempt')}: {dateTime(snapshot()?.cache?.nextRefreshAt)}</p></Show></Show>
      <Show when={!failed() && snapshot()?.status === 'running'}><p role="status">{pollExpired() ? i18n.tr('Målingen tar fortsatt tid. Oppdater status for å sjekke igjen.', 'The audit is still taking time. Refresh status to check again.') : i18n.tr('Google måler nettstedet. Dette kan ta litt tid…', 'Google is measuring the website. This can take a little time…')} <Show when={pollExpired()}><button type="button" onClick={refresh}>{i18n.tr('Oppdater status', 'Refresh status')}</button></Show></p></Show>
    </div>
      <div class="pagespeed-panel__scores"><For each={scores()}>{(score) => <div data-testid={`pagespeed-${score.key}`}><span>{score.label}</span><strong>{number(score.value)}</strong><small>{i18n.tr('Labpoeng av 100', 'Lab score out of 100')}</small></div>}</For></div>
      <dl class="pagespeed-panel__metrics"><div><dt>LCP <span>lab</span></dt><dd>{number(measurement()?.lcpMs, 0)} ms</dd></div><div><dt>CLS <span>lab</span></dt><dd>{number(measurement()?.cls, 3)}</dd></div><div><dt>TBT <span>lab</span></dt><dd>{number(measurement()?.tbtMs)} ms</dd></div></dl>
    <Show when={measurement()}>
      <p class="pagespeed-panel__timestamp">{i18n.tr('Målt', 'Measured')} {snapshot()?.report?.fetchTime ? new Intl.DateTimeFormat(localeDateTime(i18n.locale()), { dateStyle: 'medium', timeStyle: 'short' }).format(new Date(snapshot()!.report!.fetchTime)) : '—'} · Lighthouse {snapshot()?.report?.lighthouseVersion}</p>
      <Show when={snapshot()?.report?.reportingOrigin}>{(origin)=><p class="pagespeed-panel__timestamp">{i18n.tr('Målt opphav etter omdirigering','Measured origin after redirects')}: <code>{origin()}</code></p>}</Show>
      <div class="pagespeed-panel__check-summary" role="group" aria-label={i18n.tr('Oppsummering av tekniske kontroller', 'Technical check summary')}>
        <div><span>{i18n.tr('Bestått', 'Passed')}</span><strong>{checkCounts().passed}</strong></div>
        <button type="button" onClick={openDiagnostics}><span>{i18n.tr('Trenger vurdering', 'Needs review')}</span><strong>{checkCounts().review}</strong><small>{i18n.tr('Se kontroller', 'View checks')} ↗</small></button>
        <div><span>{i18n.tr('Ikke rapportert', 'Not reported')}</span><strong>{checkCounts().missing}</strong></div>
      </div>
      <ReportDisclosure ref={element => { diagnostics = element }} source="Lighthouse" title={i18n.tr('Diagnostikk og forbedringer', 'Diagnostics and improvements')}><PageSpeedDiagnostics checks={snapshot()?.report?.checks} canSave={!!props.websiteId && !!props.revision} savedIds={savedIds()} savingId={savingId()} onSave={(checkId) => void saveCheck(checkId)} /></ReportDisclosure>
      <Show when={saveError()}><p role="alert">{saveError()} <button type="button" onClick={refresh}>{i18n.tr('Oppdater status', 'Refresh status')}</button></p></Show>
    </Show>
    <footer>{i18n.tr('Lighthouse-labtest, ikke feltdata eller en Core Web Vitals-vurdering. TBT er ikke INP. Resultatet gjelder måletidspunktet, ikke valgt kvartal. Siste vellykkede rapport lagres i Synlighet og oppdateres automatisk etter 24 timer.', 'Lighthouse lab test, not field data or a Core Web Vitals assessment. TBT is not INP. Results describe the audit time, not the selected quarter. The last successful report is saved in Visibility and refreshed automatically after 24 hours.')}</footer>
  </section>
}
