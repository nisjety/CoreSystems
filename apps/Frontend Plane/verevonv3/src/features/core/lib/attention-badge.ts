import { isWaitingState, type AttentionState } from '@/shared/attention/attention-state'

const TITLE_COUNT_PREFIX = /^\(\d+\)\s/
const FAVICON_SIZE = 32
const DOT_RADIUS = FAVICON_SIZE * 0.22

type AppBadgeNavigator = Navigator & {
  setAppBadge?: (count: number) => Promise<void>
  clearAppBadge?: () => Promise<void>
}

/**
 * The number the tab shows: the distinct threads that wait on the person. Unread
 * notifications are a different count and never enter it.
 */
export function waitingThreadCount(items: readonly { threadId: string; state?: AttentionState }[]): number {
  return new Set(items.filter((item) => isWaitingState(item.state)).map((item) => item.threadId)).size
}

/** The page title with the count in front, `(2) Verevon`. A count already there is replaced, never stacked. */
export function attentionTitle(title: string, waiting: number): string {
  const base = title.replace(TITLE_COUNT_PREFIX, '')
  return waiting > 0 ? `(${waiting}) ${base}` : base
}

let dottedFavicon: { base: string; href: string } | null = null
let faviconRun = 0

/**
 * Shows the waiting count on the tab title, a dot on the favicon and the installed app's
 * badge. Each surface is best effort: a browser without one of them keeps the others.
 */
export function applyAttentionBadge(waiting: number): void {
  if (typeof document === 'undefined') return
  document.title = attentionTitle(document.title, waiting)
  setFaviconDot(waiting > 0)
  const badgeNavigator = navigator as AppBadgeNavigator
  try {
    const pending = waiting > 0 ? badgeNavigator.setAppBadge?.(waiting) : badgeNavigator.clearAppBadge?.()
    void pending?.catch(() => undefined)
  } catch {
    // Some contexts refuse the badge API outright; the title and favicon still show the count.
  }
}

function setFaviconDot(on: boolean): void {
  const link = document.querySelector<HTMLLinkElement>('link[rel="icon"]')
  if (!link) return
  const base = link.dataset.baseHref ?? link.getAttribute('href') ?? ''
  link.dataset.baseHref = base
  const run = ++faviconRun
  if (!on) {
    link.setAttribute('href', base)
    return
  }
  if (dottedFavicon?.base === base) {
    link.setAttribute('href', dottedFavicon.href)
    return
  }
  void drawDottedFavicon(base).then((href) => {
    if (run !== faviconRun) return
    dottedFavicon = { base, href }
    link.setAttribute('href', href)
  }).catch(() => undefined)
}

function drawDottedFavicon(base: string): Promise<string> {
  return new Promise((resolve, reject) => {
    const image = new Image()
    image.onload = () => {
      const canvas = document.createElement('canvas')
      canvas.width = FAVICON_SIZE
      canvas.height = FAVICON_SIZE
      const context = canvas.getContext('2d')
      if (!context) {
        reject(new Error('no 2d canvas'))
        return
      }
      context.drawImage(image, 0, 0, FAVICON_SIZE, FAVICON_SIZE)
      // The dot takes the app's danger token, so it follows the theme; it falls back to the system text colour.
      const token = getComputedStyle(document.documentElement).getPropertyValue('--color-danger').trim()
      context.beginPath()
      context.arc(FAVICON_SIZE - DOT_RADIUS, DOT_RADIUS, DOT_RADIUS, 0, Math.PI * 2)
      context.fillStyle = token || 'CanvasText'
      context.fill()
      resolve(canvas.toDataURL('image/png'))
    }
    image.onerror = () => reject(new Error('favicon did not load'))
    image.src = base
  })
}
