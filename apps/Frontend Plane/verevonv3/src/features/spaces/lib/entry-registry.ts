import type { Component } from 'solid-js'

import { linkHost, safeHref } from '@/shared/lib/safe-link'

/**
 * Typed room entries and the card registry (WS15-S9).
 *
 * A message (a human post, or an agent card of WS18-S7) can carry one typed entry (`MessageEntry` on the
 * wire: a closed `kind`, a `version` and the kind's payload). The timeline asks this registry which component
 * draws it, so a new kind (the run card of WS09, the approval panel of WS10, the room file card of WS14)
 * registers here instead of adding a branch to the timeline. A kind the build does not know, a newer version
 * of a known kind, or a payload that does not parse is drawn by the neutral fallback card and never throws.
 * A kind registered `replacesBody` draws the message text itself, as plain text, in place of the markdown body.
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

/** The tones an agent card may carry, as the server's golden schema names them. */
export type AgentCardTone = 'info' | 'progress' | 'done' | 'blocked'

/** One labelled value of an agent card, as plain text. */
export type AgentCardField = {
  readonly label: string
  readonly value: string
}

/**
 * The plain-text card a room agent posted (kind `agent_card`, WS18-S7). Its body is the message text: the
 * card draws it as text and the timeline never draws it as markdown (`replacesBody`). A payload this build
 * cannot read, or a version it does not know, leaves the title, tone and fields empty; the body still draws.
 */
export type AgentCardEntry = {
  readonly kind: 'agent_card'
  readonly version: number
  readonly title: string
  readonly tone: AgentCardTone | undefined
  readonly fields: readonly AgentCardField[]
}

/** An entry this build cannot draw: only the kind and version are kept. */
export type UnknownEntry = {
  readonly kind: 'unknown'
  readonly rawKind: string
  readonly version: number
}

/** The typed entry of one message. */
export type TurnEntry = LinkPreviewsEntry | AgentCardEntry | UnknownEntry

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
const AGENT_CARD_TONES: readonly AgentCardTone[] = ['info', 'progress', 'done', 'blocked']
const MAX_CARD_FIELDS = 8
const MAX_CARD_TITLE_CHARS = 120
const MAX_CARD_FIELD_LABEL_CHARS = 40
const MAX_CARD_FIELD_VALUE_CHARS = 200

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
 * The agent card from the wire form. Every version of the kind stays an agent card, so its body is drawn as
 * plain text whatever its payload; only version 1 with a readable `agent_card` payload fills the card.
 */
function agentCardEntry(version: number, record: Record<string, unknown>): AgentCardEntry {
  const payload =
    record.agent_card && typeof record.agent_card === 'object' ? (record.agent_card as Record<string, unknown>) : undefined
  if (version !== 1 || !payload) return { kind: 'agent_card', version, title: '', tone: undefined, fields: [] }
  const tone = AGENT_CARD_TONES.find((candidate) => candidate === payload.tone)
  const rawFields = Array.isArray(payload.fields) ? payload.fields.slice(0, MAX_CARD_FIELDS) : []
  const fields = rawFields.flatMap((field): AgentCardField[] => {
    if (!field || typeof field !== 'object') return []
    const row = field as Record<string, unknown>
    const label = text(row.label, MAX_CARD_FIELD_LABEL_CHARS)
    const value = text(row.value, MAX_CARD_FIELD_VALUE_CHARS)
    return label && value ? [{ label, value }] : []
  })
  return { kind: 'agent_card', version, title: text(payload.title, MAX_CARD_TITLE_CHARS), tone, fields }
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
  if (kind === 'agent_card') return agentCardEntry(version, record)
  if (kind !== 'link_previews' || version !== 1) return unknownEntry(kind, version)
  const raw = Array.isArray(record.link_previews) ? record.link_previews.slice(0, MAX_PREVIEWS) : []
  const items = raw.flatMap((preview) => {
    const item = linkPreviewItem(preview)
    return item ? [item] : []
  })
  return items.length > 0 ? { kind: 'link_previews', version: 1, items } : unknownEntry(kind, version)
}

/** What a card receives: the entry it was registered for, and the message text of its turn when it has one. */
export type TurnEntryCardProps<E extends TurnEntry = TurnEntry> = {
  readonly entry: E
  /** The message text as the server stored it. A card draws it as plain text, never as markdown. */
  readonly body?: string
}
export type TurnEntryCard<E extends TurnEntry = TurnEntry> = Component<TurnEntryCardProps<E>>

/** How a kind is drawn. `replacesBody`: the card draws the message text itself, so the timeline does not. */
export type TurnEntryCardOptions = {
  readonly replacesBody?: boolean
}

const cards = new Map<string, TurnEntryCard<never>>()
const bodyReplacingKinds = new Set<string>()
let fallbackCard: TurnEntryCard | undefined

/**
 * Registers the card for a kind. A later registration of the same kind replaces the earlier one (the
 * last module to load wins, which keeps hot reload working).
 */
export function registerTurnEntryCard<E extends TurnEntry>(
  kind: E['kind'],
  card: TurnEntryCard<E>,
  options?: TurnEntryCardOptions,
): void {
  cards.set(kind, card as TurnEntryCard<never>)
  if (options?.replacesBody === true) bodyReplacingKinds.add(kind)
  else bodyReplacingKinds.delete(kind)
}

/** Whether the card for this entry draws the message text in place of the timeline's markdown body. */
export function turnEntryReplacesBody(entry: TurnEntry): boolean {
  return bodyReplacingKinds.has(entry.kind)
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
