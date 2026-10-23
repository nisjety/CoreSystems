import { classifyOmniboxInput } from '@/features/dashboard/home/browser-omnibox'

// What pressing Enter (or the go button) in the browser omnibox does. The
// resolver only decides; the caller performs the navigation, the search or the
// hand-off to chat. Free text is never sent to a third-party engine: it becomes
// a Verevon search, or nothing.
export type OmniboxIntent =
  | { kind: 'navigate'; url: string }
  | { kind: 'search'; query: string }
  | { kind: 'ask'; text: string }
  | { kind: 'noop'; reason: 'empty' | 'demo' | 'search_disabled' }

export type OmniboxResolveContext = {
  /** A demo build has no live backend, so free text does nothing rather than search. */
  demo?: boolean
  /** Whether the browser omnibox search is on for this build (see {@link browserOmniboxSearchEnabled}). */
  searchEnabled: boolean
  /** The current page address: an empty field navigates there, as it always has. */
  fallbackUrl?: string | null
}

/**
 * Build-time switch `VITE_BROWSER_OMNIBOX_SEARCH`. Only `1` turns it on, so an
 * unset variable keeps the omnibox to URL navigation.
 */
export function browserOmniboxSearchEnabled(): boolean {
  return import.meta.env.VITE_BROWSER_OMNIBOX_SEARCH === '1'
}

/**
 * Resolves what the typed text asks for. An address (or a bare host) navigates
 * there; an empty field navigates to the current page when there is one;
 * anything else is a query, searched through Verevon when search is on. `ask`
 * hands the text to chat instead (the dropdown's "Ask Verevon" row).
 */
export function resolveOmniboxIntent(
  input: string,
  context: OmniboxResolveContext,
  options: { ask?: boolean } = {},
): OmniboxIntent {
  if (options.ask) {
    const text = input.trim()
    return text ? { kind: 'ask', text } : { kind: 'noop', reason: 'empty' }
  }
  const classified = classifyOmniboxInput(input)
  if (classified.kind === 'url') return { kind: 'navigate', url: classified.url }
  if (classified.kind === 'empty') {
    const fallback = context.fallbackUrl?.trim()
    return fallback ? { kind: 'navigate', url: fallback } : { kind: 'noop', reason: 'empty' }
  }
  if (context.demo) return { kind: 'noop', reason: 'demo' }
  if (!context.searchEnabled) return { kind: 'noop', reason: 'search_disabled' }
  return { kind: 'search', query: classified.query }
}
