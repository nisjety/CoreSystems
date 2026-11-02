import type { Component } from 'solid-js'

import { linkHost, safeHref } from '@/shared/lib/safe-link'

/**
 * Typed room entries and the card registry (WS15-S9).
 *
 * A human post can carry one typed entry (`MessageEntry` on the wire: a closed `kind`, a `version` and
 * the kind's payload). The timeline asks this registry which component draws it, so a new kind (the
 * run card of WS09, the approval panel of WS10, the room file card of WS14) registers here instead of
 * adding a branch to the timeline. A kind the build does not know, a newer version of a known kind, or
 * a payload that does not parse is drawn by the neutral fallback card and never throws.
 *
 * Nothing here fetches or stores anything: previews are text snapshots taken when the post was sent
 * (no image is ever loaded), and the host shown is read from the link itself.
 */

/** One link preview as the card draws it. `href` is `null` when the link is not a safe target. */
export type LinkPreviewItem = {
  readonly href: string | null
  readonly host: string
  readonly title: string
  readonly description: string
  readonly siteName: string
  /** ISO-8601, or empty when the server sent none that parses. */
  readonly fetchedAt: string
}

export type LinkPreviewsEntry = {
  readonly kind: 'link_previews'
  readonly version: 1
  readonly items: readonly LinkPreviewItem[]
}

/** An entry this build cannot draw: only the kind and version are kept. */
export type UnknownEntry = {
  readonly kind: 'unknown'
  readonly rawKind: string
  readonly version: number
}

/** The typed entry of one human post. */
export type TurnEntry = LinkPreviewsEntry | UnknownEntry

/**
 * A row of the room timeline that is not a post. Declared here so the system-notice slices
 * (WS15-S11, S12) and the timeline share one shape; this slice only draws it.
 */
export type TimelineEntry =
  | { readonly kind: 'post'; readonly threadId: string }
  | {
      readonly kind: 'system'
      readonly id: string
      readonly at: string
      readonly code: string
      readonly actorId?: string
      readonly subjectId?: string
      readonly fromRole?: string
      readonly toRole?: string
    }
  | { readonly kind: 'unknown'; readonly id: string; readonly at: string; readonly rawKind: string }

const MAX_PREVIEWS = 3
const MAX_RAW_KIND_CHARS = 40

function text(value: unknown, max: number): string {
  return typeof value === 'string' ? value.replace(/\p{C}/gu, '').trim().slice(0, max) : ''
}

function isoTime(value: unknown): string {
  if (typeof value !== 'string' || !value.trim()) return ''
  const ms = Date.parse(value.trim())
  return Number.isFinite(ms) ? new Date(ms).toISOString() : ''
}

function linkPreviewItem(value: unknown): LinkPreviewItem | undefined {
  if (!value || typeof value !== 'object') return undefined
  const record = value as Record<string, unknown>
  const url = typeof record.url === 'string' ? record.url : ''
  const finalUrl = typeof record.final_url === 'string' && record.final_url.trim() ? record.final_url : url
  // The link opened is the final URL, and only when the shared link rules accept it. The host shown is
  // read from that link, never from the server's `host` field, so a card cannot name one host and open
  // another.
  const href = safeHref(finalUrl)
  const host = href ? (linkHost(href) ?? '') : ''
  if (!href || !host) return undefined
  return {
    href,
    host,
    title: text(record.title, 200),
    description: text(record.description, 400),
    siteName: text(record.site_name, 80),
    fetchedAt: isoTime(record.fetched_at),
  }
}

function unknownEntry(kind: string, version: number): UnknownEntry {
  return { kind: 'unknown', rawKind: kind.slice(0, MAX_RAW_KIND_CHARS), version }
}

/**
 * The typed entry of a message from the wire form (`entry` of a message in the timeline or transcript
 * JSON). `undefined` when the message has none; the fallback entry when it has one this build cannot draw.
 */
export function turnEntryFromWire(value: unknown): TurnEntry | undefined {
  if (!value || typeof value !== 'object') return undefined
  const record = value as Record<string, unknown>
  const kind = typeof record.kind === 'string' ? record.kind.trim() : ''
  if (!kind) return undefined
  const version = typeof record.version === 'number' && Number.isInteger(record.version) ? record.version : 0
  if (kind !== 'link_previews' || version !== 1) return unknownEntry(kind, version)
  const raw = Array.isArray(record.link_previews) ? record.link_previews.slice(0, MAX_PREVIEWS) : []
  const items = raw.flatMap((preview) => {
    const item = linkPreviewItem(preview)
    return item ? [item] : []
  })
  return items.length > 0 ? { kind: 'link_previews', version: 1, items } : unknownEntry(kind, version)
}

/** What a card receives: the entry it was registered for. */
export type TurnEntryCardProps<E extends TurnEntry = TurnEntry> = { readonly entry: E }
export type TurnEntryCard<E extends TurnEntry = TurnEntry> = Component<TurnEntryCardProps<E>>

const cards = new Map<string, TurnEntryCard<never>>()
let fallbackCard: TurnEntryCard | undefined

/**
 * Registers the card for a kind. A later registration of the same kind replaces the earlier one (the
 * last module to load wins, which keeps hot reload working).
 */
export function registerTurnEntryCard<E extends TurnEntry>(kind: E['kind'], card: TurnEntryCard<E>): void {
  cards.set(kind, card as TurnEntryCard<never>)
}

/** Sets the neutral card used for every kind with no registration. */
export function registerFallbackTurnEntryCard(card: TurnEntryCard): void {
  fallbackCard = card
}

/**
 * The card that draws this entry: the registered one for its kind, otherwise the fallback. `undefined`
 * only when neither exists (a test that never imported the cards).
 */
export function turnEntryCard(entry: TurnEntry): TurnEntryCard | undefined {
  return (cards.get(entry.kind) as TurnEntryCard | undefined) ?? fallbackCard
}
