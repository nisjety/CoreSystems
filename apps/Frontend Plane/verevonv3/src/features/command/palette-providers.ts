/**
 * Palette providers (WS13-S6, D9).
 *
 * Every provider reads something the viewer can already see through its owning
 * plane's own gate (rooms and rosters through Control-gated reads, threads from
 * the room's already-polled projection, knowledge through Data Plane search).
 * The palette therefore cannot widen what a viewer sees; it only indexes it.
 * A failed read yields an empty group, never a fabricated one.
 */

import { readChatThreadHistory } from '@/features/chat/lib/chat-thread-history'
import { liveThreadsIn } from '@/features/spaces/lib/space-live-work'
import { getActionDescriptor, type ActionId } from '@/shared/actions/action-registry'
import { executeAction, previewAction } from '@/shared/actions/action-client'
import type { NavbarSearchResult } from '@/shared/api/navbar-client'
import {
  getAgentInstallations,
  getSpaceRoster,
  listSpaces,
  type AgentDefinitionInstallations,
  type SpaceRosterMember,
  type SpaceSummary,
} from '@/shared/api/spaces-client'
import { getSession } from '@/shared/session/session-store'
import type { CommandContext } from './command-registry'

export type PaletteGroup = 'recent' | 'messages' | 'rooms' | 'threads' | 'agents' | 'people' | 'actions' | 'knowledge' | 'chats' | 'commands'

/** Group order in the palette, top to bottom. */
export const PALETTE_GROUP_ORDER: readonly PaletteGroup[] = [
  'recent',
  'messages',
  'commands',
  'rooms',
  'threads',
  'agents',
  'people',
  'actions',
  'knowledge',
  'chats',
]

export interface PaletteItem {
  readonly id: string
  readonly group: PaletteGroup
  readonly label: string
  readonly hint?: string
  /** Navigates here when picked. */
  readonly href?: string
  /** Runs instead of navigating (commands, actions). */
  readonly run?: () => void | Promise<void>
  /** Remembered in recents when picked. */
  /** A local chat to select (chat-thread-history) before navigating to `href`. */
  readonly selectChatThreadId?: string
  readonly recentKind?: 'room' | 'thread' | 'agent' | 'person' | 'chat' | 'knowledge'
}

export const spaceHref = (spaceRef: string, tab = 'chat') => `/spaces/${encodeURIComponent(spaceRef)}#${tab}`

export function matchesQuery(query: string, ...fields: ReadonlyArray<string | null | undefined>): boolean {
  const needle = query.trim().toLocaleLowerCase()
  if (!needle) return true
  return fields.some((field) => field?.toLocaleLowerCase().includes(needle))
}

/**
 * Rooms in the order the sidebar lists them (personal first, then the
 * organization room, then the rest in the server's order); archived rooms are
 * left out so `Alt+n` and the switcher never land in a closed room.
 */
export function orderRoomsLikeSidebar(spaces: readonly SpaceSummary[]): readonly SpaceSummary[] {
  const live = spaces.filter((space) => space.archived !== true)
  const personal = live.filter((space) => space.kind === 'personal')
  const others = live
    .filter((space) => space.kind !== 'personal')
    .slice()
    .sort((a, b) => Number(b.is_organization_room === true) - Number(a.is_organization_room === true))
  return [...personal, ...others]
}

const CACHE_TTL_MS = 60_000
type Cached<T> = { at: number; value: T }
let roomsCache: Cached<readonly SpaceSummary[]> | undefined
let agentsCache: Cached<readonly AgentDefinitionInstallations[]> | undefined
const rosterCache = new Map<string, Cached<readonly SpaceRosterMember[]>>()

const isFresh = (entry: Cached<unknown> | undefined): boolean => entry !== undefined && Date.now() - entry.at < CACHE_TTL_MS

