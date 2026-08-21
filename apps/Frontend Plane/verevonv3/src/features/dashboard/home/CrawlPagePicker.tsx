import { Check, ExternalLink, Loader2, Search, X } from '@/shared/icons'
import { createMemo, createSignal, For, Show, untrack } from 'solid-js'
import type { CrawlDiscovery } from '@/shared/api/knowledge-client'
import { hostnameOf } from './knowledge-preview'

// Mirrors the gateway's batch cap (normalize_batch_body) so the UI never lets a
// user queue more pages than quarry will actually accept.
const MAX_SELECTION = 200

function pagePath(url: string): string {
  try {
    const parsed = new URL(url)
    const path = `${parsed.pathname}${parsed.search}`
    return path.length > 0 ? path : '/'
  } catch {
    return url
  }
}

/** Pick a subset of discovered pages to crawl. The chosen URLs go to quarry's
 * durable `/v1/batch`, so live workflow events reflect only this selection. */
export function CrawlPagePicker(props: {
  discovery: CrawlDiscovery
  submitting: boolean
  onCrawl: (urls: string[]) => void
  onDiscard: () => void
}) {
  const pages = createMemo(() => props.discovery.pages)
  // Mounted fresh per discovery (parent <Show keyed>); default-select up to the
  // cap so one click crawls a sensible scope.
  const [selected, setSelected] = createSignal<Set<string>>(
    untrack(() => new Set(props.discovery.pages.slice(0, MAX_SELECTION).map((page) => page.url))),
  )
  const [filter, setFilter] = createSignal('')
  const [hovered, setHovered] = createSignal<string | null>(null)

  const filtered = createMemo(() => {
    const needle = filter().trim().toLowerCase()
    if (!needle) return pages()
    return pages().filter(
      (page) =>
        pagePath(page.url).toLowerCase().includes(needle) ||
        (page.title ?? '').toLowerCase().includes(needle),
    )
  })
  const selectedCount = () => selected().size
  const overCap = () => selectedCount() > MAX_SELECTION
  const filteredAllSelected = () =>
    filtered().length > 0 && filtered().every((page) => selected().has(page.url))
  const filteredNoneSelected = () => filtered().every((page) => !selected().has(page.url))

  const toggle = (url: string) => {
    setSelected((current) => {
      const next = new Set(current)
      if (next.has(url)) next.delete(url)
      else next.add(url)
      return next
    })
  }
  const selectFiltered = () =>
    setSelected((current) => {
      const next = new Set(current)
      for (const page of filtered()) next.add(page.url)
      return next
    })
  const clearFiltered = () =>
    setSelected((current) => {
      const next = new Set(current)
      for (const page of filtered()) next.delete(page.url)
      return next
    })

  const handleCrawl = () => {
    if (selectedCount() === 0 || overCap()) return
    // Preserve discovery order; emit only the chosen URLs.
    const urls = pages()
      .filter((page) => selected().has(page.url))
      .map((page) => page.url)
    props.onCrawl(urls)
  }

  return (
    <section class="verevon-fade-up crawl-page-picker" aria-label="Velg sider å crawle">
      <div class="crawl-page-picker__head">
        <div class="crawl-page-picker__heading">
          <span class="crawl-page-picker__tag">Oppdaget · ikke crawlet ennå</span>
          <p class="crawl-page-picker__title">
            {hostnameOf(props.discovery.url || pages()[0]?.url || '')}
          </p>
          <span class="crawl-page-picker__found">{props.discovery.count} sider funnet</span>
        </div>
        <span
          class={['crawl-page-picker__stat', { 'crawl-page-picker__stat--over': overCap() }]}
        >
          {selectedCount()} valgt{overCap() ? ` · maks ${MAX_SELECTION}` : ''}
        </span>
      </div>

      <div class="crawl-page-picker__toolbar">
        <label class="crawl-page-picker__filter">
          <Search class="size-3.5" aria-hidden="true" />
          <input
            value={filter()}
            onInput={(event) => setFilter(event.currentTarget.value)}
            placeholder="Filtrer på sti, f.eks. /blogg"
            aria-label="Filtrer oppdagede sider"
          />
        </label>
        <div class="crawl-page-picker__select-actions">
          <button type="button" onClick={selectFiltered} disabled={filteredAllSelected()}>
            Velg alle
          </button>
          <button type="button" onClick={clearFiltered} disabled={filteredNoneSelected()}>
            Fjern alle
          </button>
        </div>
      </div>

      <Show
        when={filtered().length > 0}
        fallback={<p class="crawl-page-picker__empty">Ingen sider matcher filteret.</p>}
      >
        <div class="crawl-page-picker__list" aria-label="Oppdagede sider">
          <For each={filtered()}>
            {(page) => {
              const isSelected = () => selected().has(page.url)
              return (
                <div
                  class={[
                    'crawl-page-picker__row',
                    {
                      'crawl-page-picker__row--selected': isSelected(),
                      'crawl-page-picker__row--hovered': hovered() === page.url,
                    },
                  ]}
                  role="button"
                  tabindex="0"
                  aria-pressed={isSelected() ? 'true' : 'false'}
                  onClick={() => toggle(page.url)}
                  onMouseEnter={() => setHovered(page.url)}
                  onMouseLeave={() => setHovered((current) => (current === page.url ? null : current))}
                  onKeyDown={(event) => {
                    if (event.key === 'Enter' || event.key === ' ') {
                      event.preventDefault()
                      toggle(page.url)
                    }
                  }}
                >
                  <span class="crawl-page-picker__check" aria-hidden="true">
                    <Show when={isSelected()}>
                      <Check class="size-3" />
                    </Show>
                  </span>
                  <span class="crawl-page-picker__row-body">
                    <span class="crawl-page-picker__row-title">{page.title || pagePath(page.url)}</span>
                    <span class="crawl-page-picker__row-path">{pagePath(page.url)}</span>
                  </span>
                  <a
                    class="crawl-page-picker__row-open"
                    href={page.url}
                    target="_blank"
                    rel="noopener noreferrer"
                    aria-label="Åpne side i ny fane"
                    onClick={(event) => event.stopPropagation()}
                  >
                    <ExternalLink class="size-3" />
                  </a>
                </div>
              )
            }}
          </For>
        </div>
      </Show>

      <div class="crawl-page-picker__actions">
        <button
          type="button"
          class="crawl-page-picker__discard"
          onClick={() => props.onDiscard()}
          disabled={props.submitting}
        >
          <X class="size-4" /> Forkast
        </button>
        <button
          type="button"
          class="crawl-page-picker__crawl"
          onClick={handleCrawl}
          disabled={props.submitting || selectedCount() === 0 || overCap()}
          title={overCap() ? `Velg maks ${MAX_SELECTION} sider` : undefined}
        >
          <Show when={!props.submitting} fallback={<Loader2 class="size-4 dashboard-xsearch-spin" />}>
            <Check class="size-4" />
          </Show>
          Crawl {selectedCount()} valgte
        </button>
      </div>
    </section>
  )
}
