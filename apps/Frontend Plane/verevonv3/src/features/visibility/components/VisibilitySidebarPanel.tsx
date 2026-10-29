import { A, useLocation } from '@solidjs/router'
import { createSignal, For } from 'solid-js'
import { ArrowLeft, Gauge, Files, ListFilter, Network, ScanSearch } from 'lucide-solid'
import { SidebarPanelTitle, SidebarSearchField } from '@/features/core/components/sidebar/CoreSidebarPrimitives'
import { useI18n } from '@/shared/i18n'
import { visibilityViews } from '../lib/views'
import './visibility-tools.css'
export function VisibilitySidebarPanel(props:{onCollapse:()=>void}) {
 const {tr}=useI18n();const location=useLocation();const [filter,setFilter]=createSignal('')
 const icons=[Gauge,Files,ListFilter,Network,ScanSearch]
 return <div class="core-sidebar-panel visibility-sidebar">
  <A href="/engagement" class="engagement-back-link"><ArrowLeft size={14}/>{tr('Til Engasjement','Back to Engagement')}</A>
  <SidebarPanelTitle onCollapse={props.onCollapse}>{tr('Synlighet','Visibility')}</SidebarPanelTitle>
  <SidebarSearchField value={filter()} onChange={setFilter}/>
  <nav class="core-sidebar-panel-nav" aria-label={tr('Synlighet navigasjon','Visibility navigation')}><div class="core-sidebar-group__items">
   <For each={visibilityViews.filter(v=>tr(v.no,v.en).toLocaleLowerCase().includes(filter().toLocaleLowerCase()))}>{v=>{const Icon=icons[visibilityViews.indexOf(v)] ?? Gauge;return <A href={v.href} class="core-sidebar-panel-link" classList={{'verevon-sidebar-panel-active':location.pathname===v.href}} aria-current={location.pathname===v.href?'page':undefined}><Icon size={16}/><span>{tr(v.no,v.en)}</span></A>}}</For>
  </div></nav>
 </div>
}
