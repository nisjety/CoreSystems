import { ArrowLeft, ArrowUpRight, Check, ChevronLeft, ChevronRight, Plus, X } from 'lucide-solid'
import { For, Show, createEffect, createMemo, createSignal, createUniqueId, onCleanup, type JSX } from 'solid-js'
import { useI18n } from '@/shared/i18n'
import type { MarketingSource } from '@/shared/api/marketing/preparation-client'
import './connected-services.css'
import { ConnectorMark } from './ConnectorMark'

export type PulseServiceKey = 'website' | 'search_console' | 'ga4' | 'clarity' | 'web_vitals' | 'cookiebot' | 'algolia'
export const pulseServiceNames: Record<PulseServiceKey, string> = { website: 'Web', search_console: 'Search Console', ga4: 'Google Analytics', clarity: 'Microsoft Clarity', web_vitals: 'Web Vitals', cookiebot: 'Cookiebot', algolia: 'Algolia' }
const keys: PulseServiceKey[] = ['website', 'clarity', 'search_console', 'ga4', 'web_vitals', 'cookiebot', 'algolia']
const short: Record<PulseServiceKey, string> = { website: 'W', search_console: 'G', ga4: 'G', clarity: 'C', web_vitals: 'V', cookiebot: 'C', algolia: 'a' }
export const isAddedSource = (status?: string) => ['connected', 'authorization_required', 'needs_selection', 'pending_verification'].includes(status ?? '')

