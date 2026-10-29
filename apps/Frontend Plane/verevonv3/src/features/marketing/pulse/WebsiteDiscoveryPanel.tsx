import { For, Show, createEffect, createSignal, createUniqueId, onCleanup } from 'solid-js'
import { localeDateTime, useI18n } from '@/shared/i18n'
import { loadWebsiteDiscovery, startWebsiteDiscovery, type DiscoveryContext, type WebsiteDiscovery } from '../../../shared/api/marketing/discovery-client.ts'
import './website-discovery-panel.css'

/** A temporary, bounded URL inventory, not a multi-page audit or crawl completeness claim. */
export function WebsiteDiscoveryPanel(props: { site: string; websiteId?: string; revision?: number }) {
  const i18n = useI18n(); const id = createUniqueId()
  const [snapshot, setSnapshot] = createSignal<WebsiteDiscovery | null>(null)
  const [loading, setLoading] = createSignal(false)
  const [failed, setFailed] = createSignal(false)
  const [expired, setExpired] = createSignal(false)
  let generation = 0; let controller: AbortController | undefined
  let timer: ReturnType<typeof setTimeout> | undefined
  let deadline = 0
  const clearTimer = () => { if (timer !== undefined) clearTimeout(timer); timer = undefined }
  async function request(start: boolean, context: DiscoveryContext, version: number) {
    clearTimer(); setLoading(true); setFailed(false)
    const current = new AbortController(); controller = current
    try {
      const result = await (start ? startWebsiteDiscovery : loadWebsiteDiscovery)(context, current.signal)
      if (current.signal.aborted || version !== generation) return
      setSnapshot(result)
      if (result.status === 'running') {
        if (Date.now() + 3000 < deadline) timer = setTimeout(() => { void request(false, context, version) }, 3000)
        else setExpired(true)
      }
    } catch { if (!current.signal.aborted && version === generation) setFailed(true) }
    finally { if (!current.signal.aborted && version === generation) setLoading(false) }
  }
  createEffect(() => {
    const context = { site: props.site, websiteId: props.websiteId, websiteRevision: props.revision }
    const version = ++generation
    setSnapshot(null); setFailed(false); setExpired(false); setLoading(false)
    deadline = Date.now() + 45_000
    if (context.websiteId && context.websiteRevision) void request(false, { ...context, websiteId: context.websiteId, websiteRevision: context.websiteRevision }, version)
    onCleanup(() => { controller?.abort(); clearTimer() })
  })
  const refresh = (start: boolean) => {
    if (!props.websiteId || !props.revision || loading()) return
    controller?.abort(); clearTimer(); setExpired(false); deadline = Date.now() + 45_000
    void request(start, { site: props.site, websiteId: props.websiteId, websiteRevision: props.revision }, generation)
  }
  const reason = () => {
    if (snapshot()?.reason === 'not_configured') return i18n.tr('Quarry er ikke konfigurert for sideoppdagelse. Ingen sider er hentet.', 'Quarry is not configured for page discovery. No pages have been fetched.')
    if (snapshot()?.reason === 'access_denied') return i18n.tr('Quarry avviste tilgangen. Kontroller nettstedstillatelser og tjenestetilgang.', 'Quarry denied access. Check website permissions and service access.')
    if (snapshot()?.reason === 'policy_blocked') return i18n.tr('Quarry blokkerte en nettadresse eller videresending av sikkerhetshensyn. Dette er ikke en feil med Google-tilkoblingen.', 'Quarry blocked a URL or redirect under its safety policy. This is not a Google connection failure.')
    if (snapshot()?.reason === 'quota_limited') return i18n.tr('Sideoppdagelse ble avvist på grunn av kvote. Prøv igjen senere.', 'Page discovery was rejected due to quota. Try again later.')
    return i18n.tr('Sideoppdagelse kunne ikke fullføres. Ingen revisjonsresultater er tilgjengelige.', 'Page discovery could not complete. No audit results are available.')
  }
  return <section class="website-discovery-panel" aria-labelledby={`${id}-title`} data-testid="website-discovery-panel">
    <header><div><span class="website-discovery-panel__eyebrow">Quarry-v2</span><h2 id={`${id}-title`}>{i18n.tr('Sideoppdagelse', 'Page discovery')}</h2><p>{i18n.tr('Oppdag inntil 250 offentlige adresser fra nettstedet og dets eksakte www-variant.', 'Discover up to 250 public URLs on this site and its exact www counterpart.')}</p></div>
      <div class="website-discovery-panel__actions"><button type="button" disabled={!props.websiteId || !props.revision || loading()} onClick={() => refresh(false)}>{i18n.tr('Oppdater status', 'Refresh status')}</button><button type="button" disabled={!props.websiteId || !props.revision || loading() || snapshot()?.status === 'running'} onClick={() => refresh(true)}>{i18n.tr('Oppdag sider', 'Discover pages')}</button></div>
    </header>
    <div class="website-discovery-panel__state" aria-live="polite">
      <Show when={!props.websiteId || !props.revision}><p>{i18n.tr('Velg et registrert nettsted først.', 'Select a registered website first.')}</p></Show>
      <Show when={loading()}><p>{i18n.tr('Henter oppdagelsesstatus…', 'Loading discovery status…')}</p></Show>
      <Show when={failed()}><p role="alert">{i18n.tr('Status kunne ikke bekreftes. Oppdater status før du prøver å starte på nytt.', 'The status could not be confirmed. Refresh status before trying another start.')}</p></Show>
      <Show when={!failed() && snapshot()?.status === 'idle'}><p>{i18n.tr('Ingen oppdagelse er kjørt. Start når du er klar.', 'No discovery has run. Start when you are ready.')}</p></Show>
      <Show when={!failed() && snapshot()?.status === 'running'}><p>{i18n.tr('Quarry arbeider. Statuslesing starter ikke nye hentinger.', 'Quarry is working. Reading status does not start new fetches.')}</p></Show>
      <Show when={!failed() && snapshot()?.status === 'failed'}><p role="status">{reason()}</p></Show>
      <Show when={expired()}><p>{i18n.tr('Automatisk statuslesing stoppet etter 45 sekunder. Oppdater status manuelt; ikke start en ny kjøring.', 'Automatic status reading stopped after 45 seconds. Refresh status manually; do not start another run.')}</p></Show>
    </div>
    <Show when={snapshot()?.status === 'complete'}><Show when={snapshot()?.coverage}>{c=><p>{c().sitemapValid}/{c().sitemapFiles} {i18n.tr('nettstedskart lest','sitemaps read')} · {c().candidates} {i18n.tr('kandidater før filtrering','candidates before filtering')} · {c().robotsFiltered} {i18n.tr('filtrert av robots-regler','filtered by robots rules')}{c().truncated?` · ${i18n.tr('Avkortet utvalg','Truncated sample')}`:''}</p>}</Show>
      <ul class="website-discovery-panel__pages"><For each={snapshot()?.pages} fallback={<li>{i18n.tr('Ingen adresser ble returnert innenfor denne avgrensningen.', 'No URLs were returned within this bounded discovery.')}</li>}>{(page) => <li><a href={page.url} target="_blank" rel="noopener noreferrer" referrerPolicy="no-referrer"><Show when={page.title}><strong>{page.title}</strong></Show><span>{page.url}</span></a></li>}</For></ul>
      <p class="website-discovery-panel__receipt">{i18n.tr('Fullført lokalt', 'Locally completed')} {snapshot()?.discoveredAt ? new Intl.DateTimeFormat(localeDateTime(i18n.locale()), { dateStyle: 'medium', timeStyle: 'short' }).format(new Date(snapshot()!.discoveredAt!)) : '—'}</p>
    </Show>
    <footer>{i18n.tr('Delvis, midlertidig URL-inventar – ikke reviderte sider eller en fullstendig nettstedstelling. Grensen er 250 adresser; antall og tittel er ikke bevis på at sidene er kontrollert. Fullføringstidspunktet er en lokal kvittering, ikke et hentetidspunkt fra nettstedet.', 'Partial, temporary URL inventory — not audited pages or an exhaustive site count. The limit is 250 URLs; listing a URL or title does not mean its page was checked. Completion time is a local receipt, not a website fetch timestamp.')}</footer>
  </section>
}
