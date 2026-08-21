
import { ExternalLink, Layers, Loader2, X } from '@/shared/icons'
import { createSignal, For, Match, Show, Switch } from 'solid-js'
import type { NavbarSearchResult } from '@/shared/api/navbar-client'
import { safeHostname, type ImageHit, type PreviewResult, type VideoHit } from '@/shared/api/search-client'
import { useI18n } from '@/shared/i18n'
import type { ImagesStatus, SearchResultTab } from '@/features/dashboard/home/dashboard-home-types'
import { faviconUrlForResult, searchRailTips } from '@/features/dashboard/home/dashboard-search-utils'

function relevancePercent(score: number): number {
  return Math.max(0, Math.min(100, Math.round(score * 100)))
}

export function SearchResultCard(props: {
  image?: ImageHit
  index: number
  onFindSimilar?: (result: PreviewResult) => void
  onOpen?: (result: PreviewResult) => void
  result: PreviewResult
  selected?: boolean
}) {
  const i18n = useI18n()
  const favicon = () => faviconUrlForResult(props.result.url)
  const highlights = () => (props.result.highlights ?? []).filter((passage) => passage.trim().length > 0).slice(0, 2)

  return (
    <article class={{ 'dashboard-xresult': true, 'dashboard-xresult--selected': Boolean(props.selected) }}>
      <div class="dashboard-xresult__grid">
        <div class="dashboard-xresult__main">
          <span class="dashboard-xresult__num">{props.index + 1}</span>
          <span class="dashboard-xresult__body">
            <button
              type="button"
              class="dashboard-xresult__open"
              onClick={() => props.onOpen?.(props.result)}
              aria-label={i18n.tr(`Åpne ${props.result.title} i Verevon`, `Open ${props.result.title} in Verevon`)}
            >
              <span class="dashboard-xresult__host">
                <span>{props.result.hostname}</span>
                <Show when={typeof props.result.score === 'number'}>
                  <span class="dashboard-xresult__score" title={i18n.tr('Semantisk relevans', 'Semantic relevance')}>
                    {relevancePercent(props.result.score as number)}% {i18n.tr('relevant', 'relevant')}
                  </span>
                </Show>
              </span>
              <span class="dashboard-xresult__title">{props.result.title}</span>
            </button>
            <Show
              when={highlights().length > 0}
              fallback={
                <>
                  <span class="dashboard-xresult__eyebrow">{i18n.tr('AI-forklaring', 'AI explanation')}</span>
                  <span class="dashboard-xresult__expl">
                    {props.result.snippet
                      ? i18n.tr(
                          `Verevon vurderer dette som relevant fordi sidekonteksten overlapper med temaet: ${props.result.snippet}`,
                          `Verevon sees this as relevant to the query because the page context overlaps with the topic: ${props.result.snippet}`,
                        )
                      : i18n.tr(
                          'Verevon koblet denne kilden til søket ditt og kan inspisere den videre før den brukes i et svar.',
                          'Verevon matched this source to your search and can inspect it further before using it in an answer.',
                        )}
                  </span>
                </>
              }
            >
              <span class="dashboard-xresult__eyebrow">Relevante utdrag</span>
              <span class="dashboard-xresult__highlights">
                <For each={highlights()}>{(passage) => <span class="dashboard-xresult__highlight">{passage}</span>}</For>
              </span>
            </Show>
            <span class="dashboard-xresult__tags">
              <For each={[props.result.hostname, i18n.tr('webside', 'web page'), i18n.tr('kilde', 'source')]}>
                {(label) => <span>{label}</span>}
              </For>
            </span>
            <Show when={props.onFindSimilar}>
              <button
                type="button"
                class="dashboard-xresult__similar"
                onClick={() => props.onFindSimilar?.(props.result)}
              >
                <Layers class="size-3.5" aria-hidden="true" />
                {i18n.tr('Finn lignende', 'Find similar')}
              </button>
            </Show>
          </span>
        </div>
        <div class="dashboard-xresult__media">
          <Switch
            fallback={<div class="dashboard-xresult__placeholder">{props.result.hostname.slice(0, 1).toUpperCase()}</div>}
          >
            <Match when={props.image?.thumbnailUrl}>
              {(thumb) => (
                <img
                  src={thumb()}
                  alt={props.image?.title ?? ''}
                  loading="lazy"
                  referrerpolicy="no-referrer"
                  class="dashboard-xresult__img"
                />
              )}
            </Match>
            <Match when={favicon()}>
              {(icon) => (
                <div class="dashboard-xresult__favicon">
                  <img src={icon()} alt="" loading="lazy" />
                </div>
              )}
            </Match>
          </Switch>
          <Show when={favicon()}>
            {(icon) => (
              <span class="dashboard-xresult__badge">
                <img src={icon()} alt="" loading="lazy" />
              </span>
            )}
          </Show>
        </div>
      </div>
      <a
        href={props.result.url}
        target="_blank"
        rel="noopener noreferrer"
        class="dashboard-xresult__external"
        aria-label={i18n.tr(`Åpne ${props.result.title} i ny fane`, `Open ${props.result.title} in a new tab`)}
      >
        <ExternalLink class="size-3.5" aria-hidden="true" />
        {i18n.tr('Ny fane', 'New tab')}
      </a>
    </article>
  )
}