export function ConnectedServices(props: {
  sources: readonly MarketingSource[]; loading: boolean; error: boolean; canManage: boolean; selected: PulseServiceKey
  onSelect: (key: PulseServiceKey) => void; onRefresh: () => void
  onOpenReport?: () => void
  renderSetup: (key: PulseServiceKey, done: () => void) => JSX.Element
}) {
  const i18n = useI18n(), id = createUniqueId()
  const providerName = (key: PulseServiceKey) => key === 'website' ? i18n.tr('Nettsted', 'Web') : pulseServiceNames[key]
  const [setup, setSetup] = createSignal<PulseServiceKey | null>(null)
  let dialog: HTMLDialogElement | undefined, addButton: HTMLButtonElement | undefined
  const status = (key: PulseServiceKey) => key === 'website' ? 'available' : props.sources.find(source => source.key === key)?.status
  const connected = createMemo(() => keys.filter(key => key === 'website' || isAddedSource(status(key))))
  const activeIndex = () => Math.max(0, connected().indexOf(props.selected))
  const offset = (key: PulseServiceKey) => {
    const list = connected(), delta = (list.indexOf(key) - activeIndex() + list.length) % list.length
    return delta > Math.floor(list.length / 2) ? delta - list.length : delta
  }
  const position = (key: PulseServiceKey) => {
    const distance = offset(key), magnitude = Math.abs(distance)
    return `${50 + Math.sign(distance) * (magnitude === 1 ? 26.4 : magnitude >= 2 ? 45.8 + (magnitude - 2) * 20 : 0)}%`
  }
  const edgeDuplicate = createMemo(() => connected().length === 4 ? connected()[(activeIndex() + 2) % 4] : undefined)
  const description = (key: PulseServiceKey) => ({
    website: i18n.tr('Hastighet, feltdata og teknisk nettstedskvalitet.', 'Speed, field data and technical website quality.'),
    search_console: i18n.tr('Søkeord, synlighet og muligheter i Google.', 'Queries, visibility and opportunities in Google.'),
    ga4: i18n.tr('Besøk, engasjement og viktige hendelser.', 'Visits, engagement and key events.'),
    clarity: i18n.tr('Atferd, friksjon, opptak og varmekart.', 'Behavior, friction, recordings and heatmaps.'),
    web_vitals: i18n.tr('Ytelse fra nettstedets egen måleinstallasjon.', 'Performance from your own website collector.'),
    cookiebot: i18n.tr('Samtykketrender og datert cookie-inventar.', 'Consent trends and dated cookie inventory.'),
    algolia: i18n.tr('Hva besøkende søker etter, og søk uten treff.', 'What visitors search for, including searches without results.'),
  })[key]
  const stateLabel = (key: PulseServiceKey) => status(key) === 'available' ? i18n.tr('Automatisk', 'Automatic') : status(key) === 'connected' ? i18n.tr('Tilkoblet', 'Connected') : i18n.tr('Trenger oppfølging', 'Needs attention')
  const close = () => { dialog?.close?.(); setSetup(null); addButton?.focus() }
  const open = (key?: PulseServiceKey) => { setSetup(key ?? null); dialog?.showModal() }
  const move = (direction: number) => { const list = connected(); props.onSelect(list[(activeIndex() + direction + list.length) % list.length]!) }
  createEffect(() => { if (!props.loading && !connected().includes(props.selected)) props.onSelect(connected()[0] ?? 'website') })
  onCleanup(() => dialog?.close?.())
  return <section class="pulse-services" aria-labelledby={`${id}-title`} data-testid="pulse-connected-services">
    <h2 id={`${id}-title`} class="sr-only">{i18n.tr('Tilkoblede integrasjoner', 'Connected integrations')}</h2>
    <Show when={props.error}><p role="alert" class="pulse-services__notice">{i18n.tr('Tilkoblingsstatus kunne ikke kontrolleres. Rapporttilgang kontrolleres fortsatt på serveren.', 'Connection status could not be checked. Report access is still checked on the server.')} <button onClick={() => props.onRefresh()}>{i18n.tr('Prøv igjen', 'Retry')}</button></p></Show>
    <Show when={!props.loading} fallback={<p role="status" class="pulse-services__notice">{i18n.tr('Finner nettstedets tilkoblinger…', 'Finding this website’s connections…')}</p>}>
      <div class="pulse-services__viewport" role="group" aria-label={i18n.tr('Tilkoblede datakilder', 'Connected data sources')}>
        <Show when={edgeDuplicate()}>{key => <span class={`pulse-services__edge-copy pulse-services__orb pulse-services__orb--${key()}`} aria-hidden="true"><ConnectorMark provider={key()}/></span>}</Show>
        <For each={connected()}>{key => <button type="button" class="pulse-services__service" classList={{ 'is-selected': props.selected === key, 'is-far': Math.abs(offset(key)) >= 2, 'is-hidden': Math.abs(offset(key)) > 2 }} style={{ left: position(key), '--source-offset': String(offset(key)), '--source-scale': offset(key) === 0 ? '1' : Math.abs(offset(key)) === 1 ? '.787234' : '.567376' }} disabled={Math.abs(offset(key)) >= 2} tabIndex={Math.abs(offset(key)) >= 2 ? -1 : 0} aria-hidden={Math.abs(offset(key)) >= 2 ? true : undefined} aria-pressed={props.selected === key} aria-label={`${providerName(key)} · ${stateLabel(key)}`} onClick={() => props.selected === key ? props.onOpenReport ? props.onOpenReport() : open(key) : props.onSelect(key)}>
          <span class={`pulse-services__orb pulse-services__orb--${key}`} aria-hidden="true"><ConnectorMark provider={key}/><Show when={props.selected === key}><span class="pulse-services__open"><ArrowUpRight size={16} strokeWidth={2}/></span></Show></span>
          <span class="pulse-services__source-copy"><strong>{providerName(key)}<Show when={props.selected === key}><ArrowUpRight size={12} strokeWidth={1.75}/></Show></strong><span>{description(key)}</span><Show when={status(key) !== 'connected' && key !== 'website'}><small>{stateLabel(key)}</small></Show></span>
        </button>}</For>
        <Show when={connected().length > 1}><button type="button" class="pulse-services__previous" onClick={() => move(-1)} aria-label={i18n.tr('Forrige datakilde', 'Previous source')}><ChevronLeft size={18} strokeWidth={1.5}/></button><button type="button" class="pulse-services__next" onClick={() => move(1)} aria-label={i18n.tr('Neste datakilde', 'Next source')}><ChevronRight size={18} strokeWidth={1.5}/></button></Show>
      </div>
    </Show>
    <footer class="pulse-services__rail"><nav class="pulse-services__tabs" aria-label={i18n.tr('Alle tilkoblede datakilder', 'All connected sources')}><For each={connected()}>{key => <button type="button" aria-pressed={props.selected === key} onClick={() => props.onSelect(key)}>{providerName(key)}</button>}</For></nav><button ref={addButton} type="button" class="pulse-services__add" onClick={() => open()} aria-label={i18n.tr('Legg til integrasjon', 'Add integration')}><span class="pulse-services__plus" aria-hidden="true"><Plus size={16}/></span><span>{i18n.tr('Utvid Synlighet', 'Extend Visibility')}</span></button></footer>
    <dialog ref={dialog} class="pulse-extender" aria-labelledby={`${id}-extend-title`} onCancel={event => { event.preventDefault(); close() }} onClick={event => { if (event.target === dialog) close() }}>
      <header><div><span class="pulse-services__eyebrow">{i18n.tr('Bygg videre', 'Make it yours')}</span><h2 id={`${id}-extend-title`}>{setup() ? providerName(setup()!) : i18n.tr('Utvid Synlighet', 'Extend Visibility')}</h2></div><button type="button" aria-label={i18n.tr('Lukk integrasjoner', 'Close integrations')} onClick={close}><X size={20}/></button></header>
      <Show when={setup()} fallback={<><p>{i18n.tr('Koble til én gang. Vi finner tilgjengelige rapporter og henter dem i bakgrunnen.', 'Connect once. We find available reports and collect them in the background.')}</p><div class="pulse-extender__catalog"><For each={keys.filter(key => key !== 'website')}>{key => <button type="button" onClick={() => setSetup(key)}><span class={`pulse-services__orb pulse-services__orb--${key}`} aria-hidden="true">{short[key]}</span><span><strong>{providerName(key)}</strong><small>{description(key)}</small></span><Show when={isAddedSource(status(key))} fallback={<Plus size={18}/>}><Check size={18}/></Show></button>}</For></div><p class="pulse-extender__hint">{i18n.tr('Google Ads og andre annonsekilder kommer senere.', 'Google Ads and other advertising sources are coming later.')}</p></>}>
        {key => <><button type="button" class="pulse-extender__back" onClick={() => setSetup(null)}><ArrowLeft size={15}/>{i18n.tr('Alle integrasjoner', 'All integrations')}</button>{props.renderSetup(key(), () => { props.onRefresh(); close() })}</>}
      </Show>
    </dialog>
  </section>
}
