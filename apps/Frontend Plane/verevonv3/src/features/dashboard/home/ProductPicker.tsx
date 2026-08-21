import { Check, Copy, Loader2, Search, Sparkles, X } from '@/shared/icons'
import { createMemo, createSignal, For, Show, untrack } from 'solid-js'
import type { Product, ProductExtraction } from '@/shared/api/knowledge-client'
import { hostnameOf } from './knowledge-preview'

function matchesFilter(product: Product, needle: string): boolean {
  if (!needle) return true
  const hay = [product.name, product.description, ...(product.specs ?? [])]
    .filter(Boolean)
    .join(' ')
    .toLowerCase()
  // Every whitespace-separated term must appear ("m3 air" → both "m3" and "air").
  return needle
    .toLowerCase()
    .split(/\s+/)
    .filter(Boolean)
    .every((term) => hay.includes(term))
}

/** Deterministic markdown for the chosen products — images + price + specs +
 * link — built client-side so the user always gets a clean export alongside the
 * AI summary. */
function buildProductsMarkdown(host: string, products: Product[]): string {
  const lines: string[] = [`## ${host} — ${products.length} produkter`, '']
  for (const p of products) {
    lines.push(`### ${p.name}`)
    if (p.image) lines.push(`![${p.name}](${p.image})`)
    const price = [p.price, p.currency].filter(Boolean).join(' ')
    if (price) lines.push(`**Pris:** ${price}`)
    if (p.description) lines.push(p.description)
    for (const spec of p.specs ?? []) lines.push(`- ${spec}`)
    if (p.url) lines.push(`[Se produkt](${p.url})`)
    lines.push('')
  }
  return lines.join('\n').trim()
}

/** Pick products from an extracted listing (cards + natural-language filter),
 * then generate a markdown brief with an AI summary of the selection. */