export async function loadRooms(): Promise<readonly SpaceSummary[]> {
  if (roomsCache && isFresh(roomsCache)) return roomsCache.value
  try {
    const value = orderRoomsLikeSidebar(await listSpaces())
    roomsCache = { at: Date.now(), value }
    return value
  } catch {
    return roomsCache?.value ?? []
  }
}

export async function loadAgents(): Promise<readonly AgentDefinitionInstallations[]> {
  if (agentsCache && isFresh(agentsCache)) return agentsCache.value
  try {
    const value = await getAgentInstallations()
    agentsCache = { at: Date.now(), value }
    return value
  } catch {
    return agentsCache?.value ?? []
  }
}

export async function loadPeople(spaceRef: string): Promise<readonly SpaceRosterMember[]> {
  const cached = rosterCache.get(spaceRef)
  if (cached && isFresh(cached)) return cached.value
  try {
    const value = await getSpaceRoster(spaceRef)
    rosterCache.set(spaceRef, { at: Date.now(), value })
    return value
  } catch {
    return cached?.value ?? []
  }
}

/** Drops cached reads (sign-out, organization switch). */
export function clearPaletteCaches(): void {
  roomsCache = undefined
  agentsCache = undefined
  rosterCache.clear()
}

export function roomItems(rooms: readonly SpaceSummary[], query: string): readonly PaletteItem[] {
  return rooms
    .filter((room) => matchesQuery(query, room.name, room.topic))
    .map((room) => ({
      id: `room:${room.space_ref}`,
      group: 'rooms' as const,
      label: room.name,
      hint: room.topic ?? undefined,
      href: spaceHref(room.space_ref),
      recentKind: 'room' as const,
    }))
}

/** Threads already loaded by the room's own poll; no new fetch. */
export function threadItems(spaceRef: string | null, query: string): readonly PaletteItem[] {
  if (!spaceRef) return []
  return liveThreadsIn(spaceRef)
    .filter((thread) => matchesQuery(query, thread.title, thread.preview))
    .slice(0, 8)
    .map((thread) => ({
      id: `thread:${thread.thread_id}`,
      group: 'threads' as const,
      label: thread.title?.trim() || thread.preview?.trim() || thread.thread_id,
      href: `/spaces/${encodeURIComponent(spaceRef)}?thread_id=${encodeURIComponent(thread.thread_id)}#chat`,
      recentKind: 'thread' as const,
    }))
}

export function agentItems(
  agents: readonly AgentDefinitionInstallations[],
  spaceRef: string | null,
  query: string,
): readonly PaletteItem[] {
  const items: PaletteItem[] = []
  for (const agent of agents) {
    if (!matchesQuery(query, agent.name, agent.agent_ref, agent.description)) continue
    const active = agent.installations.filter((installation) => installation.status === undefined || installation.status === 'active')
    // Prefer the agent as bound in the room in scope; else its first active room.
    const target = active.find((installation) => installation.space_ref === spaceRef) ?? active[0]
    if (!target) continue
    items.push({
      id: `agent:${agent.agent_ref}`,
      group: 'agents',
      label: agent.name?.trim() || agent.agent_ref,
      hint: target.space_name,
      href: spaceHref(target.space_ref, 'agent'),
      recentKind: 'agent',
    })
  }
  return items.slice(0, 8)
}

export function peopleItems(roster: readonly SpaceRosterMember[], spaceRef: string | null, query: string): readonly PaletteItem[] {
  if (!spaceRef) return []
  return roster
    .filter((member) => member.subject_type === 'user' && matchesQuery(query, member.display_name, member.subject_id))
    .slice(0, 8)
    .map((member) => ({
      id: `person:${member.subject_id}`,
      group: 'people' as const,
      label: member.display_name || member.subject_id,
      hint: member.role,
      href: spaceHref(spaceRef, 'medlemmer'),
      recentKind: 'person' as const,
    }))
}

