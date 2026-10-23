import type { BrowserTab } from '@/shared/api/browser-client'

/**
 * Pure `@tab` mention token for the dashboard composer (E3.3-c).
 *
 * A `@tab` mention asks the agent to use the page of a Quarry browser tab as
 * context. It is mention-only: it never grants an action, never opens,
 * focuses or switches a tab, and never reads page content. The gateway must
 * re-resolve it against the live Quarry tab list before anything reaches a
 * model (that half is not in this module). Here the token is parsed from the
 * draft and resolved against the tab list the caller already holds, failing
 * closed: a mention that does not resolve to exactly one usable tab is
 * reported as unresolved with a reason and is never attached.
 *
 * Token forms (matched case-insensitively):
 *   @tab            the single active tab
 *   @tab:<tabId>    the tab with that id; ids are [A-Za-z0-9_-], 1 to 64 chars
 *
 * Nothing here touches the DOM or the network. The composer picker and the
 * chip that shows a resolved mention are not built here: where they render
 * when no dock is open is open point D9.
 */

const TAB_ID_RE = /^[A-Za-z0-9_-]{1,64}$/

// Group 1: start of text, or whitespace or opening punctuation before the "@".
// Group 2: optional explicit tab id. The token must end at text end or before
// whitespace or closing punctuation. ":" is deliberately not a terminator, so
// "@tab:" with no id is not a token, and "@tabs", "@table" and "me@tab.no"
// (no boundary before "@") are not tokens either.
const TAB_MENTION_RE = /(^|[\s([{"'])@tab(?::([A-Za-z0-9_-]{1,64}))?(?=$|[\s.,;!?)\]}"'])/gi

export type TabMention = {
  /** Index of the "@" in the draft. */
  start: number
  /** Index just past the last character of the token. */
  end: number
  /** Explicit tab id, or null for the active tab. */
  tabId: string | null
}

/** Find every `@tab` token in a composer draft, in order. */
export function parseTabMentions(draft: string): TabMention[] {
  const mentions: TabMention[] = []
  for (const match of draft.matchAll(TAB_MENTION_RE)) {
    const prefix = match[1] ?? ''
    const start = (match.index ?? 0) + prefix.length
    mentions.push({
      start,
      end: start + match[0].length - prefix.length,
      tabId: match[2] ?? null,
    })
  }
  return mentions
}

/**
 * The token text to insert for a tab, or null when the id is not a valid tab
 * id (so a bad id can never be written into a draft).
 */
export function formatTabMention(tabId?: string | null): string | null {
  if (tabId === undefined || tabId === null) return '@tab'
  return TAB_ID_RE.test(tabId) ? `@tab:${tabId}` : null
}

/** What a resolved mention contributes. It carries no grant and no page content. */
export type TabContextRef = {
  kind: 'browser-tab'
  tabId: string
  /** Tab title, else the origin and path, else the tab id. Shown to the person. */
  label: string
  /** Origin and path of the tab URL with query, fragment and credentials dropped. Null when there is no http(s) URL. */
  target: string | null
  /** Mentions never grant. Always 'none'. */
  grant: 'none'
}

export type TabMentionUnresolvedReason =
  | 'zero-retention'
  | 'no-tabs'
  | 'no-active-tab'
  | 'ambiguous-tab'
  | 'unknown-tab'

export type TabMentionResolution =
  | { status: 'resolved'; mention: TabMention; ref: TabContextRef }
  | { status: 'unresolved'; mention: TabMention; reason: TabMentionUnresolvedReason }

export type ResolveTabMentionOptions = {
  /**
   * True when the session is Zero Data Retention. Page context is refused, not
   * attached, so every mention is unresolved with 'zero-retention'.
   */
  zeroRetention?: boolean
}

/**
 * Resolve mentions against the tab list the caller holds. Entries with an
 * unusable tab id are ignored, so they can never match. The result has one
 * entry per mention, in draft order.
 */
export function resolveTabMentions(
  mentions: readonly TabMention[],
  tabs: readonly BrowserTab[] | null | undefined,
  options: ResolveTabMentionOptions = {},
): TabMentionResolution[] {
  const usable = (tabs ?? []).filter(isUsableTab)
  return mentions.map((mention) => resolveOne(mention, usable, options))
}

/**
 * Distinct resolved refs in first-seen order. Unresolved mentions are dropped.
 */
export function uniqueTabRefs(resolutions: readonly TabMentionResolution[]): TabContextRef[] {
  const seen = new Set<string>()
  const refs: TabContextRef[] = []
  for (const resolution of resolutions) {
    if (resolution.status !== 'resolved' || seen.has(resolution.ref.tabId)) continue
    seen.add(resolution.ref.tabId)
    refs.push(resolution.ref)
  }
  return refs
}

/**
 * Origin and path of an http(s) URL, with query, fragment and credentials
 * dropped. Returns null for anything else (about:, javascript:, blank, bad).
 */
export function safeTabTarget(url: string | null | undefined): string | null {
  const value = url?.trim()
  if (!value) return null
  try {
    const parsed = new URL(value)
    if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') return null
    return `${parsed.origin}${parsed.pathname}`
  } catch {
    return null
  }
}

function isUsableTab(tab: BrowserTab): boolean {
  return typeof tab.tabId === 'string' && TAB_ID_RE.test(tab.tabId)
}

function resolveOne(
  mention: TabMention,
  tabs: readonly BrowserTab[],
  options: ResolveTabMentionOptions,
): TabMentionResolution {
  if (options.zeroRetention) return unresolved(mention, 'zero-retention')
  if (tabs.length === 0) return unresolved(mention, 'no-tabs')

  if (mention.tabId === null) {
    const active = tabs.filter((tab) => tab.active === true)
    const [onlyActive] = active
    if (!onlyActive) return unresolved(mention, 'no-active-tab')
    if (active.length > 1) return unresolved(mention, 'ambiguous-tab')
    return resolved(mention, onlyActive)
  }

  const matches = tabs.filter((tab) => tab.tabId === mention.tabId)
  const [onlyMatch] = matches
  if (!onlyMatch) return unresolved(mention, 'unknown-tab')
  if (matches.length > 1) return unresolved(mention, 'ambiguous-tab')
  return resolved(mention, onlyMatch)
}

function resolved(mention: TabMention, tab: BrowserTab): TabMentionResolution {
  const target = safeTabTarget(tab.url)
  const title = tab.title?.trim() ?? ''
  return {
    status: 'resolved',
    mention,
    ref: {
      kind: 'browser-tab',
      tabId: tab.tabId,
      label: title || target || tab.tabId,
      target,
      grant: 'none',
    },
  }
}

function unresolved(mention: TabMention, reason: TabMentionUnresolvedReason): TabMentionResolution {
  return { status: 'unresolved', mention, reason }
}
