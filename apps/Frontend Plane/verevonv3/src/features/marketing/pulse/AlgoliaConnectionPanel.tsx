/* eslint-disable solid/reactivity -- Event/async helpers intentionally read current state imperatively; every completion checks the captured site generation and abort signal. */
import { For, Show, batch, createEffect, createSignal, createUniqueId, on, onCleanup } from 'solid-js'
import { useI18n } from '@/shared/i18n'
import { disconnectAlgoliaSite, loadAlgoliaApplications, loadAlgoliaConnections, loadAlgoliaIndices, loadAlgoliaMapping, loadAlgoliaSession, saveAlgoliaMapping, startAlgoliaSession, type AlgoliaGrant, type AlgoliaMapping, type AlgoliaSession } from '@/shared/api/marketing/algolia-client'
import type { DiscoveryContext } from '@/shared/api/marketing/discovery-client'
import './algolia-connection-panel.css'

type Props = { websiteId: string; site: string; revision: number; canManageIntegrations?: boolean; onMappingChanged?: () => void }
/** Account authorization, resource discovery and site mapping are separate,
 * explicit operations. Mounting never requests provider analytics or consent. */
export function AlgoliaConnectionPanel(props: Props) {
  const i18n = useI18n(), id = createUniqueId()
  const [mapping, setMapping] = createSignal<AlgoliaMapping | null>(null), [grants, setGrants] = createSignal<AlgoliaGrant[]>([])
  const [connection, setConnection] = createSignal(''), [application, setApplication] = createSignal(''), [index, setIndex] = createSignal('')
  const [applications, setApplications] = createSignal<string[]>([]), [indices, setIndices] = createSignal<string[]>([])
  const [applicationMode, setApplicationMode] = createSignal<'discovered' | 'provided'>('discovered'), [providedApplication, setProvidedApplication] = createSignal('')
  const [indexProof, setIndexProof] = createSignal<{ connectionId: string; applicationId: string } | null>(null)
  const [loading, setLoading] = createSignal(true), [busy, setBusy] = createSignal(false), [error, setError] = createSignal(''), [message, setMessage] = createSignal(''), [refresh, setRefresh] = createSignal(0)
  const [mappingLoading, setMappingLoading] = createSignal(true), [mappingError, setMappingError] = createSignal('')
  const [session, setSession] = createSignal<AlgoliaSession | null>(null), [polling, setPolling] = createSignal(false)
  const [advanced,setAdvanced]=createSignal(false)
  const allowed = () => props.canManageIntegrations ?? false
  const context = (): DiscoveryContext => ({ websiteId: props.websiteId, site: props.site, websiteRevision: props.revision })
  const active = () => grants().some((grant) => grant.id === connection() && grant.status === 'active')
  const mappingReady = () => !mappingLoading() && mapping() !== null
  const requestedApplication = () => applicationMode() === 'provided' ? providedApplication() : application()
  const eligibleApplication = () => applicationMode() === 'provided' ? /^[A-Za-z0-9]{1,64}$/.test(providedApplication()) : applications().includes(application())
  const indexProofMatches = () => indexProof()?.connectionId === connection() && indexProof()?.applicationId === requestedApplication()
  const alreadySaved=()=>mapping()?.status==='mapped'&&mapping()?.connectionId===connection()&&mapping()?.applicationId===requestedApplication()&&mapping()?.indexName===index()
  const savedApplication=(grant:string,choices:string[])=>{const saved=mapping();return saved?.status==='mapped'&&saved.connectionId===grant&&saved.applicationId&&choices.includes(saved.applicationId)?saved.applicationId:''}
  const savedIndex=(grant:string,app:string,choices:string[])=>{const saved=mapping();return saved?.status==='mapped'&&saved.connectionId===grant&&saved.applicationId===app&&saved.indexName&&choices.includes(saved.indexName)?saved.indexName:''}
  let controller: AbortController | undefined, generation = 0, choiceGeneration = 0, timer: ReturnType<typeof setTimeout> | undefined, attempts = 0, autoGrant='',autoIndex=''
  const valid = (version: number, current: AbortController) => version === generation && !current.signal.aborted && allowed()
  const clearIndexProof = (resetAttempt=true) => { ++choiceGeneration;if(resetAttempt)autoIndex=''; setIndices([]); setIndex(''); setIndexProof(null) }
  const clearChoices = () => { setApplications([]); setApplication(''); setProvidedApplication(''); setApplicationMode('discovered'); clearIndexProof() }
  const chooseGrant=(value:string)=>batch(()=>{const mode=applicationMode();clearChoices();setApplicationMode(mode);setConnection(value)})
  const chooseApplication=(value:string)=>batch(()=>{clearIndexProof();setApplication(value)})
  createEffect(() => {
    const selected = context(), version = ++generation
    refresh(); const admin = allowed(); controller?.abort(); clearTimeout(timer)
    const current = new AbortController(); controller = current
    setLoading(admin); setMappingLoading(admin); setMappingError(''); setBusy(false); setMapping(null); setGrants([]); setConnection(''); clearChoices(); setError(''); setMessage(''); setSession(null); setPolling(false)
    const mappingChoiceGeneration = choiceGeneration
    autoGrant='';autoIndex=''
    if (admin) {
      // Organization-scoped account discovery is independent of website
      // mapping permission. A failed site read must not hide a valid grant.
      void loadAlgoliaConnections(current.signal).then((connections) => {
        if (valid(version, current)) {setGrants(connections);const eligible=connections.filter(value=>value.status==='active');if(!connection()&&eligible.length===1)setConnection(eligible[0]!.id)}
      }).catch(() => { if (valid(version, current)) setError(i18n.tr('Algolia-oppsett kunne ikke lastes. Oppdater kontostatus.', 'Algolia setup could not be loaded. Refresh account status.')) }).finally(() => { if (valid(version, current)) setLoading(false) })
      void loadAlgoliaMapping(selected, current.signal).then((value) => {
        if (!valid(version, current)) return
        setMapping(value)
        // A late site read must not overwrite an explicit account selection.
        if (mappingChoiceGeneration === choiceGeneration && !connection() && value.connectionId) setConnection(value.connectionId)
        props.onMappingChanged?.()
      }).catch(() => { if (valid(version, current)) setMappingError(i18n.tr('Nettstedskoblingen kunne ikke leses. Kontooppdagelse er fortsatt tilgjengelig; kontroller arbeidsområdets og leverandørkontoens rettigheter, og oppdater status før lagring.', 'Site mapping access could not be confirmed. Account discovery remains available; check workspace and provider account permissions, then refresh status before saving.')) }).finally(() => { if (valid(version, current)) setMappingLoading(false) })
    }
    onCleanup(() => { current.abort(); clearTimeout(timer) })
  })
  async function operate(work: (current: AbortController, version: number) => Promise<void>) {
    if (!allowed() || loading() || busy() || !controller) return
    const current = controller, version = generation
    setBusy(true); setError(''); setMessage('')
    try { await work(current, version) } catch { if (valid(version, current)) setError(i18n.tr('Operasjonen kunne ikke bekreftes. Oppdater kontostatus før et nytt forsøk.', 'The operation could not be confirmed. Refresh account status before retrying.')) }
    finally { if (valid(version, current)) setBusy(false) }
  }
  const discoverApplications = () => void operate(async (current, version) => {
    if (!active()) return
    const selected = connection(); clearChoices()
    const choiceVersion = choiceGeneration
    try {
      const value = await loadAlgoliaApplications(selected, current.signal)
      if (valid(version, current) && choiceVersion === choiceGeneration && connection() === selected) { setApplications(value);setApplication(savedApplication(selected,value)||(value.length===1?value[0]!:'')); if (!value.length) setMessage(i18n.tr('Ingen tilgjengelige applikasjoner. Aktiver Productivity MCP i Algolia og kontroller kontoens rettigheter.', 'No applications available. Enable Productivity MCP in Algolia and check account permissions.')) }
    } catch {
      if (valid(version, current) && choiceVersion === choiceGeneration && connection() === selected) setError(i18n.tr('Applikasjonsoppdagelse kunne ikke bekreftes. Du kan oppgi en applikasjons-ID og kontrollere indeksene; ingen tilgang er antatt.', 'Application discovery could not be confirmed. You can provide an Application ID and verify its indices; no access is assumed.'))
    }
  })
  const discoverIndices = () => void operate(async (current, version) => {
    if (!active() || !eligibleApplication()) return
    const selected = requestedApplication(), selectedConnection = connection(); clearIndexProof(false)
    const choiceVersion = choiceGeneration
    let value: string[]
    try { value = await loadAlgoliaIndices(selectedConnection, selected, current.signal) } catch (error) {
      if (valid(version, current) && choiceVersion === choiceGeneration && connection() === selectedConnection && requestedApplication() === selected) throw error
      return
    }
    if (valid(version, current) && choiceVersion === choiceGeneration && connection() === selectedConnection && requestedApplication() === selected) { setIndices(value);setIndex(savedIndex(selectedConnection,selected,value)||(value.length===1?value[0]!:'')); setIndexProof({ connectionId: selectedConnection, applicationId: selected }); if (!value.length) setMessage(i18n.tr('Ingen indekser er tilgjengelige for denne applikasjonen.', 'No indices are available for this application.')) }
  })
  const save = () => void operate(async (current, version) => {
    if (!mappingReady() || alreadySaved() || !active() || !eligibleApplication() || !indexProofMatches() || !indices().includes(index())) return
    const selected = context(), value = await saveAlgoliaMapping(selected, { connectionId: connection(), applicationId: requestedApplication(), indexName: index() }, current.signal)
    if (!valid(version, current)) return
    setMapping(value); props.onMappingChanged?.()
    if (value.status !== 'mapped') { setMessage(i18n.tr('Kartlegging lagret, men tilgang må bekreftes før en rapport kan hentes.', 'Mapping saved, but access must be confirmed before fetching a report.')); return }
    setMessage(i18n.tr('Nettstedskobling lagret. Første rapport samles inn i bakgrunnen; lagrede resultater og status oppdateres uten nytt samtykke så lenge tilgangen er gyldig.','Site connection saved. Initial collection runs in the background; cached results and status update without new consent while access remains valid.'))
  })
  const disconnect = () => void operate(async (current, version) => {
    if (!mappingReady()) return
    const value = await disconnectAlgoliaSite(context(), current.signal)
    if (!valid(version, current)) return
    setMapping(value); setConnection(''); clearChoices(); props.onMappingChanged?.()
    setMessage(i18n.tr('Nettstedskartlegging frakoblet. Kontotilgangen beholdes; dette tilbakekaller ikke kontoens OAuth-samtykke. Ingen automatisk tilbakegang til filnøkler.', 'Site mapping disconnected. The account grant remains; this does not revoke account OAuth consent. No automatic fallback to file keys.'))
  })
  const connect = () => void operate(async (current, version) => {
    const value = await startAlgoliaSession(`${window.location.origin}/studio/pulse`, current.signal)
    if (valid(version, current)) { setSession(value); attempts = 0; setMessage(i18n.tr('Fortsett hos Algolia for å velge konto og godkjenne lesetilgang. Ingenting er koblet til nettstedet ennå.', 'Continue at Algolia to choose an account and approve read access. Nothing is mapped to the website yet.')) }
  })
  async function poll(current: AbortController, version: number, selected: AlgoliaSession) {
    if (!valid(version, current)) return
    if (++attempts > 90 || Date.parse(selected.expiresAt) <= Date.now()) { setPolling(false); setMessage(i18n.tr('Statuskontroll er avsluttet. Oppdater kontostatus etter samtykke, eller start en ny pålogging.', 'Status checking ended. Refresh account status after consent, or start a new sign-in.')); return }
    try {
      const value = await loadAlgoliaSession(selected.id, current.signal)
      if (!valid(version, current)) return
      if (value.status === 'completed') { setPolling(false); setRefresh((n) => n + 1); return }
      if (value.status !== 'pending') { setPolling(false); setSession(null); setMessage(i18n.tr('Påloggingen er utløpt eller feilet. Ingen tilgang er antatt. Start på nytt ved behov.', 'Sign-in expired or failed. No access is assumed. Start again if needed.')); return }
      timer = setTimeout(() => void poll(current, version, selected), 2000)
    } catch { if (valid(version, current)) { setPolling(false); setError(i18n.tr('Samtykkestatus kunne ikke leses. Oppdater kontostatus manuelt.', 'Consent status could not be read. Refresh account status manually.')) } }
  }
  const beginPoll = () => { if (controller && session() && !polling()) { setPolling(true); void poll(controller, generation, session()!) } }
  createEffect(on(()=>[connection(),applicationMode(),loading(),allowed(),busy()] as const,([selected,mode,isLoading,admin,isBusy])=>{
    if(!selected){autoGrant='';return}if(!admin||mode!=='discovered'||isLoading||isBusy||!active()||autoGrant===selected)return
    autoGrant=selected;void discoverApplications()
  }))
  createEffect(on(()=>[connection(),application(),applicationMode(),loading(),allowed(),busy()] as const,([grant,app,mode,isLoading,admin,isBusy])=>{
    const key=`${grant}:${app}:${mode}`;if(!admin||isLoading||isBusy||mode!=='discovered'||!active()||!app||!eligibleApplication()||indexProofMatches()||autoIndex===key)return
    autoIndex=key;void discoverIndices()
  }))
  return <Show when={allowed()}><section class="algolia-connection" aria-labelledby={`${id}-title`} data-testid="algolia-connection-panel" aria-busy={loading() || mappingLoading() || busy()}><header><div><span>Algolia OAuth</span><h2 id={`${id}-title`}>{i18n.tr('Koble konto og velg nettstedets indeks', 'Connect account and choose the site index')}</h2></div><button disabled={loading() || mappingLoading() || busy()} type="button" onClick={() => setRefresh((n) => n + 1)}>{i18n.tr('Oppdater kontostatus', 'Refresh account status')}</button></header>
    <p>{i18n.tr('Du må aktivere Productivity MCP i Algolia Dashboard. Tilgangen er kontodekkende og kun lesing innenfor dine rettigheter; Studio bruker bare indeksoppdagelse og samlet søkeanalytikk. Google-pålogging gir ikke Algolia-tilgang.', 'You must enable Productivity MCP in Algolia Dashboard. Access is account-wide and read-only within your permissions; Studio uses only index discovery and aggregate search analytics. Google sign-in does not grant Algolia access.')}</p>
    <Show when={loading()}><p role="status">{i18n.tr('Laster kontometadata…', 'Loading account metadata…')}</p></Show><Show when={error()}><p role="alert">{error()}</p></Show><Show when={message()}><p role="status">{message()}</p></Show>
    <Show when={mappingLoading()}><p role="status">{i18n.tr('Kontrollerer tilgang til nettstedskartlegging før lagring…', 'Checking site mapping access before saving…')}</p></Show><Show when={mappingError()}><p role="status">{mappingError()}</p></Show>
    <Show when={mapping()?.status === 'reconnect_required'}><p>{i18n.tr('Kontotilgangen må kobles til på nytt. Gamle rapporter er skjult.', 'Account access must reconnect. Old reports are hidden.')}</p></Show><Show when={mapping()?.status === 'access_check_required'}><p>{i18n.tr('Tilgang må kontrolleres på nytt. Ingen tilkobling eller målinger er antatt.', 'Access must be checked again. No connection or measurements are assumed.')}</p></Show>
    <div class="algolia-connection__actions"><button type="button" disabled={loading() || busy()} onClick={connect}>{i18n.tr('Logg inn med Algolia', 'Sign in with Algolia')}</button><Show when={session()}>{(value) => <a href={value().authorizationUrl} target="_blank" rel="noopener noreferrer" onClick={beginPoll}>{i18n.tr('Fortsett til Algolia', 'Continue to Algolia')}</a>}</Show></div>
    <button type="button" aria-expanded={advanced()} onClick={()=>setAdvanced(value=>!value)}>{i18n.tr('Avanserte tilkoblingsvalg','Advanced connection controls')}</button>
    <div class="algolia-connection__choices"><label for={`${id}-grant`}>{i18n.tr('Kontotilgang', 'Account grant')}<select id={`${id}-grant`} value={connection()} disabled={loading() || busy()} onChange={(event) => { chooseGrant(event.currentTarget.value) }}><option value="">{i18n.tr('Velg kontotilgang', 'Choose account grant')}</option><For each={grants()}>{(grant) => <option value={grant.id} selected={connection()===grant.id} disabled={grant.status !== 'active'}>{grant.displayName ? `${grant.displayName} · ${grant.id}` : grant.id}{grant.status === 'active' ? '' : ` · ${i18n.tr('Koble til på nytt', 'Reconnect required')}`}</option>}</For></select></label><Show when={advanced()}><button type="button" disabled={loading() || busy() || !active()} onClick={discoverApplications}>{i18n.tr('Finn applikasjoner', 'Discover applications')}</button></Show>
    <Show when={advanced()||applicationMode()==='provided'}><label class="algolia-connection__application-source" for={`${id}-application-source`}>{i18n.tr('Applikasjonskilde', 'Application source')}<select id={`${id}-application-source`} value={applicationMode()} disabled={loading() || busy()} onChange={(event) => { setApplicationMode(event.currentTarget.value === 'provided' ? 'provided' : 'discovered'); setApplication(''); setProvidedApplication(''); clearIndexProof() }}><option value="discovered">{i18n.tr('Oppdagede applikasjoner', 'Discovered applications')}</option><option value="provided">{i18n.tr('Oppgi applikasjons-ID', 'Provide an application ID')}</option></select></label></Show>
    <Show when={applicationMode() === 'provided'} fallback={<label for={`${id}-application`}>{i18n.tr('Applikasjon', 'Application')}<select id={`${id}-application`} value={application()} disabled={busy() || !applications().length} onChange={(event) => { chooseApplication(event.currentTarget.value) }}><option value="">{i18n.tr('Velg applikasjon', 'Choose application')}</option><For each={applications()}>{(value) => <option value={value}>{value}</option>}</For></select></label>}><label for={`${id}-provided-application`}>{i18n.tr('Applikasjons-ID (oppgitt av bruker)', 'Application ID (user supplied)')}<input id={`${id}-provided-application`} type="text" autocomplete="off" spellcheck={false} value={providedApplication()} disabled={loading() || !active()} aria-describedby={`${id}-provided-note`} onInput={(event) => { setProvidedApplication(event.currentTarget.value); clearIndexProof() }} /></label></Show><Show when={advanced()||applicationMode()==='provided'}><button type="button" disabled={loading() || busy() || !active() || !eligibleApplication()} onClick={discoverIndices}>{i18n.tr('Finn indekser', 'Discover indices')}</button></Show>
    <Show when={applicationMode() === 'provided'}><p class="algolia-connection__application-source" id={`${id}-provided-note`}>{i18n.tr('Oppgitt av bruker, ikke oppdaget. ID-en er ikke bevis på tilgang eller tilknytning til nettstedet. Bruk bare bokstaver og tall (1–64 tegn). Indekser må bekreftes av en lesekontroll hos Algolia før lagring; nettstedstilgang kontrolleres separat.', 'User supplied, not discovered. The ID is not proof of permission or association with this website. Use only letters and digits (1–64 characters). Indices must be confirmed by a read-only Algolia check before saving; website access is checked separately.')}</p></Show>
    <label for={`${id}-index`}>{i18n.tr('Indeks', 'Index')}<select id={`${id}-index`} value={index()} disabled={busy() || !indices().length} onChange={(event) => setIndex(event.currentTarget.value)}><option value="">{i18n.tr('Velg indeks', 'Choose index')}</option><For each={indices()}>{(value) => <option value={value}>{value}</option>}</For></select></label></div>
    <div class="algolia-connection__actions"><button type="button" disabled={loading() || busy() || !mappingReady() || alreadySaved() || !active() || !eligibleApplication() || !indexProofMatches() || !indices().includes(index())} onClick={save}>{i18n.tr('Lagre nettstedskobling', 'Save site connection')}</button><Show when={mapping() && mapping()?.status !== 'not_mapped'}><button type="button" disabled={loading() || busy() || !mappingReady()} onClick={disconnect}>{i18n.tr('Koble fra dette nettstedet', 'Disconnect this site')}</button><p>{mapping()?.applicationId} · {mapping()?.indexName}</p></Show></div><p>{i18n.tr('Nettstedskartlegging endrer ikke indeksen, Google/Clarity eller kontoens øvrige tilganger. API-nøkler skal ikke legges inn her.', 'Site mapping does not change the index, Google/Clarity or other account grants. Do not enter API keys here.')}</p>
  </section></Show>
}
