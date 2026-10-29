import { createUniqueId, For, Show } from 'solid-js'
import { anbudWorkspace, anbudFilters, type TenderFilters } from '@/features/anbud/lib/anbud-state'
import { useI18n } from '@/shared/i18n'

/** Filters only fields returned by the tender API; no fabricated catalogue facets. */
export function TenderFiltersPanel() {
  const { tr } = useI18n()
  const filterId = createUniqueId()
  const update = (key: keyof TenderFilters, value: string) => anbudFilters.setFilters(prev => ({ ...prev, [key]: value }))
  return (
    <div class="anbud-facet-panel" aria-label={tr('Anbudsfilter', 'Tender filters')}>
      <div class="anbud-facet-heading"><strong>{tr('Filtrer anbud', 'Filter tenders')}</strong><button type="button" onClick={anbudFilters.reset}>{tr('Nullstill', 'Reset')}</button></div>
      <For each={[
        { key: 'noticeType' as const, no: 'KUNNGJØRINGSTYPE', en: 'NOTICE TYPE', options: [{ value: 'all', no: 'Alle kunngjøringer', en: 'All notices' }, { value: 'competition', no: 'Konkurranse', en: 'Competition' }, { value: 'planning', no: 'Planlegging', en: 'Planning' }, { value: 'result', no: 'Resultat', en: 'Result' }, { value: 'other', no: 'Andre', en: 'Other' }] },
        { key: 'status' as const, no: 'STATUS', en: 'STATUS', options: [{ value: 'all', no: 'Alle', en: 'All' }, { value: 'active', no: 'Aktiv frist', en: 'Active deadline' }, { value: 'expired', no: 'Utgått frist', en: 'Expired deadline' }] },
        { key: 'verdict' as const, no: 'KVALIFISERING', en: 'QUALIFICATION', options: [{ value: 'all', no: 'Alle', en: 'All' }, { value: 'pass', no: 'Kan by', en: 'Can bid' }, { value: 'conditional', no: 'Med forbehold', en: 'With conditions' }, { value: 'fail', no: 'Kan ikke by', en: 'Cannot bid' }] },
        { key: 'deadline' as const, no: 'TILBUDSFRIST', en: 'DEADLINE', options: [{ value: 'all', no: 'Alle frister', en: 'All deadlines' }, { value: 'soon', no: 'Neste 14 dager', en: 'Next 14 days' }] },
      ]}>{group => <Show when={group.key !== 'verdict' || anbudWorkspace.view() !== 'all'}><details open class="anbud-facet"><summary>{tr(group.no, group.en)}</summary><fieldset><legend class="sr-only">{tr(group.no, group.en)}</legend><For each={group.options}>{option => <label><input type="radio" name={`${filterId}-${group.key}`} aria-label={tr(option.no, option.en)} aria-describedby={`${filterId}-${group.key}-${option.value}-count`} checked={anbudFilters.filters()[group.key] === option.value} onChange={() => update(group.key, option.value)} /><span>{tr(option.no, option.en)}</span><Show when={anbudWorkspace.view() === 'all'}><span class="anbud-facet-count" id={`${filterId}-${group.key}-${option.value}-count`}>{anbudFilters.facetsBusy() ? '…' : anbudFilters.facets()[group.key]?.[option.value] ?? '—'}</span></Show></label>}</For></fieldset></details></Show>}</For>
      <details open class="anbud-facet"><summary>{tr('INNKJØPER', 'BUYER')}</summary><label><span class="sr-only">{tr('Filtrer innkjøper', 'Filter buyer')}</span><input type="search" placeholder={tr('Navn på innkjøper…', 'Buyer name…')} value={anbudFilters.filters().buyer} onInput={e => update('buyer', e.currentTarget.value)} /></label><FacetChoices facet="buyer" /></details>
      <details class="anbud-facet"><summary>{tr('STED FOR GJENNOMFØRING', 'PLACE OF PERFORMANCE')}</summary><label>{tr('Regionkode (NUTS)', 'Region code (NUTS)')}<input placeholder="NO08" value={anbudFilters.filters().region} onInput={e => update('region', e.currentTarget.value.trim())} /></label><FacetChoices facet="region" /></details>
      <details class="anbud-facet"><summary>{tr('TJENESTER OG BRANSJE', 'SERVICES AND INDUSTRY')}</summary><label>{tr('CPV-kode', 'CPV code')}<input placeholder="72000000" value={anbudFilters.filters().cpv} onInput={e => update('cpv', e.currentTarget.value.trim())} /></label><FacetChoices facet="cpv" /></details>
      <p class="anbud-filter-note">{tr('Søker i alle lagrede kunngjøringer.', 'Searches all stored notices.')}</p>
    </div>
  )
}

function FacetChoices(props: { facet: 'buyer' | 'region' | 'cpv' }) {
  const choices = () => {
    const term = anbudFilters.filters()[props.facet].toLocaleLowerCase()
    const counts = anbudFilters.facets()[props.facet] ?? {}
    return Object.entries(counts).filter(([label]) => label.toLocaleLowerCase().includes(term)).sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0])).slice(0, 6)
  }
  const choose = (label: string) => {
    const facet = props.facet
    anbudFilters.setFilters(prev => ({ ...prev, [facet]: prev[facet] === label ? '' : label }))
  }
  return <Show when={anbudWorkspace.view() === 'all'}><div class="anbud-facet-choices"><For each={choices()}>{([label, count]) => <button type="button" title={label} aria-pressed={anbudFilters.filters()[props.facet] === label} onClick={() => choose(label)}><span>{label}</span><span class="anbud-facet-count">{anbudFilters.facetsBusy() ? '…' : count}</span></button>}</For></div></Show>
}
