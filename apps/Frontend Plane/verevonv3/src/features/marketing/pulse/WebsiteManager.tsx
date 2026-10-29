import { For, Show, createEffect, createMemo, createSignal, on, onCleanup, untrack } from 'solid-js'
import { useI18n } from '@/shared/i18n'
import { GoogleResourcePicker } from './GoogleResourcePicker'
import { addWebsite, saveWebsite, verifyWebsite, connectWebsiteGoogle, websiteGoogleStatus, saveClarityMapping, saveGA4ReportingSite, ga4ReportingSiteOptions, type Website, type GoogleAuthorization } from '../../../shared/api/marketing/website-client.ts'
import './website-manager.css'
import { WebsitePreparationPanel } from './WebsitePreparationPanel'

type Props = { mode?: 'websites' | 'full'; canManageIntegrations?: boolean; websites: readonly Website[]; selected?: Website; onSelect: (id: string) => void; onUpdated: (website: Website) => void; onClose: () => void }

function canMapGA4ReportingSite(website: Website): boolean {
  return website.integrations.ga4.status === 'connected' && /^\d{1,20}$/.test(website.integrations.ga4.propertyId ?? '')
}

/** A backend registry, not a browser list. Provider creation is intentionally not exposed. */
export function WebsiteManager(props: Props) {
  const i18n = useI18n()
  const canManageIntegrations = () => props.canManageIntegrations ?? true
  const [input, setInput] = createSignal('')
  const [editing, setEditing] = createSignal(false)
  const [advanced, setAdvanced] = createSignal(false)
  const [busy, setBusy] = createSignal(false)
  const [error, setError] = createSignal('')
  const [message, setMessage] = createSignal('')
  const [authorization, setAuthorization] = createSignal<GoogleAuthorization | null>(null)
  const [clarity, setClarity] = createSignal('')
  const [ga4ReportingSite, setGA4ReportingSite] = createSignal('')
  let controller: AbortController | undefined
  let generation = 0
  const identity = createMemo(() => props.selected ? `${props.selected.id}:${props.selected.site}` : '')
  createEffect(on(identity, () => {
    const site = props.selected?.site
    generation++
    controller?.abort()
    setBusy(false); setEditing(false); setInput(''); setError(''); setMessage(''); setAuthorization(null)
    setClarity(site ? props.selected?.integrations.clarity.projectId ?? '' : '')
    setGA4ReportingSite(site ? props.selected?.integrations.ga4.reportingSite ?? site : '')
  }))
  createEffect(on(canManageIntegrations, (allowed) => {
    if (allowed) return
    generation++
    controller?.abort()
    setBusy(false); setEditing(false); setInput(''); setError(''); setMessage(''); setAuthorization(null)
  }))
  onCleanup(() => { generation++; controller?.abort() })
  async function perform<T>(run: (signal: AbortSignal) => Promise<T>, apply: (value: T) => void) {
    if (!canManageIntegrations() || busy()) return
    const version = generation
    controller = new AbortController(); const current = controller
    setBusy(true); setError(''); setMessage('')
    try { const value = await run(current.signal); if (canManageIntegrations() && !current.signal.aborted && version === generation) apply(value) }
    catch (failure) { if (!current.signal.aborted && version === generation) setError(failure instanceof Error ? failure.message : i18n.tr('Forespørselen mislyktes.', 'Request failed.')) }
    finally { if (!current.signal.aborted && version === generation) setBusy(false) }
  }
  const update = (website: Website) => { props.onUpdated(website); setMessage(i18n.tr('Nettstedet er oppdatert. Tilgang er kontrollert separat for hver kilde.', 'Website updated. Access is checked separately for every source.')) }
  function submit(event: SubmitEvent) {
    event.preventDefault()
    const selected = props.selected
    const edit = editing(); const site = input()
    void perform((signal) => edit && selected ? saveWebsite(selected.id, site, selected.revision, signal) : addWebsite(site, signal), (website) => untrack(() => { props.onUpdated(website); setInput(''); setEditing(false) }))
  }
  function checkGoogle() {
    const selected = props.selected; const session = authorization()
    if (!selected || !session) return
    void perform(async (signal) => {
      const result = await websiteGoogleStatus(selected.id, session.sessionId, signal)
      return { ...result, website: result.status === 'completed' ? await verifyWebsite(selected.id, signal) : undefined }
    }, (result) => untrack(() => {
      setMessage(result.status === 'completed' ? i18n.tr('Google er tilkoblet og nettstedskoblingene er kontrollert.', 'Google connected and website mappings checked.') : result.status === 'pending' ? i18n.tr('Venter på Google-samtykke i den nye fanen.', 'Waiting for Google consent in the new tab.') : i18n.tr('Google-tilkoblingen ble ikke fullført. Start tilkoblingen på nytt.', 'Google connection did not finish. Start the connection again.'))
      if (result.status === 'completed') setAuthorization(null)
      if (result.website) props.onUpdated(result.website)
    }))
  }
  function submitClarity(event: SubmitEvent) {
    event.preventDefault()
    const selected = props.selected; const projectId = clarity()
    if (selected) void perform((signal) => saveClarityMapping(selected.id, projectId, selected.revision, signal), update)
  }
  function submitGA4ReportingSite(event: SubmitEvent) {
    event.preventDefault()
    const selected = props.selected
    const reportingSite = ga4ReportingSite()
    if (!selected || !canMapGA4ReportingSite(selected)) return
    void perform((signal) => saveGA4ReportingSite(selected.id, selected.site, reportingSite, selected.revision, signal), update)
  }
  const statusLabel = (status: string) => ({
    connected: i18n.tr('Tilkoblet', 'Connected'), available: i18n.tr('Tilgjengelig', 'Available'), unmapped: i18n.tr('Ikke koblet', 'Not mapped'), needs_connection: i18n.tr('Koble til Google', 'Connect Google'), not_connected: i18n.tr('Ikke tilkoblet', 'Not connected'),
    requires_consent: i18n.tr('Trenger samtykke', 'Consent required'), needs_selection: i18n.tr('Trenger valg av prosjekt', 'Project selection required'), not_found: i18n.tr('Ingen samsvarende tilgang funnet', 'No matching access found'), source_unavailable: i18n.tr('Kilden er utilgjengelig', 'Source unavailable'), pending_verification: i18n.tr('Venter på prosjektverifisering', 'Pending project verification'),
  })[status] ?? status
  return <section id="marketing-pulse-sites" class="website-manager" aria-label={i18n.tr('Nettsider', 'Websites')} aria-busy={busy()}>
    <header><div><strong>{i18n.tr('Nettsider og integrasjoner', 'Websites and integrations')}</strong><p>{i18n.tr('Felles nettstedskontekst for mennesker og AI.', 'Shared website context for people and AI.')}</p></div><button type="button" onClick={() => props.onClose()} aria-label={i18n.tr('Lukk', 'Close')}>×</button></header>
    <div class="website-manager__list"><For each={props.websites} fallback={<p>{i18n.tr('Ingen nettsteder lagt til ennå.', 'No websites added yet.')}</p>}>{(website) => <button type="button" aria-pressed={props.selected?.id === website.id} classList={{ 'is-active': props.selected?.id === website.id }} onClick={() => props.onSelect(website.id)}>{new URL(website.site).hostname}</button>}</For></div>
    <Show when={canManageIntegrations()} fallback={<p class="website-manager__notice">{i18n.tr('En administrator i arbeidsområdet må administrere nettsteder og integrasjoner. Du kan velge nettsted og lese eksisterende koblinger.', 'A workspace administrator must manage websites and integrations. You can select a website and read existing mappings.')}</p>}>
    <form onSubmit={submit} class="website-manager__url-form"><label for="marketing-pulse-site-input">{i18n.tr('Nettadresse', 'Website URL')}</label><div><input id="marketing-pulse-site-input" type="text" inputmode="url" autocomplete="url" placeholder="https://example.com/" value={input()} onInput={(event) => setInput(event.currentTarget.value)} required disabled={busy()} /><button type="submit" disabled={busy()}>{editing() ? i18n.tr('Lagre nettadresse', 'Save URL') : i18n.tr('Legg til nettsted', 'Add website')}</button></div><Show when={editing()}><p>{i18n.tr('Endring av URL nullstiller nettstedskoblinger. Leverandørtilgang kontrolleres på nytt.', 'Changing the URL resets site mappings. Provider access is checked again.')}</p><button type="button" onClick={() => { setEditing(false); setInput('') }}>{i18n.tr('Avbryt', 'Cancel')}</button></Show></form>
    </Show>
    <Show when={props.selected}>{(selected) => <div class="website-manager__detail">
      <div class="website-manager__selected"><strong>{new URL(selected().site).hostname}</strong><Show when={canManageIntegrations()}><button type="button" disabled={busy()} onClick={() => { setEditing(true); setInput(selected().site) }}>{i18n.tr('Endre URL', 'Edit URL')}</button></Show></div>
      <Show when={props.mode === 'websites'}><p>{i18n.tr('Offentlige kontroller starter når du legger til nettadressen. Bruk + i Synlighet for å koble til flere kontoer.', 'Public checks start when you add the URL. Use + in Visibility to connect more accounts.')}</p><button type="button" aria-expanded={advanced()} onClick={() => setAdvanced(value => !value)}>{i18n.tr('Avanserte datakildeinnstillinger', 'Advanced source settings')}</button></Show>
      <Show when={props.mode !== 'websites' || advanced()}>
      <dl class="website-manager__mappings">
        <For each={['searchConsole', 'ga4', 'clarity', 'pageSpeed'] as const}>{(key) => <div data-testid={`website-${key}`}><dt>{{ searchConsole: 'Search Console', ga4: 'Google Analytics 4', clarity: 'Microsoft Clarity', pageSpeed: 'PageSpeed Insights' }[key]}</dt><dd>{statusLabel(selected().integrations[key].status)}</dd></div>}</For>
      </dl>
      <p class="website-manager__explanation">{i18n.tr('Hver leverandør kontrollerer kontotilgang til sine rapporter. Rapporttilgang er ikke bevis på eierskap til nettstedet.', 'Each provider checks account access to its reports. Reporting access is not proof of website ownership.')}</p>
      <WebsitePreparationPanel websiteId={selected().id} site={selected().site} revision={selected().revision} canManageIntegrations={canManageIntegrations()} />
      <Show when={canManageIntegrations()}>
      <div class="website-manager__actions"><button type="button" disabled={busy()} onClick={() => { void perform((signal) => verifyWebsite(selected().id, signal), update) }}>{i18n.tr('Kontroller tilganger', 'Check access')}</button><button type="button" disabled={busy()} onClick={() => { void perform((signal) => connectWebsiteGoogle(selected().id, signal), setAuthorization) }}>{i18n.tr('Koble til Google-konto', 'Connect Google account')}</button></div>
      <GoogleResourcePicker website={selected()} canManage={canManageIntegrations()} onUpdated={props.onUpdated}/><Show when={authorization()}>{(session) => <div class="website-manager__notice"><p>{i18n.tr('Fortsett hos Google for å godkjenne lesetilgang til Search Console og Analytics. Ingen prosjekter opprettes automatisk.', 'Continue at Google to approve read-only Search Console and Analytics access. No projects are created automatically.')}</p><a href={session().authorizationUrl} target="_blank" rel="noopener noreferrer">{i18n.tr('Fortsett til Google', 'Continue to Google')}</a><button type="button" disabled={busy()} onClick={checkGoogle}>{i18n.tr('Sjekk tilkobling', 'Check connection')}</button></div>}</Show>
      <details><summary>{i18n.tr('Microsoft Clarity-prosjekt', 'Microsoft Clarity project')}</summary><p>{i18n.tr('Lagre prosjekt-ID som en ventende kobling. Dette beviser ikke eierskap og kobler ikke til private data.', 'Save a project ID as a pending mapping. This does not prove ownership or connect private data.')}</p><form onSubmit={submitClarity}><label for="website-clarity-id">{i18n.tr('Clarity prosjekt-ID', 'Clarity project ID')}</label><input id="website-clarity-id" value={clarity()} onInput={(event) => setClarity(event.currentTarget.value)} pattern="[a-z0-9]{4,32}" required disabled={busy()} /><button type="submit" disabled={busy()}>{i18n.tr('Lagre ventende kobling', 'Save pending mapping')}</button></form></details>
      </Show>
      <section class="website-manager__ga4-reporting">
        <p data-testid="ga4-current-reporting-host">{i18n.tr('Gjeldende GA4-rapporteringsvert', 'Current GA4 reporting host')}: <strong>{new URL(selected().integrations.ga4.reportingSite ?? selected().site).hostname}</strong></p>
        <Show when={canManageIntegrations()}>
        <Show when={canMapGA4ReportingSite(selected())} fallback={<p>{i18n.tr('Koble til en GA4-egenskap med gjeldende rapporttilgang før rapporteringsverten endres.', 'Connect a GA4 property with current reporting access before changing the reporting host.')}</p>}>
          <form onSubmit={submitGA4ReportingSite}>
            <label for="website-ga4-reporting-site">{i18n.tr('GA4-rapporteringsvert', 'GA4 reporting host')}</label>
            <select id="website-ga4-reporting-site" value={ga4ReportingSite()} onChange={(event) => setGA4ReportingSite(event.currentTarget.value)} disabled={busy()}>
              <For each={ga4ReportingSiteOptions(selected().site)}>{(option) => <option value={option}>{new URL(option).hostname}</option>}</For>
            </select>
            <button type="submit" disabled={busy()}>{i18n.tr('Lagre GA4-rapporteringsvert', 'Save GA4 reporting host')}</button>
          </form>
        </Show>
        </Show>
      </section>
      </Show>
    </div>}</Show>
    <Show when={message()}><p role="status" class="website-manager__notice">{message()}</p></Show>
    <Show when={error()}><p role="alert" class="website-manager__error">{error()}</p></Show>
    <footer>{i18n.tr('Nettsteder lagres i Marketing Core. Bare valgt nettsted-ID lagres i denne nettleseren.', 'Websites are stored in Marketing Core. Only the selected website ID is stored in this browser.')}</footer>
  </section>
}
