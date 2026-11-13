import { For, Show, createEffect, createSignal, onCleanup, untrack } from 'solid-js'
import { narrationState } from '@/shared/activity/narration'
import { toolPresentation } from '@/shared/chat-nodes'
import { loadSearchImages, safeHostname, type ImageHit } from '@/shared/api/search-client'
import { useI18n } from '@/shared/i18n'
import { isSafeLinkHref } from '../lib/safe-href'
import type { ChatToolCall } from './chat-types'
import { searchQueryFromArgs } from './chat-normalizers'
import './ChatImageStrip.css'

/**
 * `VITE_CHAT_IMAGE_STRIP=true` shows the image results of a settled web-search answer under the answer, each linked to
 * the page it came from (F-t3-044, W4-IO-1). Unset, or any value but `true` (the default), no answer shows a strip.
 * Exact string, as the other chat switches.
 */
export const chatImageStripEnabled = () => import.meta.env.VITE_CHAT_IMAGE_STRIP === 'true'

/** The most images one strip shows. */
const MAX_STRIP_IMAGES = 6

/**
 * Only the web-search tools. The other tools that present as a search (a knowledge lookup, a provider or shipping
 * lookup) are not web queries, and their text must not be sent to an image search.
 */
const WEB_SEARCH_TOOLS: ReadonlySet<string> = new Set(['web_search', 'web.search'])

/** A thumbnail is loaded only from an http(s) address. */
const HTTP_URL_PATTERN = /^https?:\/\//i

/** How far outside the viewport a strip may be and still start its request. */
const VIEW_MARGIN = '240px'

/** One image the strip shows: its thumbnail, the page it links to, and that page's host. */
export type StripImage = {
  host: string
  thumbnailUrl: string
  title: string
  url: string
}

/**
 * The query of this answer's first web search that finished without a failure, or `undefined` when none did. The query
 * is read from the call's args, as the search card reads it, and nothing is stored.
 */
export function webSearchQueryOf(calls: readonly ChatToolCall[]): string | undefined {
  for (const call of calls) {
    if (!WEB_SEARCH_TOOLS.has(call.name.trim().toLowerCase())) continue
    if (toolPresentation(call).intent !== 'search') continue
    // The same test the activity line uses, so a search it counts as failed or still running gets no strip.
    if (narrationState(call) !== 'done') continue
    const query = searchQueryFromArgs(call.args)?.trim()
    if (query) return query
  }
  return undefined
}

/**
 * The images a strip shows, from the search payload: at most six, each with a safe page link and an http(s)
 * thumbnail. A hit that fails either check is dropped, so a link that is not safe is never shown as a link.
 */
export function stripImagesOf(images: readonly ImageHit[] | undefined): StripImage[] {
  const shown: StripImage[] = []
  for (const hit of images ?? []) {
    if (shown.length >= MAX_STRIP_IMAGES) break
    const url = typeof hit.url === 'string' ? hit.url.trim() : ''
    const thumbnailUrl = typeof hit.thumbnailUrl === 'string' ? hit.thumbnailUrl.trim() : ''
    if (!isSafeLinkHref(url) || !isSafeLinkHref(thumbnailUrl) || !HTTP_URL_PATTERN.test(thumbnailUrl)) continue
    shown.push({
      host: safeHostname(url),
      thumbnailUrl,
      title: typeof hit.title === 'string' ? hit.title.trim() : '',
      url,
    })
  }
  return shown
}

/**
 * The images each turn's strip settled on, for this session: a turn is asked for once, and its answer is kept here,
 * none included. Held in memory only. Nothing is written to storage, and no image is fetched beyond its thumbnail.
 */
const settledStrips = new Map<string, readonly StripImage[]>()

/** The request for a turn that has not settled yet. A second mount of the same turn shares it rather than asking again. */
const pendingStrips = new Map<string, Promise<readonly StripImage[]>>()

/**
 * The images for a turn, asked for at most once per session. No signal is passed: a request cancelled when the turn
 * leaves the screen would be asked again on the next mount, which would send a second search for the same turn. A
 * failed search settles as no images.
 */
function askForStrip(turnId: string, query: string): Promise<readonly StripImage[]> {
  const settled = settledStrips.get(turnId)
  if (settled) return Promise.resolve(settled)
  const pending = pendingStrips.get(turnId)
  if (pending) return pending
  const asked = loadSearchImages(query)
    .then((payload) => stripImagesOf(payload.images))
    .catch((): StripImage[] => [])
    .then((images) => {
      settledStrips.set(turnId, images)
      pendingStrips.delete(turnId)
      return images
    })
  pendingStrips.set(turnId, asked)
  return asked
}

/**
 * The image strip under a settled web-search answer (F-t3-044, W4-IO-1): thumbnails from the answer's own search, each
 * a link to the page it came from. The images are asked for once per turn, when the strip first comes into view, and
 * the answer is kept for the session. Nothing shows while the request runs, or when the search found no images.
 */
export function ChatImageStrip(props: { turnId: string; query: string }) {
  const i18n = useI18n()
  // Read once: the strip is mounted for one settled turn, and that turn's query does not change once it has settled.
  const turnId = untrack(() => props.turnId)
  const query = untrack(() => props.query)
  const [images, setImages] = createSignal<readonly StripImage[] | null>(settledStrips.get(turnId) ?? null)
  let root: HTMLDivElement | undefined
  let disposed = false

  const request = () => {
    void askForStrip(turnId, query).then((next) => {
      if (!disposed) setImages(next)
    })
  }

  // Solid v2 has no onMount: a two-phase effect with a constant compute runs once after mount.
  createEffect(
    () => undefined,
    () => {
      // A turn asked for before already shows its images, from the session's cache.
      if (!root || settledStrips.has(turnId)) return
      if (typeof IntersectionObserver === 'undefined') {
        request()
        return
      }
      const observer = new IntersectionObserver(
        (entries) => {
          if (!entries.some((entry) => entry.isIntersecting)) return
          observer.disconnect()
          request()
        },
        { rootMargin: VIEW_MARGIN },
      )
      observer.observe(root)
      return () => observer.disconnect()
    },
  )

  onCleanup(() => {
    disposed = true
  })

  return (
    <div class="verevon-chat-image-strip" ref={(el) => { root = el }}>
      <Show when={images()}>
        {(list) => (
          <Show when={list().length > 0}>
            <p class="verevon-chat-image-strip__label">{i18n.tr('Bilder fra websøket', 'Images from the web search')}</p>
            <ul class="verevon-chat-image-strip__list">
              <For each={list()}>
                {(image) => (
                  <li class="verevon-chat-image-strip__item">
                    <a class="verevon-chat-image-strip__link" href={image.url} target="_blank" rel="noopener noreferrer">
                      <img src={image.thumbnailUrl} alt="" loading="lazy" referrerpolicy="no-referrer" />
                      <span class="verevon-chat-image-strip__caption">
                        <span class="verevon-chat-image-strip__title">{image.title || image.host}</span>
                        <span class="verevon-chat-image-strip__host">{image.host}</span>
                      </span>
                    </a>
                  </li>
                )}
              </For>
            </ul>
          </Show>
        )}
      </Show>
    </div>
  )
}