export function WebPageViewer(props: { result: PreviewResult | null }) {
  const i18n = useI18n()
  return (
    <section class="dashboard-webview" aria-label={i18n.tr('Websidevisning i Verevon', 'Web page view in Verevon')}>
      <Show
        when={props.result}
        fallback={
          <div class="dashboard-webview__empty">
            <p class="dashboard-xsearch-eyebrow">{i18n.tr('Webvisning', 'Web view')}</p>
            <h3>{i18n.tr('Velg et søkeresultat', 'Choose a search result')}</h3>
            <p>
              {i18n.tr(
                'Verevon åpner siden her, slik at JavaScript og layout kan lastes uten at du forlater arbeidsflaten.',
                'Verevon opens the page here so JavaScript and layout can load without leaving the workspace.',
              )}
            </p>
          </div>
        }
      >
        {(result) => (
          <>
            <div class="dashboard-webview__bar">
              <div>
                <p>{safeHostname(result().url)}</p>
                <span>{result().title}</span>
              </div>
              <a href={result().url} target="_blank" rel="noopener noreferrer">
                <ExternalLink class="size-3.5" aria-hidden="true" />
                {i18n.tr('Ny fane', 'New tab')}
              </a>
            </div>
            <iframe
              src={result().url}
              title={result().title}
              class="dashboard-webview__frame"
              loading="eager"
              referrerpolicy="strict-origin-when-cross-origin"
              sandbox="allow-scripts allow-same-origin allow-forms allow-popups allow-popups-to-escape-sandbox allow-presentation"
            />
            <p class="dashboard-webview__hint">
              {i18n.tr(
                'Hvis kilden blokkerer innebygging, bruk Ny fane. Sider som tillater iframe kjører egne scripts her inne.',
                'If the source blocks embedding, use New tab. Pages that allow iframes run their own scripts here.',
              )}
            </p>
          </>
        )}
      </Show>
    </section>
  )
}

export function ImageResultsPanel(props: {
  images: ImageHit[]
  imagesError: string | null
  imagesStatus: ImagesStatus
  query: string
}) {
  const i18n = useI18n()
  return (
    <Switch>
      <Match when={props.imagesStatus === 'loading' || props.imagesStatus === 'idle'}>
        <div class="dashboard-ximages">
          <For each={[152, 120, 176, 136, 168, 112, 144, 160]}>
            {(height) => <div class="dashboard-ximages__skeleton" style={{ height: `${height}px` }} />}
          </For>
        </div>
      </Match>
      <Match when={props.imagesStatus === 'error'}>
        <div class="dashboard-xsearch-error">
          {props.imagesError ?? i18n.tr('Bildesøket kunne ikke fullføres.', 'Image search could not be completed.')}
        </div>
      </Match>
      <Match when={props.images.length === 0}>
        <div class="dashboard-xsearch-empty">
          {i18n.tr(`Fant ingen bilder for ${props.query}.`, `No images found for ${props.query}.`)}
        </div>
      </Match>
      <Match when={true}>
        <div class="dashboard-ximages">
          <For each={props.images}>
            {(image) => (
              <a href={image.url} target="_blank" rel="noopener noreferrer" class="dashboard-ximages__item">
                <img src={image.thumbnailUrl} alt={image.title ?? ''} loading="lazy" referrerpolicy="no-referrer" />
                <div class="dashboard-ximages__cap">
                  <p>{image.title ?? i18n.tr('Bildetreff', 'Image result')}</p>
                  <small>
                    {i18n.tr(
                      'Verevon kan bruke dette visuelle treffet til å forklare kontekst, layout, produktdetaljer eller stedssignaler.',
                      'Verevon can use this visual to explain context, layout, product details, or place cues.',
                    )}
                  </small>
                </div>
              </a>
            )}
          </For>
        </div>
      </Match>
    </Switch>
  )
}

