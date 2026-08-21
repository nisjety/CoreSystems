
import { X } from '@/shared/icons'
import { createSignal, For, Show } from 'solid-js'
import {
  emptyWebSearchFilters,
  type SearchTimeRange,
  type SearchTopic,
  type WebSearchFilters,
} from '@/shared/api/search-client'
import { hasActiveFilters } from '@/features/dashboard/home/dashboard-search-utils'

const topicOptions: Array<{ label: string; value: SearchTopic | null }> = [
  { label: 'Generelt', value: null },
  { label: 'Nyheter', value: 'news' },
  { label: 'Finans', value: 'finance' },
]

const timeRangeOptions: Array<{ label: string; value: SearchTimeRange | '' }> = [
  { label: 'Når som helst', value: '' },
  { label: 'Siste døgn', value: 'day' },
  { label: 'Siste uke', value: 'week' },
  { label: 'Siste måned', value: 'month' },
  { label: 'Siste år', value: 'year' },
]

/** Strip protocol, `www.`, and any path so chips hold a bare hostname. */
function normalizeDomain(value: string): string {
  const trimmed = value.trim().toLowerCase()
  if (!trimmed) return ''
  try {
    const url = new URL(trimmed.includes('://') ? trimmed : `https://${trimmed}`)
    return url.hostname.replace(/^www\./, '')
  } catch {
    return trimmed.replace(/^https?:\/\//, '').replace(/^www\./, '').split('/')[0] ?? ''
  }
}

/**
 * Exa-style filter bar for the expanded web search: recency, topic, exact-match,
 * and domain include/exclude chips. Stateless w.r.t. the search — it only emits
 * a new {@link WebSearchFilters} object; the panel decides when to re-run.
 */
export function SearchFilterBar(props: {
  filters: WebSearchFilters
  onChange: (filters: WebSearchFilters) => void
}) {
  const [domainDraft, setDomainDraft] = createSignal('')
  const [domainMode, setDomainMode] = createSignal<'exclude' | 'include'>('include')

  const includeDomains = () => props.filters.includeDomains ?? []
  const excludeDomains = () => props.filters.excludeDomains ?? []

  const addDomain = () => {
    const value = normalizeDomain(domainDraft())
    if (!value) return
    const key = domainMode() === 'include' ? 'includeDomains' : 'excludeDomains'
    const current = props.filters[key] ?? []
    if (!current.includes(value)) {
      props.onChange({ ...props.filters, [key]: [...current, value] })
    }
    setDomainDraft('')
  }

  const removeDomain = (mode: 'exclude' | 'include', value: string) => {
    const key = mode === 'include' ? 'includeDomains' : 'excludeDomains'
    props.onChange({ ...props.filters, [key]: (props.filters[key] ?? []).filter((d) => d !== value) })
  }

  return (
    <div class="dashboard-xfilters" role="group" aria-label="Søkefiltre">
      <div class="dashboard-xfilter">
        <span class="dashboard-xfilter__label">Tema</span>
        <div class="dashboard-xfilter__segment">
          <For each={topicOptions}>
            {(option) => (
              <button
                type="button"
                class={{ 'dashboard-xfilter__segment--active': (props.filters.topic ?? null) === option.value }}
                aria-pressed={(props.filters.topic ?? null) === option.value ? 'true' : 'false'}
                onClick={() => props.onChange({ ...props.filters, topic: option.value })}
              >
                {option.label}
              </button>
            )}
          </For>
        </div>
      </div>

      <div class="dashboard-xfilter">
        <label class="dashboard-xfilter__label" for="dashboard-xfilter-recency">Tidsrom</label>
        <select
          id="dashboard-xfilter-recency"
          class="dashboard-xfilter__select"
          value={props.filters.timeRange ?? ''}
          onChange={(event) => {
            const value = event.currentTarget.value
            props.onChange({ ...props.filters, timeRange: value ? (value as SearchTimeRange) : null })
          }}
        >
          <For each={timeRangeOptions}>{(option) => <option value={option.value}>{option.label}</option>}</For>
        </select>
      </div>

      <button
        type="button"
        class={['dashboard-xfilter__toggle', { 'dashboard-xfilter__toggle--active': Boolean(props.filters.exactMatch) }]}
        aria-pressed={Boolean(props.filters.exactMatch) ? 'true' : 'false'}
        onClick={() => props.onChange({ ...props.filters, exactMatch: !props.filters.exactMatch })}
      >
        Eksakt treff
      </button>

      <div class="dashboard-xfilter dashboard-xfilter--domains">
        <div class="dashboard-xfilter__segment dashboard-xfilter__segment--mode">
          <button
            type="button"
            class={{ 'dashboard-xfilter__segment--active': domainMode() === 'include' }}
            aria-pressed={domainMode() === 'include' ? 'true' : 'false'}
            onClick={() => setDomainMode('include')}
          >
            Bare
          </button>
          <button
            type="button"
            class={{ 'dashboard-xfilter__segment--active': domainMode() === 'exclude' }}
            aria-pressed={domainMode() === 'exclude' ? 'true' : 'false'}
            onClick={() => setDomainMode('exclude')}
          >
            Utelat
          </button>
        </div>
        <input
          class="dashboard-xfilter__domain-input"
          value={domainDraft()}
          placeholder="domene.no"
          autocomplete="off"
          aria-label={domainMode() === 'include' ? 'Bare disse domenene' : 'Utelat disse domenene'}
          onInput={(event) => setDomainDraft(event.currentTarget.value)}
          onKeyDown={(event) => {
            if (event.key === 'Enter') {
              event.preventDefault()
              addDomain()
            }
          }}
        />
      </div>

      <Show when={includeDomains().length > 0 || excludeDomains().length > 0}>
        <div class="dashboard-xfilter__chips">
          <For each={includeDomains()}>
            {(domain) => (
              <span class="dashboard-xchip dashboard-xchip--include">
                <span>{domain}</span>
                <button type="button" aria-label={`Fjern ${domain}`} onClick={() => removeDomain('include', domain)}>
                  <X class="size-3" aria-hidden="true" />
                </button>
              </span>
            )}
          </For>
          <For each={excludeDomains()}>
            {(domain) => (
              <span class="dashboard-xchip dashboard-xchip--exclude">
                <span>−{domain}</span>
                <button type="button" aria-label={`Fjern ${domain}`} onClick={() => removeDomain('exclude', domain)}>
                  <X class="size-3" aria-hidden="true" />
                </button>
              </span>
            )}
          </For>
        </div>
      </Show>

      <Show when={hasActiveFilters(props.filters)}>
        <button
          type="button"
          class="dashboard-xfilter__reset"
          onClick={() => props.onChange({ ...emptyWebSearchFilters })}
        >
          Nullstill filtre
        </button>
      </Show>
    </div>
  )
}
