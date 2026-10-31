import { browserSessionFromPreview } from '@/features/dashboard/home/browser-session'
import { hostnameOf, type ScrapePreview } from '@/features/dashboard/home/knowledge-preview'

/**
 * The browser session controller (WS9.F) owns a session without a scrape. The
 * session model in `browser-session.ts` is built from a `ScrapePreview`, so the
 * controller starts from an empty preview: no blocks, no markdown, no scrape
 * source. The session attaches on top of it exactly as it does for the composer.
 */
export function emptyBrowserPreview(url: string, title?: string): ScrapePreview {
  return {
    blocks: [],
    charCount: 0,
    description: '',
    markdown: '',
    source: 'empty',
    title: title?.trim() || hostnameOf(url),
    url,
  }
}

/** The browser view model for a preview. Re-exported under the controller's name. */
export const viewModelOf = browserSessionFromPreview