export function VideoResultsPanel(props: { error: string | null; query: string; status: ImagesStatus; videos: VideoHit[] }) {
  const i18n = useI18n()
  const [playing, setPlaying] = createSignal<string | null>(null)

  return (
    <Switch>
      <Match when={props.status === 'loading' || props.status === 'idle'}>
        <div class="dashboard-xsearch-status">
          <Loader2 class="size-4 dashboard-xsearch-spin" aria-hidden="true" />
          {i18n.tr('Laster videoer...', 'Loading videos...')}
        </div>
      </Match>
      <Match when={props.status === 'error'}>
        <div class="dashboard-xsearch-error">
          {props.error ?? i18n.tr('Videosøk kunne ikke fullføres.', 'Video search could not be completed.')}
        </div>
      </Match>
      <Match when={props.videos.length === 0}>
        <SearchVerticalEmpty label={i18n.tr('video', 'video')} query={props.query} />
      </Match>
      <Match when={true}>
        <div class="dashboard-xvideos">
          <For each={props.videos}>
            {(video) => (
              <Show
                when={playing() === video.url && video.embedUrl}
                fallback={
                  <a
                    href={video.url}
                    target="_blank"
                    rel="noopener noreferrer"
                    class="dashboard-xvideo"
                    onClick={(event) => {
                      if (video.embedUrl) {
                        event.preventDefault()
                        setPlaying(video.url)
                      }
                    }}
                  >
                    <div class="dashboard-xvideo__thumb">
                      <Show when={video.thumbnailUrl}>
                        {(thumb) => <img src={thumb()} alt="" loading="lazy" referrerpolicy="no-referrer" />}
                      </Show>
                      <span class="dashboard-xvideo__play">▶</span>
                    </div>
                    <div class="dashboard-xvideo__body">
                      <p class="dashboard-xvideo__host">
                        {safeHostname(video.url)}{video.length ? ` · ${video.length}` : ''}
                      </p>
                      <h3 class="dashboard-xvideo__title">{video.title ?? safeHostname(video.url)}</h3>
                      <p class="dashboard-xvideo__desc">
                        {video.author ? `${video.author} · ` : ''}
                        {i18n.tr(
                          `Klikk for å spille av${video.embedUrl ? ' her' : ' på kilden'}.`,
                          `Click to play${video.embedUrl ? ' here' : ' on the source'}.`,
                        )}
                      </p>
                    </div>
                  </a>
                }
              >
                <div class="dashboard-xvideo dashboard-xvideo--playing">
                  <div class="dashboard-xvideo__thumb">
                    <iframe
                      src={video.embedUrl ?? ''}
                      title={video.title ?? i18n.tr('Video', 'Video')}
                      style={{ width: '100%', height: '100%', border: '0' }}
                      allow="accelerometer; autoplay; clipboard-write; encrypted-media; gyroscope; picture-in-picture"
                      allowfullscreen
                      loading="lazy"
                    />
                  </div>
                  <div class="dashboard-xvideo__body">
                    <h3 class="dashboard-xvideo__title">{video.title ?? safeHostname(video.url)}</h3>
                  </div>
                </div>
              </Show>
            )}
          </For>
        </div>
      </Match>
    </Switch>
  )
}

