import { A, useLocation, useNavigate } from '@solidjs/router'
import { createSignal, For, Match, Show, Switch } from 'solid-js'
import { Settings2, ArrowLeft } from 'lucide-solid'
import { anbudState, anbudWorkspace, anbudFilters, anbudDetail } from '@/features/anbud/lib/anbud-state'
import { TenderDetailSidebar } from '@/features/anbud/components/TenderDetailSidebar'
import { TenderFiltersPanel } from '@/features/anbud/components/TenderFiltersPanel'
import { anbudStages } from '@/features/anbud/lib/pipeline'
import { SidebarPanelTitle, SidebarSearchField } from '@/features/core/components/sidebar/CoreSidebarPrimitives'
import { useI18n } from '@/shared/i18n'

export function AnbudExpandedSidebarPanel(props: { onCollapse: () => void }) {
  const { tr } = useI18n()
  const location = useLocation()
  const navigate = useNavigate()
  const [search, setSearch] = createSignal('')
  function openPipeline(stage: string) {
    anbudWorkspace.setStage(stage)
    anbudWorkspace.setView('pipeline')
    if (location.pathname !== '/anbud') navigate('/anbud')
  }
  return (
    <div class="core-sidebar-panel core-anbud-sidebar">
      <A href="/engagement" class="engagement-back-link"><ArrowLeft size={14}/>{tr('Til Engasjement', 'Back to Engagement')}</A>
      <SidebarPanelTitle onCollapse={props.onCollapse}>{tr('Anbud', 'Tenders')}</SidebarPanelTitle>
      <Show when={!anbudDetail.id()} fallback={<TenderDetailSidebar />}>
      <Show when={anbudWorkspace.view() === 'pipeline'}><SidebarSearchField value={search()} onChange={setSearch} /></Show>
      <nav class="core-sidebar-panel-nav" aria-label={tr('Anbud navigasjon', 'Tender navigation')}>
        <Switch>
          <Match when={anbudWorkspace.view() === 'all'}><TenderFiltersPanel /></Match>
          <Match when={anbudWorkspace.view() === 'radar'}><div class="anbud-facet-panel"><strong>{tr('Filtrer radar', 'Filter radar')}</strong><details open class="anbud-facet"><summary>{tr('KONTRAKTSMATCH', 'CONTRACT MATCH')}</summary><fieldset><legend class="sr-only">{tr('Kontraktsmatch', 'Contract match')}</legend><For each={[{ id: 'all', no: 'Alle kontrakter', en: 'All contracts' }, { id: 'exact', no: 'Eksakt match', en: 'Exact match' }, { id: 'near', no: 'Nær match', en: 'Near match' }]}>{item => <label><input type="radio" name="radar-reach" checked={anbudFilters.radarReach() === item.id} onChange={() => anbudFilters.setRadarReach(item.id)} />{tr(item.no, item.en)}</label>}</For></fieldset></details><label>{tr('Søk i kontrakter', 'Search contracts')}<input type="search" value={anbudFilters.radarBuyer()} onInput={e => anbudFilters.setRadarBuyer(e.currentTarget.value)} /></label><p class="anbud-filter-note">{tr('Kontrakter som utløper de neste 12 månedene.', 'Contracts ending within the next 12 months.')}</p></div></Match>
          <Match when={anbudWorkspace.view() === 'pipeline'}><section class="core-sidebar-group">
          <div class="core-sidebar-group__header"><span class="verevon-sidebar-group-title">{tr('Oppfølging', 'Pipeline')}</span></div>
          <div class="core-sidebar-group__items">
            <For each={anbudStages.filter(item => tr(item.no, item.en).toLocaleLowerCase().includes(search().toLocaleLowerCase()))}>
              {item => <button type="button" class="anbud-stage" aria-current={anbudWorkspace.view() === 'pipeline' && anbudWorkspace.stage() === item.id ? 'page' : undefined} onClick={() => openPipeline(item.id)}>
                <item.icon size={16} /><span>{tr(item.no, item.en)}</span><span class="anbud-count">{item.id === 'inbox' && anbudWorkspace.matches()?.inboxTotal !== undefined ? anbudWorkspace.matches()!.inboxTotal : (anbudWorkspace.matches()?.results ?? []).filter(t => (anbudWorkspace.stages()[t.noticeId] ?? 'inbox') === item.id).length}</span>
              </button>}
            </For>
          </div>
          <p class="anbud-session-note">{tr('Status lagres for organisasjonen. Valgte anbud beholdes også når de ikke lenger matcher.', 'Stages are saved for your organisation. Tracked tenders remain even when they stop matching.')}</p>
        </section></Match>
        </Switch>
        <div class="anbud-sidebar-profile"><Settings2 size={18} /><h3>{tr('Kundeprofil', 'Company profile')}</h3><p>{anbudState.submitted() ? tr('Søket bruker den valgte kundeprofilen.', 'Search uses your selected company profile.') : tr('Fortell oss hva kunden leverer, så finner vi relevante anbud.', 'Tell us what your customer delivers to find relevant tenders.')}</p><button type="button" class="anbud-button" onClick={() => anbudWorkspace.setProfileOpen(true)}>{tr('Rediger profil', 'Edit profile')}</button>
          <Show when={anbudWorkspace.view() === 'all'}><p>{tr('Alle lagrede kunngjøringer vises uten krav om kundeprofil.', 'All stored notices are available without a company profile.')}</p></Show>
        </div>
      </nav>
      </Show>
    </div>
  )
}
