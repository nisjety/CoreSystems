/**
 * Lanes, sentences and salience for the attention inbox (WS11-S7).
 *
 * Presentation only. Nothing here decides who may see a row: the gateway already dropped every
 * row of a room the viewer left, and a title arrives only when Session Core released it. This
 * module turns a row's closed kind word into one honest sentence, groups rows into lanes and
 * orders them with the same salience words the Space activity feed uses (`ActivitySalience`,
 * reused unchanged): failures rise, reads recede.
 */
import type { ActivitySalience } from '@/features/spaces/lib/activity-grammar'
import { spaceThreadHref } from '@/features/spaces/lib/space-thread-deep-link'
import type {
  AttentionItem,
  AttentionItemKind,
  AttentionNeedsYou,
} from '@/shared/api/spaces-attention-items-client'

type Tr = (no: string, en: string) => string

/** The inbox lanes, in the order they are drawn. */
export type AttentionLane = 'needs_you' | 'mentions' | 'threads' | 'agents' | 'invites' | 'reminders'

export const ATTENTION_LANES: readonly AttentionLane[] = [
  'needs_you',
  'mentions',
  'threads',
  'agents',
  'invites',
  'reminders',
]

const LANE_OF_KIND: Readonly<Record<AttentionItemKind, Exclude<AttentionLane, 'needs_you'>>> = {
  mention: 'mentions',
  reply: 'threads',
  agent_done: 'agents',
  agent_failed: 'agents',
  invite: 'invites',
  reminder: 'reminders',
}

export function laneOfKind(kind: AttentionItemKind): Exclude<AttentionLane, 'needs_you'> {
  return LANE_OF_KIND[kind]
}

export function laneLabel(lane: AttentionLane, tr: Tr): string {
  switch (lane) {
    case 'needs_you':
      return tr('Trenger deg', 'Needs you')
    case 'mentions':
      return tr('Omtaler', 'Mentions')
    case 'threads':
      return tr('Tråder', 'Threads')
    case 'agents':
      return tr('Agenter', 'Agents')
    case 'invites':
      return tr('Invitasjoner', 'Invites')
    case 'reminders':
      return tr('Påminnelser', 'Reminders')
  }
}

/** How loudly a row speaks. A failed run outranks a mention; a finished run recedes. */
export function itemSalience(kind: AttentionItemKind): ActivitySalience {
  switch (kind) {
    case 'agent_failed':
      return 'critical'
    case 'mention':
    case 'invite':
    case 'reminder':
      return 'attention'
    case 'reply':
      return 'normal'
    case 'agent_done':
      return 'muted'
  }
}

const SALIENCE_RANK: Readonly<Record<ActivitySalience, number>> = {
  critical: 0,
  attention: 1,
  normal: 2,
  muted: 3,
}

/** Within a lane: salience first, then the most recently updated. Does not mutate its input. */
export function orderItems(items: readonly AttentionItem[]): AttentionItem[] {
  return [...items].sort(
    (a, b) =>
      SALIENCE_RANK[itemSalience(a.kind)] - SALIENCE_RANK[itemSalience(b.kind)] || b.updatedAt - a.updatedAt,
  )
}

/** Open rows by lane. `needs_you` is live-derived and held apart, so it is never in this map. */
export function groupByLane(
  items: readonly AttentionItem[],
): Readonly<Record<Exclude<AttentionLane, 'needs_you'>, AttentionItem[]>> {
  const lanes: Record<Exclude<AttentionLane, 'needs_you'>, AttentionItem[]> = {
    mentions: [],
    threads: [],
    agents: [],
    invites: [],
    reminders: [],
  }
  for (const item of orderItems(items)) lanes[laneOfKind(item.kind)].push(item)
  return lanes
}

function who(item: AttentionItem, tr: Tr): string {
  if (item.actor.kind === 'agent') return tr('En agent', 'An agent')
  if (item.actor.kind === 'user') return tr('Noen', 'Someone')
  return tr('Verevon', 'Verevon')
}

/** The room a row belongs to, never a raw ref when a name is known. */
export function roomName(item: { spaceName: string | null }, tr: Tr): string {
  return item.spaceName ?? tr('et rom', 'a room')
}

/** One sentence per row. The title, when released, is shown beside it and never inside it. */
export function itemSentence(item: AttentionItem, tr: Tr): string {
  const room = roomName(item, tr)
  const more = item.count > 1 ? ` (${item.count})` : ''
  switch (item.kind) {
    case 'mention':
      return tr(`${who(item, tr)} nevnte deg i ${room}${more}`, `${who(item, tr)} mentioned you in ${room}${more}`)
    case 'reply':
      return tr(`Nye svar i en tråd du følger i ${room}${more}`, `New replies in a thread you follow in ${room}${more}`)
    case 'invite':
      return tr(`Du er invitert til ${room}`, `You were invited to ${room}`)
    case 'agent_done':
      return tr(`Agenten din er ferdig i ${room}${more}`, `Your agent finished in ${room}${more}`)
    case 'agent_failed':
      return tr(`Agenten din feilet i ${room}${more}`, `Your agent failed in ${room}${more}`)
    case 'reminder':
      return tr(`Påminnelse i ${room}${more}`, `Reminder in ${room}${more}`)
  }
}

export function needsYouSentence(entry: AttentionNeedsYou, tr: Tr): string {
  return tr(
    `En agent venter på godkjenning i ${roomName(entry, tr)}`,
    `An agent is waiting for your approval in ${roomName(entry, tr)}`,
  )
}

/** Where a row leads: the post when it names one, otherwise the room. */
export function itemHref(item: { spaceRef: string; threadId: string | null }): string {
  return spaceThreadHref(item.spaceRef, item.threadId)
}

/** The title line, or the honest reason there is none. `null` means draw nothing. */
export function headlineText(
  entry: { headline: string | null; headlineState: 'ok' | 'hidden' | 'restricted'; threadId: string | null },
  tr: Tr,
): string | null {
  if (entry.headline) return entry.headline
  if (entry.threadId === null) return null
  if (entry.headlineState === 'hidden') return tr('Forhåndsvisning er skjult', 'Preview hidden')
  if (entry.headlineState === 'restricted') return tr('Ikke tilgjengelig for deg', 'Not available to you')
  return null
}

/** Snooze presets, in milliseconds from now. All inside the server's one minute to 30 day window. */
export const SNOOZE_PRESETS: readonly { readonly id: string; readonly ms: number }[] = [
  { id: 'hour', ms: 60 * 60 * 1000 },
  { id: 'tomorrow', ms: 24 * 60 * 60 * 1000 },
  { id: 'week', ms: 7 * 24 * 60 * 60 * 1000 },
]

export function snoozeLabel(id: string, tr: Tr): string {
  switch (id) {
    case 'hour':
      return tr('1 time', '1 hour')
    case 'tomorrow':
      return tr('I morgen', 'Tomorrow')
    default:
      return tr('1 uke', '1 week')
  }
}