export function VerevonResultsSection(props: { loading: boolean; results: NavbarSearchResult[] }) {
  const i18n = useI18n()
  return (
    <Show when={props.loading || props.results.length > 0}>
      <section class="dashboard-xsearch-verevon">
        <p class="dashboard-xsearch-eyebrow">
          {i18n.tr('Fra Verevon', 'From Verevon')}
          <Show when={props.loading}>
            <Loader2 class="size-3 dashboard-xsearch-spin" aria-hidden="true" />
          </Show>
        </p>
        <Show
          when={props.results.length > 0}
          fallback={<p class="dashboard-xsearch-verevon__hint">{i18n.tr('Søker i selskapets kunnskap...', 'Searching company knowledge...')}</p>}
        >
          <div class="dashboard-xsearch-verevon__list">
            <For each={props.results}>
              {(result) => (
                <a href={result.href || '/knowledge'} link class="dashboard-xsearch-verevon__item">
                  <span class="dashboard-xsearch-verevon__badge" aria-hidden="true">V</span>
                  <span class="dashboard-xsearch-verevon__body">
                    <span class="dashboard-xsearch-verevon__title">{result.label}</span>
                    <Show when={result.excerpt}>
                      <span class="dashboard-xsearch-verevon__excerpt">{result.excerpt}</span>
                    </Show>
                  </span>
                  <Show when={result.source}>
                    <span class="dashboard-xsearch-verevon__source">{result.source}</span>
                  </Show>
                </a>
              )}
            </For>
          </div>
        </Show>
      </section>
    </Show>
  )
}

export function MapGuidePanel(props: { query: string; results: PreviewResult[] }) {
  const i18n = useI18n()
  const places = () => props.results.slice(0, 4)

  return (
    <div class="dashboard-xmap">
      <div class="dashboard-xmap__canvas">
        <p class="dashboard-xsearch-eyebrow">{i18n.tr('AI-kartguide', 'AI map guide')}</p>
        <h3 class="dashboard-xmap__title">{props.query}</h3>
        <div class="dashboard-xmap__plane">
          <For each={places()}>
            {(_place, index) => (
              <span
                class="dashboard-xmap__pin"
                style={{ left: `${18 + index() * 18}%`, top: `${24 + (index() % 2) * 34}%` }}
              >
                {index() + 1}
              </span>
            )}
          </For>
        </div>
      </div>
      <div class="dashboard-xmap__list">
        <For each={places()}>
          {(place, index) => (
            <a href={place.url} target="_blank" rel="noopener noreferrer" class="dashboard-xmap__item">
              <span class="dashboard-xmap__num">{index() + 1}</span>
              <span class="dashboard-xmap__meta">
                <span class="dashboard-xmap__name">{place.title}</span>
                <span class="dashboard-xmap__host">{place.hostname}</span>
              </span>
              <ExternalLink class="size-3.5 shrink-0" aria-hidden="true" />
            </a>
          )}
        </For>
      </div>
    </div>
  )
}

export function ShoppingResultsPanel(props: { query: string; results: PreviewResult[] }) {
  const i18n = useI18n()
  const items = () => props.results.slice(0, 5)

  return (
    <Show when={items().length > 0} fallback={<SearchVerticalEmpty label={i18n.tr('shopping', 'shopping')} query={props.query} />}>
      <div class="dashboard-xshop">
        <section class="dashboard-xshop__intro">
          <p class="dashboard-xsearch-eyebrow">{i18n.tr('AI shopping-/bookingguide', 'AI shopping/booking guide')}</p>
          <p>
            {i18n.tr(
              'Verevon sammenligner bookingreferanser, shopping-sider, lignende artikler og kildetroverdighet før en handling anbefales.',
              'Verevon compares booking references, shopping pages, similar articles, and source credibility before recommending an action.',
            )}
          </p>
        </section>
        <For each={items()}>
          {(item, index) => (
            <a href={item.url} target="_blank" rel="noopener noreferrer" class="dashboard-xshop__item">
              <span class="dashboard-xshop__num">{index() + 1}</span>
              <span class="dashboard-xshop__body">
                <span class="dashboard-xshop__host">{item.hostname}</span>
                <span class="dashboard-xshop__title">{item.title}</span>
                <span class="dashboard-xshop__desc">
                  {item.snippet ?? i18n.tr('Mulig kjøps-, booking-, referanse- eller sammenligningskilde.', 'Potential buying, booking, reference, or comparison source.')}
                </span>
              </span>
            </a>
          )}
        </For>
      </div>
    </Show>
  )
}

