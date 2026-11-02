// WS11-S7: the attention inbox client.
//
// Wire (apps/gateway/src/domains/spaces/attention.rs):
//   GET  /api/v1/spaces/attention/items?state=&kind=&limit=&cursor=
//        -> { data: { items[], needs_you[], mode, gaps[], next_cursor } }
//   POST /api/v1/spaces/attention/items/{item_id}/state   {state, snooze_until?} -> { data: { item_id, state, snoozed_until } }
//   GET|PUT /api/v1/spaces/{ref}/threads/{thread_id}/follow   {state} -> { data: { state, reason } }
//
// A row is ids, a closed kind word and times. A title (`headline`) is read at render under the
// viewer's current access and is `null` with `headline_state` `hidden` or `restricted` whenever it
// may not be shown. Nothing here is persisted in the browser.
import { requestJson } from './http'

export type AttentionItemKind = 'mention' | 'reply' | 'invite' | 'agent_done' | 'agent_failed' | 'reminder'
export type AttentionItemState = 'open' | 'snoozed' | 'done'
export type AttentionHeadlineState = 'ok' | 'hidden' | 'restricted'
export type ThreadFollowState = 'following' | 'muted' | 'none'

export const ATTENTION_ITEM_KINDS: readonly AttentionItemKind[] = [
  'mention',
  'reply',
  'invite',
  'agent_done',
  'agent_failed',
  'reminder',
]

export type AttentionActor = {
  readonly kind: 'user' | 'agent' | 'system'
  readonly subjectId: string | null
  readonly agentRef: string | null
}

export type AttentionItem = {
  readonly itemId: string
  readonly kind: AttentionItemKind
  readonly spaceRef: string
  readonly spaceName: string | null
  readonly threadId: string | null
  readonly messageId: string | null
  readonly runId: string | null
  readonly actor: AttentionActor
  /** How many signals the row folded together (coalescing). */
  readonly count: number
  readonly state: AttentionItemState
  readonly snoozedUntil: number | null
  readonly createdAt: number
  readonly updatedAt: number
  readonly headline: string | null
  readonly headlineState: AttentionHeadlineState
}

/** An approval waiting on the viewer, derived live by Session Core; never a stored item. */
export type AttentionNeedsYou = {
  readonly approvalId: string
  readonly spaceRef: string
  readonly spaceName: string | null
  readonly threadId: string | null
  readonly runId: string | null
  readonly requestedAt: number | null
  readonly headline: string | null
  readonly headlineState: AttentionHeadlineState
}

export type AttentionItemsPage = {
  readonly items: readonly AttentionItem[]
  readonly needsYou: readonly AttentionNeedsYou[]
  /** `zdr_refs_only`: the org keeps no text, so no titles and no live approvals are shown. */
  readonly mode: 'live' | 'zdr_refs_only'
  /** Sources the gateway could not read (`space_decisions`, `session_core`); a gap is unknown, not empty. */
  readonly gaps: readonly string[]
  readonly nextCursor: string | null
}

type Wire = Record<string, unknown>
const isRecord = (value: unknown): value is Wire => !!value && typeof value === 'object' && !Array.isArray(value)
const text = (value: unknown): string | null => (typeof value === 'string' && value !== '' ? value : null)
const time = (value: unknown): number | null =>
  typeof value === 'number' && Number.isFinite(value) ? value : null

function isKind(value: unknown): value is AttentionItemKind {
  return typeof value === 'string' && (ATTENTION_ITEM_KINDS as readonly string[]).includes(value)
}

function headlineStateOf(value: unknown): AttentionHeadlineState {
  return value === 'ok' || value === 'hidden' ? value : 'restricted'
}

function actorFrom(value: unknown): AttentionActor {
  const wire = isRecord(value) ? value : {}
  const kind = wire.kind === 'user' || wire.kind === 'agent' ? wire.kind : 'system'
  return { kind, subjectId: text(wire.subject_id), agentRef: text(wire.agent_ref) }
}

function itemFrom(value: unknown): AttentionItem | undefined {
  if (!isRecord(value)) return undefined
  const itemId = text(value.item_id)
  const spaceRef = text(value.space_ref)
  if (!itemId || !spaceRef || !isKind(value.kind)) return undefined
  const state: AttentionItemState =
    value.state === 'done' || value.state === 'snoozed' ? value.state : 'open'
  const count = typeof value.count === 'number' && value.count > 0 ? Math.floor(value.count) : 1
  return {
    itemId,
    kind: value.kind,
    spaceRef,
    spaceName: text(value.space_name),
    threadId: text(value.thread_id),
    messageId: text(value.message_id),
    runId: text(value.run_id),
    actor: actorFrom(value.actor),
    count,
    state,
    snoozedUntil: time(value.snoozed_until),
    createdAt: time(value.created_at) ?? 0,
    updatedAt: time(value.updated_at) ?? 0,
    headline: text(value.headline),
    headlineState: headlineStateOf(value.headline_state),
  }
}