export function chatItems(query: string): readonly PaletteItem[] {
  const needle = query.trim()
  if (needle.length < 2) return []
  return readChatThreadHistory()
    .filter((item) => matchesQuery(needle, item.title, item.preview))
    .slice(0, 5)
    .map((item) => ({
      id: `chat:${item.threadId}`,
      group: 'chats' as const,
      label: item.title,
      hint: item.preview,
      href: '/chat',
      selectChatThreadId: item.threadId,
      recentKind: 'chat' as const,
    }))
}

export function knowledgeItems(results: readonly NavbarSearchResult[]): readonly PaletteItem[] {
  return results.map((result) => ({
    id: `knowledge:${result.id}`,
    group: 'knowledge' as const,
    label: result.label,
    hint: result.excerpt,
    href: result.href,
    recentKind: 'knowledge' as const,
  }))
}

/**
 * Actions the palette may run. Only registry entries that are low risk and need
 * no approval, and only when the current context supplies their whole input.
 * Everything else stays hidden until a schema-driven argument prompt exists.
 * Phase 1 has one.
 */
const ACTION_INPUTS: Partial<Record<ActionId, (ctx: CommandContext) => unknown | null>> = {
  'spaces.mark_read': (ctx) => (ctx.spaceRef ? { spaceRef: ctx.spaceRef } : null),
  // WS11-S3: needs no context; moves only the caller's own read positions (gateway: spaces/attention.rs).
  'spaces.mark_all_read': () => ({}),
}

export interface PaletteActionLabel {
  readonly no: string
  readonly en: string
}

const ACTION_LABELS: Partial<Record<ActionId, PaletteActionLabel>> = {
  'spaces.mark_read': { no: 'Merk rommet som lest', en: 'Mark this room read' },
  'spaces.mark_all_read': { no: 'Merk alle rom som lest', en: 'Mark all rooms read' },
}

export type PaletteActionOutcome = 'done' | 'needs_input' | 'needs_approval' | 'failed'

/** Preview, refuse anything that is not plainly safe, then execute through the shared action client. */
export async function runPaletteAction(actionId: ActionId, input: unknown): Promise<PaletteActionOutcome> {
  try {
    const preview = await previewAction(actionId, input)
    if (preview.requiresApproval || preview.risk !== 'low') return 'needs_approval'
    if (preview.missingInputs.length > 0) return 'needs_input'
    const session = getSession()
    const userId = session.user?.id
    const orgId = session.activeOrg?.id
    if (!userId || !orgId) return 'failed'
    await executeAction(actionId, { type: 'human', userId, orgId }, input)
    return 'done'
  } catch {
    return 'failed'
  }
}

export function actionItems(
  ctx: CommandContext,
  query: string,
  tr: (no: string, en: string) => string,
  onOutcome: (outcome: PaletteActionOutcome) => void,
): readonly PaletteItem[] {
  const items: PaletteItem[] = []
  for (const [id, resolve] of Object.entries(ACTION_INPUTS) as Array<[ActionId, (ctx: CommandContext) => unknown | null]>) {
    const descriptor = getActionDescriptor(id)
    if (!descriptor || descriptor.risk !== 'low' || descriptor.requiresApproval) continue
    const input = resolve(ctx)
    if (input === null) continue
    const label = ACTION_LABELS[id]
    const text = label ? tr(label.no, label.en) : descriptor.label
    if (!matchesQuery(query, text, descriptor.label)) continue
    items.push({
      id: `action:${id}`,
      group: 'actions',
      label: text,
      run: async () => onOutcome(await runPaletteAction(id, input)),
    })
  }
  return items
}

/** The room in scope from a pathname (`/spaces/:ref[/...]`), or null outside a room. */
export function spaceRefFromPathname(pathname: string): string | null {
  if (!pathname.startsWith('/spaces/')) return null
  const encoded = pathname.slice('/spaces/'.length).split('/')[0]
  if (!encoded) return null
  try {
    return decodeURIComponent(encoded)
  } catch {
    return encoded
  }
}