function SearchVerticalEmpty(props: { label: string; query: string }) {
  const i18n = useI18n()
  return (
    <div class="dashboard-xsearch-empty">
      {i18n.tr(
        `Verevon trenger mer pålitelig ${props.label}-grunnlag for ${props.query}. Prøv et mer spesifikt sted, produkt, merke eller kildenavn.`,
        `Verevon needs more reliable ${props.label} evidence for ${props.query}. Try a more specific place, product, brand, or source name.`,
      )}
    </div>
  )
}

export function SearchInsightRail(props: {
  activeTab: SearchResultTab
  answer: string
  images: ImageHit[]
  query: string
  sourceItems: Array<{ hostname: string; title: string; url: string }>
}) {
  const i18n = useI18n()
  return (
    <div class="dashboard-xrail">
      <section class="dashboard-xrail__card">
        <p class="dashboard-xsearch-eyebrow">{i18n.tr('Kilder', 'Sources')}</p>
        <ExpandedSourcesList sourceItems={props.sourceItems} />
      </section>

      <section class="dashboard-xrail__card">
        <p class="dashboard-xsearch-eyebrow">{i18n.tr('AI-notater', 'AI notes')}</p>
        <p class="dashboard-xrail__notes">
          {props.answer
            || i18n.tr(
              `Verevon organiserer ${searchTabEvidenceLabel(props.activeTab, 'no')}-grunnlag for "${props.query}" på tvers av kilder, visuelt materiale og nyttige neste handlinger.`,
              `Verevon is organizing ${searchTabEvidenceLabel(props.activeTab, 'en')} evidence for "${props.query}" across sources, visuals, and useful next actions.`,
            )}
        </p>
      </section>

      <section class="dashboard-xrail__card">
        <p class="dashboard-xsearch-eyebrow">{i18n.tr('Tips', 'Tips')}</p>
        <ul class="dashboard-xrail__tips">
          <For each={searchRailTips(props.activeTab, i18n.locale())}>
            {(tip) => (
              <li>
                <span class="dashboard-xrail__dot" aria-hidden="true" />
                <span>{tip}</span>
              </li>
            )}
          </For>
        </ul>
      </section>

      <Show when={props.images[0]?.thumbnailUrl}>
        {(thumb) => (
          <section class="dashboard-xrail__media">
            <img src={thumb()} alt={props.images[0]?.title ?? ''} loading="lazy" referrerpolicy="no-referrer" />
            <p>
              {i18n.tr(
                'Visuell kontekst kan hjelpe Verevon med å forklare steder, produkter, skjermbilder eller layoutspesifikke detaljer.',
                'Visual context can help Verevon explain places, products, screenshots, or layout-specific details.',
              )}
            </p>
          </section>
        )}
      </Show>
    </div>
  )
}

/** Paste-a-URL/text seed for an ad-hoc "find similar" query. */
export function SimilarSeedForm(props: { onSubmit: (value: string) => void }) {
  const i18n = useI18n()
  const [value, setValue] = createSignal('')

  return (
    <form
      class="dashboard-xsimilar-seed"
      onSubmit={(event) => {
        event.preventDefault()
        const trimmed = value().trim()
        if (!trimmed) return
        props.onSubmit(trimmed)
        setValue('')
      }}
    >
      <Layers class="size-4 shrink-0 text-[#9A9188]" aria-hidden="true" />
      <input
        value={value()}
        onInput={(event) => setValue(event.currentTarget.value)}
        placeholder={i18n.tr('Lim inn en URL eller tekst for å finne lignende sider...', 'Paste a URL or text to find similar pages...')}
        autocomplete="off"
        aria-label={i18n.tr('Finn lignende fra URL eller tekst', 'Find similar from URL or text')}
      />
      <button type="submit" disabled={!value().trim()}>{i18n.tr('Finn lignende', 'Find similar')}</button>
    </form>
  )
}