function needsYouFrom(value: unknown): AttentionNeedsYou | undefined {
  if (!isRecord(value)) return undefined
  const approvalId = text(value.approval_id)
  const spaceRef = text(value.space_ref)
  if (!approvalId || !spaceRef) return undefined
  return {
    approvalId,
    spaceRef,
    spaceName: text(value.space_name),
    threadId: text(value.thread_id),
    runId: text(value.run_id),
    requestedAt: time(value.requested_at),
    headline: text(value.headline),
    headlineState: headlineStateOf(value.headline_state),
  }
}

/** The pure parse of the items body, exported for its test. A row it does not understand is dropped. */
export function attentionItemsFromWire(data: unknown): AttentionItemsPage {
  const body = isRecord(data) ? data : {}
  return {
    items: (Array.isArray(body.items) ? body.items : [])
      .map(itemFrom)
      .filter((item): item is AttentionItem => item !== undefined),
    needsYou: (Array.isArray(body.needs_you) ? body.needs_you : [])
      .map(needsYouFrom)
      .filter((item): item is AttentionNeedsYou => item !== undefined),
    mode: body.mode === 'zdr_refs_only' ? 'zdr_refs_only' : 'live',
    gaps: Array.isArray(body.gaps) ? body.gaps.filter((gap): gap is string => typeof gap === 'string').slice(0, 10) : [],
    nextCursor: text(body.next_cursor),
  }
}

export async function listAttentionItems(input: {
  readonly state?: AttentionItemState
  readonly kind?: AttentionItemKind
  readonly limit?: number
  readonly cursor?: string | null
} = {}): Promise<AttentionItemsPage> {
  const params = new URLSearchParams()
  params.set('state', input.state ?? 'open')
  if (input.kind) params.set('kind', input.kind)
  if (input.limit) params.set('limit', String(input.limit))
  if (input.cursor) params.set('cursor', input.cursor)
  return attentionItemsFromWire(await requestJson<unknown>(`/api/v1/spaces/attention/items?${params.toString()}`))
}

export type AttentionItemStateChange =
  | { readonly state: 'done' | 'open' }
  | { readonly state: 'snoozed'; readonly snoozeUntil: number }

/** Mark one of the viewer's own items done, open or snoozed. */
export async function setAttentionItemState(
  itemId: string,
  change: AttentionItemStateChange,
): Promise<{ readonly itemId: string; readonly state: AttentionItemState; readonly snoozedUntil: number | null }> {
  const wire = await requestJson<Wire>(`/api/v1/spaces/attention/items/${encodeURIComponent(itemId)}/state`, {
    method: 'POST',
    body: JSON.stringify(
      change.state === 'snoozed'
        ? { state: 'snoozed', snooze_until: change.snoozeUntil }
        : { state: change.state },
    ),
  })
  return {
    itemId: text(wire.item_id) ?? itemId,
    state: wire.state === 'done' || wire.state === 'snoozed' ? wire.state : 'open',
    snoozedUntil: time(wire.snoozed_until),
  }
}

function followStateOf(value: unknown): ThreadFollowState {
  return value === 'following' || value === 'muted' ? value : 'none'
}

export async function getThreadFollow(spaceRef: string, threadId: string): Promise<ThreadFollowState> {
  const wire = await requestJson<Wire>(
    `/api/v1/spaces/${encodeURIComponent(spaceRef)}/threads/${encodeURIComponent(threadId)}/follow`,
  )
  return followStateOf(wire.state)
}

/** Follow, mute or clear the viewer's follow of one thread. Setting one is refused 412 under ZDR. */
export async function setThreadFollow(
  spaceRef: string,
  threadId: string,
  state: ThreadFollowState,
): Promise<ThreadFollowState> {
  const wire = await requestJson<Wire>(
    `/api/v1/spaces/${encodeURIComponent(spaceRef)}/threads/${encodeURIComponent(threadId)}/follow`,
    { method: 'PUT', body: JSON.stringify({ state }) },
  )
  return followStateOf(wire.state)
}