export function ProductPicker(props: {
  extraction: ProductExtraction
  summarizing: boolean
  summary: string | null
  onSummarize: (selected: Product[], focus: string) => void
  onDiscard: () => void
}) {
  const products = createMemo(() => props.extraction.products)
  const host = createMemo(() => hostnameOf(props.extraction.url || products()[0]?.url || ''))
  const [selected, setSelected] = createSignal<Set<number>>(
    untrack(() => new Set(props.extraction.products.map((_, index) => index))),
  )
  const [filter, setFilter] = createSignal('')
  const [copied, setCopied] = createSignal(false)

  const visible = createMemo(() => {
    const needle = filter().trim()
    return products()
      .map((product, index) => ({ product, index }))
      .filter(({ product }) => matchesFilter(product, needle))
  })
  const selectedProducts = createMemo(() =>
    products().filter((_, index) => selected().has(index)),
  )
  const selectedCount = () => selected().size
  const visibleAllSelected = () =>
    visible().length > 0 && visible().every(({ index }) => selected().has(index))

  const toggle = (index: number) => {
    setSelected((current) => {
      const next = new Set(current)
      if (next.has(index)) next.delete(index)
      else next.add(index)
      return next
    })
  }
  const selectVisible = () =>
    setSelected((current) => {
      const next = new Set(current)
      for (const { index } of visible()) next.add(index)
      return next
    })
  const clearVisible = () =>
    setSelected((current) => {
      const next = new Set(current)
      for (const { index } of visible()) next.delete(index)
      return next
    })

  const markdown = createMemo(() => buildProductsMarkdown(host(), selectedProducts()))

  const copyMarkdown = async () => {
    try {
      await navigator.clipboard.writeText(markdown())
      setCopied(true)
      window.setTimeout(() => setCopied(false), 1600)
    } catch {
      // Clipboard blocked (insecure context / permissions) — non-fatal.
    }
  }

  return (
    <section class="verevon-fade-up product-picker" aria-label="Velg produkter å skrape">
      <div class="product-picker__head">
        <div class="product-picker__heading">
          <span class="product-picker__tag">Produkter · forhåndsvisning</span>
          <p class="product-picker__title">{host()}</p>
          <span class="product-picker__found">
            {props.extraction.count} produkter funnet
            <Show when={props.extraction.source === 'enhanced'}>
              {' '}· via proxy
            </Show>
          </span>
        </div>
        <span class="product-picker__stat">{selectedCount()} valgt</span>
      </div>

      <div class="product-picker__toolbar">
        <label class="product-picker__filter">
          <Search class="size-3.5" aria-hidden="true" />
          <input
            value={filter()}
            onInput={(event) => setFilter(event.currentTarget.value)}
            placeholder="Be Verevon finne: f.eks. MacBook Air M3 under 15000"
            aria-label="Filtrer produkter"
          />
        </label>
        <div class="product-picker__select-actions">
          <button type="button" onClick={selectVisible} disabled={visibleAllSelected()}>Velg alle</button>
          <button type="button" onClick={clearVisible} disabled={visible().every(({ index }) => !selected().has(index))}>Fjern alle</button>
        </div>
      </div>

      <Show
        when={visible().length > 0}
        fallback={<p class="product-picker__empty">Ingen produkter matcher — juster søket, eller prøv en annen side.</p>}
      >
        <div class="product-picker__grid" aria-label="Produkter">
          <For each={visible()}>
            {({ product, index }) => {
              const isSelected = () => selected().has(index)
              return (
                <div
                  class={['product-card', { 'product-card--selected': isSelected() }]}
                  role="button"
                  tabindex="0"
                  aria-pressed={isSelected() ? 'true' : 'false'}
                  onClick={() => toggle(index)}
                  onKeyDown={(event) => {
                    if (event.key === 'Enter' || event.key === ' ') {
                      event.preventDefault()
                      toggle(index)
                    }
                  }}
                >
                  <span class="product-card__check" aria-hidden="true">
                    <Show when={isSelected()}>
                      <Check class="size-3" />
                    </Show>
                  </span>
                  <Show when={product.image}>
                    <img
                      class="product-card__img"
                      src={product.image}
                      alt={product.name}
                      loading="lazy"
                      onError={(event) => { (event.currentTarget as HTMLImageElement).style.display = 'none' }}
                    />
                  </Show>
                  <span class="product-card__name">{product.name}</span>
                  <Show when={product.price}>
                    <span class="product-card__price">{[product.price, product.currency].filter(Boolean).join(' ')}</span>
                  </Show>
                  <Show when={(product.specs ?? []).length > 0}>
                    <span class="product-card__specs">
                      <For each={(product.specs ?? []).slice(0, 4)}>
                        {(spec) => <span class="product-card__spec">{spec}</span>}
                      </For>
                    </span>
                  </Show>
                </div>
              )
            }}
          </For>
        </div>
      </Show>

      <div class="product-picker__actions">
        <button type="button" class="product-picker__discard" onClick={() => props.onDiscard()} disabled={props.summarizing}>
          <X class="size-4" /> Forkast
        </button>
        <button
          type="button"
          class="product-picker__copy"
          onClick={() => void copyMarkdown()}
          disabled={selectedCount() === 0}
        >
          <Copy class="size-4" /> {copied() ? 'Kopiert!' : 'Kopier markdown'}
        </button>
        <button
          type="button"
          class="product-picker__summarize"
          onClick={() => props.onSummarize(selectedProducts(), filter().trim())}
          disabled={props.summarizing || selectedCount() === 0}
        >
          <Show when={!props.summarizing} fallback={<Loader2 class="size-4 dashboard-xsearch-spin" />}>
            <Sparkles class="size-4" />
          </Show>
          AI-sammendrag av {selectedCount()}
        </button>
      </div>

      <Show when={props.summary}>
        {(text) => (
          <div class="product-picker__summary" aria-label="AI-sammendrag">
            <span class="product-picker__summary-tag"><Sparkles class="size-3" /> AI-sammendrag</span>
            <div class="product-picker__summary-body">{text()}</div>
          </div>
        )}
      </Show>
    </section>
  )
}