/** Neighbour list for an Exa-style "find similar" lookup, with a close control. */
export function SimilarResultsPanel(props: {
  error: string | null
  label: string
  onClose: () => void
  onOpen?: (result: PreviewResult) => void
  query: string
  results: PreviewResult[]
  status: ImagesStatus
}) {
  const i18n = useI18n()
  return (
    <section class="dashboard-xsimilar" aria-label={i18n.tr('Lignende sider', 'Similar pages')}>
      <div class="dashboard-xsimilar__head">
        <p class="dashboard-xsearch-eyebrow">
          {i18n.tr('Lignende sider', 'Similar pages')}
          <span class="dashboard-xsimilar__seed">{props.label}</span>
        </p>
        <button type="button" class="dashboard-xsimilar__close" onClick={() => props.onClose()} aria-label={i18n.tr('Lukk lignende sider', 'Close similar pages')}>
          <X class="size-3.5" aria-hidden="true" />
        </button>
      </div>
      <Switch>
        <Match when={props.status === 'loading' || props.status === 'idle'}>
          <div class="dashboard-xsearch-status">
            <Loader2 class="size-4 dashboard-xsearch-spin" aria-hidden="true" />
            {i18n.tr('Finner lignende sider...', 'Finding similar pages...')}
          </div>
        </Match>
        <Match when={props.status === 'error'}>
          <div class="dashboard-xsearch-error">
            {props.error ?? i18n.tr('Kunne ikke finne lignende sider.', 'Could not find similar pages.')}
          </div>
        </Match>
        <Match when={props.results.length === 0}>
          <div class="dashboard-xsearch-empty">
            {i18n.tr(`Fant ingen lignende sider for ${props.query}.`, `No similar pages found for ${props.query}.`)}
          </div>
        </Match>
        <Match when={true}>
          <div class="dashboard-xsimilar__list">
            <For each={props.results}>
              {(result, index) => (
                <button
                  type="button"
                  class="dashboard-xsimilar__item"
                  onClick={() => props.onOpen?.(result)}
                >
                  <span class="dashboard-xsimilar__num">{index() + 1}</span>
                  <span class="dashboard-xsimilar__body">
                    <span class="dashboard-xsimilar__host">
                      {result.hostname}
                      <Show when={typeof result.score === 'number'}>
                        <span class="dashboard-xresult__score">{relevancePercent(result.score as number)}%</span>
                      </Show>
                    </span>
                    <span class="dashboard-xsimilar__title">{result.title}</span>
                    <Show when={result.snippet}>
                      <span class="dashboard-xsimilar__snippet">{result.snippet}</span>
                    </Show>
                  </span>
                  <ExternalLink class="size-3.5 shrink-0" aria-hidden="true" />
                </button>
              )}
            </For>
          </div>
        </Match>
      </Switch>
    </section>
  )
}

function searchTabEvidenceLabel(tab: SearchResultTab, locale: 'en' | 'no'): string {
  const labels: Record<SearchResultTab, { en: string; no: string }> = {
    Images: { en: 'image', no: 'bilde' },
    Info: { en: 'information', no: 'informasjons' },
    Map: { en: 'map', no: 'kart' },
    Shopping: { en: 'shopping', no: 'shopping' },
    Videos: { en: 'video', no: 'video' },
  }
  return labels[tab][locale]
}

function ExpandedSourcesList(props: { sourceItems: Array<{ hostname: string; title: string; url: string }> }) {
  const i18n = useI18n()
  return (
    <Show
      when={props.sourceItems.length > 0}
      fallback={<p class="dashboard-xrail__empty">{i18n.tr('Kilder vises når Verevon har sikre treff.', 'Sources appear when Verevon has confident matches.')}</p>}
    >
      <div class="dashboard-xrail__sources">
        <For each={props.sourceItems}>
          {(source) => (
            <a href={source.url} target="_blank" rel="noopener noreferrer" class="dashboard-xrail__source">
              <span class="dashboard-xrail__source-badge">{source.hostname.slice(0, 1).toUpperCase()}</span>
              <span class="dashboard-xrail__source-body">
                <span class="dashboard-xrail__source-title">{source.title}</span>
                <span class="dashboard-xrail__source-host">{source.hostname}</span>
              </span>
              <ExternalLink class="size-3.5 shrink-0" aria-hidden="true" />
            </a>
          )}
        </For>
      </div>
    </Show>
  )
}
