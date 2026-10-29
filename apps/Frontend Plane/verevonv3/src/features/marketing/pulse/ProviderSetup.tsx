import { GoogleResourcePicker } from './GoogleResourcePicker'
import { Show, createEffect, createSignal, on, onCleanup } from 'solid-js'
import { useI18n } from '@/shared/i18n'
import { connectWebsiteGoogle, websiteGoogleStatus, verifyWebsite, saveClarityMapping, type GoogleAuthorization, type Website } from '@/shared/api/marketing/website-client'
import { AlgoliaConnectionPanel } from './AlgoliaConnectionPanel'
import type { PulseServiceKey } from './ConnectedServices'

export function ProviderSetup(props: { provider: PulseServiceKey; website: Website; canManage: boolean; onUpdated: (value: Website) => void; onCheck: () => Promise<'ready' | 'pending' | 'error'> }) {
  const i18n = useI18n()
  const [busy, setBusy] = createSignal(false), [error, setError] = createSignal(''), [notice, setNotice] = createSignal('')
  const [authorization, setAuthorization] = createSignal<GoogleAuthorization | null>(null), [project, setProject] = createSignal('')
  let controller: AbortController | undefined, timer: ReturnType<typeof setTimeout> | undefined
  createEffect(on(() => `${props.website.id}|${props.website.site}|${props.provider}|${props.canManage}`, () => {
    controller?.abort(); clearTimeout(timer); setBusy(false); setError(''); setNotice(''); setAuthorization(null); setProject(props.website.integrations.clarity.projectId ?? '')
  }))
  onCleanup(() => { controller?.abort(); clearTimeout(timer) })
  async function checkSetup() {
    if (busy()) return
    setBusy(true); setNotice(''); setError('')
    const result = await props.onCheck()
    setBusy(false)
    if (result === 'error') setError(i18n.tr('Oppsettet eller rapporten kunne ikke kontrolleres. Prøv igjen; andre tilkoblinger påvirkes ikke.', 'Setup or reporting could not be checked. Retry; other connections are unaffected.'))
    else setNotice(result === 'ready' ? i18n.tr('Tilkoblingen er klar. Tilgjengelige rapporter oppdateres.', 'The connection is ready. Available reports are being updated.') : i18n.tr('Venter på fullført oppsett i Integration Core. Ingen tilkobling er lagt til ennå.', 'Waiting for completed setup in Integration Core. No connection has been added yet.'))
  }
  async function poll(session: GoogleAuthorization, current: AbortController) {
    if (current.signal.aborted) return
    if (Date.now() >= Date.parse(session.expiresAt)) { setBusy(false); setNotice(i18n.tr('Tilkoblingen utløp. Prøv igjen.', 'The connection expired. Try again.')); return }
    try {
      const value = await websiteGoogleStatus(props.website.id, session.sessionId, current.signal)
      if (current.signal.aborted) return
      if (value.status === 'completed') {
        const website = await verifyWebsite(props.website.id, current.signal)
        if (current.signal.aborted) return
        setBusy(false); setAuthorization(null); props.onUpdated(website); props.onCheck()
        setNotice(i18n.tr('Google er tilkoblet. Tilgjengelige rapporter hentes automatisk.', 'Google is connected. Available reports are collected automatically.')); return
      }
      if (value.status !== 'pending') { setBusy(false); setNotice(i18n.tr('Tilkoblingen ble ikke fullført. Du kan prøve igjen.', 'The connection did not complete. You can try again.')); return }
    } catch { if (current.signal.aborted) return }
    timer = setTimeout(() => { void poll(session, current) }, 3000)
  }
  async function connectGoogle() {
    if (!props.canManage || busy()) return
    controller?.abort(); clearTimeout(timer); controller = new AbortController(); const current = controller
    // Open synchronously from the human gesture, then navigate only to the
    // strict Google authorization URL validated by the shared API client.
    const popup = window.open('about:blank', '_blank')
    if (popup) popup.opener = null
    setBusy(true); setError(''); setNotice('')
    try {
      const session = await connectWebsiteGoogle(props.website.id, current.signal)
      if (current.signal.aborted) { popup?.close(); return }
      setAuthorization(session)
      if (popup) popup.location.href = session.authorizationUrl
      void poll(session, current)
    } catch {
      popup?.close(); if (!current.signal.aborted) { setBusy(false); setError(i18n.tr('Google-tilkoblingen kunne ikke startes. Prøv igjen.', 'The Google connection could not be started. Try again.')) }
    }
  }
  async function saveProject(event: SubmitEvent) {
    event.preventDefault(); if (!props.canManage || busy()) return
    controller?.abort(); controller = new AbortController(); const current = controller
    setBusy(true); setError('')
    try { const website = await saveClarityMapping(props.website.id, project(), props.website.revision, current.signal); if (!current.signal.aborted) { props.onUpdated(website); props.onCheck(); setNotice(i18n.tr('Prosjektet er valgt. Privat rapporttilgang krever fortsatt eksportnøkkelen i Integration Core.', 'Project selected. Private reports still require the export credential in Integration Core.')) } }
    catch { if (!current.signal.aborted) setError(i18n.tr('Prosjektet kunne ikke lagres. Oppdater nettstedet og prøv igjen.', 'The project could not be saved. Refresh the website and retry.')) }
    finally { if (!current.signal.aborted) setBusy(false) }
  }
  return <div class="pulse-provider-setup" aria-busy={busy()}>
    <Show when={props.canManage} fallback={<p>{i18n.tr('En administrator i arbeidsområdet kan legge til eller reparere denne tilkoblingen.', 'A workspace administrator can add or repair this connection.')}</p>}>
      <Show when={props.provider === 'search_console' || props.provider === 'ga4'}>
        <p>{i18n.tr('Én Google-tilkobling gir lesetilgang til tilgjengelige Search Console- og Analytics-rapporter. Vi finner nettstedets egenskaper og riktig rapporteringsvert.', 'One Google connection provides read access to available Search Console and Analytics reports. We find the website’s properties and reporting host.')}</p>
        <button type="button" disabled={busy()} onClick={() => { void connectGoogle() }}>{busy() ? i18n.tr('Venter på Google…', 'Waiting for Google…') : i18n.tr('Koble til Google', 'Connect Google')}</button>
        <Show when={authorization()}>{session => <p><a href={session().authorizationUrl} target="_blank" rel="noopener noreferrer">{i18n.tr('Fortsett hos Google hvis vinduet ikke åpnet seg', 'Continue at Google if the window did not open')}</a></p>}</Show>
        <GoogleResourcePicker website={props.website} canManage={props.canManage} onUpdated={props.onUpdated}/><p>{i18n.tr('Har du allerede koblet til? Vi kontrollerer nye tilganger automatisk. Flere passende prosjekter kan kreve et valg.', 'Already connected? New access is checked automatically. Multiple matching projects may require a selection.')}</p>
      </Show>
      <Show when={props.provider === 'algolia'}><AlgoliaConnectionPanel websiteId={props.website.id} site={props.website.site} revision={props.website.revision} canManageIntegrations={props.canManage} onMappingChanged={props.onCheck}/></Show>
      <Show when={props.provider === 'clarity'}>
        <p>{i18n.tr('Clarity bruker en prosjekteksportnøkkel, ikke Googles innlogging. Velg prosjektet her. En administrator legger nøkkelen i Integration Core én gang.', 'Clarity uses a project export credential, not Google sign-in. Select the project here. An administrator adds its credential to Integration Core once.')}</p>
        <form onSubmit={event => { void saveProject(event) }}><label for="pulse-clarity-project">{i18n.tr('Clarity prosjekt-ID', 'Clarity project ID')}</label><input id="pulse-clarity-project" required pattern="[a-z0-9]{4,32}" value={project()} onInput={event => setProject(event.currentTarget.value)} disabled={busy()}/><button type="submit" disabled={busy()}>{i18n.tr('Velg prosjekt', 'Select project')}</button></form>
        <p><a href="https://clarity.microsoft.com/projects" target="_blank" rel="noopener noreferrer">{i18n.tr('Åpne Clarity-prosjekter', 'Open Clarity projects')} ↗</a></p>
      </Show>
      <Show when={props.provider === 'cookiebot'}><p>{i18n.tr('Koble eksisterende Cookiebot-rapporter til nettstedet. I dette oppsettet brukes domenegruppe og Data API-nøkkel.', 'Connect existing Cookiebot reports to this website. This setup uses a domain group and Data API key.')}</p><ol><li>{i18n.tr('Finn nettstedets domenegruppe i Cookiebot.', 'Find this website’s domain group in Cookiebot.')}</li><li>{i18n.tr('En administrator legger Data API-nøkkelen og domenegruppen i Integration Core. Ikke lim inn nøkkelen her.', 'An administrator adds the Data API key and domain group in Integration Core. Do not paste the key here.')}</li><li>{i18n.tr('Kontroller oppsettet nedenfor. Integrasjonen vises når koblingen er klar.', 'Check setup below. The integration appears when its binding is ready.')}</li></ol><a href="https://manage.cookiebot.com/" target="_blank" rel="noopener noreferrer">{i18n.tr('Åpne Cookiebot', 'Open Cookiebot')} ↗</a></Show>
      <Show when={props.provider === 'web_vitals'}><p>{i18n.tr('Web Vitals trenger en samtykkestyrt måleinstallasjon på nettstedet og en sikker eksportkobling i Integration Core. Vi installerer ikke sporingskode uten en egen publiseringshandling.', 'Web Vitals needs a consent-controlled collector on the website and a secure export binding in Integration Core. Tracking code is not installed without a separate publication action.')}</p><p>{i18n.tr('Når installasjonen er koblet til, vises panelet automatisk. Utviklingsdata holdes atskilt fra produksjon.', 'Once the installation is connected, its panel appears automatically. Development data stays separate from production.')}</p></Show>
      <Show when={props.provider !== 'search_console' && props.provider !== 'ga4' && props.provider !== 'algolia'}><p><button type="button" disabled={busy()} onClick={() => { void checkSetup() }}>{i18n.tr(busy() ? 'Kontrollerer…' : 'Kontroller oppsett', busy() ? 'Checking…' : 'Check setup')}</button></p></Show>
    </Show>
    <Show when={notice()}><p role="status">{notice()}</p></Show><Show when={error()}><p role="alert">{error()}</p></Show>
  </div>
}
